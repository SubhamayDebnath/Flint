//! In-memory offline index + ranked parallel search.
//!
//! Files live in two big string arenas (paths and lower-cased names) plus a 16-byte record per
//! entry. That removes two heap allocations per file compared with `Box<str>` pairs, which is
//! roughly 40-60 MB less RAM and much better cache behaviour for a million-file index.
use crate::sys;
use parking_lot::RwLock;
use rayon::prelude::*;
use std::{
    collections::{hash_map::DefaultHasher, HashSet},
    fs::File,
    hash::Hasher,
    io::{BufRead, BufReader, BufWriter, Write},
    os::windows::fs::MetadataExt,
    panic::{catch_unwind, AssertUnwindSafe},
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
        Arc, OnceLock,
    },
    thread,
    time::Duration,
};
use walkdir::{DirEntry, WalkDir};

/// Hits returned per query.
pub const LIMIT: usize = 10;
/// With a warm cache, wait before the first rescan so login stays snappy.
const BOOT_SCAN_DELAY: Duration = Duration::from_secs(10);
/// Full all-drive rescan interval (background priority, so it stays light).
const RESCAN_EVERY: Duration = Duration::from_secs(30 * 60);
/// Deep enough for real folders, shallow enough to survive junction loops.
const MAX_DEPTH: usize = 32;
/// Longest query we ever search for.
pub const MAX_QUERY: usize = 256;
/// Below this many entries a sequential scan beats the thread-pool hand-off.
const PAR_MIN: usize = 20_000;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    App,
    File,
    Folder,
    Command,
    Calc,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub enum Cat {
    All,
    Apps,
    Files,
    Folders,
}

impl Cat {
    pub const ORDER: [Cat; 4] = [Cat::All, Cat::Apps, Cat::Files, Cat::Folders];
    pub fn label(self) -> &'static str {
        match self {
            Cat::All => "All",
            Cat::Apps => "Apps",
            Cat::Files => "Files",
            Cat::Folders => "Folders",
        }
    }
    pub fn cycle(self, delta: i32) -> Cat {
        let i = Cat::ORDER.iter().position(|&c| c == self).unwrap_or(0) as i32;
        Cat::ORDER[(i + delta).rem_euclid(4) as usize]
    }
}

#[derive(Clone)]
pub struct Hit {
    pub name: String,
    pub path: String,
    pub kind: Kind,
    /// Needs a second Enter before it runs (restart, shut down, ...).
    pub confirm: bool,
}

// --------------------------------------------------------------------------------------------
// file arena
// --------------------------------------------------------------------------------------------

#[derive(Clone, Copy)]
struct Meta {
    p_off: u32,
    l_off: u32,
    p_len: u16,
    l_len: u16,
    /// offset of the file name inside the path
    name_off: u16,
    folder: bool,
}

#[derive(Default)]
pub struct Files {
    paths: String,
    lowers: String,
    meta: Vec<Meta>,
}

impl Files {
    fn push(&mut self, path: &str, lower: &str, folder: bool) {
        if path.len() > u16::MAX as usize
            || lower.len() > u16::MAX as usize
            || self.paths.len() + path.len() >= u32::MAX as usize - 1
            || self.lowers.len() + lower.len() >= u32::MAX as usize - 1
        {
            return;
        }
        let name_off = path.rfind('\\').map_or(0, |i| i + 1) as u16;
        self.meta.push(Meta {
            p_off: self.paths.len() as u32,
            l_off: self.lowers.len() as u32,
            p_len: path.len() as u16,
            l_len: lower.len() as u16,
            name_off,
            folder,
        });
        self.paths.push_str(path);
        self.lowers.push_str(lower);
    }

    fn merge(chunks: Vec<Files>) -> Files {
        let n: usize = chunks.iter().map(|c| c.meta.len()).sum();
        let pb: usize = chunks.iter().map(|c| c.paths.len()).sum();
        let lb: usize = chunks.iter().map(|c| c.lowers.len()).sum();
        let mut out = Files {
            paths: String::with_capacity(pb),
            lowers: String::with_capacity(lb),
            meta: Vec::with_capacity(n),
        };
        for c in chunks {
            if out.paths.len() + c.paths.len() >= u32::MAX as usize - 1
                || out.lowers.len() + c.lowers.len() >= u32::MAX as usize - 1
            {
                break;
            }
            let (po, lo) = (out.paths.len() as u32, out.lowers.len() as u32);
            out.paths.push_str(&c.paths);
            out.lowers.push_str(&c.lowers);
            out.meta.extend(c.meta.iter().map(|m| Meta {
                p_off: m.p_off + po,
                l_off: m.l_off + lo,
                ..*m
            }));
        }
        out
    }

