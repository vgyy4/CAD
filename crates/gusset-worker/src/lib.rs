//! The out-of-process importer (ADR 0008 §4).
//!
//! The parent ([`import_out_of_process`]) starts `gusset-worker` from the install
//! directory with an empty environment, sends the file bytes over stdin, and reads
//! kernel-native B-rep bodies back over stdout. The worker sandboxes itself before
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
/// Hard cap on a worker's response, whatever the input size.
pub const MAX_OUTPUT: u64 = 1 << 30;

/// Largest response accepted for an input of `input_len` bytes: 16 × the input plus
/// 64 MiB (native B-rep is far more compact than STEP/IGES text, so a real import stays
/// well below), never more than [`MAX_OUTPUT`]. A compromised worker therefore cannot
/// make the app buffer gigabytes for a small file (security review #12).
#[must_use]
pub fn max_output_for(input_len: usize) -> u64 {
    (input_len as u64).saturating_mul(16).saturating_add(64 << 20).min(MAX_OUTPUT)
}
pub const MAX_BODIES: u32 = 100_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Step = 1,
    Iges = 2,
}

impl Format {
    pub fn from_u8(v: u8) -> Option<Format> {
        match v {
            1 => Some(Format::Step),
            2 => Some(Format::Iges),
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
    if !worker.is_absolute() {
        return Err(ImportError::Spawn("worker path must be absolute".into()));
    }
    let mut child = std::process::Command::new(worker)
        .env_clear()
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| ImportError::Spawn(e.to_string()))?;
    let _guard = gusset_sandbox::contain_child(&child, limits).map_err(ImportError::Spawn)?;
    let req = encode_request(format, data);
    let mut stdin = child.stdin.take().ok_or_else(|| ImportError::Spawn("no stdin".into()))?;
    let writer = std::thread::spawn(move || {
        let _ = stdin.write_all(&req);
    });
    let mut stdout = child.stdout.take().ok_or_else(|| ImportError::Spawn("no stdout".into()))?;
    let cap = max_output_for(data.len());
    let overflow = std::sync::Arc::new(AtomicBool::new(false));
    let overflow2 = overflow.clone();
    let reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        // One byte over the cap is enough to know it was exceeded.
        let _ = (&mut stdout).take(cap.saturating_add(1)).read_to_end(&mut buf);
        if buf.len() as u64 > cap {
            overflow2.store(true, Ordering::SeqCst);
        }
        buf
    });
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break s,
            Ok(None) if cancel.load(Ordering::Relaxed) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(ImportError::Cancelled);
            }
            Ok(None) if overflow.load(Ordering::SeqCst) => {
                // The worker sent more than the input can justify: stop it now.
                let _ = child.kill();
                let _ = child.wait();
                return Err(ImportError::OutputTooLarge);
            }
            Ok(None) if start.elapsed() > timeout => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(ImportError::TimedOut);
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(5)),
            Err(e) => return Err(ImportError::Crashed(e.to_string())),
        }
    };
    let _ = writer.join();
    let out = reader.join().map_err(|_| ImportError::Protocol("reader thread failed".into()))?;
    if out.len() as u64 > cap {
        return Err(ImportError::OutputTooLarge);
    }
    if !status.success() {
        return Err(ImportError::Crashed(format!("exit status {status}")));
    }
    let (result, report) = decode_response(&out).map_err(ImportError::Protocol)?;
    let bodies = result.map_err(ImportError::Failed)?;
    Ok(Imported { bodies, sandbox_report: report })
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
    fn output_cap_follows_the_input_size() {
        assert_eq!(max_output_for(0), 64 << 20);
        assert_eq!(max_output_for(1 << 20), (16 << 20) + (64 << 20));
        assert_eq!(max_output_for(1 << 30), MAX_OUTPUT);
        const { assert!(MAX_OUTPUT < 4 << 30) };
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

    /// Security review #12: a (compromised) worker that floods stdout is stopped at a
    /// bound derived from the input size.
    #[cfg(unix)]
    #[test]
    fn flooding_worker_is_stopped() {
        let w = script("flood", "exec /usr/bin/head -c 200000000 /dev/zero");
        let r =
            import_out_of_process(&w, Format::Step, b"x", &gusset_sandbox::Limits::default(), Duration::from_secs(60));
        assert_eq!(r.unwrap_err(), ImportError::OutputTooLarge);
        let _ = std::fs::remove_dir_all(w.parent().unwrap());
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
