//! Launcher window state shared between the UI thread, the hotkey thread and the tray.
use crate::sys;
use eframe::egui::Context;
use parking_lot::Mutex;
use std::sync::{
    atomic::{AtomicBool, AtomicIsize, AtomicU32, Ordering},
    OnceLock,
};

// ---- layout (logical pixels) ----------------------------------------------------------------
pub const WIDTH: f32 = 480.0;
pub const INPUT_H: f32 = 48.0;
pub const ROW_H: f32 = 40.0;
pub const LIST_PAD: f32 = 6.0;

pub static HWND: AtomicIsize = AtomicIsize::new(0);
pub static VISIBLE: AtomicBool = AtomicBool::new(false);
/// Bumped on every show: the UI resets itself when it sees a new value.
pub static SHOW_GEN: AtomicU32 = AtomicU32::new(0);
/// Bumped after every index scan: the UI drops its result cache.
pub static INDEX_GEN: AtomicU32 = AtomicU32::new(0);
pub static GEO: Mutex<sys::Geo> = Mutex::new(sys::Geo {
    x: 0,
    y: 0,
    work_top: 0,
    work_bottom: 0,
    scale: 1.0,
});
pub static CTX: OnceLock<Context> = OnceLock::new();

fn repaint() {
    if let Some(c) = CTX.get() {
        c.request_repaint();
    }
}

pub fn show() {
    let raw = HWND.load(Ordering::SeqCst);
    if raw == 0 {
        return;
    }
    SHOW_GEN.fetch_add(1, Ordering::SeqCst);
    VISIBLE.store(true, Ordering::SeqCst);
    *GEO.lock() = sys::place_window(raw, WIDTH, INPUT_H);
    sys::focus_window(raw);
    repaint();
}

/// New panel height (logical px).
pub fn resize(height: f32) {
    let raw = HWND.load(Ordering::SeqCst);
    if raw != 0 {
        sys::resize_window(raw, &GEO.lock(), WIDTH, height);
    }
}

pub fn hide() {
    if VISIBLE.swap(false, Ordering::SeqCst) {
        let raw = HWND.load(Ordering::SeqCst);
        if raw != 0 {
            sys::hide_window(raw);
        }
    }
}

pub fn toggle() {
    if VISIBLE.load(Ordering::SeqCst) {
        hide();
    } else {
        show();
    }
}

pub fn index_changed() {
    INDEX_GEN.fetch_add(1, Ordering::SeqCst);
    if VISIBLE.load(Ordering::SeqCst) {
        repaint();
    }
}
