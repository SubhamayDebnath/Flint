//! Thin Win32 layer: icons, window placement, shortcuts, system commands, autostart.
use std::{
    io::Read,
    mem::size_of,
    os::windows::process::CommandExt,
    path::PathBuf,
    process::{Command, Stdio},
    sync::mpsc,
    thread,
    time::Duration,
};
use windows::{
    core::{w, Interface, BOOL, PCWSTR, PWSTR},
    Win32::{
        Foundation::{
            CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, FALSE, HANDLE, HWND, LPARAM, POINT,
            RECT, SIZE, TRUE,
        },
        Graphics::{
            Dwm::{DwmSetWindowAttribute, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND},
            Gdi::*,
        },
        Storage::FileSystem::{GetDriveTypeW, GetLogicalDrives, WIN32_FIND_DATAW},
        System::{
            Com::*,
            Threading::{
                AttachThreadInput, CreateEventW, CreateMutexW, GetCurrentThread,
                GetCurrentThreadId, OpenEventW, OpenProcess, QueryFullProcessImageNameW, SetEvent,
                SetThreadPriority, WaitForSingleObject, EVENT_MODIFY_STATE, INFINITE,
                PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, THREAD_PRIORITY,
            },
        },
        UI::{
            HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI},
            Input::KeyboardAndMouse::{RegisterHotKey, MOD_ALT, MOD_NOREPEAT, VK_SPACE},
            Shell::*,
            WindowsAndMessaging::*,
        },
    },
};
use winreg::{enums::HKEY_CURRENT_USER, RegKey};

const NO_WINDOW: u32 = 0x0800_0000;

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

/// Initialise COM once per thread (needed by shell icons, shortcuts and ShellExecute).
pub fn com_init() {
    thread_local!(static DONE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) });
    DONE.with(|d| {
        if !d.get() {
            unsafe {
                let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            }
            d.set(true);
        }
    });
}

/// THREAD_MODE_BACKGROUND_BEGIN: lowers CPU, I/O and memory priority of the calling thread,
/// so indexing never competes with the foreground apps.
const THREAD_MODE_BACKGROUND_BEGIN: THREAD_PRIORITY = THREAD_PRIORITY(0x0001_0000);

pub fn lower_thread_priority() {
    unsafe {
        let _ = SetThreadPriority(GetCurrentThread(), THREAD_MODE_BACKGROUND_BEGIN);
    }
}

/// Absolute path of a built-in Windows tool. Never resolved through the current directory / PATH,
/// so a planted `explorer.exe` or `shutdown.exe` can never be launched by mistake.
pub fn system_exe(name: &str) -> PathBuf {
    let root =
        PathBuf::from(std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into()));
    if name.eq_ignore_ascii_case("explorer.exe") {
        root.join(name)
    } else {
        root.join("System32").join(name)
    }
}

// --------------------------------------------------------------------------------------------
// drives, shortcuts, Store apps
// --------------------------------------------------------------------------------------------

/// All local fixed drives (C:\, D:\, ...).
pub fn fixed_drives() -> Vec<String> {
    unsafe {
        let mask = GetLogicalDrives();
        (0..26u8)
            .filter(|&i| mask >> i & 1 == 1)
            .map(|i| format!("{}:\\", (b'A' + i) as char))
            .filter(|p| GetDriveTypeW(PCWSTR(wide(p).as_ptr())) == 3) // DRIVE_FIXED
            .collect()
    }
}

/// Resolve a .lnk to its lower-cased target path.
pub fn resolve_lnk(path: &str) -> Option<String> {
    unsafe {
        com_init();
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).ok()?;
        let pf: IPersistFile = link.cast().ok()?;
        pf.Load(PCWSTR(wide(path).as_ptr()), STGM_READ).ok()?;
        let mut buf = [0u16; 520];
        link.GetPath(&mut buf, std::ptr::null_mut::<WIN32_FIND_DATAW>(), 0)
            .ok()?;
        let n = buf.iter().position(|&c| c == 0).unwrap_or(0);
        (n > 0).then(|| String::from_utf16_lossy(&buf[..n]).to_lowercase())
    }
}

