//! The out-of-process importer (ADR 0008 §4).
//!
//! It also checks kernel-native B-rep blobs from project files before the app parses
//! them in-process ([`validate_native_out_of_process`], [`trust`]; security review #3).
//!
//! The parent ([`import_out_of_process`]) starts `gusset-worker` from the install
//! directory with an empty environment, sends the file bytes over stdin, and reads
//! kernel-native B-rep bodies back over stdout. IGES is the exception: OCCT's IGES reader
//! only reads from a file name, so the parent writes the bytes to a fresh private
//! directory ([`PrivateDir`]), tells the worker the path on its command line (it must be
//! known before the sandbox is applied), and the sandbox allows read access to that one
//! file; the directory is deleted whatever the outcome. The worker sandboxes itself before
//! reading anything. A malicious file can only crash or time out the worker; the parent
//! turns that into an [`ImportError`].
//!
//! The response frame is located by its magic and verified by BLAKE3, so stray library
//! output on stdout (OCCT prints diagnostics there) cannot corrupt the protocol.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

pub const REQUEST_MAGIC: &[u8; 8] = b"GWRQ1\0\0\0";
pub const RESPONSE_MAGIC: &[u8; 8] = b"\0GWRSP1\0";
/// Largest input file accepted.
pub const MAX_INPUT: u64 = 1 << 30;
/// Hard cap on a worker response, whatever the input size (see [`output_cap`]).
pub const MAX_OUTPUT: u64 = 1 << 30;
pub const MAX_BODIES: u32 = 100_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Step = 1,
    Iges = 2,
    /// A kernel-native B-rep blob (OCCT `BinTools`) to validate and re-serialize.
    Brep = 3,
}

impl Format {
    pub fn from_u8(v: u8) -> Option<Format> {
        match v {
            1 => Some(Format::Step),
            2 => Some(Format::Iges),
            3 => Some(Format::Brep),
            _ => None,
        }
    }
    pub fn from_extension(ext: &str) -> Option<Format> {
        match ext.to_ascii_lowercase().as_str() {
            "step" | "stp" => Some(Format::Step),
            "iges" | "igs" => Some(Format::Iges),
            _ => None,
        }
    }
}

/// Command-line flag naming the one input file the worker may read (IGES).
pub const INPUT_FILE_FLAG: &str = "--input-file";

/// Why an IGES import cannot run when the sandbox could not confine the worker to its
/// input file. The worker never falls back to running unsandboxed.
pub const IGES_NEEDS_LANDLOCK: &str = "IGES import needs Linux Landlock (kernel 5.13+) to read the file inside the \
sandbox, and it is not available here; the file was not read. Convert the file to STEP and import that instead";

/// Worker side: the file an IGES import may read. `requested` is the path from the
/// command line, `granted` the file the sandbox actually confined the worker to
/// ([`gusset_sandbox::Report::read_file`]). Both must name the same file.
///
/// # Errors
/// If no file was passed or the sandbox did not grant it.
pub fn iges_input<'a>(requested: Option<&'a Path>, granted: Option<&Path>) -> Result<&'a Path, String> {
    let file = requested.ok_or("IGES import needs an input file")?;
    if granted == Some(file) { Ok(file) } else { Err(IGES_NEEDS_LANDLOCK.into()) }
}

/// A fresh temporary directory only the current user can enter (mode 0700 on Unix),
/// deleted with everything in it when dropped.
#[derive(Debug)]
pub struct PrivateDir {
    path: PathBuf,
}

impl PrivateDir {
    /// Creates the directory under the system temp directory with an unpredictable name.
    ///
    /// # Errors
    /// If no directory could be created.
    pub fn new() -> std::io::Result<PrivateDir> {
        use std::hash::{BuildHasher, Hasher};
        // Canonical, so the path handed to the sandbox has no symlinks in it.
        let base = std::fs::canonicalize(std::env::temp_dir())?;
        let mut last = None;
        for _ in 0..32 {
            let rnd = std::collections::hash_map::RandomState::new().build_hasher().finish();
            let path = base.join(format!("gusset-import-{}-{rnd:016x}", std::process::id()));
            #[cfg_attr(not(unix), allow(unused_mut))]
            let mut b = std::fs::DirBuilder::new();
            #[cfg(unix)]
            std::os::unix::fs::DirBuilderExt::mode(&mut b, 0o700);
            match b.create(&path) {
                Ok(()) => return Ok(PrivateDir { path }),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => last = Some(e),
                Err(e) => return Err(e),
            }
        }
        Err(last.unwrap_or_else(|| std::io::Error::other("could not create a private directory")))
    }

