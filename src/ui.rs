//! The launcher panel, drawn natively with egui (immediate mode, OpenGL). No browser engine.
use crate::{
    calc,
    index::{Cat, Hit, Index, Kind, LIMIT, MAX_QUERY},
    sys, win,
};
use eframe::egui::{
    self,
    text::{CCursor, CCursorRange, LayoutJob},
    Align, Align2, Color32, ColorImage, Context, CornerRadius, FontData, FontDefinitions,
    FontFamily, FontId, Frame, Key, Margin, Modifiers, Pos2, Rect, Sense, TextEdit, TextureHandle,
    TextureOptions, UiBuilder, Vec2, ViewportCommand,
};
use egui_phosphor::regular as ph;
use std::{
    collections::{HashMap, VecDeque},
    path::PathBuf,
    sync::{
        atomic::Ordering,
        mpsc::{channel, Receiver, Sender},
        Arc,
    },
    thread,
    time::{Duration, Instant},
};

// ---- design tokens (same look as the web version) ------------------------------------------
/// Dark panel at 60 % opacity over the Windows Acrylic blur.
const BG: Color32 = Color32::from_rgba_premultiplied(6, 6, 6, 153);
const FG: Color32 = Color32::from_rgb(244, 244, 245);
const MUTED: Color32 = Color32::from_rgb(161, 161, 170);
const SUBTLE: Color32 = Color32::from_rgb(113, 113, 122);
const HOVER: Color32 = Color32::from_rgba_premultiplied(26, 26, 26, 26);
const CHIP: Color32 = HOVER;
const WARN: Color32 = Color32::from_rgb(251, 146, 60);

// ---- timing ---------------------------------------------------------------------------------
/// Wait this long after the last keystroke before searching.
const DEBOUNCE: Duration = Duration::from_millis(120);
/// ...and this long when the previous answer was "no results": the user is clearly still
/// typing, so the backend is left alone and the list does not flicker.
const DEBOUNCE_EMPTY: Duration = Duration::from_millis(400);
const CACHE_SIZE: usize = 80;
const ICON_CACHE: usize = 300;
const ICON_PX: i32 = 48;

type Key2 = (Cat, String);

struct SearchReq {
    id: u64,
    query: String,
    cat: Cat,
}
struct SearchRes {
    id: u64,
    query: String,
    key: Key2,
    hits: Vec<Hit>,
}

// ---- icons ----------------------------------------------------------------------------------
enum Slot {
    Loading,
    Ready(TextureHandle),
    Missing,
}

struct Icons {
    tx: Sender<(String, String)>,
    rx: Receiver<(String, Option<ColorImage>)>,
    map: HashMap<String, Slot>,
    order: VecDeque<String>,
}

impl Icons {
    fn new(ctx: &Context) -> Self {
        let (tx, jobs) = channel::<(String, String)>();
        let (done, rx) = channel();
        let ctx = ctx.clone();
        thread::spawn(move || {
            while let Ok((key, path)) = jobs.recv() {
                let img = sys::icon_rgba(&path, ICON_PX)
                    .map(|(w, h, px)| ColorImage::from_rgba_unmultiplied([w, h], &px));
                if done.send((key, img)).is_err() {
                    break;
                }
                ctx.request_repaint();
            }
        });
        Self {
            tx,
            rx,
            map: HashMap::new(),
            order: VecDeque::new(),
        }
    }

    fn poll(&mut self, ctx: &Context) {
        while let Ok((key, img)) = self.rx.try_recv() {
            let slot = match img {
                Some(i) => {
                    Slot::Ready(ctx.load_texture(format!("icon:{key}"), i, TextureOptions::LINEAR))
                }
                None => Slot::Missing,
            };
            if let Some(s) = self.map.get_mut(&key) {
                *s = slot;
            }
        }
    }

    /// Shell icons are fetched once per key: per file for exe/ico/lnk/..., per extension otherwise.
    fn key(h: &Hit) -> String {
        match h.kind {
            Kind::Folder => "folder".into(),
            Kind::File => {
                let ext = h.path.rsplit_once('.').map_or("", |x| x.1).to_lowercase();
                if matches!(ext.as_str(), "exe" | "ico" | "lnk" | "msi" | "scr") {
                    h.path.clone()
                } else {
                    format!("ext:{ext}")
                }
            }
            _ => h.path.clone(),
        }
    }