    fn len(&self) -> usize {
        self.meta.len()
    }

    fn is_empty(&self) -> bool {
        self.meta.is_empty()
    }

    fn path(&self, m: &Meta) -> &str {
        let s = m.p_off as usize;
        self.paths.get(s..s + m.p_len as usize).unwrap_or("")
    }

    fn lower(&self, m: &Meta) -> &str {
        let s = m.l_off as usize;
        self.lowers.get(s..s + m.l_len as usize).unwrap_or("")
    }

    fn name(&self, m: &Meta) -> &str {
        self.path(m).get(m.name_off as usize..).unwrap_or("")
    }

    /// Hash of the content: lets us skip rewriting an unchanged cache file.
    fn fingerprint(&self) -> u64 {
        let mut h = DefaultHasher::new();
        h.write(self.paths.as_bytes());
        h.write_usize(self.meta.len());
        h.finish()
    }

    /// Best `k` entries by score across all cores (lower is better).
    fn top_k(&self, k: usize, f: impl Fn(&Meta) -> Option<i64> + Sync) -> Vec<(i64, usize)> {
        let push = |mut v: Vec<(i64, usize)>, i: usize| {
            if let Some(s) = f(&self.meta[i]) {
                if v.len() < k || s < v.last().map_or(i64::MAX, |x| x.0) {
                    let pos = v.partition_point(|x| x.0 <= s);
                    v.insert(pos, (s, i));
                    v.truncate(k);
                }
            }
            v
        };
        let n = self.len();
        if n < PAR_MIN {
            return (0..n).fold(Vec::new(), push);
        }
        (0..n)
            .into_par_iter()
            .fold(Vec::new, push)
            .reduce(Vec::new, |mut a, b| {
                a.extend(b);
                a.sort_unstable_by_key(|x| x.0);
                a.truncate(k);
                a
            })
    }
}

// --------------------------------------------------------------------------------------------
// apps
// --------------------------------------------------------------------------------------------

#[derive(Clone)]
pub struct App {
    name: String,
    lower: String,
    path: String,
    /// lower-cased exe path (empty for Store apps)
    target: String,
}

#[derive(Default)]
pub struct Index {
    apps: RwLock<Vec<App>>,
    files: RwLock<Files>,
    scanning: AtomicBool,
    /// Number of finished scans (the Store-app refresh runs only every few scans).
    scans: AtomicU32,
    /// Fingerprint of the file cache on disk.
    saved: AtomicU64,
    /// Called after every finished scan (the UI drops its result cache).
    on_scan: OnceLock<Box<dyn Fn() + Send + Sync>>,
}

struct ScanGuard<'a>(&'a AtomicBool);
impl Drop for ScanGuard<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

// "Only user & app folders": skip system / driver / build noise and hidden+system attributes.
const SKIP_DIRS: &[&str] = &[
    "windows",
    "program files",
    "program files (x86)",
    "programdata",
    "appdata",
    "$recycle.bin",
    "system volume information",
    "node_modules",
    "recovery",
    "perflogs",
    "msocache",
    "windows.old",
    "documents and settings",
    "config.msi",
    "intel",
    "amd",
    "nvidia",
];
const SKIP_EXT: &[&str] = &[
    "dll",
    "sys",
    "ini",
    "dat",
    "tmp",
    "log",
    "lnk",
    "drv",
    "mui",
    "cab",
    "bak",
    "db",
    "manifest",
    "cat",
    "inf",
    "etl",
    "pf",
    "blf",
    "regtrans-ms",
    "lock",
    "cache",
];

fn keep(e: &DirEntry) -> bool {
    let n = e.file_name().to_string_lossy();
    if n.starts_with(['.', '$', '~']) {
        return false;
    }
    let Ok(m) = e.metadata() else { return false };
    if m.file_attributes() & 0x6 != 0 {
        return false; // hidden | system
    }
    !(m.is_dir() && SKIP_DIRS.contains(&n.to_lowercase().as_str()))
}

fn add_entry(out: &mut Files, e: &DirEntry) {
    let lower = e.file_name().to_string_lossy().to_lowercase();
    let folder = e.file_type().is_dir();
    if !folder && SKIP_EXT.contains(&lower.rsplit_once('.').map_or("", |x| x.1)) {
        return;
    }
    out.push(&e.path().to_string_lossy(), &lower, folder);
}