/// Store / UWP apps as (name, shell path). Gives up after 20 s so a stuck PowerShell can
/// never block indexing forever.
pub fn uwp_apps() -> Vec<(String, String)> {
    let script = "[Console]::OutputEncoding=[Text.Encoding]::UTF8; Get-StartApps | ForEach-Object { $_.Name + '|' + $_.AppID }";
    let ps = system_exe(r"WindowsPowerShell\v1.0\powershell.exe");
    let Ok(mut child) = Command::new(ps)
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .creation_flags(NO_WINDOW)
        .spawn()
    else {
        return vec![];
    };
    let Some(mut stdout) = child.stdout.take() else {
        let _ = child.kill();
        return vec![];
    };
    // read on a helper thread so a full pipe can never deadlock the wait
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stdout.read_to_end(&mut buf);
        let _ = tx.send(buf);
    });
    let bytes = match rx.recv_timeout(Duration::from_secs(20)) {
        Ok(b) => {
            let _ = child.wait();
            b
        }
        Err(_) => {
            let _ = child.kill();
            let _ = child.wait();
            return vec![];
        }
    };
    String::from_utf8_lossy(&bytes)
        .lines()
        .filter_map(|l| l.trim().split_once('|'))
        .filter(|(_, id)| id.contains('!'))
        .map(|(n, id)| (n.to_string(), format!("shell:AppsFolder\\{id}")))
        .collect()
}

// --------------------------------------------------------------------------------------------
// icons
// --------------------------------------------------------------------------------------------

/// Shell icon as straight RGBA pixels: (width, height, bytes).
pub fn icon_rgba(path: &str, px: i32) -> Option<(usize, usize, Vec<u8>)> {
    unsafe {
        com_init();
        let factory: IShellItemImageFactory = SHCreateItemFromParsingName(
            PCWSTR(wide(path).as_ptr()),
            None::<&windows::Win32::System::Com::IBindCtx>,
        )
        .ok()?;
        let hbmp = factory
            .GetImage(
                SIZE { cx: px, cy: px },
                SIIGBF_ICONONLY | SIIGBF_BIGGERSIZEOK,
            )
            .ok()?;

        let mut bm = BITMAP::default();
        GetObjectW(
            hbmp.into(),
            size_of::<BITMAP>() as i32,
            Some(&mut bm as *mut _ as *mut _),
        );
        let (w, h) = (bm.bmWidth, bm.bmHeight);
        if w <= 0 || h <= 0 {
            let _ = DeleteObject(hbmp.into());
            return None;
        }
        let mut bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h, // top-down
                biPlanes: 1,
                biBitCount: 32,
                biCompression: 0, // BI_RGB
                ..Default::default()
            },
            ..Default::default()
        };
        let mut buf = vec![0u8; (w * h * 4) as usize];
        let hdc = GetDC(None);
        let lines = GetDIBits(
            hdc,
            hbmp,
            0,
            h as u32,
            Some(buf.as_mut_ptr() as *mut _),
            &mut bmi,
            DIB_RGB_COLORS,
        );
        ReleaseDC(None, hdc);
        let _ = DeleteObject(hbmp.into());
        if lines == 0 {
            return None;
        }

        // BGRA (premultiplied by the shell) -> straight RGBA
        let no_alpha = buf.chunks_exact(4).all(|p| p[3] == 0);
        for p in buf.chunks_exact_mut(4) {
            p.swap(0, 2);
            if no_alpha {
                p[3] = 255;
            } else if p[3] != 0 && p[3] != 255 {
                let a = p[3] as u32;
                for c in &mut p[..3] {
                    *c = ((*c as u32 * 255 + a / 2) / a).min(255) as u8;
                }
            }
        }
        Some((w as usize, h as usize, buf))
    }
}

// --------------------------------------------------------------------------------------------
// opening things
// --------------------------------------------------------------------------------------------

pub fn open(path: &str) {
    unsafe {
        com_init(); // ShellExecute must run on a COM-initialised thread
        let r = ShellExecuteW(
            None,
            w!("open"),
            PCWSTR(wide(path).as_ptr()),
            None,
            None,
            SW_SHOWNORMAL,
        );
        // <= 32 means ShellExecute failed: fall back to Explorer
        if (r.0 as isize) <= 32 {
            let _ = Command::new(system_exe("explorer.exe")).arg(path).spawn();
        }
    }
}

pub fn reveal(path: &str) {
    let _ = Command::new(system_exe("explorer.exe"))
        .raw_arg(format!("/select,\"{path}\""))
        .spawn();
}

struct Ctx {
    target: String,
    found: bool,
}