    fn get(&mut self, h: &Hit) -> Option<&TextureHandle> {
        let key = Self::key(h);
        if !self.map.contains_key(&key) {
            if self.order.len() >= ICON_CACHE {
                if let Some(old) = self.order.pop_front() {
                    self.map.remove(&old);
                }
            }
            self.order.push_back(key.clone());
            self.map.insert(key.clone(), Slot::Loading);
            let _ = self.tx.send((key.clone(), h.path.clone()));
        }
        match self.map.get(&key) {
            Some(Slot::Ready(t)) => Some(t),
            _ => None,
        }
    }
}

fn command_glyph(id: &str) -> &'static str {
    match id {
        "lock" => ph::LOCK,
        "sleep" => ph::MOON,
        "hibernate" | "shutdown" | "startup" => ph::POWER,
        "signout" => ph::SIGN_OUT,
        "restart" => ph::ARROW_CLOCKWISE,
        "trash" | "recyclebin" => ph::TRASH,
        "taskmgr" => ph::ACTIVITY,
        "desktop" | "display" => ph::MONITOR,
        "snip" => ph::SCISSORS,
        "flushdns" => ph::NETWORK,
        "reindex" | "update" => ph::ARROWS_CLOCKWISE,
        "thispc" => ph::LAPTOP,
        "downloads" => ph::DOWNLOAD_SIMPLE,
        "documents" => ph::FOLDER_OPEN,
        "settings" => ph::GEAR,
        "wifi" => ph::WIFI_HIGH,
        "bluetooth" => ph::BLUETOOTH,
        "sound" => ph::SPEAKER_HIGH,
        "apps" => ph::SQUARES_FOUR,
        "storage" => ph::HARD_DRIVE,
        "control" => ph::SLIDERS_HORIZONTAL,
        "eventvwr" => ph::SCROLL,
        "resmon" => ph::GAUGE,
        "sysinfo" => ph::INFO,
        "cmd" | "powershell" | "terminal" => ph::TERMINAL_WINDOW,
        _ => ph::POWER,
    }
}

// ---- setup ----------------------------------------------------------------------------------
pub fn setup(ctx: &Context) {
    // Segoe UI is the Windows UI font: native look and nothing to bundle.
    let mut fonts = FontDefinitions::default();
    let win_fonts =
        PathBuf::from(std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into()))
            .join("Fonts");
    let mut load = |name: &str, files: &[&str], first: bool| {
        for f in files {
            if let Ok(bytes) = std::fs::read(win_fonts.join(f)) {
                fonts
                    .font_data
                    .insert(name.to_owned(), Arc::new(FontData::from_owned(bytes)));
                let fam = fonts.families.entry(FontFamily::Proportional).or_default();
                if first {
                    fam.insert(0, name.to_owned());
                } else {
                    fam.push(name.to_owned());
                }
                break;
            }
        }
    };
    load("segoe", &["segoeui.ttf"], true);
    // Bengali / Hindi / Tamil ... file names
    load(
        "nirmala",
        &["Nirmala.ttf", "Nirmala.ttc", "nirmala.ttf"],
        false,
    );
    egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);
    ctx.set_fonts(fonts);

    let mut v = egui::Visuals::dark();
    v.panel_fill = Color32::TRANSPARENT;
    v.window_fill = Color32::TRANSPARENT;
    v.override_text_color = Some(FG);
    v.selection.bg_fill = Color32::from_rgba_premultiplied(60, 60, 60, 60);
    v.selection.stroke = egui::Stroke::new(1.0, FG);
    ctx.set_visuals(v);
    ctx.global_style_mut(|s| {
        s.animation_time = 0.0; // snappy: no fades
        s.spacing.item_spacing = Vec2::ZERO;
        s.interaction.selectable_labels = false;
    });
}

// ---- the app --------------------------------------------------------------------------------
pub struct FlintApp {
    index: Arc<Index>,
    _tray: Option<tray_icon::TrayIcon>,

    query: String,
    cat: Cat,
    sel: usize,
    confirming: Option<String>,
    cursor_at_end: bool,

