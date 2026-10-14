//! The app side of the kernel worker process (ADR 0022): starts `gusset-kernel-worker`
//! sandboxed and contained (process group / job object, empty environment, memory budget),
//! talks to it over its stdin/stdout, and turns its death into a readable reason.
//!
//! Use [`production_kernel`], or [`connector`] with [`RemoteKernel::new`].

use gusset_kernel::Kernel;
use gusset_kernel_remote::{
    Connect, Interrupt, NativeGate, PoolKernel, RemoteKernel, StreamTransport, Transport, TransportError,
};
use std::borrow::Cow;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, ExitStatus, Stdio};
use std::sync::{Arc, Mutex};

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
    guard: Arc<Mutex<Guard>>,
    stream: StreamTransport<ChildStdin, BufReader<ChildStdout>>,
}

/// The containment, shared with [`Transport::interrupter`]: killing goes through it only
/// until the worker is reaped (after that its process group id may be reused).
#[derive(Debug)]
struct Guard {
    guard: gusset_sandbox::ChildGuard,
    reaped: bool,
}

impl Guard {
    fn kill(&self) {
        if !self.reaped {
            self.guard.kill_all();
        }
    }
}

fn lock(g: &Mutex<Guard>) -> std::sync::MutexGuard<'_, Guard> {
    g.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
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
        Ok(KernelProcess {
            child,
            guard: Arc::new(Mutex::new(Guard { guard, reaped: false })),
            stream: StreamTransport::new(stdin, BufReader::new(stdout)),
        })
    }

    /// Kills the worker (and anything it started) and returns how it ended.
    fn reap(&mut self) -> String {
        // A worker that broke its stream on purpose has usually exited already; give it a
        // moment so its real exit status (signal, code) is reported rather than our kill.
        let mut exited = false;
        for _ in 0..50 {
            if matches!(gusset_sandbox::has_exited(&mut self.child), Ok(true)) {
                exited = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        let mut g = lock(&self.guard);
        // Outside Linux `has_exited` reaps the worker, after which its process group id may
        // be reused: no group kill then (the worker starts no processes of its own).
        g.reaped |= exited && !cfg!(target_os = "linux");
        g.kill();
        let _ = self.child.kill();
        let r = self.child.wait();
        g.reaped = true;
        drop(g);
        match r {
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

    fn is_alive(&mut self) -> bool {
        let exited = matches!(gusset_sandbox::has_exited(&mut self.child), Ok(true));
        if exited && !cfg!(target_os = "linux") {
            lock(&self.guard).reaped = true;
        }
        !exited
    }

    fn interrupter(&self) -> Option<Interrupt> {
        let g = Arc::clone(&self.guard);
        Some(Arc::new(move || lock(&g).kill()))
    }
}

impl Drop for KernelProcess {
    fn drop(&mut self) {
        let mut g = lock(&self.guard);
        g.kill();
        let _ = self.child.kill();
        let _ = self.child.wait();
        g.reaped = true;
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

type SpawnReply = std::sync::mpsc::Sender<Result<KernelProcess, String>>;

/// A [`Connect`] that starts the worker at `worker` with `memory_bytes` each time the
/// kernel needs a new one.
///
/// Workers are started from one thread that lives as long as the connector: on Linux the
/// sandbox ends a worker when the *thread* that started it exits (`PR_SET_PDEATHSIG`), so
/// a worker started from a short-lived thread (a project's model thread) would be killed
/// with it (review H1).
#[must_use]
pub fn connector(worker: PathBuf, memory_bytes: u64) -> Connect {
    let (tx, rx) = std::sync::mpsc::channel::<SpawnReply>();
    let path = worker.clone();
    let spawner = std::thread::Builder::new().name("gusset-kernel-spawn".into()).spawn(move || {
        while let Ok(reply) = rx.recv() {
            let _ = reply.send(KernelProcess::spawn(&path, memory_bytes));
        }
    });
    let tx = Mutex::new(spawner.is_ok().then_some(tx));
    Box::new(move || {
        let via_spawner = lock_any(&tx).as_ref().and_then(|tx| {
            let (reply, result) = std::sync::mpsc::channel();
            tx.send(reply).ok()?;
            result.recv().ok()
        });
        let p = match via_spawner {
            Some(r) => r?,
            None => KernelProcess::spawn(&worker, memory_bytes)?,
        };
        Ok(Box::new(p) as Box<dyn Transport>)
    })
}

fn lock_any<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Vets B-rep blobs in the app before the worker parses them, exactly as the in-process
/// kernel does (ADR 0008 §4): trusted when this process wrote or validated them, else
/// re-serialized by the sandboxed import worker first.
///
/// Blobs the kernel worker wrote may go back to it as they are, but they are not added to
/// the app's trust list: the worker is a sandbox whose output the app never parses itself
/// (review L3).
#[derive(Debug)]
pub struct OcctGate {
    import_worker: Option<PathBuf>,
    worker_written: Mutex<std::collections::HashSet<[u8; 32]>>,
}

/// Most remembered worker-written blobs; the oldest are forgotten (then vetted again).
const MAX_WORKER_WRITTEN: usize = 4096;

impl OcctGate {
    /// With the import worker installed next to the current executable, if any.
    #[must_use]
    pub fn next_to_exe() -> OcctGate {
        OcctGate {
            import_worker: gusset_worker::default_worker_path().filter(|p| p.is_file()),
            worker_written: Mutex::new(std::collections::HashSet::new()),
        }
    }
}

impl NativeGate for OcctGate {
    fn written(&self, bytes: &[u8]) {
        let mut set = lock_any(&self.worker_written);
        if set.len() >= MAX_WORKER_WRITTEN {
            set.clear();
        }
        set.insert(*blake3::hash(bytes).as_bytes());
    }

    fn vet<'a>(&self, bytes: &'a [u8]) -> gusset_kernel::KResult<Cow<'a, [u8]>> {
        if lock_any(&self.worker_written).contains(blake3::hash(bytes).as_bytes()) {
            return Ok(Cow::Borrowed(bytes));
        }
        gusset_kernel_occt::vet_native(self.import_worker.as_deref(), bytes)
    }
}

/// Kernel workers for parallel rebuilds (ADR 0024 §3): one per interactive thread (a
/// rebuild is work the user waits for), each with at least 2 GiB of the memory budget,
/// at most 4.
fn pool_size(cpu: &gusset_hw::CpuInfo, budget: u64) -> usize {
    let threads = gusset_hw::budgets::interactive_threads(cpu.logical_cores);
    let by_memory = usize::try_from(budget / (2 << 30)).unwrap_or(1);
    threads.min(by_memory).clamp(1, 4)
}

/// How long the first start of the worker may take before the app falls back.
const PING_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(20);

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
                let cpu = gusset_hw::CpuInfo::detect();
                let budget = gusset_hw::budgets::kernel_worker_bytes(cpu.total_ram_bytes);
                let workers = pool_size(&cpu, budget);
                // One gate for all workers: a shape one worker wrote may go to another.
                let gate = Arc::new(OcctGate::next_to_exe());
                let member = |memory: u64| {
                    RemoteKernel::new("occt", connector(worker.clone(), memory))
                        .with_native_gate(Box::new(gate.clone()))
                };
                let remote = member(budget / workers as u64);
                // A worker that hangs on start must not hang the app: stop it after a while.
                let flag = Arc::new(std::sync::atomic::AtomicBool::new(false));
                let timer = Arc::clone(&flag);
                let _ = std::thread::Builder::new().name("gusset-kernel-ping".into()).spawn(move || {
                    std::thread::sleep(PING_TIMEOUT);
                    timer.store(true, std::sync::atomic::Ordering::SeqCst);
                });
                let pinged = gusset_kernel::cancel::scope(&flag, || remote.ping());
                flag.store(false, std::sync::atomic::Ordering::SeqCst);
                match pinged {
                    // The other workers start when first used.
                    Ok(()) if workers > 1 => {
                        let mut members = vec![remote];
                        members.extend((1..workers).map(|_| member(budget / workers as u64)));
                        return Ok((Arc::new(PoolKernel::new("occt", members)), None));
                    }
                    Ok(()) => return Ok((Arc::new(remote), None)),
                    Err(e) => format!("the kernel worker did not start: {}", e.message),
                }
            }
        }
    };
    let k = gusset_kernel_occt::OcctKernel::new().map_err(|e| e.to_string())?;
    Ok((Arc::new(k), Some(format!("{why}; the geometry kernel runs inside this process, so a crash in it closes it"))))
}
