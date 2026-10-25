mod index;
mod sys;

use index::{Hit, Index};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, LogicalSize, Manager, PhysicalPosition, State, WebviewWindow, WindowEvent,
};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};
use tauri_plugin_global_shortcut::ShortcutState;

const WIDTH: f64 = 480.0;
const ICON_PX: i32 = 48;

/// True while the start-up warm-up (off-screen show) is still running.
static WARMING: AtomicBool = AtomicBool::new(false);

/// Render the page once, far off-screen, so the very first Alt+Space is instant.
fn warm_up(app: &AppHandle) {
    let Some(w) = app.get_webview_window("main") else {
        return;
    };
    WARMING.store(true, Ordering::SeqCst);
    let _ = w.set_position(PhysicalPosition::new(-32_000, -32_000));
    let _ = w.show();
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(700));
        // if the user already opened Flint, `show` cleared the flag: leave the window alone
        if WARMING
            .compare_exchange(true, false, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
        {
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.hide();
            }
        }
    });
}

/// Autostart is switched on only the first time, so turning it off in
/// Windows Settings -> Startup apps is respected from then on.
fn first_run() -> bool {
    let Some(dir) = std::env::var_os("LOCALAPPDATA").map(|b| PathBuf::from(b).join("Flint")) else {
        return true;
    };
    let marker = dir.join(".autostart-set");
    if marker.exists() {
        return false;
    }
    let _ = std::fs::create_dir_all(&dir);
    let _ = std::fs::write(marker, b"1");
    true
}

fn place(w: &WebviewWindow) {
    let app = w.app_handle();
    let mon = app
        .cursor_position()
        .ok()
        .and_then(|p| w.monitor_from_point(p.x, p.y).ok().flatten())
        .or_else(|| w.primary_monitor().ok().flatten());
    if let Some(m) = mon {
        let (s, p) = (m.size(), m.position());
        let ww = w
            .outer_size()
            .map(|x| x.width as i32)
            .unwrap_or((WIDTH * m.scale_factor()) as i32);
        let x = p.x + (s.width as i32 - ww) / 2;
        let y = p.y + (s.height as f64 * 0.14) as i32; // between top and centre
        let _ = w.set_position(PhysicalPosition::new(x, y));
    }
}

fn show(app: &AppHandle) {
    WARMING.store(false, Ordering::SeqCst);
    if let Some(w) = app.get_webview_window("main") {
        place(&w);
        let _ = w.show();
        let _ = w.set_focus();
        let _ = app.emit("flint-show", ());
    }
}

fn toggle(app: &AppHandle) {
    match app.get_webview_window("main") {
        Some(w) if !WARMING.load(Ordering::SeqCst) && w.is_visible().unwrap_or(false) => {
            let _ = w.hide();
        }
        _ => show(app),
    }
}

#[tauri::command(async)]
fn search(index: State<'_, Arc<Index>>, query: String, category: String) -> Vec<Hit> {
    index.search(&query, &category)
}

fn rescan_async(index: &Arc<Index>) {
    let i = index.clone();
    std::thread::spawn(move || i.rescan());
}

#[tauri::command(async)]
fn launch(
    window: WebviewWindow,
    index: State<'_, Arc<Index>>,
    kind: String,
    path: String,
    reveal: bool,
) {
    if path.trim().is_empty() {
        let _ = window.hide();
        return;
    }
    match kind.as_str() {
        "command" => sys::run_command(&path, || rescan_async(index.inner())),
        "app" if !reveal => {
            if !sys::activate_running(&index.app_target(&path)) {
                sys::open(&path);
            }
        }
        _ if reveal && !path.starts_with("shell:") => sys::reveal(&path),
        _ => sys::open(&path),
    }
    let _ = window.hide();
}

#[tauri::command(async)]
fn reindex(index: State<'_, Arc<Index>>) {
    rescan_async(index.inner());
}

#[tauri::command]
fn hide(window: WebviewWindow) {
    let _ = window.hide();
}

#[tauri::command]
fn resize(window: WebviewWindow, height: f64) {
    let _ = window.set_size(LogicalSize::new(WIDTH, height));
}

#[tauri::command(async)]
fn icon(path: String) -> Option<String> {
    sys::icon_data_url(&path, ICON_PX)
}

pub fn run() {
    let index = Arc::new(Index::default());

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| show(app)))
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_shortcuts(["alt+space"])
                .expect("invalid shortcut")
                .with_handler(|app, _, ev| {
                    if ev.state() == ShortcutState::Pressed {
                        toggle(app);
                    }
                })
                .build(),
        )
        .manage(index.clone())
        .invoke_handler(tauri::generate_handler![
            search, launch, reindex, hide, resize, icon
        ])
        .on_window_event(|w, e| {
            // hide on blur, but never while the off-screen warm-up is running
            if let WindowEvent::Focused(false) = e {
                if !WARMING.load(Ordering::SeqCst) {
                    let _ = w.hide();
                }
            }
        })
        .setup(move |app| {
            if first_run() {
                let autostart = app.autolaunch();
                if !autostart.is_enabled().unwrap_or(false) {
                    let _ = autostart.enable();
                }
            }

            // tell the UI to drop cached results whenever the index changes
            let handle = app.handle().clone();
            index.on_scan(move || {
                let _ = handle.emit("flint-index", ());
            });
            index.start();

            let menu = Menu::with_items(
                app,
                &[
                    &MenuItem::with_id(app, "open", "Open Flint", true, None::<&str>)?,
                    &MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?,
                ],
            )?;
            TrayIconBuilder::new()
                .icon(app.default_window_icon().unwrap().clone())
                .tooltip("Flint")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, e| match e.id.as_ref() {
                    "open" => show(app),
                    "quit" => app.exit(0),
                    _ => {}
                })
                .on_tray_icon_event(|tray, e| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = e
                    {
                        show(tray.app_handle());
                    }
                })
                .build(app)?;

            if let Some(window) = app.get_webview_window("main") {
                let _ = window_vibrancy::apply_acrylic(&window, None);
            }
            warm_up(app.handle());
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running Flint");
}