    // search pipeline
    search_tx: Sender<SearchReq>,
    search_rx: Receiver<SearchRes>,
    req_id: u64,
    pending: Option<Instant>,
    hits: Vec<Hit>,
    /// the query `hits` belongs to (prevents a "No results" flash while typing)
    answered: String,
    last_key: Key2,
    last_empty: Option<Key2>,
    cache: HashMap<Key2, Vec<Hit>>,
    cache_order: VecDeque<Key2>,

    // derived
    calc: Option<String>,
    items: Vec<Hit>,

    icons: Icons,

    // window bookkeeping
    last_h: f32,
    show_gen: u32,
    index_gen: u32,
    shown_at: Instant,
    seen_focus: bool,
    want_focus: bool,
    /// eframe always shows the window after its first painted frame; Flint hides it again
    /// (it starts off-screen, so nothing is ever visible).
    boot_frames: u8,
}

impl FlintApp {
    pub fn new(ctx: &Context, index: Arc<Index>, tray: Option<tray_icon::TrayIcon>) -> Self {
        let (search_tx, req_rx) = channel::<SearchReq>();
        let (res_tx, search_rx) = channel::<SearchRes>();
        {
            let (index, ctx) = (index.clone(), ctx.clone());
            thread::spawn(move || {
                while let Ok(mut req) = req_rx.recv() {
                    while let Ok(newer) = req_rx.try_recv() {
                        req = newer; // only the latest keystroke matters
                    }
                    let hits = index.search(&req.query, req.cat);
                    let key = (req.cat, req.query.trim().to_string());
                    let _ = res_tx.send(SearchRes {
                        id: req.id,
                        query: req.query,
                        key,
                        hits,
                    });
                    ctx.request_repaint();
                }
            });
        }
        Self {
            index,
            _tray: tray,
            query: String::new(),
            cat: Cat::All,
            sel: 0,
            confirming: None,
            cursor_at_end: true,
            search_tx,
            search_rx,
            req_id: 0,
            pending: None,
            hits: vec![],
            answered: String::new(),
            last_key: (Cat::All, String::new()),
            last_empty: None,
            cache: HashMap::new(),
            cache_order: VecDeque::new(),
            calc: None,
            items: vec![],
            icons: Icons::new(ctx),
            last_h: win::INPUT_H,
            show_gen: win::SHOW_GEN.load(Ordering::SeqCst),
            index_gen: win::INDEX_GEN.load(Ordering::SeqCst),
            shown_at: Instant::now(),
            seen_focus: false,
            want_focus: true,
            boot_frames: 0,
        }
    }

    // ---- lifecycle ------------------------------------------------------------------------
    fn on_show(&mut self, ctx: &Context) {
        self.query.clear();
        self.cat = Cat::All;
        self.sel = 0;
        self.confirming = None;
        self.hits.clear();
        self.answered.clear();
        self.last_key = (Cat::All, String::new());
        self.last_empty = None;
        self.pending = None;
        self.req_id += 1;
        self.calc = None;
        self.rebuild_items();
        self.last_h = win::INPUT_H;
        self.seen_focus = false;
        self.shown_at = Instant::now();
        self.want_focus = true;
        ctx.send_viewport_cmd(ViewportCommand::InnerSize(Vec2::new(
            win::WIDTH,
            win::INPUT_H,
        )));
        ctx.send_viewport_cmd(ViewportCommand::Focus);
    }

    fn on_index_changed(&mut self) {
        self.cache.clear();
        self.cache_order.clear();
        self.last_empty = None;
        // search again so the visible list reflects the new index
        if !self.query.trim().is_empty() {
            self.last_key = (Cat::All, "\u{0}".into());
        }
    }

