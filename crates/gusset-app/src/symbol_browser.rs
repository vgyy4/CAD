//! The schematic symbol library browser (ADR 0057): the user names a folder of
//! `.kicad_sym` files (a preference; Gusset never downloads KiCad libraries), a worker
//! thread indexes them (name, description, keywords, units) with a cache keyed by file
//! size and time, the panel lists and filters the symbols, previews the picked one with
//! the schematic's symbol renderer and "Place" arms the Place tool, which sends
//! `sch.add_symbol`. Parsing never runs on the UI thread; file and symbol counts are
//! bounded.

use gusset_pcb::Limits;
use gusset_pcb::sch::LibSymbol;
use gusset_pcb::symlib::{self, LibraryInfo};
use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

/// Most library files read from the folder (name order).
pub const MAX_FILES: usize = 1000;
/// Largest library file read (bytes); larger ones are skipped and counted.
pub const MAX_FILE_BYTES: u64 = 32 << 20;
/// Most symbols the list shows at once.
pub const MAX_SHOWN: usize = 2000;

/// What the cache remembers of a file: its size and time at the read, and the index.
type Cached = (u64, Option<SystemTime>, Result<Arc<LibraryInfo>, String>);

/// The indexes kept between scans, by file path.
pub type Cache = HashMap<PathBuf, Cached>;

/// The `.kicad_sym` files of a folder (or the one file named), in name order, at most
/// [`MAX_FILES`].
pub fn library_files(folder: &Path) -> Vec<PathBuf> {
    let is_lib = |p: &Path| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("kicad_sym"));
    if folder.is_file() {
        return if is_lib(folder) { vec![folder.to_path_buf()] } else { Vec::new() };
    }
    let Ok(rd) = std::fs::read_dir(folder) else { return Vec::new() };
    let mut v: Vec<PathBuf> =
        rd.flatten().map(|e| e.path()).filter(|p| is_lib(p) && p.is_file()).take(100_000).collect();
    v.sort();
    v.truncate(MAX_FILES);
    v
}

/// Reads a library file's text, refusing files past [`MAX_FILE_BYTES`] and non-UTF-8 text.
pub fn read_library(path: &Path) -> Result<String, String> {
    let f = std::fs::File::open(path).map_err(|e| e.kind().to_string())?;
    if !f.metadata().map_err(|e| e.kind().to_string())?.is_file() {
        return Err("not a file".into());
    }
    let mut bytes = Vec::new();
    f.take(MAX_FILE_BYTES + 1).read_to_end(&mut bytes).map_err(|e| e.kind().to_string())?;
    if bytes.len() as u64 > MAX_FILE_BYTES {
        return Err("file too large".into());
    }
    String::from_utf8(bytes).map_err(|_| "not UTF-8 text".into())
}

/// A file's name for display: the last path part without control characters.
fn file_name(p: &Path) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().chars().filter(|c| !c.is_control()).take(255).collect())
        .unwrap_or_default()
}

/// The index of one file, from the cache when its size and time are unchanged.
pub fn index_file(path: &Path, cache: &Mutex<Cache>) -> Result<Arc<LibraryInfo>, String> {
    let meta = std::fs::metadata(path).map_err(|e| e.kind().to_string())?;
    let stamp = (meta.len(), meta.modified().ok());
    if let Ok(c) = cache.lock()
        && let Some((len, time, r)) = c.get(path)
        && (*len, *time) == stamp
    {
        return r.clone();
    }
    let r = read_library(path).and_then(|text| {
        symlib::index(&file_name(path), &text, &Limits::default()).map(Arc::new).map_err(|e| e.to_string())
    });
    if let Ok(mut c) = cache.lock() {
        // The cache is bounded like the folder.
        if c.len() >= 2 * MAX_FILES {
            c.clear();
        }
        c.insert(path.to_path_buf(), (stamp.0, stamp.1, r.clone()));
    }
    r
}

/// What the worker found so far.
#[derive(Debug, Default)]
struct Shared {
    /// Bumped by every new scan; a worker of an older scan stops.
    generation: u64,
    libs: Vec<Arc<LibraryInfo>>,
    /// (file name, why) of files that did not index.
    errors: Vec<(String, String)>,
    /// Files in the folder, and how many were handled.
    total: usize,
    handled: usize,
    done: bool,
}