    /// The directory.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Writes `data` to a new file `name` in the directory (mode 0600 on Unix) and
    /// returns its path.
    ///
    /// # Errors
    /// If the file cannot be created or written.
    pub fn write_file(&self, name: &str, data: &[u8]) -> std::io::Result<PathBuf> {
        let path = self.path.join(name);
        let mut o = std::fs::OpenOptions::new();
        o.write(true).create_new(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut o, 0o600);
        let mut f = o.open(&path)?;
        f.write_all(data)?;
        f.flush()?;
        Ok(path)
    }
}

impl Drop for PrivateDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// The most stdout the parent accepts from a worker for an input of `input_len` bytes.
///
/// Measured with OCCT's samples, the B-rep output of a STEP import is about 0.3× the
/// input and a validated B-rep blob is about 1× its input. Allowing 4× plus 16 MiB
/// (for the frame, the report and library chatter) leaves wide headroom while keeping
/// a compromised worker from making the app buffer gigabytes (security review #12).
pub fn output_cap(input_len: u64) -> u64 {
    input_len.saturating_mul(4).saturating_add(16 << 20).min(MAX_OUTPUT)
}

/// Encodes a request frame.
pub fn encode_request(format: Format, data: &[u8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(data.len() + 49);
    v.extend_from_slice(REQUEST_MAGIC);
    v.push(format as u8);
    v.extend_from_slice(&(data.len() as u64).to_le_bytes());
    v.extend_from_slice(data);
    v.extend_from_slice(blake3::hash(data).as_bytes());
    v
}

/// Decodes and verifies a request frame.
pub fn decode_request(b: &[u8]) -> Result<(Format, &[u8]), String> {
    let rest = b.strip_prefix(REQUEST_MAGIC.as_slice()).ok_or("bad request magic")?;
    let (&fmt, rest) = rest.split_first().ok_or("truncated request")?;
    let format = Format::from_u8(fmt).ok_or("unknown format")?;
    let (len, rest) = rest.split_at_checked(8).ok_or("truncated request")?;
    let len = u64::from_le_bytes(len.try_into().map_err(|_| "truncated request")?);
    if len > MAX_INPUT {
        return Err("input too large".into());
    }
    let (data, hash) =
        rest.split_at_checked(usize::try_from(len).map_err(|_| "input too large")?).ok_or("truncated request")?;
    if hash != blake3::hash(data).as_bytes() {
        return Err("request checksum mismatch".into());
    }
    Ok((format, data))
}

/// One imported body: a display name and kernel-native B-rep bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Body {
    pub name: String,
    pub brep: Vec<u8>,
}

/// Encodes a response frame (worker side).
pub fn encode_response(result: &Result<Vec<Body>, String>, report: &str) -> Vec<u8> {
    let mut body = Vec::new();
    match result {
        Ok(bodies) => {
            body.push(0u8);
            body.extend_from_slice(&u32::try_from(bodies.len()).unwrap_or(u32::MAX).to_le_bytes());
            for b in bodies {
                let name = b.name.as_bytes();
                body.extend_from_slice(&u32::try_from(name.len()).unwrap_or(0).to_le_bytes());
                body.extend_from_slice(name);
                body.extend_from_slice(&(b.brep.len() as u64).to_le_bytes());
                body.extend_from_slice(&b.brep);
            }
        }
        Err(msg) => {
            body.push(1u8);
            let m = msg.as_bytes();
            body.extend_from_slice(&u32::try_from(m.len()).unwrap_or(0).to_le_bytes());
            body.extend_from_slice(m);
        }
    }
    let rep = report.as_bytes();
    body.extend_from_slice(&u32::try_from(rep.len()).unwrap_or(0).to_le_bytes());
    body.extend_from_slice(rep);
    let mut out = Vec::with_capacity(body.len() + 48);
    out.extend_from_slice(RESPONSE_MAGIC);
    out.extend_from_slice(&(body.len() as u64).to_le_bytes());
    out.extend_from_slice(&body);
    out.extend_from_slice(blake3::hash(&body).as_bytes());
    out
}

