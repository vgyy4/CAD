//! The app side of the kernel worker process (ADR 0022): starts `gusset-kernel-worker`
//! sandboxed and contained (process group / job object, empty environment, memory budget),
//! talks to it over its stdin/stdout, and turns its death into a readable reason.
//!
//! Use [`production_kernel`], or [`connector`] with [`RemoteKernel::new`].

use gusset_kernel::Kernel;
use gusset_kernel_remote::{Connect, NativeGate, RemoteKernel, StreamTransport, Transport, TransportError};
use std::borrow::Cow;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, ExitStatus, Stdio};
use std::sync::Arc;

/// Command-line flag carrying the memory budget in MiB.
pub const MEMORY_FLAG: &str = "--memory-mb";
/// Budget when none is given: 4 GiB.
pub const DEFAULT_MEMORY_MB: u64 = 4096;
/// RLIMIT_CPU value meaning "no limit" in practice (115 days of CPU time).
pub const NO_CPU_LIMIT: u64 = 10_000_000;

/// The worker binary next to the current executable (install directory).
#[must_use]
pub fn default_worker_path() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let name = if cfg!(windows) { "gusset-kernel-worker.exe" } else { "gusset-kernel-worker" };
    Some(exe.parent()?.join(name))
}

/// A running kernel worker.
#[derive(Debug)]
pub struct KernelProcess {
    child: Child,
    guard: gusset_sandbox::ChildGuard,
    stream: StreamTransport<ChildStdin, BufReader<ChildStdout>>,
}

impl KernelProcess {
    /// Starts the worker at `worker` (an absolute path) with a memory budget.
    ///
    /// # Errors
    /// The binary could not be started or contained.
    pub fn spawn(worker: &Path, memory_bytes: u64) -> Result<KernelProcess, String> {
        if !worker.is_absolute() {
            return Err("the kernel worker path must be absolute".into());
        }
        let mb = (memory_bytes >> 20).max(64);
        let mut cmd = Command::new(worker);
        cmd.arg(MEMORY_FLAG)
            .arg(mb.to_string())
            .env_clear()
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        gusset_sandbox::configure_command(&mut cmd);
        let mut child = cmd.spawn().map_err(|e| format!("{}: {e}", worker.display()))?;
        let limits = gusset_sandbox::Limits { memory_bytes, cpu_seconds: NO_CPU_LIMIT, read_file: None };
        let guard = match gusset_sandbox::contain_child(&child, &limits) {
            Ok(g) => g,
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(e);
            }
        };
        let (Some(stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
            guard.kill_all();
            let _ = child.kill();
            let _ = child.wait();
            return Err("the kernel worker has no stdin/stdout".into());
        };
        Ok(KernelProcess { child, guard, stream: StreamTransport::new(stdin, BufReader::new(stdout)) })
    }