/// The preview worker's answer for one symbol.
type PreviewSlot = Arc<Mutex<Option<Result<Arc<LibSymbol>, String>>>>;

/// A library symbol chosen in the list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pick {
    /// The library's file name.
    pub file: String,
    pub name: String,
}

/// What "Place" asks for: the Place tool's library file, symbol and unit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Placement {
    pub library: String,
    pub symbol: String,
    pub unit: u32,
}

/// The browser's state, kept across frames.
pub struct Browser {
    /// The folder of `.kicad_sym` files (a preference).
    pub folder: String,
    scanned: Option<String>,
    pub filter: String,
    pub picked: Option<Pick>,
    /// The unit shown in the preview and placed.
    pub unit: u32,
    shared: Arc<Mutex<Shared>>,
    cache: Arc<Mutex<Cache>>,
    /// Filter results: (library, symbol) indexes into the worker's libraries.
    results: Vec<(usize, usize)>,
    /// What the results were computed for: filter, libraries known, scan finished.
    results_key: Option<(String, usize, bool)>,
    libs: Vec<Arc<LibraryInfo>>,
    preview_key: Option<Pick>,
    preview_slot: Option<PreviewSlot>,
    preview: Option<Result<Arc<LibSymbol>, String>>,
}

impl std::fmt::Debug for Browser {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Browser").field("folder", &self.folder).field("picked", &self.picked).finish_non_exhaustive()
    }
}

impl Default for Browser {
    fn default() -> Self {
        Browser {
            folder: String::new(),
            scanned: None,
            filter: String::new(),
            picked: None,
            unit: 1,
            shared: Arc::new(Mutex::new(Shared::default())),
            cache: Arc::new(Mutex::new(Cache::new())),
            results: Vec::new(),
            results_key: None,
            libs: Vec::new(),
            preview_key: None,
            preview_slot: None,
            preview: None,
        }
    }
}

impl Browser {
    /// Starts a scan of the folder on a worker thread (the previous one stops). With an
    /// empty folder the lists are emptied.
    pub fn rescan(&mut self) {
        let folder = self.folder.trim().to_string();
        self.scanned = Some(self.folder.clone());
        self.results_key = None;
        self.libs.clear();
        let generation = {
            let Ok(mut s) = self.shared.lock() else { return };
            s.generation += 1;
            s.libs.clear();
            s.errors.clear();
            s.total = 0;
            s.handled = 0;
            s.done = folder.is_empty();
            s.generation
        };
        if folder.is_empty() {
            return;
        }
        let (shared, cache) = (Arc::clone(&self.shared), Arc::clone(&self.cache));
        let spawned = std::thread::Builder::new().name("symbol-index".into()).spawn({
            let shared = Arc::clone(&shared);
            move || scan(&PathBuf::from(folder), generation, &shared, &cache)
        });
        if spawned.is_err()
            && let Ok(mut s) = shared.lock()
        {
            s.errors.push((String::new(), "no thread".into()));
            s.done = true;
        }
    }

    /// Call every frame: starts a scan when the folder changed, takes the worker's
    /// progress, refilters when the filter or the libraries changed, and starts or takes
    /// the preview of the picked symbol. Returns whether work is still running (the
    /// caller asks for repaints then).
    pub fn poll(&mut self) -> bool {
        if self.scanned.as_deref() != Some(self.folder.as_str()) {
            self.rescan();
        }
        let done = {
            let Ok(s) = self.shared.lock() else { return false };
            if s.libs.len() != self.libs.len() {
                self.libs.clone_from(&s.libs);
            }
            s.done
        };
        let key = (self.filter.clone(), self.libs.len(), done);
        if self.results_key.as_ref() != Some(&key) {
            self.results = symlib::search(&self.libs, &self.filter, MAX_SHOWN);
            self.results_key = Some(key);
        }
        self.poll_preview();
        !done || self.preview_slot.is_some()
    }

