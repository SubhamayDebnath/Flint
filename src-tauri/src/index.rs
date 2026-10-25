//! In-memory offline index + ranked parallel search.
use crate::sys;
use parking_lot::RwLock;
use rayon::prelude::*;
use serde::Serialize;
use std::{
    collections::HashSet,
    fs::File,
    io::{BufRead, BufReader, BufWriter, Write},
    os::windows::fs::MetadataExt,
    panic::{catch_unwind, AssertUnwindSafe},
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicU32, Ordering},
        Arc, OnceLock,
    },
    thread,
    time::Duration,
};
use walkdir::{DirEntry, WalkDir};

/// Hits returned per query.
const LIMIT: usize = 10;
/// With a warm cache, wait before the first rescan so login stays snappy.
const BOOT_SCAN_DELAY: Duration = Duration::from_secs(10);
/// Full all-drive rescan interval (background priority, so it stays light).
const RESCAN_EVERY: Duration = Duration::from_secs(30 * 60);
/// Deep enough for real folders, shallow enough to survive junction loops.
const MAX_DEPTH: usize = 32;
/// Longest query we ever search for.
const MAX_QUERY: usize = 256;
/// Below this many entries a sequential scan beats the thread-pool hand-off.
const PAR_MIN: usize = 20_000;

#[derive(Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    App,
    File,
    Folder,
    Command,
}

/// A file or folder. `lower` is the lower-cased file name used for matching.
pub struct Entry {
    path: Box<str>,
    lower: Box<str>,
    kind: Kind,
}

#[derive(Clone)]
pub struct App {
    name: String,
    lower: String,
    path: String,
    /// lower-cased exe path (empty for Store apps)
    target: String,
}

#[derive(Serialize)]
pub struct Hit {
    name: String,
    path: String,
    kind: Kind,
}