struct Cursor<'a>(&'a [u8]);

impl<'a> Cursor<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
        let (a, b) = self.0.split_at_checked(n).ok_or("truncated response")?;
        self.0 = b;
        Ok(a)
    }
    fn u32(&mut self) -> Result<u32, String> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().map_err(|_| "truncated")?))
    }
    fn u64(&mut self) -> Result<u64, String> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().map_err(|_| "truncated")?))
    }
}

/// Finds and verifies the response frame in a worker's stdout.
pub fn decode_response(out: &[u8]) -> Result<(Result<Vec<Body>, String>, String), String> {
    let pos = out
        .windows(RESPONSE_MAGIC.len())
        .rposition(|w| w == RESPONSE_MAGIC)
        .ok_or("the import worker sent no response")?;
    let mut c = Cursor(out.get(pos + RESPONSE_MAGIC.len()..).unwrap_or_default());
    let len = usize::try_from(c.u64()?).map_err(|_| "response too large")?;
    let body = c.take(len)?;
    let hash = c.take(32)?;
    if hash != blake3::hash(body).as_bytes() {
        return Err("import worker response is corrupted".into());
    }
    let mut b = Cursor(body);
    let status = b.take(1)?.first().copied().unwrap_or(1);
    let result = if status == 0 {
        let n = b.u32()?;
        if n > MAX_BODIES {
            return Err("too many bodies".into());
        }
        let mut v = Vec::new();
        for _ in 0..n {
            let nl = b.u32()? as usize;
            let name = String::from_utf8_lossy(b.take(nl)?).into_owned();
            let dl = usize::try_from(b.u64()?).map_err(|_| "body too large")?;
            v.push(Body { name, brep: b.take(dl)?.to_vec() });
        }
        Ok(v)
    } else {
        let ml = b.u32()? as usize;
        Err(String::from_utf8_lossy(b.take(ml)?).into_owned())
    };
    let rl = b.u32()? as usize;
    let report = String::from_utf8_lossy(b.take(rl)?).into_owned();
    Ok((result, report))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportError {
    TooLarge,
    /// The worker could not be started.
    Spawn(String),
    /// The worker crashed or was killed (likely a malformed or hostile file).
    Crashed(String),
    TimedOut,
    /// The caller cancelled the import (the worker was killed).
    Cancelled,
    /// The worker sent more output than the input can justify.
    OutputTooLarge,
    /// The file was parsed but could not be imported.
    Failed(String),
    Protocol(String),
}

impl std::fmt::Display for ImportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ImportError::TooLarge => write!(f, "the file is larger than {} MiB", MAX_INPUT >> 20),
            ImportError::Spawn(e) => write!(f, "could not start the import worker: {e}"),
            ImportError::Crashed(e) => {
                write!(f, "the importer stopped while reading this file ({e}); the file may be damaged or unsupported")
            }
            ImportError::TimedOut => f.write_str("importing took too long and was stopped"),
            ImportError::Cancelled => f.write_str("the import was cancelled"),
            ImportError::OutputTooLarge => f.write_str("the import worker sent too much data; the import was stopped"),
            ImportError::Failed(e) => write!(f, "import failed: {e}"),
            ImportError::Protocol(e) => write!(f, "import worker error: {e}"),
        }
    }
}

impl std::error::Error for ImportError {}

/// Result of an import, including what the worker's sandbox achieved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Imported {
    pub bodies: Vec<Body>,
    pub sandbox_report: String,
}