    fn handle_focus(&mut self, ctx: &Context) {
        let focused = ctx.input(|i| i.viewport().focused) == Some(true);
        if focused {
            self.seen_focus = true;
        } else if self.seen_focus || self.shown_at.elapsed() > Duration::from_millis(700) {
            win::hide(); // clicked elsewhere: behave like a launcher
        } else {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
    }

    // ---- search pipeline ------------------------------------------------------------------
    fn rebuild_items(&mut self) {
        let mut v = Vec::with_capacity(LIMIT);
        if let Some(c) = &self.calc {
            v.push(Hit {
                name: format!("= {c}"),
                path: "Press Enter to copy".into(),
                kind: Kind::Calc,
                confirm: false,
            });
        }
        v.extend(self.hits.iter().cloned());
        v.truncate(LIMIT);
        if self.sel >= v.len() {
            self.sel = 0;
        }
        self.items = v;
    }

    fn apply_hits(&mut self, hits: Vec<Hit>, answered: String, key: Key2) {
        let same = hits.len() == self.hits.len()
            && hits
                .iter()
                .zip(&self.hits)
                .all(|(a, b)| a.path == b.path && a.kind == b.kind);
        self.last_empty = hits.is_empty().then_some(key);
        self.answered = answered;
        if !same {
            self.hits = hits;
            self.sel = 0;
            self.confirming = None;
            self.rebuild_items();
        }
    }

    fn remember(&mut self, key: Key2, hits: &[Hit]) {
        if self.cache_order.len() >= CACHE_SIZE {
            if let Some(old) = self.cache_order.pop_front() {
                self.cache.remove(&old);
            }
        }
        if self.cache.insert(key.clone(), hits.to_vec()).is_none() {
            self.cache_order.push_back(key);
        }
    }

    /// If `prev` found nothing, then any text that only *extends* it cannot find anything either.
    fn provably_empty(&self, key: &Key2) -> bool {
        let Some((pc, pq)) = &self.last_empty else {
            return false;
        };
        *pc == key.0
            && pq.chars().count() >= 2
            && !pq.starts_with(':')
            && !key.1.starts_with(':')
            && !pq.split_whitespace().any(|t| t.starts_with('.')) // extension filters don't narrow
            && key.1.starts_with(pq.as_str())
    }

    fn detect_query_change(&mut self) {
        let q = self.query.trim().to_string();
        let key = (self.cat, q.clone());
        if key == self.last_key {
            // only whitespace changed: same results
            if self.pending.is_none() && !q.is_empty() {
                self.answered = self.query.clone();
            }
            return;
        }
        self.last_key = key.clone();
        self.sel = 0;
        self.confirming = None;
        self.calc = calc::eval(&self.query);
        self.req_id += 1;

        if q.is_empty() {
            self.hits.clear();
            self.answered.clear();
            self.pending = None;
            self.last_empty = None;
        } else if let Some(h) = self.cache.get(&key).cloned() {
            // cached: applied instantly (instant backspace)
            self.pending = None;
            self.apply_hits(h, self.query.clone(), key);
        } else if self.provably_empty(&key) {
            // no backend call, no flicker
            self.pending = None;
            self.apply_hits(vec![], self.query.clone(), key);
        } else {
            self.pending = Some(Instant::now());
        }
        self.rebuild_items();
    }

    fn run_debounce(&mut self, ctx: &Context) {
        let Some(t) = self.pending else { return };
        let wait = if self.hits.is_empty() && !self.answered.is_empty() {
            DEBOUNCE_EMPTY
        } else {
            DEBOUNCE
        };
        let el = t.elapsed();
        if el >= wait {
            self.pending = None;
            let _ = self.search_tx.send(SearchReq {
                id: self.req_id,
                query: self.query.clone(),
                cat: self.cat,
            });
        } else {
            ctx.request_repaint_after(wait - el);
        }
    }

    fn poll_search(&mut self) {
        while let Ok(r) = self.search_rx.try_recv() {
            if r.id != self.req_id {
                continue; // stale answer
            }
            self.remember(r.key.clone(), &r.hits);
            self.apply_hits(r.hits, r.query, r.key);
            self.calc = calc::eval(&self.query);
            self.rebuild_items();
        }
    }

    // ---- actions --------------------------------------------------------------------------
    fn ghost(&self) -> String {
        let (Some(cur), true) = (self.items.get(self.sel), !self.query.is_empty()) else {
            return String::new();
        };
        if cur.kind == Kind::Calc || self.query.starts_with(':') {
            return String::new();
        }
        let n = self.query.chars().count();
        if cur
            .name
            .to_lowercase()
            .starts_with(&self.query.to_lowercase())
            && cur.name.chars().count() > n
        {
            cur.name.chars().skip(n).collect()
        } else {
            String::new()
        }
    }

    fn activate(&mut self, ctx: &Context, i: usize, reveal: bool) {
        let Some(h) = self.items.get(i).cloned() else {
            return;
        };
        if h.kind == Kind::Calc {
            ctx.copy_text(self.calc.clone().unwrap_or_default());
            win::hide();
            return;
        }
        if h.path.trim().is_empty() {
            win::hide();
            return;
        }
        // restart / shut down / sign out need a second Enter
        if h.confirm && self.confirming.as_deref() != Some(h.path.as_str()) {
            self.confirming = Some(h.path);
            return;
        }
        win::hide();
        let index = self.index.clone();
        // never block the UI thread (ShellExecute can stall on network drives)
        thread::spawn(move || launch(&index, &h, reveal));
    }

    fn handle_keys(&mut self, ctx: &Context) {
        let ghost_ok = self.cursor_at_end && !self.ghost().is_empty();
        let n = self.items.len();
        let (down, up, tab, back_tab, s_enter, enter, esc, right, reindex) = ctx.input_mut(|i| {
            (
                i.consume_key(Modifiers::NONE, Key::ArrowDown),
                i.consume_key(Modifiers::NONE, Key::ArrowUp),
                i.consume_key(Modifiers::NONE, Key::Tab),
                i.consume_key(Modifiers::SHIFT, Key::Tab),
                i.consume_key(Modifiers::SHIFT, Key::Enter),
                i.consume_key(Modifiers::NONE, Key::Enter),
                i.consume_key(Modifiers::NONE, Key::Escape),
                ghost_ok && i.consume_key(Modifiers::NONE, Key::ArrowRight),
                i.consume_key(Modifiers::COMMAND, Key::R),
            )
        });
        if down && n > 0 {
            self.sel = (self.sel + 1) % n;
            self.confirming = None;
        }
        if up && n > 0 {
            self.sel = (self.sel + n - 1) % n;
            self.confirming = None;
        }
        if tab || back_tab {
            self.cat = self.cat.cycle(if back_tab { -1 } else { 1 });
        }
        if s_enter {
            self.activate(ctx, self.sel, true);
        } else if enter {
            self.activate(ctx, self.sel, false);
        }
        if esc {
            win::hide();
        }
        if right {
            let g = self.ghost();
            self.query.push_str(&g);
            self.set_cursor_end(ctx);
        }
        if reindex {
            self.cache.clear();
            self.cache_order.clear();
            let i = self.index.clone();
            thread::spawn(move || i.rescan());
        }
    }

    fn set_cursor_end(&self, ctx: &Context) {
        let id = egui::Id::new("flint-input");
        let mut st = TextEdit::load_state(ctx, id).unwrap_or_default();
        st.cursor
            .set_char_range(Some(CCursorRange::one(CCursor::new(
                self.query.chars().count(),
            ))));
        st.store(ctx, id);
    }

    fn effective_cat(&self) -> Cat {
        let q = self.query.to_lowercase();
        for (p, c) in [
            (":app", Cat::Apps),
            (":file", Cat::Files),
            (":folder", Cat::Folders),
        ] {
            if let Some(rest) = q.strip_prefix(p) {
                if rest.is_empty() || rest.starts_with(char::is_whitespace) {
                    return c;
                }
            }
        }
        self.cat
    }
}

fn launch(index: &Arc<Index>, h: &Hit, reveal: bool) {
    match h.kind {
        Kind::Command => {
            let i = index.clone();
            sys::run_command(&h.path, move || i.rescan());
        }
        Kind::App if !reveal => {
            if !sys::activate_running(&index.app_target(&h.path)) {
                sys::open(&h.path);
            }
        }
        _ if reveal && !h.path.starts_with("shell:") => sys::reveal(&h.path),
        _ => sys::open(&h.path),
    }
}

fn subtitle(h: &Hit) -> String {
    match h.kind {
        Kind::File | Kind::Folder => h
            .path
            .rfind('\\')
            .map_or(h.path.as_str(), |i| &h.path[..i])
            .to_string(),
        Kind::Command => "System command".into(),
        Kind::Calc => h.path.clone(),
        Kind::App => String::new(),
    }
}

fn fit(ui: &egui::Ui, text: &str, size: f32, color: Color32, max_w: f32) -> Arc<egui::Galley> {
    let mut job = LayoutJob::simple_singleline(text.to_owned(), FontId::proportional(size), color);
    job.wrap.max_width = max_w.max(1.0);
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    ui.painter().layout_job(job)
}

impl eframe::App for FlintApp {
    fn clear_color(&self, _v: &egui::Visuals) -> [f32; 4] {
        [0.0; 4] // fully transparent: the Acrylic blur shows through the 60 % panel
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.run(&ctx, ui);
    }
}

impl FlintApp {
    fn run(&mut self, ctx: &Context, root: &mut egui::Ui) {
        if self.boot_frames < 2 && win::SHOW_GEN.load(Ordering::SeqCst) == self.show_gen {
            self.boot_frames += 1;
            if self.boot_frames == 1 {
                ctx.request_repaint(); // frame 2 hides the window
            } else {
                sys::hide_window(win::HWND.load(Ordering::SeqCst));
                win::VISIBLE.store(false, Ordering::SeqCst);
            }
            return;
        }
        self.boot_frames = 2;
        let g = win::SHOW_GEN.load(Ordering::SeqCst);
        if g != self.show_gen {
            self.show_gen = g;
            self.on_show(ctx);
        }
        let ig = win::INDEX_GEN.load(Ordering::SeqCst);
        if ig != self.index_gen {
            self.index_gen = ig;
            self.on_index_changed();
        }
        self.poll_search();
        self.icons.poll(ctx);
        self.handle_focus(ctx);
        self.handle_keys(ctx);
        self.detect_query_change();
        self.run_debounce(ctx);

        let ghost = self.ghost();
        let rows = self.items.len();
        let show_empty = !self.query.trim().is_empty()
            && rows == 0
            && self.answered == self.query
            && self.pending.is_none();
        let list_rows = if show_empty { 1 } else { rows };
        let want_h = win::INPUT_H
            + if list_rows > 0 {
                win::LIST_PAD * 2.0 + list_rows as f32 * win::ROW_H
            } else {
                0.0
            };
        // native window height always matches the panel (a command only when it changes)
        if (want_h - self.last_h).abs() > 0.5 {
            self.last_h = want_h;
            win::resize(want_h);
            ctx.request_repaint();
        }

        let effective = self.effective_cat();
        let mut picked: Option<usize> = None;
        let mut hovered: Option<usize> = None;
        let pointer_moved = ctx.input(|i| i.pointer.delta() != Vec2::ZERO);

        egui::CentralPanel::default()
            .frame(Frame::NONE.fill(BG).corner_radius(CornerRadius::same(8)))
            .show(root, |ui| {
                let full = ui.max_rect();
                let w = full.width();

                // ---- input row -----------------------------------------------------------
                let row = Rect::from_min_size(full.min, Vec2::new(w, win::INPUT_H));
                let mut right_x = row.right() - 16.0;
                let icon_c = Pos2::new(right_x - 9.0, row.center().y);
                ui.painter().text(
                    icon_c,
                    Align2::CENTER_CENTER,
                    ph::MAGNIFYING_GLASS,
                    FontId::proportional(18.0),
                    MUTED,
                );
                right_x -= 18.0 + 12.0;
                if effective != Cat::All {
                    let g = ui.painter().layout_no_wrap(
                        effective.label().to_owned(),
                        FontId::proportional(11.0),
                        MUTED,
                    );
                    let cw = g.size().x + 12.0;
                    let r = Rect::from_min_size(
                        Pos2::new(right_x - cw, row.center().y - 10.0),
                        Vec2::new(cw, 20.0),
                    );
                    ui.painter().rect_filled(r, 4.0, CHIP);
                    ui.painter().galley(
                        Pos2::new(r.left() + 6.0, r.center().y - g.size().y / 2.0),
                        g,
                        MUTED,
                    );
                    right_x -= cw + 12.0;
                }
                let edit_rect = Rect::from_min_max(
                    Pos2::new(row.left() + 16.0, row.top()),
                    Pos2::new(right_x, row.bottom()),
                );
                let mut child = ui.new_child(
                    UiBuilder::new()
                        .max_rect(edit_rect)
                        .layout(egui::Layout::left_to_right(Align::Center)),
                );
                let font = FontId::proportional(14.0);
                let out = TextEdit::singleline(&mut self.query)
                    .id(egui::Id::new("flint-input"))
                    .font(font.clone())
                    .frame(Frame::NONE)
                    .margin(Margin::ZERO)
                    .desired_width(edit_rect.width())
                    .char_limit(MAX_QUERY)
                    .text_color(FG)
                    .hint_text(
                        egui::RichText::new("Search apps, files and commands…")
                            .color(SUBTLE)
                            .font(font.clone()),
                    )
                    .lock_focus(true)
                    .show(&mut child);
                if self.want_focus || !out.response.has_focus() {
                    out.response.request_focus();
                    self.want_focus = false;
                }
                self.cursor_at_end = out.cursor_range.map_or(true, |r| {
                    r.primary.index.0 >= self.query.chars().count() && r.secondary == r.primary
                });
                if !ghost.is_empty() && out.galley.size().x < edit_rect.width() - 40.0 {
                    ui.painter().text(
                        Pos2::new(out.galley_pos.x + out.galley.size().x, out.galley_pos.y),
                        Align2::LEFT_TOP,
                        &ghost,
                        font,
                        SUBTLE,
                    );
                }

                // ---- results -------------------------------------------------------------
                if list_rows == 0 {
                    return;
                }
                let mut y = row.bottom() + win::LIST_PAD;
                for i in 0..rows {
                    let h = self.items[i].clone();
                    let r = Rect::from_min_size(
                        Pos2::new(full.left() + win::LIST_PAD, y),
                        Vec2::new(w - win::LIST_PAD * 2.0, win::ROW_H),
                    );
                    y += win::ROW_H;
                    let resp = ui.interact(r, egui::Id::new(("row", i)), Sense::click());
                    if resp.hovered() && pointer_moved {
                        hovered = Some(i);
                    }
                    if resp.clicked() {
                        picked = Some(i);
                    }
                    if i == self.sel {
                        ui.painter().rect_filled(r, 6.0, HOVER);
                    }
                    // icon
                    let ir = Rect::from_center_size(
                        Pos2::new(r.left() + 12.0 + 12.0, r.center().y),
                        Vec2::splat(24.0),
                    );
                    match h.kind {
                        Kind::Command | Kind::Calc => {
                            let glyph = if h.kind == Kind::Calc {
                                ph::CALCULATOR
                            } else {
                                command_glyph(&h.path)
                            };
                            ui.painter().text(
                                ir.center(),
                                Align2::CENTER_CENTER,
                                glyph,
                                FontId::proportional(20.0),
                                MUTED,
                            );
                        }
                        _ => {
                            if let Some(t) = self.icons.get(&h) {
                                ui.painter().image(
                                    t.id(),
                                    ir,
                                    Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                                    Color32::WHITE,
                                );
                            } else {
                                let glyph = match h.kind {
                                    Kind::App => ph::APP_WINDOW,
                                    Kind::Folder => ph::FOLDER,
                                    _ => ph::FILE,
                                };
                                ui.painter().text(
                                    ir.center(),
                                    Align2::CENTER_CENTER,
                                    glyph,
                                    FontId::proportional(20.0),
                                    MUTED,
                                );
                            }
                        }
                    }
                    // name + subtitle
                    let tx = ir.right() + 12.0;
                    let avail = r.right() - 12.0 - tx;
                    let name = fit(ui, &h.name, 14.0, FG, avail * 0.55);
                    let nw = name.size().x;
                    ui.painter().galley(
                        Pos2::new(tx, r.center().y - name.size().y / 2.0),
                        name,
                        FG,
                    );
                    let (sub, col) = if self.confirming.as_deref() == Some(h.path.as_str()) {
                        ("Press Enter again to confirm".to_string(), WARN)
                    } else {
                        (subtitle(&h), SUBTLE)
                    };
                    if !sub.is_empty() {
                        let sx = tx + nw + 12.0;
                        let g = fit(ui, &sub, 12.0, col, r.right() - 12.0 - sx);
                        ui.painter()
                            .galley(Pos2::new(sx, r.center().y - g.size().y / 2.0), g, col);
                    }
                }
                if show_empty {
                    let r = Rect::from_min_size(
                        Pos2::new(full.left() + win::LIST_PAD, y),
                        Vec2::new(w - win::LIST_PAD * 2.0, win::ROW_H),
                    );
                    ui.painter().text(
                        Pos2::new(r.left() + 12.0, r.center().y),
                        Align2::LEFT_CENTER,
                        "No results",
                        FontId::proportional(13.0),
                        SUBTLE,
                    );
                }
            });

        if let Some(i) = hovered {
            if i != self.sel {
                self.sel = i;
                self.confirming = None;
                ctx.request_repaint();
            }
        }
        if let Some(i) = picked {
            self.activate(ctx, i, false);
        }
    }
}
