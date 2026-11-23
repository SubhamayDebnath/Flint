//! Self-install: double-click `flint.exe` once and it installs itself for the current user
//! (no admin, no wizard): copies itself to %LOCALAPPDATA%\Programs\Flint, registers in
//! Settings > Apps, adds a Start-menu shortcut, starts with Windows, and keeps running in the tray.
use crate::sys;
use std::{
    os::windows::process::CommandExt,
    path::{Path, PathBuf},
    process::Command,
};
use winreg::{enums::HKEY_CURRENT_USER, RegKey};

const UNINSTALL_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\Flint";
const NO_WINDOW: u32 = 0x0800_0000;

fn dir() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA").map(|b| PathBuf::from(b).join("Programs").join("Flint"))
}

fn shortcut_path() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(|b| {
        PathBuf::from(b)
            .join(r"Microsoft\Windows\Start Menu\Programs")
            .join("Flint.lnk")
    })
}

fn same(a: &Path, b: &Path) -> bool {
    a.to_string_lossy()
        .eq_ignore_ascii_case(&b.to_string_lossy())
}

/// Should this run install itself first?
/// No for: debug builds, `--no-install`, portable ZIP, the installed copy, MSI (Program Files).
pub fn needed() -> bool {
    if cfg!(debug_assertions) || std::env::args().any(|a| a == "--no-install") || sys::is_portable()
    {
        return false;
    }
    let (Ok(exe), Some(d)) = (std::env::current_exe(), dir()) else {
        return false;
    };
    if same(exe.parent().unwrap_or(Path::new("")), &d) {
        return false;
    }
    let pf = std::env::var("ProgramFiles")
        .unwrap_or_default()
        .to_lowercase();
    !(!pf.is_empty() && exe.to_string_lossy().to_lowercase().starts_with(&pf))
}

fn kill_running() {
    let _ = Command::new(sys::system_exe("taskkill.exe"))
        .args(["/F", "/IM", "flint.exe", "/FI"])
        .arg(format!("PID ne {}", std::process::id()))
        .creation_flags(NO_WINDOW)
        .status();
    std::thread::sleep(std::time::Duration::from_millis(600));
}

fn make_shortcut(target: &Path, dir: &Path, lnk: &Path) -> bool {
    sys::create_shortcut(
        &target.to_string_lossy(),
        &dir.to_string_lossy(),
        &lnk.to_string_lossy(),
    )
}

/// Install (or update) and start the installed copy. Returns true when the caller should exit.
pub fn install_and_launch() -> bool {
    let (Ok(me), Some(d)) = (std::env::current_exe(), dir()) else {
        return false;
    };
    kill_running();
    let dest = d.join("flint.exe");
    if std::fs::create_dir_all(&d).is_err() || std::fs::copy(&me, &dest).is_err() {
        sys::message(
            "Flint",
            "Could not install Flint (copy failed). Running without installing.",
        );
        return false;
    }
    if let Some(lnk) = shortcut_path() {
        if let Some(p) = lnk.parent() {
            let _ = std::fs::create_dir_all(p);
        }
        make_shortcut(&dest, &d, &lnk);
    }
    // Settings > Apps > Installed apps
    if let Ok((k, _)) = RegKey::predef(HKEY_CURRENT_USER).create_subkey(UNINSTALL_KEY) {
        let exe = dest.to_string_lossy().to_string();
        let _ = k.set_value("DisplayName", &"Flint");
        let _ = k.set_value("DisplayVersion", &env!("CARGO_PKG_VERSION"));
        let _ = k.set_value("Publisher", &"subhamay");
        let _ = k.set_value("DisplayIcon", &exe);
        let _ = k.set_value("InstallLocation", &d.to_string_lossy().to_string());
        let _ = k.set_value("UninstallString", &format!("\"{exe}\" --uninstall-app"));
        let _ = k.set_value(
            "QuietUninstallString",
            &format!("\"{exe}\" --uninstall-app"),
        );
        let _ = k.set_value("NoModify", &1u32);
        let _ = k.set_value("NoRepair", &1u32);
        let _ = k.set_value("EstimatedSize", &12_000u32);
    }
    // start with Windows (points at the installed copy)
    sys::set_autostart_path(true, &dest);
    let _ = std::fs::create_dir_all(
        std::env::var_os("LOCALAPPDATA")
            .map(|b| PathBuf::from(b).join("Flint"))
            .unwrap_or_default(),
    );
    let _ = Command::new(&dest).current_dir(&d).spawn();
    true
}

/// Remove everything the installer added (called from Settings > Apps > Uninstall).
pub fn uninstall_app() {
    kill_running();
    sys::set_autostart(false);
    let _ = RegKey::predef(HKEY_CURRENT_USER).delete_subkey_all(UNINSTALL_KEY);
    if let Some(l) = shortcut_path() {
        let _ = std::fs::remove_file(l);
    }
    if let Some(d) = dir() {
        // the running exe cannot delete itself: let a short-lived cmd do it
        let _ = Command::new(sys::system_exe("cmd.exe"))
            .raw_arg(format!(
                "/c ping -n 3 127.0.0.1 >nul & rmdir /s /q \"{}\"",
                d.display()
            ))
            .creation_flags(NO_WINDOW)
            .spawn();
    }
    if let Some(c) = std::env::var_os("LOCALAPPDATA").map(|b| PathBuf::from(b).join("Flint")) {
        let _ = std::fs::remove_dir_all(c);
    }
}
