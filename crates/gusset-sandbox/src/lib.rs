//! Sandboxing for the out-of-process import worker (ADR 0008 §4).
//!
//! - [`lockdown`] runs *inside* the worker before it reads untrusted bytes:
//!   - Linux: die-with-parent, resource limits, Landlock (no filesystem, no TCP and,
//!     where the kernel supports it, no signals or abstract UNIX sockets outside the
//!     worker), and a seccomp allow-list (see `allowed_syscalls`).
//!   - macOS: resource limits only (the sandbox profile is a known gap, see Report).
//! - [`configure_command`] and [`contain_child`] run in the *parent*: the worker gets
//!   its own process group (Unix) or a job object with a memory cap, one process and
//!   kill-on-close (Windows). [`ChildGuard::kill_all`] kills everything the worker
//!   started, so nothing outlives a timeout.
//!
//! Everything degrades visibly: [`Report`] lists what was and was not applied, and the
//! app shows it in Diagnostics.

#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod win;

/// Resource limits for one import.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    pub memory_bytes: u64,
    pub cpu_seconds: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Limits { memory_bytes: 4 << 30, cpu_seconds: 600 }
    }
}

/// What a lockdown achieved.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    pub applied: Vec<String>,
    pub missing: Vec<String>,
}

impl Report {
    pub fn to_text(&self) -> String {
        format!("sandbox applied: [{}]; missing: [{}]", self.applied.join(", "), self.missing.join(", "))
    }
}

/// Locks down the current process. Call once, early, before touching untrusted input.
pub fn lockdown(limits: &Limits) -> Report {
    let mut r = Report::default();
    #[cfg(target_os = "linux")]
    match unix::die_with_parent() {
        Ok(true) => r.applied.push("exit with parent".into()),
        // The parent died before we started: nobody will read our output.
        Ok(false) => std::process::exit(9),
        Err(e) => r.missing.push(format!("exit with parent ({e})")),
    }
    #[cfg(unix)]
    unix_limits(limits, &mut r);
    #[cfg(target_os = "linux")]
    {
        linux_landlock(&mut r);
        linux_seccomp(&mut r);
    }
    #[cfg(target_os = "macos")]
    r.missing.push("macOS sandbox profile".into());
    #[cfg(windows)]
    {
        let _ = limits;
        r.applied.push("job object (applied by the parent)".into());
        r.missing.push("AppContainer / low-integrity token".into());
    }
    r
}

#[cfg(unix)]
fn unix_limits(limits: &Limits, r: &mut Report) {
    use rlimit::Resource;
    let mut set = |res: Resource, v: u64, name: &str| match res.set(v, v) {
        Ok(()) => r.applied.push(format!("rlimit {name}")),
        Err(e) => r.missing.push(format!("rlimit {name} ({e})")),
    };
    // RLIMIT_AS is not enforced on macOS; RLIMIT_DATA is the closest equivalent.
    #[cfg(target_os = "linux")]
    set(Resource::AS, limits.memory_bytes, "address space");
    #[cfg(not(target_os = "linux"))]
    set(Resource::DATA, limits.memory_bytes, "data");
    set(Resource::CPU, limits.cpu_seconds, "cpu");
    set(Resource::CORE, 0, "core dumps");
}

#[cfg(target_os = "linux")]
fn linux_landlock(r: &mut Report) {
    use landlock::{ABI, Access, AccessFs, AccessNet, Ruleset, RulesetAttr, RulesetStatus, Scope};
    // V6 adds scoping of signals and abstract UNIX sockets to the worker's own domain.
    // Best-effort: older kernels enforce what they support, reported as partial.
    let abi = ABI::V6;
    let result = Ruleset::default()
        .handle_access(AccessFs::from_all(abi))
        .and_then(|rs| rs.handle_access(AccessNet::from_all(abi)))
        .and_then(|rs| rs.scope(Scope::from_all(abi)))
        .and_then(|rs| rs.create())
        .and_then(|rs| rs.restrict_self());
    match result {
        Ok(status) => match status.ruleset {
            RulesetStatus::FullyEnforced => {
                r.applied.push("landlock (no filesystem, no TCP, signals and abstract sockets scoped)".into());
            }
            RulesetStatus::PartiallyEnforced => {
                r.applied.push("landlock (partial: older kernel)".into());
                r.missing.push("landlock network rules or scopes (kernel too old)".into());
            }
            RulesetStatus::NotEnforced => r.missing.push("landlock (not supported by this kernel)".into()),
        },
        Err(e) => r.missing.push(format!("landlock ({e})")),
    }
}