unsafe extern "system" fn enum_cb(hwnd: HWND, lp: LPARAM) -> BOOL {
    let ctx = &mut *(lp.0 as *mut Ctx);
    if !IsWindowVisible(hwnd).as_bool() || GetWindowTextLengthW(hwnd) == 0 {
        return TRUE;
    }
    if GetWindowLongW(hwnd, GWL_EXSTYLE) as u32 & WS_EX_TOOLWINDOW.0 != 0 {
        return TRUE;
    }
    let mut pid = 0u32;
    GetWindowThreadProcessId(hwnd, Some(&mut pid));
    if let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let ok =
            QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len)
                .is_ok();
        let _ = CloseHandle(h);
        if ok && String::from_utf16_lossy(&buf[..len as usize]).to_lowercase() == ctx.target {
            if IsIconic(hwnd).as_bool() {
                let _ = ShowWindow(hwnd, SW_RESTORE);
            }
            let _ = SetForegroundWindow(hwnd);
            ctx.found = true;
            return FALSE;
        }
    }
    TRUE
}

/// Bring an already running app (matched by exe path) to the foreground.
pub fn activate_running(target_exe_lower: &str) -> bool {
    if target_exe_lower.is_empty() {
        return false;
    }
    let mut ctx = Ctx {
        target: target_exe_lower.to_string(),
        found: false,
    };
    unsafe {
        let _ = EnumWindows(Some(enum_cb), LPARAM(&mut ctx as *mut Ctx as isize));
    }
    ctx.found
}

// --------------------------------------------------------------------------------------------
// launcher window: placement, show / hide, focus
// --------------------------------------------------------------------------------------------

fn hwnd(raw: isize) -> HWND {
    HWND(raw as *mut _)
}

/// Work area (screen minus taskbar) of the monitor under the mouse, plus its DPI scale.
/// Falls back to the primary monitor.
pub fn work_area_under_cursor() -> (RECT, RECT, f32) {
    unsafe {
        let mut pt = POINT::default();
        let _ = GetCursorPos(&mut pt);
        let mon = MonitorFromPoint(pt, MONITOR_DEFAULTTOPRIMARY);
        let mut mi = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        let fallback = RECT {
            left: 0,
            top: 0,
            right: 1920,
            bottom: 1040,
        };
        let (rect, full) = if GetMonitorInfoW(mon, &mut mi).as_bool() {
            (mi.rcWork, mi.rcMonitor)
        } else {
            (fallback, fallback)
        };
        let (mut dx, mut dy) = (96u32, 96u32);
        let _ = GetDpiForMonitor(mon, MDT_EFFECTIVE_DPI, &mut dx, &mut dy);
        (rect, full, dx.max(96) as f32 / 96.0)
    }
}

/// Where the launcher sits (physical pixels).
#[derive(Clone, Copy, Default)]
pub struct Geo {
    pub x: i32,
    /// top of the search bar (14 % down the screen)
    pub y: i32,
    pub work_top: i32,
    pub work_bottom: i32,
    pub scale: f32,
}

/// Show the window horizontally centred, 14 % from the top (the original Flint position), on
/// the monitor under the mouse. Returns the geometry for `resize_window`.
pub fn place_window(raw: isize, width: f32, bar_h: f32) -> Geo {
    let (work, full, s) = work_area_under_cursor();
    let ww = work.right - work.left;
    let w = ((width * s).round() as i32).min(ww);
    let h = (bar_h * s).round() as i32;
    let g = Geo {
        x: work.left + (ww - w) / 2,
        // original Flint position: 14 % down the screen (between top and centre)
        y: (full.top + ((full.bottom - full.top) as f32 * 0.14) as i32)
            .clamp(work.top, work.bottom - h),
        work_top: work.top,
        work_bottom: work.bottom,
        scale: s,
    };
    unsafe {
        let _ = SetWindowPos(
            hwnd(raw),
            Some(HWND_TOPMOST),
            g.x,
            g.y,
            w,
            h,
            SWP_NOACTIVATE | SWP_SHOWWINDOW,
        );
    }
    g
}

/// Resize for a new list height. The bar stays at the centre; only when the list would reach the
/// taskbar is the whole panel moved up just enough to stay clear of it.
pub fn resize_window(raw: isize, g: &Geo, width: f32, height: f32) {
    let w = (width * g.scale).round() as i32;
    let h = (height * g.scale).round() as i32;
    let y = g.y.min(g.work_bottom - h).max(g.work_top);
    unsafe {
        let _ = SetWindowPos(hwnd(raw), None, g.x, y, w, h, SWP_NOACTIVATE | SWP_NOZORDER);
    }
}