    fn poll_preview(&mut self) {
        // A finished preview.
        let finished = self.preview_slot.as_ref().and_then(|slot| slot.lock().ok().and_then(|mut g| g.take()));
        if let Some(r) = finished {
            self.preview = Some(r);
            self.preview_slot = None;
        }
        if self.preview_key == self.picked {
            return;
        }
        self.preview_key.clone_from(&self.picked);
        self.preview = None;
        self.preview_slot = None;
        let Some(p) = self.picked.clone() else { return };
        let path = Path::new(self.folder.trim()).join(&p.file);
        let path = if Path::new(self.folder.trim()).is_file() { PathBuf::from(self.folder.trim()) } else { path };
        let slot: PreviewSlot = Arc::new(Mutex::new(None));
        let out = Arc::clone(&slot);
        let spawned = std::thread::Builder::new().name("symbol-preview".into()).spawn(move || {
            let r = read_library(&path).and_then(|t| {
                symlib::load_symbol(&t, &p.name, &Limits::default()).map(Arc::new).map_err(|e| e.to_string())
            });
            if let Ok(mut g) = out.lock() {
                *g = Some(r);
            }
        });
        if spawned.is_ok() {
            self.preview_slot = Some(slot);
        } else {
            self.preview = Some(Err("no thread".into()));
        }
    }

    /// The listed symbols for the current filter: (library, symbol info).
    pub fn rows(&self) -> impl Iterator<Item = (&LibraryInfo, &symlib::SymbolInfo)> {
        self.results.iter().filter_map(|&(l, s)| {
            let lib = self.libs.get(l)?;
            Some((lib.as_ref(), lib.symbols.get(s)?))
        })
    }

    pub fn row_count(&self) -> usize {
        self.results.len()
    }

    /// The libraries read so far.
    pub fn libraries(&self) -> usize {
        self.libs.len()
    }

    /// (files handled, files in the folder, scan finished).
    pub fn progress(&self) -> (usize, usize, bool) {
        self.shared.lock().map_or((0, 0, true), |s| (s.handled, s.total, s.done))
    }

    /// Files that did not index: (name, why).
    pub fn errors(&self) -> Vec<(String, String)> {
        self.shared.lock().map(|s| s.errors.clone()).unwrap_or_default()
    }

    /// Picks a symbol (the unit goes back to 1).
    pub fn pick(&mut self, file: &str, name: &str) {
        let p = Pick { file: file.to_string(), name: name.to_string() };
        if self.picked.as_ref() != Some(&p) {
            self.picked = Some(p);
            self.unit = 1;
        }
    }

    /// The picked symbol's preview: `None` while loading or nothing is picked.
    pub fn preview(&self) -> Option<&Result<Arc<LibSymbol>, String>> {
        self.preview.as_ref()
    }

    /// The units of the picked symbol.
    pub fn units(&self) -> u32 {
        match &self.preview {
            Some(Ok(l)) => l.unit_count(),
            _ => 1,
        }
    }

    /// What Place sends: the library file path in the folder, the symbol and the unit.
    pub fn placement(&self) -> Option<Placement> {
        let p = self.picked.as_ref()?;
        let folder = self.folder.trim();
        let library = if Path::new(folder).is_file() {
            folder.to_string()
        } else {
            Path::new(folder).join(&p.file).to_string_lossy().into_owned()
        };
        Some(Placement { library, symbol: p.name.clone(), unit: self.unit.clamp(1, self.units()) })
    }
}

/// The worker: indexes every library file of the folder, publishing each as it is done.
fn scan(folder: &Path, generation: u64, shared: &Mutex<Shared>, cache: &Mutex<Cache>) {
    let files = library_files(folder);
    let current = |s: &Shared| s.generation == generation;
    {
        let Ok(mut s) = shared.lock() else { return };
        if !current(&s) {
            return;
        }
        s.total = files.len();
    }
    for f in files {
        let r = index_file(&f, cache);
        let Ok(mut s) = shared.lock() else { return };
        if !current(&s) {
            return;
        }
        s.handled += 1;
        match r {
            Ok(l) => s.libs.push(l),
            Err(why) => s.errors.push((file_name(&f), why)),
        }
    }
    if let Ok(mut s) = shared.lock()
        && current(&s)
    {
        s.done = true;
    }
}