/// Walk one drive; its top-level entries are scanned in parallel, one arena chunk each.
fn scan_root(root: &str) -> Vec<Files> {
    let Ok(rd) = std::fs::read_dir(root) else {
        return vec![];
    };
    let tops: Vec<PathBuf> = rd.filter_map(Result::ok).map(|e| e.path()).collect();
    tops.par_iter()
        .map(|p| {
            let mut out = Files::default();
            for e in WalkDir::new(p)
                .max_depth(MAX_DEPTH)
                .into_iter()
                .filter_entry(keep)
                .filter_map(Result::ok)
            {
                add_entry(&mut out, &e);
            }
            out
        })
        .collect()
}

fn scan_lnk() -> Vec<App> {
    let mut seen = HashSet::new();
    let mut found: Vec<(String, String, String)> = vec![]; // (name, lower, path)
    let roots = ["ProgramData", "APPDATA"]
        .into_iter()
        .filter_map(|v| std::env::var(v).ok())
        .map(|b| PathBuf::from(b).join(r"Microsoft\Windows\Start Menu\Programs"));
    for root in roots {
        for e in WalkDir::new(root).into_iter().filter_map(Result::ok) {
            let p = e.path();
            if !p.extension().is_some_and(|x| x.eq_ignore_ascii_case("lnk")) {
                continue;
            }
            let Some(name) = p.file_stem().map(|s| s.to_string_lossy().into_owned()) else {
                continue;
            };
            let lower = name.to_lowercase();
            if lower.contains("uninstall") || !seen.insert(lower.clone()) {
                continue;
            }
            found.push((name, lower, p.to_string_lossy().into_owned()));
        }
    }
    // resolving shortcuts is COM work: do it in parallel
    found
        .into_par_iter()
        .map(|(name, lower, path)| {
            let target = sys::resolve_lnk(&path).unwrap_or_default();
            App {
                name,
                lower,
                path,
                target,
            }
        })
        .collect()
}

fn scan_uwp() -> Vec<App> {
    sys::uwp_apps()
        .into_iter()
        .map(|(name, path)| App {
            lower: name.to_lowercase(),
            name,
            path,
            target: String::new(),
        })
        .collect()
}

// --------------------------------------------------------------------------------------------
// disk cache
// --------------------------------------------------------------------------------------------

fn cache_path(name: &str) -> Option<PathBuf> {
    std::env::var("LOCALAPPDATA")
        .ok()
        .map(|b| PathBuf::from(b).join("Flint").join(name))
}

/// Write a cache file atomically (temp file + rename).
fn write_cache(name: &str, body: impl FnOnce(&mut BufWriter<File>) -> std::io::Result<()>) {
    let Some(p) = cache_path(name) else { return };
    if let Some(d) = p.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let tmp = p.with_extension("new");
    let Ok(f) = File::create(&tmp) else { return };
    let mut w = BufWriter::with_capacity(1 << 20, f);
    if body(&mut w).and_then(|_| w.flush()).is_ok() {
        drop(w);
        let _ = std::fs::rename(&tmp, &p);
    }
}

/// Instant start: last session's file list (refreshed in the background).
fn load_files_cache() -> Files {
    let mut out = Files::default();
    let Some(p) = cache_path("index.txt") else {
        return out;
    };
    let Ok(f) = File::open(p) else { return out };
    for l in BufReader::with_capacity(1 << 20, f)
        .lines()
        .map_while(Result::ok)
    {
        let Some((k, path)) = l.split_once('|') else {
            continue;
        };
        let folder = match k {
            "F" => false,
            "D" => true,
            _ => continue,
        };
        let name = path.rsplit('\\').next().unwrap_or(path);
        out.push(path, &name.to_lowercase(), folder);
    }
    out
}

fn save_files_cache(v: &Files) {
    write_cache("index.txt", |w| {
        for m in &v.meta {
            writeln!(w, "{}|{}", if m.folder { 'D' } else { 'F' }, v.path(m))?;
        }
        Ok(())
    });
}

/// Apps are cached too, so the first search after boot never waits for PowerShell.
fn load_apps_cache() -> Vec<App> {
    let Some(p) = cache_path("apps.txt") else {
        return vec![];
    };
    let Ok(text) = std::fs::read_to_string(p) else {
        return vec![];
    };
    text.lines()
        .filter_map(|l| {
            let mut f = l.splitn(3, '\t');
            let (name, path, target) = (f.next()?, f.next()?, f.next()?);
            Some(App {
                lower: name.to_lowercase(),
                name: name.into(),
                path: path.into(),
                target: target.into(),
            })
        })
        .collect()
}