/// Runs the worker at `worker` (an absolute path in the install directory).
pub fn import_out_of_process(
    worker: &Path,
    format: Format,
    data: &[u8],
    limits: &gusset_sandbox::Limits,
    timeout: Duration,
) -> Result<Imported, ImportError> {
    import_out_of_process_cancellable(worker, format, data, limits, timeout, &AtomicBool::new(false))
}

/// [`import_out_of_process`] that kills the worker as soon as `cancel` becomes true.
pub fn import_out_of_process_cancellable(
    worker: &Path,
    format: Format,
    data: &[u8],
    limits: &gusset_sandbox::Limits,
    timeout: Duration,
    cancel: &AtomicBool,
) -> Result<Imported, ImportError> {
    if data.len() as u64 > MAX_INPUT {
        return Err(ImportError::TooLarge);
    }
    let cap = output_cap(data.len() as u64);
    let out = if format == Format::Iges {
        // The reader needs a file name: hand over a private file the sandbox lets the
        // worker read, and delete it afterwards whatever happens (drop).
        let dir = PrivateDir::new().map_err(|e| ImportError::Spawn(format!("no private temp directory ({e})")))?;
        let file = dir
            .write_file("input.igs", data)
            .map_err(|e| ImportError::Spawn(format!("could not write the temporary IGES file ({e})")))?;
        let limits = limits.clone().with_read_file(file.clone());
        let args = [INPUT_FILE_FLAG.into(), file.into_os_string()];
        // No bytes on stdin: the request carries the format only.
        run_worker(worker, &args, encode_request(format, &[]), cap, &limits, timeout, cancel)?
    } else {
        run_worker(worker, &[], encode_request(format, data), cap, limits, timeout, cancel)?
    };
    let (result, report) = decode_response(&out).map_err(ImportError::Protocol)?;
    let bodies = result.map_err(ImportError::Failed)?;
    Ok(Imported { bodies, sandbox_report: report })
}

/// Removes private import directories (`gusset-import-*` in the temp dir) left behind
/// when a Gusset process was killed during an IGES import (security re-review 7).
/// Only directories older than `min_age` are touched (no import runs that long), never
/// symlinks. Returns how many were removed. Best effort.
pub fn sweep_stale_import_dirs(min_age: Duration) -> usize {
    match std::fs::canonicalize(std::env::temp_dir()) {
        Ok(base) => sweep_stale_import_dirs_in(&base, min_age),
        Err(_) => 0,
    }
}

/// [`sweep_stale_import_dirs`] in `base` instead of the system temp directory.
pub fn sweep_stale_import_dirs_in(base: &Path, min_age: Duration) -> usize {
    let Ok(rd) = std::fs::read_dir(base) else { return 0 };
    let mut n = 0;
    for e in rd.flatten() {
        if !e.file_name().to_str().is_some_and(|s| s.starts_with("gusset-import-")) {
            continue;
        }
        let Ok(md) = std::fs::symlink_metadata(e.path()) else { continue };
        let old = md.modified().ok().and_then(|m| m.elapsed().ok()).is_some_and(|age| age >= min_age);
        if md.file_type().is_dir() && old && std::fs::remove_dir_all(e.path()).is_ok() {
            n += 1;
        }
    }
    n
}

/// Parses an untrusted kernel-native B-rep blob in the sandboxed worker and returns the
/// worker's re-serialization of it (written by OCCT's writer from the parsed shape).
///
/// Use the returned bytes, not `data`, for the in-process parse: then the app's
/// `BinTools::Read` only ever sees writer-produced data. A blob that crashes, hangs or
/// exhausts the parser only takes down the worker.
pub fn validate_native_out_of_process(
    worker: &Path,
    data: &[u8],
    limits: &gusset_sandbox::Limits,
    timeout: Duration,
) -> Result<Vec<u8>, ImportError> {
    let out = import_out_of_process(worker, Format::Brep, data, limits, timeout)?;
    let mut bodies = out.bodies.into_iter();
    match (bodies.next(), bodies.next()) {
        (Some(b), None) => Ok(b.brep),
        _ => Err(ImportError::Protocol("the import worker returned an unexpected number of shapes".into())),
    }
}