/// Syscalls the worker may make after lockdown (an allow-list; everything else fails
/// with `EPERM`). Measured with `strace -f` on the real worker importing OCCT's sample
/// STEP/IGES files and validating B-rep blobs (ADR 0008 §4): after lockdown it only
/// reads stdin, allocates, and writes stdout. Entries with argument filters:
/// - `clone` only with `CLONE_THREAD` and no namespace flags (threads, never processes);
///   `clone3` (whose flags live in memory seccomp cannot read) fails with `ENOSYS`, which
///   makes glibc fall back to `clone`.
/// - `kill`/`tgkill` only to our own pid (`abort()`), never to other processes.
/// - `mmap`/`mprotect` never with `PROT_EXEC` (no new code after lockdown).
/// - `ioctl` only `TCGETS` (isatty), `prctl` only `PR_SET_NAME`/`PR_GET_NAME`.
///
/// `open`/`openat` fail with `EACCES` even where Landlock is unavailable. Syscall numbers
/// with the x32 bit (`0x4000_0000`) are not on the list, so the x32 ABI is denied too.
#[cfg(target_os = "linux")]
fn allowed_syscalls(pid: u32) -> Result<std::collections::BTreeMap<i64, Vec<seccompiler::SeccompRule>>, String> {
    use seccompiler::{SeccompCmpArgLen as Len, SeccompCmpOp as Op, SeccompCondition as Cond, SeccompRule as Rule};
    let e = |e: seccompiler::BackendError| e.to_string();
    let cond = |arg: u8, op: Op, v: u64| Cond::new(arg, Len::Qword, op, v).map_err(e);
    let rule = |c: Vec<Cond>| Rule::new(c).map_err(e);
    let unconditional: &[i64] = &[
        libc::SYS_read,
        libc::SYS_readv,
        libc::SYS_pread64,
        libc::SYS_write,
        libc::SYS_writev,
        libc::SYS_close,
        libc::SYS_lseek,
        libc::SYS_fstat,
        libc::SYS_newfstatat,
        libc::SYS_statx,
        libc::SYS_fcntl,
        libc::SYS_brk,
        libc::SYS_munmap,
        libc::SYS_mremap,
        libc::SYS_madvise,
        libc::SYS_futex,
        libc::SYS_set_robust_list,
        libc::SYS_rseq,
        libc::SYS_sched_yield,
        libc::SYS_sched_getaffinity,
        libc::SYS_getpid,
        libc::SYS_gettid,
        libc::SYS_getuid,
        libc::SYS_geteuid,
        libc::SYS_getgid,
        libc::SYS_getegid,
        libc::SYS_getppid,
        libc::SYS_times,
        libc::SYS_getrusage,
        libc::SYS_clock_gettime,
        libc::SYS_clock_getres,
        libc::SYS_clock_nanosleep,
        libc::SYS_nanosleep,
        libc::SYS_gettimeofday,
        libc::SYS_getrandom,
        libc::SYS_rt_sigaction,
        libc::SYS_rt_sigprocmask,
        libc::SYS_rt_sigreturn,
        libc::SYS_sigaltstack,
        libc::SYS_prlimit64,
        libc::SYS_uname,
        libc::SYS_sysinfo,
        libc::SYS_ppoll,
        libc::SYS_exit,
        libc::SYS_exit_group,
        // Refused with a specific errno by the earlier filters (see `linux_seccomp`).
        libc::SYS_clone3,
        libc::SYS_openat,
        #[cfg(target_arch = "x86_64")]
        libc::SYS_open,
        #[cfg(target_arch = "x86_64")]
        libc::SYS_poll,
        #[cfg(target_arch = "x86_64")]
        libc::SYS_stat,
        #[cfg(target_arch = "x86_64")]
        libc::SYS_lstat,
        #[cfg(target_arch = "x86_64")]
        libc::SYS_getrlimit,
    ];
    let mut m: std::collections::BTreeMap<i64, Vec<Rule>> = unconditional.iter().map(|&s| (s, vec![])).collect();
    let ns_flags = (libc::CLONE_NEWNS
        | libc::CLONE_NEWCGROUP
        | libc::CLONE_NEWUTS
        | libc::CLONE_NEWIPC
        | libc::CLONE_NEWUSER
        | libc::CLONE_NEWPID
        | libc::CLONE_NEWNET) as u64;
    let thread = libc::CLONE_THREAD as u64;
    m.insert(libc::SYS_clone, vec![rule(vec![cond(0, Op::MaskedEq(thread | ns_flags), thread)?])?]);
    let pid = u64::from(pid);
    m.insert(libc::SYS_kill, vec![rule(vec![cond(0, Op::Eq, pid)?])?]);
    m.insert(libc::SYS_tgkill, vec![rule(vec![cond(0, Op::Eq, pid)?])?]);
    let no_exec = || cond(2, Op::MaskedEq(libc::PROT_EXEC as u64), 0);
    m.insert(libc::SYS_mmap, vec![rule(vec![no_exec()?])?]);
    m.insert(libc::SYS_mprotect, vec![rule(vec![no_exec()?])?]);
    // `TCGETS` is `c_ulong` on glibc and `c_int` on musl.
    #[allow(clippy::unnecessary_cast)]
    let tcgets = libc::TCGETS as u64;
    m.insert(libc::SYS_ioctl, vec![rule(vec![cond(1, Op::Eq, tcgets)?])?]);
    m.insert(
        libc::SYS_prctl,
        vec![
            rule(vec![cond(0, Op::Eq, libc::PR_SET_NAME as u64)?])?,
            rule(vec![cond(0, Op::Eq, libc::PR_GET_NAME as u64)?])?,
        ],
    );
    Ok(m)
}

