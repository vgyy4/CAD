//! The app side of the kernel worker process (ADR 0022): starts `gusset-kernel-worker`
//! sandboxed and contained (process group / job object, empty environment, memory budget),
//! talks to it over its stdin/stdout, and turns its death into a readable reason.
//!
//! Use [`connector`] with [`gusset_kernel_remote::RemoteKernel::new`].

use gusset_kernel_remote::{Connect, StreamTransport, Transport, TransportError};
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, ExitStatus, Stdio};

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
        // NTSTATUS values of the usual hard faults.
        return match code as u32 {
            0xC000_0005 => "crashed (access violation)".into(),
            0xC000_00FD => "crashed (stack overflow)".into(),
            0xC000_0017 | 0xC000_012D => "ran out of memory".into(),
            0xC000_0409 => "aborted (fail-fast)".into(),
            c => format!("exited with status {c:#x}"),
        };
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