/// Blobs this process (or, with [`trust::persist_to`], this installation) produced or
/// validated in the worker, by BLAKE3 hash. Only these are parsed in-process directly.
pub mod trust {
    use std::collections::HashSet;
    use std::io::Write;
    use std::path::{Path, PathBuf};
    use std::sync::{Mutex, PoisonError};

    /// Upper bound on remembered hashes (32 bytes each); beyond it, blobs are re-checked.
    const MAX_ENTRIES: usize = 1 << 20;
    /// Largest trust file read at startup.
    const MAX_FILE: u64 = 80 << 20;

    #[derive(Debug, Default)]
    struct State {
        hashes: HashSet<[u8; 32]>,
        file: Option<PathBuf>,
        /// This process only: hash of an untrusted blob -> the worker's canonical
        /// re-serialization of it (most recent last). Never persisted.
        canonical: Vec<([u8; 32], std::sync::Arc<Vec<u8>>)>,
    }

    /// Canonical blobs remembered per process.
    const MAX_CANONICAL: usize = 64;

    static STATE: Mutex<Option<State>> = Mutex::new(None);

    fn with<R>(f: impl FnOnce(&mut State) -> R) -> R {
        let mut g = STATE.lock().unwrap_or_else(PoisonError::into_inner);
        f(g.get_or_insert_with(State::default))
    }

    /// The worker's canonical bytes for an untrusted blob validated earlier in this
    /// process. Callers parse these, never the untrusted original (security re-review 2).
    pub fn canonical_for(data: &[u8]) -> Option<std::sync::Arc<Vec<u8>>> {
        let h = *blake3::hash(data).as_bytes();
        with(|s| s.canonical.iter().rev().find(|(k, _)| *k == h).map(|(_, v)| v.clone()))
    }

    /// Remembers the canonical bytes of a validated untrusted blob (this process only)
    /// and trusts the canonical bytes, which OCCT's writer produced.
    pub fn remember_canonical(data: &[u8], canonical: Vec<u8>) -> std::sync::Arc<Vec<u8>> {
        let h = *blake3::hash(data).as_bytes();
        let c = std::sync::Arc::new(canonical);
        with(|s| {
            s.canonical.retain(|(k, _)| *k != h);
            if s.canonical.len() >= MAX_CANONICAL {
                s.canonical.remove(0);
            }
            s.canonical.push((h, c.clone()));
        });
        trust(&c);
        c
    }

    /// Whether `data` was produced by this installation's writer (in-process or as a
    /// worker's canonical output). Untrusted originals are never recorded here.
    pub fn is_trusted(data: &[u8]) -> bool {
        let h = *blake3::hash(data).as_bytes();
        with(|s| s.hashes.contains(&h))
    }

    /// Records `data` as produced by this installation or validated by the worker.
    pub fn trust(data: &[u8]) {
        let h = *blake3::hash(data).as_bytes();
        let file = with(|s| {
            if s.hashes.len() >= MAX_ENTRIES || !s.hashes.insert(h) {
                return None;
            }
            s.file.clone()
        });
        if let Some(f) = file {
            let _ = append(&f, &h);
        }
    }