#[cfg(target_os = "linux")]
fn linux_seccomp(r: &mut Report) {
    use seccompiler::{BpfProgram, SeccompAction, SeccompFilter, TargetArch};
    use std::collections::BTreeMap;
    let arch = if cfg!(target_arch = "x86_64") {
        TargetArch::x86_64
    } else if cfg!(target_arch = "aarch64") {
        TargetArch::aarch64
    } else {
        r.missing.push("seccomp (unsupported architecture)".into());
        return;
    };
    let build = |rules, mismatch, matched| -> Result<BpfProgram, String> {
        SeccompFilter::new(rules, mismatch, matched, arch)
            .map_err(|e| e.to_string())?
            .try_into()
            .map_err(|e: seccompiler::BackendError| e.to_string())
    };
    // Errno filters first: each fails one call with a specific errno and allows the
    // rest. An errno result outranks "allow" whichever filter returns it, so these hold
    // although the allow-list (installed last, because it refuses the prctl/seccomp
    // calls needed to install more filters) allows these calls.
    let single = |sys: i64, errno: i32| {
        build(BTreeMap::from([(sys, vec![])]), SeccompAction::Allow, SeccompAction::Errno(errno as u32))
    };
    #[allow(unused_mut)]
    let mut filters = vec![single(libc::SYS_clone3, libc::ENOSYS), single(libc::SYS_openat, libc::EACCES)];
    #[cfg(target_arch = "x86_64")]
    filters.push(single(libc::SYS_open, libc::EACCES));
    // Then the allow-list; anything not on it fails with EPERM.
    filters.push(
        allowed_syscalls(std::process::id())
            .and_then(|rules| build(rules, SeccompAction::Errno(libc::EPERM as u32), SeccompAction::Allow)),
    );
    let result = filters
        .into_iter()
        .try_for_each(|p| p.and_then(|p| seccompiler::apply_filter_all_threads(&p).map_err(|e| e.to_string())));
    match result {
        Ok(()) => r.applied.push("seccomp allow-list (no exec, processes, sockets, files, signals to others)".into()),
        Err(e) => r.missing.push(format!("seccomp ({e})")),
    }
}

/// Prepares the worker command before spawning: on Unix the worker leads a new process
/// group, so [`ChildGuard::kill_all`] can reach anything it forks.
pub fn configure_command(cmd: &mut std::process::Command) {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    #[cfg(not(unix))]
    let _ = cmd;
}