pub fn hide_window(raw: isize) {
    unsafe {
        let _ = ShowWindow(hwnd(raw), SW_HIDE);
    }
}

/// Reliable "bring to front" for a window that was just shown by a global hotkey.
pub fn focus_window(raw: isize) {
    unsafe {
        let me = hwnd(raw);
        let fg = GetForegroundWindow();
        let fg_thread = GetWindowThreadProcessId(fg, None);
        let my_thread = GetCurrentThreadId();
        let attach = fg_thread != 0 && fg_thread != my_thread;
        if attach {
            let _ = AttachThreadInput(my_thread, fg_thread, true);
        }
        let _ = BringWindowToTop(me);
        let _ = SetForegroundWindow(me);
        if attach {
            let _ = AttachThreadInput(my_thread, fg_thread, false);
        }
    }
}

/// Rounded corners on the borderless window (Windows 11; ignored on Windows 10).
pub fn round_corners(raw: isize) {
    unsafe {
        let pref = DWMWCP_ROUND;
        let _ = DwmSetWindowAttribute(
            hwnd(raw),
            DWMWA_WINDOW_CORNER_PREFERENCE,
            &pref as *const _ as *const _,
            size_of::<i32>() as u32,
        );
    }
}

// --------------------------------------------------------------------------------------------
// global hotkey (Alt + Space) and single instance
// --------------------------------------------------------------------------------------------

/// Registers Alt+Space and calls `on_press` for every press. Runs its own message loop.
pub fn spawn_hotkey(on_press: impl Fn() + Send + 'static) {
    thread::spawn(move || unsafe {
        if RegisterHotKey(None, 1, MOD_ALT | MOD_NOREPEAT, VK_SPACE.0 as u32).is_err() {
            return; // another program owns Alt+Space; the tray icon still opens Flint
        }
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            if msg.message == WM_HOTKEY {
                on_press();
            }
        }
    });
}

const SHOW_EVENT: PCWSTR = w!("Local\\Flint.ShowEvent");
const MUTEX: PCWSTR = w!("Local\\Flint.SingleInstance");

/// Returns true when this is the first instance. A second instance wakes the first and exits.
pub fn single_instance() -> bool {
    unsafe {
        let m = CreateMutexW(None, false, MUTEX);
        let already = m.is_ok() && GetLastError() == ERROR_ALREADY_EXISTS;
        if already {
            if let Ok(ev) = OpenEventW(EVENT_MODIFY_STATE, false, SHOW_EVENT) {
                let _ = SetEvent(ev);
                let _ = CloseHandle(ev);
            }
            return false;
        }
        // keep the mutex handle alive for the life of the process
        std::mem::forget(m);
        true
    }
}

/// Calls `on_signal` whenever a second instance is started.
pub fn spawn_show_listener(on_signal: impl Fn() + Send + 'static) {
    unsafe {
        let Ok(ev) = CreateEventW(None, false, false, SHOW_EVENT) else {
            return;
        };
        let raw = ev.0 as isize;
        thread::spawn(move || loop {
            let h = HANDLE(raw as *mut _);
            if WaitForSingleObject(h, INFINITE).0 == 0 {
                on_signal();
            } else {
                break;
            }
        });
    }
}

// --------------------------------------------------------------------------------------------
// autostart (HKCU\...\Run)
// --------------------------------------------------------------------------------------------

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";

pub fn autostart_enabled() -> bool {
    RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(RUN_KEY)
        .and_then(|k| k.get_value::<String, _>("Flint"))
        .is_ok()
}

pub fn set_autostart(on: bool) {
    if let Ok(exe) = std::env::current_exe() {
        set_autostart_path(on, &exe);
    }
}

pub fn set_autostart_path(on: bool, exe: &std::path::Path) {
    let Ok((key, _)) = RegKey::predef(HKEY_CURRENT_USER).create_subkey(RUN_KEY) else {
        return;
    };
    if on {
        let _ = key.set_value("Flint", &format!("\"{}\"", exe.display()));
    } else {
        let _ = key.delete_value("Flint");
    }
}