    /// Keeps the trusted set in `path` (one hex hash per line, owner-only), so blobs are
    /// validated once per installation instead of once per process. Loads existing
    /// entries. A symlinked or oversized file is ignored (and not written).
    ///
    /// # Errors
    /// If the file exists but cannot be read.
    pub fn persist_to(path: &Path) -> std::io::Result<usize> {
        let mut loaded = Vec::new();
        match std::fs::symlink_metadata(path) {
            Ok(m) if m.file_type().is_symlink() || !m.is_file() || m.len() > MAX_FILE => {
                return Err(std::io::Error::other("the trusted-blob list is not a regular file of sane size"));
            }
            Ok(_) => {
                for line in std::fs::read_to_string(path)?.lines() {
                    let mut h = [0u8; 32];
                    if parse_hex(line.trim(), &mut h) {
                        loaded.push(h);
                    }
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
        let n = loaded.len();
        with(|s| {
            for h in loaded {
                if s.hashes.len() >= MAX_ENTRIES {
                    break;
                }
                s.hashes.insert(h);
            }
            s.file = Some(path.to_path_buf());
        });
        Ok(n)
    }

    fn parse_hex(s: &str, out: &mut [u8; 32]) -> bool {
        if s.len() != 64 || !s.is_ascii() {
            return false;
        }
        for (i, b) in out.iter_mut().enumerate() {
            match s.get(i * 2..i * 2 + 2).and_then(|d| u8::from_str_radix(d, 16).ok()) {
                Some(v) => *b = v,
                None => return false,
            }
        }
        true
    }

    fn append(path: &Path, h: &[u8; 32]) -> std::io::Result<()> {
        let mut o = std::fs::OpenOptions::new();
        o.create(true).append(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            o.mode(0o600).custom_flags(libc::O_NOFOLLOW);
        }
        let mut f = o.open(path)?;
        let mut line = String::with_capacity(65);
        for b in h {
            line.push_str(&format!("{b:02x}"));
        }
        line.push('\n');
        f.write_all(line.as_bytes())
    }

    #[cfg(test)]
    pub(crate) fn reset_for_test() {
        with(|s| *s = State::default());
    }
}

/// How long to wait for the worker's stdout to close after the worker is gone.
const DRAIN_TIMEOUT: Duration = Duration::from_secs(5);

/// Spawns the worker, sends `request`, and returns everything it wrote to stdout.
///
/// The worker leads its own process group (Unix) or job (Windows). Whatever happens —
/// normal exit, crash or timeout — everything in that group is killed before the worker
/// is reaped, so no descendant can outlive the call or hold the pipe open, and reading
/// stdout has a deadline.
fn run_worker(
    worker: &Path,
    args: &[std::ffi::OsString],
    request: Vec<u8>,
    cap: u64,
    limits: &gusset_sandbox::Limits,
    timeout: Duration,
    cancel: &AtomicBool,
) -> Result<Vec<u8>, ImportError> {
    if !worker.is_absolute() {
        return Err(ImportError::Spawn("worker path must be absolute".into()));
    }
    let mut cmd = std::process::Command::new(worker);
    cmd.args(args)
        .env_clear()
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null());
    gusset_sandbox::configure_command(&mut cmd);
    let mut child = cmd.spawn().map_err(|e| ImportError::Spawn(e.to_string()))?;
    let guard = match gusset_sandbox::contain_child(&child, limits) {
        Ok(g) => g,
        Err(e) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(ImportError::Spawn(e));
        }
    };
    let finish = |child: &mut std::process::Child| {
        guard.kill_all();
        let _ = child.kill();
        child.wait()
    };
    let (Some(mut stdin), Some(mut stdout)) = (child.stdin.take(), child.stdout.take()) else {
        let _ = finish(&mut child);
        return Err(ImportError::Spawn("no stdin/stdout".into()));
    };
    let writer = std::thread::spawn(move || {
        let _ = stdin.write_all(&request);
    });
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        // One byte past the cap tells "too large" apart from "exactly the cap".
        let _ = (&mut stdout).take(cap.saturating_add(1)).read_to_end(&mut buf);
        let _ = tx.send(buf);
    });
    let too_large = || ImportError::OutputTooLarge;
    let start = Instant::now();
    let mut early = None;
    loop {
        if early.is_none()
            && let Ok(buf) = rx.try_recv()
        {
            if buf.len() as u64 > cap {
                let _ = finish(&mut child);
                return Err(too_large());
            }
            early = Some(buf);
        }
        match gusset_sandbox::has_exited(&mut child) {
            Ok(true) => break,
            Ok(false) if cancel.load(Ordering::Relaxed) => {
                let _ = finish(&mut child);
                return Err(ImportError::Cancelled);
            }
            Ok(false) if start.elapsed() > timeout => {
                let _ = finish(&mut child);
                return Err(ImportError::TimedOut);
            }
            Ok(false) => std::thread::sleep(Duration::from_millis(5)),
            Err(e) => {
                let _ = finish(&mut child);
                return Err(ImportError::Crashed(e.to_string()));
            }
        }
    }
    // The worker has exited but is not reaped yet: kill anything it left behind.
    let status = finish(&mut child).map_err(|e| ImportError::Crashed(e.to_string()))?;
    let _ = writer.join();
    let out = match early {
        Some(buf) => buf,
        None => rx
            .recv_timeout(DRAIN_TIMEOUT)
            .map_err(|_| ImportError::Protocol("the import worker's output did not close".into()))?,
    };
    if !status.success() {
        return Err(ImportError::Crashed(format!("exit status {status}")));
    }
    if out.len() as u64 > cap {
        return Err(too_large());
    }
    Ok(out)
}

