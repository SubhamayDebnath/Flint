//! Thin Win32 layer: icons, shortcuts, window activation, system commands.
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
    core::{Interface, PCWSTR, PWSTR},
    Win32::{
        Foundation::{CloseHandle, BOOL, FALSE, HWND, LPARAM, SIZE, TRUE},
        Graphics::Gdi::*,
        Storage::FileSystem::{GetDriveTypeW, GetLogicalDrives, WIN32_FIND_DATAW},
        System::{
            Com::*,
            Threading::{
                GetCurrentThread, OpenProcess, QueryFullProcessImageNameW, SetThreadPriority,
                PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, THREAD_PRIORITY,
            },
        },
        UI::{Shell::*, WindowsAndMessaging::*},
    },
};

const NO_WINDOW: u32 = 0x0800_0000;

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

/// Initialise COM once per thread.
fn com_init() {
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
/// so indexing never competes with the foreground apps (important on low-end hardware).
const THREAD_MODE_BACKGROUND_BEGIN: THREAD_PRIORITY = THREAD_PRIORITY(0x0001_0000);

/// Keep background indexing from competing with the UI.
pub fn lower_thread_priority() {
    unsafe {
        let _ = SetThreadPriority(GetCurrentThread(), THREAD_MODE_BACKGROUND_BEGIN);
    }
}

/// Absolute path of a built-in Windows tool. Never resolved through the current directory / PATH,
/// so a planted `explorer.exe` or `shutdown.exe` can never be launched by mistake.
fn system_exe(name: &str) -> PathBuf {
    let root =
        PathBuf::from(std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into()));
    if name.eq_ignore_ascii_case("explorer.exe") {
        root.join(name)
    } else {
        root.join("System32").join(name)
    }
}

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

/// High-resolution icon as a base64 PNG data URL.
pub fn icon_data_url(path: &str, px: i32) -> Option<String> {
    use base64::{engine::general_purpose::STANDARD, Engine};
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
            hbmp,
            size_of::<BITMAP>() as i32,
            Some(&mut bm as *mut _ as *mut _),
        );
        let (w, h) = (bm.bmWidth, bm.bmHeight);
        if w <= 0 || h <= 0 {
            let _ = DeleteObject(hbmp);
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
        let _ = DeleteObject(hbmp);
        if lines == 0 {
            return None;
        }

        let no_alpha = buf.chunks_exact(4).all(|p| p[3] == 0);
        for p in buf.chunks_exact_mut(4) {
            p.swap(0, 2); // BGRA -> RGBA
            if no_alpha {
                p[3] = 255;
            }
        }
        let mut out = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut out, w as u32, h as u32);
            enc.set_color(png::ColorType::Rgba);
            enc.set_depth(png::BitDepth::Eight);
            enc.set_compression(png::Compression::Fast);
            enc.write_header().ok()?.write_image_data(&buf).ok()?;
        }
        Some(format!("data:image/png;base64,{}", STANDARD.encode(out)))
    }
}