#[derive(Default)]
pub struct Index {
    apps: RwLock<Vec<App>>,
    files: RwLock<Vec<Entry>>,
    scanning: AtomicBool,
    /// Number of finished scans (the Store-app refresh runs only every few scans).
    scans: AtomicU32,
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

fn make(e: &DirEntry) -> Option<Entry> {
    let lower = e.file_name().to_string_lossy().to_lowercase();
    let kind = if e.file_type().is_dir() {
        Kind::Folder
    } else {
        if SKIP_EXT.contains(&lower.rsplit_once('.').map_or("", |x| x.1)) {
            return None;
        }
        Kind::File
    };
    Some(Entry {
        path: e.path().to_string_lossy().into(),
        lower: lower.into(),
        kind,
    })
}

/// Walk one drive; its top-level entries are scanned in parallel.
fn scan_root(root: &str) -> Vec<Entry> {
    let Ok(rd) = std::fs::read_dir(root) else {
        return vec![];
    };
    let tops: Vec<PathBuf> = rd.filter_map(Result::ok).map(|e| e.path()).collect();
    tops.par_iter()
        .flat_map_iter(|p| {
            WalkDir::new(p)
                .max_depth(MAX_DEPTH)
                .into_iter()
                .filter_entry(keep)
                .filter_map(Result::ok)
                .filter_map(|e| make(&e))
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
fn load_files_cache() -> Vec<Entry> {
    let Some(p) = cache_path("index.txt") else {
        return vec![];
    };
    let Ok(f) = File::open(p) else { return vec![] };
    BufReader::with_capacity(1 << 20, f)
        .lines()
        .map_while(Result::ok)
        .filter_map(|l| {
            let (k, path) = l.split_once('|')?;
            let kind = match k {
                "F" => Kind::File,
                "D" => Kind::Folder,
                _ => return None,
            };
            let name = path.rsplit('\\').next()?;
            Some(Entry {
                lower: name.to_lowercase().into(),
                path: path.into(),
                kind,
            })
        })
        .collect()
}

fn save_files_cache(v: &[Entry]) {
    write_cache("index.txt", |w| {
        for e in v {
            writeln!(
                w,
                "{}|{}",
                if e.kind == Kind::File { 'F' } else { 'D' },
                e.path
            )?;
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

/// Best `k` entries by score across all cores.
fn top_k<'a>(
    items: &'a [Entry],
    k: usize,
    f: impl Fn(&Entry) -> Option<i64> + Sync,
) -> Vec<(i64, &'a Entry)> {
    let push = |mut v: Vec<(i64, &'a Entry)>, e: &'a Entry| {
        if let Some(s) = f(e) {
            if v.len() < k || s < v.last().map_or(i64::MAX, |x| x.0) {
                let pos = v.partition_point(|x| x.0 <= s);
                v.insert(pos, (s, e));
                v.truncate(k);
            }
        }
        v
    };
    if items.len() < PAR_MIN {
        return items.iter().fold(Vec::new(), push);
    }
    items
        .par_iter()
        .fold(Vec::new, push)
        .reduce(Vec::new, |mut a, b| {
            a.extend(b);
            a.sort_unstable_by_key(|x| x.0);
            a.truncate(k);
            a
        })
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
        let (uwp, files) = thread::scope(|s| {
            let uwp = s.spawn(move || if refresh_uwp { scan_uwp() } else { Vec::new() });
            // scales with the machine: half the logical cores, never below 2 or above 6
            // 2-4 cores -> 2 | 6 cores -> 3 | 8 cores -> 4 | 12+ cores -> 6
            let threads = thread::available_parallelism().map_or(2, |n| (n.get() / 2).clamp(2, 6));
            let drives = sys::fixed_drives();
            let walk = |d: &Vec<String>| -> Vec<Entry> {
                d.par_iter().flat_map_iter(|d| scan_root(d)).collect()
            };
            let files: Vec<Entry> = match rayon::ThreadPoolBuilder::new()
                .num_threads(threads)
                .start_handler(|_| sys::lower_thread_priority())
                .build()
            {
                Ok(pool) => pool.install(|| walk(&drives)),
                Err(_) => walk(&drives), // fall back to the global pool
            };
            (uwp.join().unwrap_or_default(), files)
        });

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

        let mut files = files;
        files.shrink_to_fit(); // give the over-allocation back
        apps.shrink_to_fit();

        save_apps_cache(&apps);
        save_files_cache(&files);
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

    pub fn search(&self, raw: &str, category: &str) -> Vec<Hit> {
        let lowered: String = raw
            .trim()
            .chars()
            .take(MAX_QUERY)
            .collect::<String>()
            .to_lowercase();
        let mut q = lowered.as_str();
        let mut cat = category;
        // :app / :file / :folder prefixes override the category
        for (prefix, c) in [(":app", "apps"), (":file", "files"), (":folder", "folders")] {
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

        let all = cat == "all";
        let want_apps = ext.is_none() && (all || cat == "apps");
        let want_files = all || cat == "files";
        let want_folders = ext.is_none() && (all || cat == "folders");

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
                    },
                )
            }));
        }

        if want_files || want_folders {
            let files = self.files.read();
            let ext = ext.as_deref();
            let found = top_k(&files, LIMIT, |e| {
                match e.kind {
                    Kind::File if want_files => {}
                    Kind::Folder if want_folders => {}
                    _ => return None,
                }
                if let Some(x) = ext {
                    if !(e.lower.len() > x.len() && e.lower.ends_with(x)) {
                        return None;
                    }
                }
                score(&e.lower, &terms).map(|s| s + if e.kind == Kind::File { 300 } else { 0 })
            });
            c.extend(found.into_iter().map(|(s, e)| {
                let name = e.path.rsplit('\\').next().unwrap_or(&e.path).to_string();
                (
                    s,
                    Hit {
                        name,
                        path: e.path.to_string(),
                        kind: e.kind,
                    },
                )
            }));
        }

        c.sort_unstable_by_key(|x| x.0);
        c.into_iter().take(LIMIT).map(|x| x.1).collect()
    }
}