/// The worker binary next to the current executable (install directory).
pub fn default_worker_path() -> Option<std::path::PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let name = if cfg!(windows) { "gusset-worker.exe" } else { "gusset-worker" };
    Some(exe.parent()?.join(name))
}

/// The session [`gusset_commands::session::Importer`] for STEP/IGES: the sandboxed
/// worker process. Front ends install it with [`WorkerImporter::next_to_exe`].
#[derive(Debug, Clone)]
pub struct WorkerImporter {
    worker: PathBuf,
    limits: gusset_sandbox::Limits,
    timeout: Duration,
}

/// Default import timeout.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(600);

impl WorkerImporter {
    /// An importer using the worker binary at `worker` (must be absolute).
    #[must_use]
    pub fn new(worker: PathBuf) -> Self {
        WorkerImporter { worker, limits: gusset_sandbox::Limits::default(), timeout: DEFAULT_TIMEOUT }
    }

    /// The worker installed next to the running executable, if it is there.
    #[must_use]
    pub fn next_to_exe() -> Option<Self> {
        let p = default_worker_path()?;
        std::fs::metadata(&p).ok().filter(std::fs::Metadata::is_file)?;
        Some(WorkerImporter::new(p))
    }

    /// Changes the timeout.
    #[must_use]
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }
}