pub fn open(path: &str) {
    unsafe {
        let r = ShellExecuteW(
            None,
            windows::core::w!("open"),
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

pub enum Run {
    /// Opened through the shell (apps, .msc, ms-settings:, shell: folders)
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
}

use Run::{Exe, Open, Reindex, Trash};
pub const COMMANDS: &[Cmd] = &[
    // power
    Cmd {
        id: "lock",
        name: "Lock",
        keys: &["lock"],
        run: Exe("rundll32.exe", "user32.dll,LockWorkStation"),
    },
    Cmd {
        id: "sleep",
        name: "Sleep",
        keys: &["sleep"],
        run: Exe("rundll32.exe", "powrprof.dll,SetSuspendState 0,1,0"),
    },
    Cmd {
        id: "hibernate",
        name: "Hibernate",
        keys: &["hibernate"],
        run: Exe("shutdown.exe", "/h"),
    },
    Cmd {
        id: "signout",
        name: "Sign out",
        keys: &["sign out", "signout", "log off", "logoff"],
        run: Exe("shutdown.exe", "/l"),
    },
    Cmd {
        id: "restart",
        name: "Restart",
        keys: &["restart", "reboot"],
        run: Exe("shutdown.exe", "/r /t 0"),
    },
    Cmd {
        id: "shutdown",
        name: "Shut down",
        keys: &["shutdown", "shut down", "power off"],
        run: Exe("shutdown.exe", "/s /t 0"),
    },
    // quick actions
    Cmd {
        id: "trash",
        name: "Empty trash",
        keys: &["empty trash", "empty recycle bin", "empty bin"],
        run: Trash,
    },
    Cmd {
        id: "recyclebin",
        name: "Open trash",
        keys: &["trash", "recycle bin", "open trash"],
        run: Open("shell:RecycleBinFolder"),
    },
    Cmd {
        id: "taskmgr",
        name: "Task manager",
        keys: &["task manager", "taskmgr"],
        run: Open("taskmgr.exe"),
    },
    Cmd {
        id: "desktop",
        name: "Show desktop",
        keys: &["show desktop", "desktop"],
        run: Exe(
            "explorer.exe",
            "shell:::{3080F90D-D7AD-11D9-BD98-0000FFFFFFFF}",
        ),
    },
    Cmd {
        id: "snip",
        name: "Screenshot",
        keys: &["screenshot", "snip", "snipping tool"],
        run: Open("ms-screenclip:"),
    },
    Cmd {
        id: "flushdns",
        name: "Flush DNS",
        keys: &["flush dns", "dns"],
        run: Exe("ipconfig.exe", "/flushdns"),
    },
    Cmd {
        id: "reindex",
        name: "Re-index files",
        keys: &["reindex", "re-index", "rebuild index", "refresh index"],
        run: Reindex,
    },
    // folders
    Cmd {
        id: "thispc",
        name: "This PC",
        keys: &["this pc", "my computer"],
        run: Open("shell:MyComputerFolder"),
    },
    Cmd {
        id: "downloads",
        name: "Downloads",
        keys: &["downloads"],
        run: Open("shell:Downloads"),
    },
    Cmd {
        id: "documents",
        name: "Documents",
        keys: &["documents"],
        run: Open("shell:Personal"),
    },
    // settings
    Cmd {
        id: "settings",
        name: "Settings",
        keys: &["settings"],
        run: Open("ms-settings:"),
    },
    Cmd {
        id: "wifi",
        name: "Wi-Fi settings",
        keys: &["wifi", "wi-fi", "network"],
        run: Open("ms-settings:network-wifi"),
    },
    Cmd {
        id: "bluetooth",
        name: "Bluetooth settings",
        keys: &["bluetooth"],
        run: Open("ms-settings:bluetooth"),
    },
    Cmd {
        id: "display",
        name: "Display settings",
        keys: &["display", "resolution", "night light"],
        run: Open("ms-settings:display"),
    },
    Cmd {
        id: "sound",
        name: "Sound settings",
        keys: &["sound", "volume", "audio"],
        run: Open("ms-settings:sound"),
    },
    Cmd {
        id: "apps",
        name: "Installed apps",
        keys: &["installed apps", "apps & features", "uninstall"],
        run: Open("ms-settings:appsfeatures"),
    },
    Cmd {
        id: "startup",
        name: "Startup apps",
        keys: &["startup apps", "startup"],
        run: Open("ms-settings:startupapps"),
    },
    Cmd {
        id: "storage",
        name: "Storage settings",
        keys: &["storage", "disk space"],
        run: Open("ms-settings:storagesense"),
    },
    Cmd {
        id: "update",
        name: "Windows Update",
        keys: &["windows update", "update"],
        run: Open("ms-settings:windowsupdate"),
    },
    // system tools
    Cmd {
        id: "control",
        name: "Control panel",
        keys: &["control panel", "control"],
        run: Open("control.exe"),
    },
    Cmd {
        id: "devmgr",
        name: "Device manager",
        keys: &["device manager", "devices"],
        run: Open("devmgmt.msc"),
    },
    Cmd {
        id: "diskmgmt",
        name: "Disk management",
        keys: &["disk management", "partitions"],
        run: Open("diskmgmt.msc"),
    },
    Cmd {
        id: "services",
        name: "Services",
        keys: &["services"],
        run: Open("services.msc"),
    },
    Cmd {
        id: "eventvwr",
        name: "Event viewer",
        keys: &["event viewer", "logs"],
        run: Open("eventvwr.msc"),
    },
    Cmd {
        id: "resmon",
        name: "Resource monitor",
        keys: &["resource monitor", "performance"],
        run: Open("resmon.exe"),
    },
    Cmd {
        id: "sysinfo",
        name: "System information",
        keys: &["system information", "msinfo"],
        run: Open("msinfo32.exe"),
    },
    Cmd {
        id: "regedit",
        name: "Registry editor",
        keys: &["registry editor", "regedit", "registry"],
        run: Open("regedit.exe"),
    },
    Cmd {
        id: "cmd",
        name: "Command prompt",
        keys: &["command prompt", "cmd"],
        run: Open("cmd.exe"),
    },
    Cmd {
        id: "powershell",
        name: "PowerShell",
        keys: &["powershell"],
        run: Open("powershell.exe"),
    },
    Cmd {
        id: "terminal",
        name: "Windows Terminal",
        keys: &["terminal", "windows terminal"],
        run: Open("wt.exe"),
    },
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