    /// Kills the worker (and anything it started) and returns how it ended.
    fn reap(&mut self) -> String {
        // A worker that broke its stream on purpose has usually exited already; give it a
        // moment so its real exit status (signal, code) is reported rather than our kill.
        for _ in 0..50 {
            if matches!(gusset_sandbox::has_exited(&mut self.child), Ok(true)) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        self.guard.kill_all();
        let _ = self.child.kill();
        match self.child.wait() {
            Ok(status) => exit_reason(status),
            Err(e) => format!("could not be reaped ({e})"),
        }
    }
}

impl Transport for KernelProcess {
    fn call(&mut self, request: &[u8]) -> Result<Vec<u8>, TransportError> {
        match self.stream.call(request) {
            Ok(reply) => Ok(reply),
            Err(TransportError::Died { reason }) => {
                let how = self.reap();
                Err(TransportError::Died { reason: format!("{how} ({reason})") })
            }
            Err(e) => {
                self.reap();
                Err(e)
            }
        }
    }
}

impl Drop for KernelProcess {
    fn drop(&mut self) {
        self.guard.kill_all();
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// How a worker ended, in words.
#[must_use]
pub fn exit_reason(status: ExitStatus) -> String {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(sig) = status.signal() {
            return match sig {
                6 => "aborted (SIGABRT)".into(),
                9 => "was killed (SIGKILL: out of memory, or stopped)".into(),
                11 => "crashed (segmentation fault)".into(),
                7 => "crashed (bus error)".into(),
                4 => "crashed (illegal instruction)".into(),
                8 => "crashed (arithmetic exception)".into(),
                24 => "ran out of CPU time (SIGXCPU)".into(),
                s => format!("ended by signal {s}"),
            };
        }
    }
    #[cfg(windows)]
    if let Some(code) = status.code() {
        // NTSTATUS values of the usual hard faults; other error statuses in hex, plain
        // exit codes in decimal below.
        match code.cast_unsigned() {
            0xC000_0005 => return "crashed (access violation)".into(),
            0xC000_00FD => return "crashed (stack overflow)".into(),
            0xC000_0017 | 0xC000_012D => return "ran out of memory".into(),
            0xC000_0409 => return "aborted (fail-fast)".into(),
            c if c >= 0xC000_0000 => return format!("crashed (status {c:#x})"),
            _ => {}
        }
    }
    match status.code() {
        Some(c) => format!("exited with status {c}"),
        None => "ended".into(),
    }
}

/// A [`Connect`] that starts the worker at `worker` with `memory_bytes` each time the
/// kernel needs a new one.
#[must_use]
pub fn connector(worker: PathBuf, memory_bytes: u64) -> Connect {
    Box::new(move || KernelProcess::spawn(&worker, memory_bytes).map(|p| Box::new(p) as Box<dyn Transport>))
}

/// Vets B-rep blobs in the app before the worker parses them, exactly as the in-process
/// kernel does (ADR 0008 §4): trusted when this process wrote or validated them, else
/// re-serialized by the sandboxed import worker first.
#[derive(Debug, Clone)]
pub struct OcctGate {
    import_worker: Option<PathBuf>,
}

impl OcctGate {
    /// With the import worker installed next to the current executable, if any.
    #[must_use]
    pub fn next_to_exe() -> OcctGate {
        OcctGate { import_worker: gusset_worker::default_worker_path().filter(|p| p.is_file()) }
    }
}

impl NativeGate for OcctGate {
    fn written(&self, bytes: &[u8]) {
        gusset_worker::trust::trust(bytes);
    }

    fn vet<'a>(&self, bytes: &'a [u8]) -> gusset_kernel::KResult<Cow<'a, [u8]>> {
        gusset_kernel_occt::vet_native(self.import_worker.as_deref(), bytes)
    }
}

/// Environment variable that runs the kernel in the app process instead of the worker
/// (debugging, or a platform where the worker cannot start). Any non-empty value.
pub const IN_PROCESS_ENV: &str = "GUSSET_KERNEL_IN_PROCESS";

/// The kernel the app and `gusset mcp` use: OCCT in the worker process next to the
/// executable, with a memory budget from the machine's RAM, so a crash or an exhausted
/// budget fails one operation instead of closing the app (ADR 0022). Falls back to OCCT
/// in this process, with a note saying why, when the worker is missing, cannot start,
/// or [`IN_PROCESS_ENV`] is set.
///
/// # Errors
/// OCCT is not available at all (the caller decides whether to use the mock kernel).
pub fn production_kernel() -> Result<(Arc<dyn Kernel>, Option<String>), String> {
    let why = if std::env::var_os(IN_PROCESS_ENV).is_some_and(|v| !v.is_empty()) {
        format!("{IN_PROCESS_ENV} is set")
    } else {
        match default_worker_path().filter(|p| p.is_file()) {
            None => "the kernel worker is not installed next to this program".to_owned(),
            Some(worker) => {
                let ram = gusset_hw::CpuInfo::detect().total_ram_bytes;
                let remote = RemoteKernel::new("occt", connector(worker, gusset_hw::budgets::kernel_worker_bytes(ram)))
                    .with_native_gate(Box::new(OcctGate::next_to_exe()));
                match remote.ping() {
                    Ok(()) => return Ok((Arc::new(remote), None)),
                    Err(e) => format!("the kernel worker did not start: {}", e.message),
                }
            }
        }
    };
    let k = gusset_kernel_occt::OcctKernel::new().map_err(|e| e.to_string())?;
    Ok((Arc::new(k), Some(format!("{why}; the geometry kernel runs inside this process, so a crash in it closes it"))))
}