/// Parent-side containment of a spawned worker. On Windows this assigns the child to a
/// job object (memory cap, single process, killed when the guard drops). On Unix it
/// remembers the worker's process group (see [`configure_command`]).
pub fn contain_child(child: &std::process::Child, limits: &Limits) -> Result<ChildGuard, String> {
    #[cfg(windows)]
    {
        win::contain(child, limits).map(|job| ChildGuard { job })
    }
    #[cfg(not(windows))]
    {
        let _ = limits;
        Ok(ChildGuard { pgid: child.id() })
    }
}

/// Keeps the containment alive; dropping it kills the worker on Windows.
#[derive(Debug)]
pub struct ChildGuard {
    #[cfg(windows)]
    job: win::Job,
    #[cfg(not(windows))]
    pgid: u32,
}

impl ChildGuard {
    /// Kills the worker and everything it started. Call it *before* reaping the worker
    /// (`Child::wait`): until then the worker's pid, and so its process group id, cannot
    /// be reused by an unrelated process. On Windows closing the job (dropping the guard)
    /// does the same; this also terminates the job's processes immediately.
    pub fn kill_all(&self) {
        #[cfg(unix)]
        let _ = unix::kill_group(self.pgid);
        #[cfg(windows)]
        self.job.terminate();
        #[cfg(not(any(unix, windows)))]
        let _ = self.pgid;
    }
}

/// Whether `child` has exited. On Linux this does not reap it (see
/// [`ChildGuard::kill_all`]); elsewhere it falls back to `try_wait`.
pub fn has_exited(child: &mut std::process::Child) -> std::io::Result<bool> {
    #[cfg(target_os = "linux")]
    {
        unix::exited_without_reaping(child.id())
    }
    #[cfg(not(target_os = "linux"))]
    {
        child.try_wait().map(|s| s.is_some())
    }
}

#[cfg(all(test, target_os = "linux"))]
#[allow(unsafe_code, clippy::unwrap_used, clippy::undocumented_unsafe_blocks)]
mod tests {
    use super::*;
    use std::io::Read;
    use std::time::{Duration, Instant};

    /// Runs `probe` in a locked-down child: lockdown is irreversible, so this re-runs the
    /// test binary with an env marker. Returns (stdout, stderr) of the child.
    fn in_locked_child(test: &str, probe: impl FnOnce()) -> Option<(String, String)> {
        if std::env::var_os("GUSSET_SANDBOX_CHILD").is_some() {
            let rep = lockdown(&Limits { memory_bytes: 2 << 30, cpu_seconds: 30 });
            eprintln!("{}", rep.to_text());
            probe();
            return None;
        }
        let exe = std::env::current_exe().unwrap();
        let out = std::process::Command::new(exe)
            .args(["--exact", test, "--nocapture", "--test-threads=1"])
            .env("GUSSET_SANDBOX_CHILD", "1")
            .output()
            .unwrap();
        Some((String::from_utf8_lossy(&out.stdout).into_owned(), String::from_utf8_lossy(&out.stderr).into_owned()))
    }

    fn errno() -> i32 {
        std::io::Error::last_os_error().raw_os_error().unwrap_or(0)
    }

    #[test]
    fn lockdown_blocks_exec_and_files() {
        let Some((stdout, stderr)) = in_locked_child("tests::lockdown_blocks_exec_and_files", || {
            let exec = std::process::Command::new("/bin/true").status();
            let file = std::fs::read("/etc/hostname").or_else(|_| std::fs::read("/etc/passwd"));
            println!("EXEC_BLOCKED={} FILE_BLOCKED={}", exec.is_err(), file.is_err());
        }) else {
            return;
        };
        assert!(stdout.contains("EXEC_BLOCKED=true"), "stdout: {stdout}\nstderr: {stderr}");
        // seccomp refuses open/openat even where Landlock is unavailable.
        assert!(stdout.contains("FILE_BLOCKED=true"), "stdout: {stdout}\nstderr: {stderr}");
    }