/// Create a .lnk file.
pub fn create_shortcut(target: &str, work_dir: &str, lnk: &str) -> bool {
    unsafe {
        com_init();
        let Ok(link) = CoCreateInstance::<_, IShellLinkW>(&ShellLink, None, CLSCTX_INPROC_SERVER)
        else {
            return false;
        };
        let _ = link.SetPath(PCWSTR(wide(target).as_ptr()));
        let _ = link.SetWorkingDirectory(PCWSTR(wide(work_dir).as_ptr()));
        let _ = link.SetDescription(w!("Flint launcher"));
        let Ok(pf) = link.cast::<IPersistFile>() else {
            return false;
        };
        pf.Save(PCWSTR(wide(lnk).as_ptr()), true).is_ok()
    }
}

/// Small information box.
pub fn message(title: &str, text: &str) {
    unsafe {
        let _ = MessageBoxW(
            None,
            PCWSTR(wide(text).as_ptr()),
            PCWSTR(wide(title).as_ptr()),
            MB_OK | MB_ICONINFORMATION,
        );
    }
}

/// Folder next to the exe holds `portable.flag` in the portable ZIP.
pub fn is_portable() -> bool {
    std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(|p| p.join("portable.flag").exists()))
        .unwrap_or(false)
}

/// Autostart is switched on only the first time (installed builds), so turning it off in
/// Windows Settings -> Startup apps or in the tray menu is respected from then on.
pub fn first_run_autostart() {
    if is_portable() {
        return;
    }
    let Some(dir) = std::env::var_os("LOCALAPPDATA").map(|b| PathBuf::from(b).join("Flint")) else {
        return;
    };
    let marker = dir.join(".autostart-set");
    if marker.exists() {
        return;
    }
    let _ = std::fs::create_dir_all(&dir);
    let _ = std::fs::write(marker, b"1");
    if !autostart_enabled() {
        set_autostart(true);
    }
}

// --------------------------------------------------------------------------------------------
// quick commands
// --------------------------------------------------------------------------------------------

pub enum Run {
    /// Opened through the shell (apps, ms-settings:, shell: folders)
    Open(&'static str),
    /// Hidden background process
    Exe(&'static str, &'static str),
    Trash,
    Reindex,
}

pub struct Cmd {
    pub id: &'static str,
    pub name: &'static str,
    pub keys: &'static [&'static str],
    pub run: Run,
    /// Needs a second Enter: ending the session can lose unsaved work.
    pub confirm: bool,
}

use Run::{Exe, Open, Reindex, Trash};

const fn cmd(id: &'static str, name: &'static str, keys: &'static [&'static str], run: Run) -> Cmd {
    Cmd {
        id,
        name,
        keys,
        run,
        confirm: false,
    }
}

const fn risky(
    id: &'static str,
    name: &'static str,
    keys: &'static [&'static str],
    run: Run,
) -> Cmd {
    Cmd {
        id,
        name,
        keys,
        run,
        confirm: true,
    }
}