fn save_apps_cache(v: &[App]) {
    write_cache("apps.txt", |w| {
        for a in v {
            writeln!(w, "{}\t{}\t{}", a.name, a.path, a.target)?;
        }
        Ok(())
    });
}

// --------------------------------------------------------------------------------------------
// scoring
// --------------------------------------------------------------------------------------------

/// Lower is better. Every term must occur in `lower`.
fn score(lower: &str, terms: &[String]) -> Option<i64> {
    let b = lower.as_bytes();
    let mut s = lower.len() as i64;
    for t in terms {
        let p = lower.find(t.as_str())?;
        let class = match p {
            0 => 0,
            _ if matches!(b[p - 1], b' ' | b'-' | b'_' | b'.' | b'(' | b'[') => 1,
            _ => 2,
        };
        s += class * 1000 + p as i64;
    }
    Some(s)
}

impl Index {
    /// Register the callback fired after each scan (call once, before `start`).
    pub fn on_scan(&self, f: impl Fn() + Send + Sync + 'static) {
        let _ = self.on_scan.set(Box::new(f));
    }

    pub fn start(self: &Arc<Self>) {
        let me = self.clone();
        thread::spawn(move || {
            // read the caches at normal priority: this is what makes the first search instant
            let apps = load_apps_cache();
            let files = load_files_cache();
            let warm = !apps.is_empty() || !files.is_empty();
            if !apps.is_empty() {
                *me.apps.write() = apps;
            }
            if !files.is_empty() {
                me.saved.store(files.fingerprint(), Ordering::Relaxed);
                *me.files.write() = files;
            }
            // everything after this point is background work
            sys::lower_thread_priority();
            if warm {
                thread::sleep(BOOT_SCAN_DELAY);
            }
            loop {
                // a panic in one scan must never stop future scans
                let _ = catch_unwind(AssertUnwindSafe(|| me.rescan()));
                thread::sleep(RESCAN_EVERY);
            }
        });
    }

    /// Rebuild the index: Start Menu apps first, then Store apps and files in parallel.
    pub fn rescan(&self) {
        if self.scanning.swap(true, Ordering::SeqCst) {
            return;
        }
        let _guard = ScanGuard(&self.scanning);

        let mut apps = scan_lnk();
        if self.apps.read().is_empty() {
            *self.apps.write() = apps.clone(); // cold start: make Start Menu apps searchable now
        }

        // PowerShell (Get-StartApps) is the costliest part: refresh Store apps every 2nd scan
        let n = self.scans.fetch_add(1, Ordering::SeqCst);
        let refresh_uwp = n % 2 == 0
            || !self
                .apps
                .read()
                .iter()
                .any(|a| a.path.starts_with("shell:"));
        let (uwp, chunks) = thread::scope(|s| {
            let uwp = s.spawn(move || if refresh_uwp { scan_uwp() } else { Vec::new() });
            // scales with the machine: half the logical cores, never below 2 or above 6
            let threads = thread::available_parallelism().map_or(2, |n| (n.get() / 2).clamp(2, 6));
            let drives = sys::fixed_drives();
            let walk = |d: &Vec<String>| -> Vec<Files> {
                d.par_iter().flat_map_iter(|d| scan_root(d)).collect()
            };
            let chunks = match rayon::ThreadPoolBuilder::new()
                .num_threads(threads)
                .start_handler(|_| sys::lower_thread_priority())
                .build()
            {
                Ok(pool) => pool.install(|| walk(&drives)),
                Err(_) => walk(&drives), // fall back to the global pool
            };
            (uwp.join().unwrap_or_default(), chunks)
        });
        let files = Files::merge(chunks);

        // PowerShell can fail: keep the Store apps we already know instead of dropping them
        let uwp = if uwp.is_empty() {
            self.apps
                .read()
                .iter()
                .filter(|a| a.path.starts_with("shell:"))
                .cloned()
                .collect()
        } else {
            uwp
        };
        let seen: HashSet<&str> = apps.iter().map(|a| a.lower.as_str()).collect();
        let extra: Vec<App> = uwp
            .into_iter()
            .filter(|a| !seen.contains(a.lower.as_str()))
            .collect();
        apps.extend(extra);
        apps.shrink_to_fit();

        save_apps_cache(&apps);
        // an unchanged file list is not written again (saves ~100 MB of disk I/O per scan)
        let fp = files.fingerprint();
        if self.saved.swap(fp, Ordering::Relaxed) != fp {
            save_files_cache(&files);
        }
        *self.apps.write() = apps;
        *self.files.write() = files;
        if let Some(f) = self.on_scan.get() {
            f();
        }
    }