    /// Security review #6 / #14: processes, user namespaces, io_uring, signals to other
    /// processes, executable memory and the x32 ABI are refused; threads still work.
    #[test]
    fn lockdown_refuses_processes_namespaces_uring_and_signals() {
        let Some((stdout, stderr)) =
            in_locked_child("tests::lockdown_refuses_processes_namespaces_uring_and_signals", || {
                let mut res = Vec::new();
                let pid = unsafe { libc::fork() };
                if pid == 0 {
                    unsafe { libc::_exit(0) };
                }
                res.push(format!("fork={}", pid < 0));
                let flags = (libc::CLONE_NEWUSER | libc::SIGCHLD) as libc::c_long;
                let r = unsafe { libc::syscall(libc::SYS_clone, flags, 0, 0, 0, 0) };
                if r == 0 {
                    unsafe { libc::_exit(0) };
                }
                res.push(format!("clone_newuser={}", r < 0));
                let r = unsafe { libc::syscall(libc::SYS_clone3, 0usize, 0usize) };
                res.push(format!("clone3_enosys={}", r < 0 && errno() == libc::ENOSYS));
                let mut params = [0u8; 120];
                let r = unsafe { libc::syscall(libc::SYS_io_uring_setup, 1, params.as_mut_ptr()) };
                res.push(format!("io_uring={}", r < 0));
                let parent = unsafe { libc::getppid() };
                let r = unsafe { libc::kill(parent, 0) };
                res.push(format!("kill_other={}", r < 0 && errno() == libc::EPERM));
                res.push(format!("kill_group={}", unsafe { libc::kill(0, 0) } < 0));
                res.push(format!("kill_self_ok={}", unsafe { libc::kill(libc::getpid(), 0) } == 0));
                let p = unsafe {
                    libc::mmap(
                        std::ptr::null_mut(),
                        4096,
                        libc::PROT_READ | libc::PROT_EXEC,
                        libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
                        -1,
                        0,
                    )
                };
                res.push(format!("mmap_exec={}", p == libc::MAP_FAILED));
                res.push(format!("socket={}", unsafe { libc::socket(libc::AF_UNIX, libc::SOCK_STREAM, 0) } < 0));
                #[cfg(target_arch = "x86_64")]
                {
                    let r = unsafe { libc::syscall(0x4000_0000 | libc::SYS_getpid) };
                    res.push(format!("x32={}", r < 0 && errno() == libc::EPERM));
                }
                let t = std::thread::spawn(|| 41 + 1).join();
                res.push(format!("threads_ok={}", t.ok() == Some(42)));
                println!("PROBES {}", res.join(" "));
            })
        else {
            return;
        };
        assert!(stderr.contains("seccomp allow-list"), "stderr: {stderr}");
        let line = stdout
            .lines()
            .find_map(|l| l.find("PROBES ").and_then(|i| l.get(i..)))
            .unwrap_or_else(|| panic!("{stdout}\n{stderr}"));
        for (k, _) in line.split_whitespace().skip(1).filter_map(|kv| kv.split_once('=')) {
            assert!(line.contains(&format!("{k}=true")), "{k} not refused/allowed as expected: {line}\n{stderr}");
        }
        assert!(line.contains("threads_ok=true") && line.contains("kill_other=true"), "{line}");
    }

    /// Security review #6: a grandchild holding the worker's stdout must not outlive
    /// `kill_all`, so reading the pipe finishes promptly.
    #[test]
    fn kill_all_reaches_grandchildren() {
        let mut cmd = std::process::Command::new("/bin/sh");
        cmd.args(["-c", "sleep 30 & sleep 30"]).stdout(std::process::Stdio::piped());
        configure_command(&mut cmd);
        let mut child = cmd.spawn().unwrap();
        let guard = contain_child(&child, &Limits::default()).unwrap();
        let mut out = child.stdout.take().unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut v = Vec::new();
            let _ = out.read_to_end(&mut v);
            let _ = tx.send(());
        });
        std::thread::sleep(Duration::from_millis(200));
        assert!(!has_exited(&mut child).unwrap());
        let t = Instant::now();
        guard.kill_all();
        assert!(has_exited(&mut child).is_ok());
        let _ = child.wait();
        rx.recv_timeout(Duration::from_secs(5)).expect("a grandchild kept the pipe open");
        assert!(t.elapsed() < Duration::from_secs(5));
    }
}