impl gusset_commands::session::Importer for WorkerImporter {
    fn import(&self, format: &str, bytes: &[u8], cancel: &AtomicBool) -> Result<Vec<(String, Vec<u8>)>, String> {
        let f = Format::from_extension(format).ok_or_else(|| format!("'{format}' is not imported by the worker"))?;
        let r = import_out_of_process_cancellable(&self.worker, f, bytes, &self.limits, self.timeout, cancel)
            .map_err(|e| e.to_string())?;
        Ok(r.bodies.into_iter().map(|b| (b.name, b.brep)).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn untrusted_originals_are_never_trusted_only_their_canonical_bytes() {
        // Security re-review 2.
        let original = b"hostile original bytes for the canonical test".to_vec();
        let canonical = b"writer-produced canonical bytes for the test".to_vec();
        assert!(trust::canonical_for(&original).is_none());
        let c = trust::remember_canonical(&original, canonical.clone());
        assert_eq!(*c, canonical);
        assert!(!trust::is_trusted(&original), "the original must stay untrusted");
        assert!(trust::is_trusted(&canonical));
        assert_eq!(trust::canonical_for(&original).as_deref(), Some(&canonical));
    }

    #[test]
    fn trust_set_and_persistent_store() {
        trust::reset_for_test();
        assert!(!trust::is_trusted(b"blob-a"));
        trust::trust(b"blob-a");
        assert!(trust::is_trusted(b"blob-a"));
        let dir = std::env::temp_dir().join(format!("gusset-trust-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("trusted-blobs");
        assert_eq!(trust::persist_to(&f).unwrap(), 0);
        trust::trust(b"blob-b");
        trust::reset_for_test();
        assert!(!trust::is_trusted(b"blob-b"));
        assert_eq!(trust::persist_to(&f).unwrap(), 1);
        assert!(trust::is_trusted(b"blob-b") && !trust::is_trusted(b"blob-a"));
        #[cfg(unix)]
        {
            let link = dir.join("link");
            std::os::unix::fs::symlink(&f, &link).unwrap();
            assert!(trust::persist_to(&link).is_err());
        }
        trust::reset_for_test();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn iges_needs_an_enforced_file_allowance() {
        let f = Path::new("/tmp/x/input.igs");
        assert_eq!(iges_input(Some(f), Some(f)), Ok(f));
        assert_eq!(iges_input(Some(f), None).unwrap_err(), IGES_NEEDS_LANDLOCK);
        assert_eq!(iges_input(Some(f), Some(Path::new("/other"))).unwrap_err(), IGES_NEEDS_LANDLOCK);
        assert!(iges_input(None, Some(f)).is_err());
        assert!(IGES_NEEDS_LANDLOCK.contains("Landlock (kernel 5.13+)"));
    }

    #[cfg(unix)]
    #[test]
    fn private_dir_is_private_and_removed() {
        use std::os::unix::fs::PermissionsExt;
        let d = PrivateDir::new().unwrap();
        let path = d.path().to_path_buf();
        let f = d.write_file("input.igs", b"data").unwrap();
        assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o700);
        assert_eq!(std::fs::metadata(&f).unwrap().permissions().mode() & 0o777, 0o600);
        assert!(d.write_file("input.igs", b"again").is_err(), "files are created exclusively");
        assert_ne!(PrivateDir::new().unwrap().path(), path);
        drop(d);
        assert!(!path.exists());
    }

    #[test]
    fn output_cap_follows_input_size() {
        assert_eq!(output_cap(0), 16 << 20);
        assert_eq!(output_cap(1 << 20), (4 << 20) + (16 << 20));
        assert_eq!(output_cap(MAX_INPUT), MAX_OUTPUT);
        assert_eq!(output_cap(u64::MAX), MAX_OUTPUT);
    }

    #[cfg(unix)]
    fn script(tag: &str, body: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let d = std::env::temp_dir().join(format!("gusset-fake-worker-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let p = d.join("worker.sh");
        std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        p
    }

    /// Code review M6: an import can be cancelled; the worker is killed.
    #[cfg(unix)]
    #[test]
    fn cancel_kills_the_worker() {
        let w = script("cancel", "exec /bin/sleep 30");
        let cancel = std::sync::Arc::new(AtomicBool::new(false));
        let c2 = cancel.clone();
        let t = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(200));
            c2.store(true, Ordering::SeqCst);
        });
        let start = Instant::now();
        let r = import_out_of_process_cancellable(
            &w,
            Format::Step,
            b"x",
            &gusset_sandbox::Limits::default(),
            Duration::from_secs(60),
            &cancel,
        );
        assert_eq!(r.unwrap_err(), ImportError::Cancelled);
        assert!(start.elapsed() < Duration::from_secs(10));
        t.join().unwrap();
        let _ = std::fs::remove_dir_all(w.parent().unwrap());
    }

    #[test]
    fn frames_roundtrip_and_resist_noise() {
        let req = encode_request(Format::Step, b"ISO-10303-21;");
        assert_eq!(decode_request(&req).unwrap(), (Format::Step, &b"ISO-10303-21;"[..]));
        let mut bad = req.clone();
        if let Some(b) = bad.get_mut(20) {
            *b ^= 1;
        }
        assert!(decode_request(&bad).is_err());
        let bodies = vec![Body { name: "A".into(), brep: vec![1, 2, 3] }];
        let mut out = b"OCCT says hello\n".to_vec();
        out.extend(encode_response(&Ok(bodies.clone()), "rep"));
        let (r, rep) = decode_response(&out).unwrap();
        assert_eq!(r.unwrap(), bodies);
        assert_eq!(rep, "rep");
        let mut corrupt = out.clone();
        let n = corrupt.len();
        if let Some(b) = corrupt.get_mut(n - 40) {
            *b ^= 0xff;
        }
        assert!(decode_response(&corrupt).is_err());
        assert!(decode_response(b"no frame").is_err());
        let (r, _) = decode_response(&encode_response(&Err("nope".into()), "")).unwrap();
        assert_eq!(r.unwrap_err(), "nope");
    }
}