    pub fn app_target(&self, path: &str) -> String {
        self.apps
            .read()
            .iter()
            .find(|a| a.path == path)
            .map(|a| a.target.clone())
            .unwrap_or_default()
    }

    pub fn search(&self, raw: &str, category: Cat) -> Vec<Hit> {
        let lowered: String = raw
            .trim()
            .chars()
            .take(MAX_QUERY)
            .collect::<String>()
            .to_lowercase();
        let mut q = lowered.as_str();
        let mut cat = category;
        // :app / :file / :folder prefixes override the category
        for (prefix, c) in [
            (":app", Cat::Apps),
            (":file", Cat::Files),
            (":folder", Cat::Folders),
        ] {
            if let Some(rest) = q.strip_prefix(prefix) {
                if rest.is_empty() || rest.starts_with(' ') {
                    cat = c;
                    q = rest.trim();
                    break;
                }
            }
        }
        if q.is_empty() {
            return vec![];
        }

        // `.ext` tokens filter by extension; everything else is a name term
        let mut ext: Option<String> = None;
        let mut terms: Vec<String> = vec![];
        for t in q.split_whitespace() {
            match t.strip_prefix('.') {
                Some(e) if !e.is_empty() && !e.contains('.') => ext = Some(t.to_string()),
                _ => terms.push(t.to_string()),
            }
        }

        let all = cat == Cat::All;
        let want_apps = ext.is_none() && (all || cat == Cat::Apps);
        let want_files = all || cat == Cat::Files;
        let want_folders = ext.is_none() && (all || cat == Cat::Folders);

        let mut c: Vec<(i64, Hit)> = Vec::with_capacity(2 * LIMIT);

        // quick commands (max 4 so they never crowd out results)
        if all && ext.is_none() && q.len() >= 2 {
            let mut m: Vec<(i64, &sys::Cmd)> = sys::COMMANDS
                .iter()
                .filter_map(|cmd| {
                    cmd.keys
                        .iter()
                        .filter(|k| k.starts_with(q))
                        .map(|&k| match () {
                            _ if k == q => -10_000,
                            _ if q.len() >= 3 => -3_500 + (k.len() - q.len()) as i64,
                            _ => -2_500,
                        })
                        .min()
                        .map(|s| (s, cmd))
                })
                .collect();
            m.sort_unstable_by_key(|x| x.0);
            c.extend(m.into_iter().take(4).map(|(s, cmd)| {
                (
                    s,
                    Hit {
                        name: cmd.name.into(),
                        path: cmd.id.into(),
                        kind: Kind::Command,
                        confirm: cmd.confirm,
                    },
                )
            }));
        }

        if want_apps {
            let apps = self.apps.read();
            let mut m: Vec<(i64, &App)> = apps
                .iter()
                .filter_map(|a| score(&a.lower, &terms).map(|s| (s - 3000, a)))
                .collect();
            m.sort_unstable_by_key(|x| x.0);
            c.extend(m.into_iter().take(LIMIT).map(|(s, a)| {
                (
                    s,
                    Hit {
                        name: a.name.clone(),
                        path: a.path.clone(),
                        kind: Kind::App,
                        confirm: false,
                    },
                )
            }));
        }

        if want_files || want_folders {
            let files = self.files.read();
            let ext = ext.as_deref();
            let found = files.top_k(LIMIT, |m| {
                if m.folder {
                    if !want_folders {
                        return None;
                    }
                } else if !want_files {
                    return None;
                }
                let lower = files.lower(m);
                if let Some(x) = ext {
                    if !(lower.len() > x.len() && lower.ends_with(x)) {
                        return None;
                    }
                }
                score(lower, &terms).map(|s| s + if m.folder { 0 } else { 300 })
            });
            c.extend(found.into_iter().map(|(s, i)| {
                let m = &files.meta[i];
                (
                    s,
                    Hit {
                        name: files.name(m).to_string(),
                        path: files.path(m).to_string(),
                        kind: if m.folder { Kind::Folder } else { Kind::File },
                        confirm: false,
                    },
                )
            }));
        }

        c.sort_unstable_by_key(|x| x.0);
        c.into_iter().take(LIMIT).map(|x| x.1).collect()
    }
}
