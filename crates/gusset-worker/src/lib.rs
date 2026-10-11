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
use std::path::Path;
use std::time::{Duration, Instant};

pub const REQUEST_MAGIC: &[u8; 8] = b"GWRQ1\0\0\0";
pub const RESPONSE_MAGIC: &[u8; 8] = b"\0GWRSP1\0";
/// Largest input file accepted.
pub const MAX_INPUT: u64 = 1 << 30;
/// Largest response accepted from a worker.
pub const MAX_OUTPUT: u64 = 4 << 30;
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
    if data.len() as u64 > MAX_INPUT {
        return Err(ImportError::TooLarge);
    }
    let out = run_worker(worker, encode_request(format, data), limits, timeout)?;
    let (result, report) = decode_response(&out).map_err(ImportError::Protocol)?;
    let bodies = result.map_err(ImportError::Failed)?;
    Ok(Imported { bodies, sandbox_report: report })
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
    request: Vec<u8>,
    limits: &gusset_sandbox::Limits,
    timeout: Duration,
) -> Result<Vec<u8>, ImportError> {
    if !worker.is_absolute() {
        return Err(ImportError::Spawn("worker path must be absolute".into()));
    }
    let mut cmd = std::process::Command::new(worker);
    cmd.env_clear()
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
        let _ = (&mut stdout).take(MAX_OUTPUT).read_to_end(&mut buf);
        let _ = tx.send(buf);
    });
    let start = Instant::now();
    loop {
        match gusset_sandbox::has_exited(&mut child) {
            Ok(true) => break,
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
    let out = rx
        .recv_timeout(DRAIN_TIMEOUT)
        .map_err(|_| ImportError::Protocol("the import worker's output did not close".into()))?;
    if !status.success() {
        return Err(ImportError::Crashed(format!("exit status {status}")));
    }
    Ok(out)
}

/// The worker binary next to the current executable (install directory).
pub fn default_worker_path() -> Option<std::path::PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let name = if cfg!(windows) { "gusset-worker.exe" } else { "gusset-worker" };
    Some(exe.parent()?.join(name))
}

#[cfg(test)]
mod tests {
    use super::*;

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
