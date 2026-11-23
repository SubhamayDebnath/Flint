//! Flint: instant, offline, keyboard-first launcher. 100 % native Rust (egui + Win32).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod calc;
mod index;
mod sys;
mod ui;
mod win;

use eframe::egui;
use index::Index;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::sync::{atomic::Ordering, Arc};
use tray_icon::{
    menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem},
    MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent,
};

const ICON_PNG: &[u8] = include_bytes!("../assets/icon.png");

/// Decode the bundled PNG into straight RGBA.
fn icon_rgba() -> Option<(Vec<u8>, u32, u32)> {
    let mut reader = png::Decoder::new(std::io::Cursor::new(ICON_PNG))
        .read_info()
        .ok()?;
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).ok()?;
    buf.truncate(info.buffer_size());
    let rgba = match info.color_type {
        png::ColorType::Rgba => buf,
        png::ColorType::Rgb => buf
            .chunks_exact(3)
            .flat_map(|p| [p[0], p[1], p[2], 255])
            .collect(),
        _ => return None,
    };
    Some((rgba, info.width, info.height))
}

fn build_tray() -> Option<tray_icon::TrayIcon> {
    let (rgba, w, h) = icon_rgba()?;
    let icon = tray_icon::Icon::from_rgba(rgba, w, h).ok()?;

    let menu = Menu::new();
    let open = MenuItem::with_id("open", "Open Flint", true, None);
    let auto = CheckMenuItem::with_id(
        "autostart",
        "Start with Windows",
        true,
        sys::autostart_enabled(),
        None,
    );
    let quit = MenuItem::with_id("quit", "Quit", true, None);
    menu.append_items(&[&open, &auto, &PredefinedMenuItem::separator(), &quit])
        .ok()?;

    // events are delivered on the UI thread's message loop
    MenuEvent::set_event_handler(Some(|e: MenuEvent| match e.id.as_ref() {
        "open" => win::show(),
        "autostart" => sys::set_autostart(!sys::autostart_enabled()),
        "quit" => std::process::exit(0),
        _ => {}
    }));
    TrayIconEvent::set_event_handler(Some(|e: TrayIconEvent| {
        if let TrayIconEvent::Click {
            button: MouseButton::Left,
            button_state: MouseButtonState::Up,
            ..
        } = e
        {
            win::show();
        }
    }));

    TrayIconBuilder::new()
        .with_icon(icon)
        .with_tooltip("Flint")
        .with_menu(Box::new(menu))
        .with_menu_on_left_click(false)
        .build()
        .ok()
}

fn main() -> eframe::Result {
    // called by the MSI uninstaller: remove the Run-key entry of the current user, then exit
    if std::env::args().any(|a| a == "--uninstall") {
        sys::set_autostart(false);
        return Ok(());
    }
    if !sys::single_instance() {
        return Ok(()); // already running: the first instance was asked to show itself
    }
    sys::first_run_autostart();

    let index = Arc::new(Index::default());

    let mut viewport = egui::ViewportBuilder::default()
        .with_title("Flint")
        .with_inner_size([win::WIDTH, win::INPUT_H])
        .with_decorations(false)
        .with_transparent(true)
        .with_resizable(false)
        .with_always_on_top()
        .with_taskbar(false)
        .with_position([-32_000.0, -32_000.0]) // starts off-screen; shown only by Alt+Space
        .with_active(false)
        .with_visible(false);
    if let Some((rgba, width, height)) = icon_rgba() {
        viewport = viewport.with_icon(egui::IconData {
            rgba,
            width,
            height,
        });
    }

    let options = eframe::NativeOptions {
        viewport,
        renderer: eframe::Renderer::Glow,
        multisampling: 0,
        depth_buffer: 0,
        stencil_buffer: 0,
        ..Default::default()
    };

    eframe::run_native(
        "Flint",
        options,
        Box::new(move |cc| {
            // native window handle: placement, show / hide, Acrylic
            if let Ok(h) = cc.window_handle() {
                if let RawWindowHandle::Win32(w) = h.as_raw() {
                    win::HWND.store(w.hwnd.get(), Ordering::SeqCst);
                    sys::round_corners(w.hwnd.get());
                }
            }
            // thin shadow around the borderless window
            if let Some(w) = cc.winit_window() {
                use winit::platform::windows::WindowExtWindows;
                w.set_undecorated_shadow(true);
            }
            let _ = window_vibrancy::apply_acrylic(cc, None);
            let _ = win::CTX.set(cc.egui_ctx.clone());
            ui::setup(&cc.egui_ctx);

            index.on_scan(win::index_changed);
            index.start();

            sys::spawn_hotkey(win::toggle);
            sys::spawn_show_listener(win::show);
            let tray = build_tray();

            Ok(Box::new(ui::FlintApp::new(&cc.egui_ctx, index, tray)))
        }),
    )
}