/// Safety review (v0.3): commands that open tools able to delete data or break Windows were
/// removed: Registry editor, Disk management (format / delete volume), Device manager
/// (uninstall drivers) and Services (disable system services). Session-ending commands now
/// ask for a second Enter. "Empty trash" stays by design.
pub const COMMANDS: &[Cmd] = &[
    // power
    cmd(
        "lock",
        "Lock",
        &["lock"],
        Exe("rundll32.exe", "user32.dll,LockWorkStation"),
    ),
    cmd(
        "sleep",
        "Sleep",
        &["sleep"],
        Exe("rundll32.exe", "powrprof.dll,SetSuspendState 0,1,0"),
    ),
    risky(
        "hibernate",
        "Hibernate",
        &["hibernate"],
        Exe("shutdown.exe", "/h"),
    ),
    risky(
        "signout",
        "Sign out",
        &["sign out", "signout", "log off", "logoff"],
        Exe("shutdown.exe", "/l"),
    ),
    risky(
        "restart",
        "Restart",
        &["restart", "reboot"],
        Exe("shutdown.exe", "/r /t 0"),
    ),
    risky(
        "shutdown",
        "Shut down",
        &["shutdown", "shut down", "power off"],
        Exe("shutdown.exe", "/s /t 0"),
    ),
    // quick actions
    cmd(
        "trash",
        "Empty trash",
        &["empty trash", "empty recycle bin", "empty bin"],
        Trash,
    ),
    cmd(
        "recyclebin",
        "Open trash",
        &["trash", "recycle bin", "open trash"],
        Open("shell:RecycleBinFolder"),
    ),
    cmd(
        "taskmgr",
        "Task manager",
        &["task manager", "taskmgr"],
        Open("taskmgr.exe"),
    ),
    cmd(
        "desktop",
        "Show desktop",
        &["show desktop", "desktop"],
        Exe(
            "explorer.exe",
            "shell:::{3080F90D-D7AD-11D9-BD98-0000FFFFFFFF}",
        ),
    ),
    cmd(
        "snip",
        "Screenshot",
        &["screenshot", "snip", "snipping tool"],
        Open("ms-screenclip:"),
    ),
    cmd(
        "flushdns",
        "Flush DNS",
        &["flush dns", "dns"],
        Exe("ipconfig.exe", "/flushdns"),
    ),
    cmd(
        "reindex",
        "Re-index files",
        &["reindex", "re-index", "rebuild index", "refresh index"],
        Reindex,
    ),
    // folders
    cmd(
        "thispc",
        "This PC",
        &["this pc", "my computer"],
        Open("shell:MyComputerFolder"),
    ),
    cmd(
        "downloads",
        "Downloads",
        &["downloads"],
        Open("shell:Downloads"),
    ),
    cmd(
        "documents",
        "Documents",
        &["documents"],
        Open("shell:Personal"),
    ),
    // settings
    cmd("settings", "Settings", &["settings"], Open("ms-settings:")),
    cmd(
        "wifi",
        "Wi-Fi settings",
        &["wifi", "wi-fi", "network"],
        Open("ms-settings:network-wifi"),
    ),
    cmd(
        "bluetooth",
        "Bluetooth settings",
        &["bluetooth"],
        Open("ms-settings:bluetooth"),
    ),
    cmd(
        "display",
        "Display settings",
        &["display", "resolution", "night light"],
        Open("ms-settings:display"),
    ),
    cmd(
        "sound",
        "Sound settings",
        &["sound", "volume", "audio"],
        Open("ms-settings:sound"),
    ),
    cmd(
        "apps",
        "Installed apps",
        &["installed apps", "apps & features", "uninstall"],
        Open("ms-settings:appsfeatures"),
    ),
    cmd(
        "startup",
        "Startup apps",
        &["startup apps", "startup"],
        Open("ms-settings:startupapps"),
    ),
    cmd(
        "storage",
        "Storage settings",
        &["storage", "disk space"],
        Open("ms-settings:storagesense"),
    ),
    cmd(
        "update",
        "Windows Update",
        &["windows update", "update"],
        Open("ms-settings:windowsupdate"),
    ),
    // read-only system tools
    cmd(
        "control",
        "Control panel",
        &["control panel", "control"],
        Open("control.exe"),
    ),
    cmd(
        "eventvwr",
        "Event viewer",
        &["event viewer", "logs"],
        Open("eventvwr.msc"),
    ),
    cmd(
        "resmon",
        "Resource monitor",
        &["resource monitor", "performance"],
        Open("resmon.exe"),
    ),
    cmd(
        "sysinfo",
        "System information",
        &["system information", "msinfo"],
        Open("msinfo32.exe"),
    ),
    cmd(
        "cmd",
        "Command prompt",
        &["command prompt", "cmd"],
        Open("cmd.exe"),
    ),
    cmd(
        "powershell",
        "PowerShell",
        &["powershell"],
        Open("powershell.exe"),
    ),
    cmd(
        "terminal",
        "Windows Terminal",
        &["terminal", "windows terminal"],
        Open("wt.exe"),
    ),
];

/// `reindex` is invoked for the Re-index command (the caller owns the index).
pub fn run_command(id: &str, reindex: impl FnOnce()) {
    let Some(cmd) = COMMANDS.iter().find(|c| c.id == id) else {
        return;
    };
    match &cmd.run {
        Open(target) => open(target),
        Exe(exe, args) => {
            let _ = Command::new(system_exe(exe))
                .creation_flags(NO_WINDOW)
                .raw_arg(args)
                .spawn();
        }
        // 7 = NOCONFIRMATION | NOPROGRESSUI | NOSOUND
        Trash => unsafe {
            let _ = SHEmptyRecycleBinW(None, PCWSTR::null(), 7);
        },
        Reindex => reindex(),
    }
}
