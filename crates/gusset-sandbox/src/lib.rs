//! Sandboxing for the out-of-process import worker (ADR 0008 §4).
//!
//! - [`lockdown`] runs *inside* the worker before it reads untrusted bytes:
//!   - Linux: die-with-parent, resource limits, Landlock (no filesystem, no TCP and,
//!     where the kernel supports it, no signals or abstract UNIX sockets outside the
//!     worker), and a seccomp allow-list (see `allowed_syscalls`).
//!   - Optionally ([`Limits::read_file`]) read access to exactly one file, for formats
//!     whose reader needs a file name (IGES): Landlock grants `ReadFile` on that file and
//!     seccomp then allows read-only `openat`. Without enforced Landlock `openat` stays
//!     denied and [`Report::read_file`] is `None`.
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Limits {
    pub memory_bytes: u64,
    pub cpu_seconds: u64,
    /// One file the worker may open read-only (IGES input: OCCT's IGES reader only
    /// reads from a file name). On Linux this needs Landlock; see [`lockdown`].
    pub read_file: Option<std::path::PathBuf>,
}

impl Default for Limits {
    fn default() -> Self {
        Limits { memory_bytes: 4 << 30, cpu_seconds: 600, read_file: None }
    }
}

impl Limits {
    /// These limits plus a read-only allowance for exactly `path`.
    #[must_use]
    pub fn with_read_file(mut self, path: std::path::PathBuf) -> Self {
        self.read_file = Some(path);
        self
    }
}

/// What a lockdown achieved.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    pub applied: Vec<String>,
    pub missing: Vec<String>,
    /// The single file the worker may read, if one was requested *and* the sandbox
    /// could restrict it to that file (Linux: Landlock enforced). `None` means opening
    /// files stays denied.
    pub read_file: Option<std::path::PathBuf>,
}

impl Report {
    pub fn to_text(&self) -> String {
        let file = match &self.read_file {
            Some(p) => format!("; read-only file allowance: {}", p.display()),
            None => String::new(),
        };
        format!("sandbox applied: [{}]; missing: [{}]{file}", self.applied.join(", "), self.missing.join(", "))
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
        // openat is only ever allowed when Landlock confines it to the one file.
        let granted = match limits.read_file.as_deref() {
            Some(path) => linux_landlock_with_file(path, &mut r),
            None => {
                linux_landlock(&mut r);
                false
            }
        };
        linux_seccomp(granted, &mut r);
        if granted {
            r.read_file.clone_from(&limits.read_file);
        }
    }
    #[cfg(target_os = "macos")]
    r.missing.push("macOS sandbox profile".into());
    #[cfg(windows)]
    {
        r.applied.push("job object (applied by the parent)".into());
        r.missing.push("AppContainer / low-integrity token".into());
    }
    // No filesystem sandbox exists on these platforms (see `missing`): the worker simply
    // reads the file with the parent's file permissions.
    #[cfg(not(target_os = "linux"))]
    if let Some(p) = &limits.read_file {
        r.read_file = Some(p.clone());
        r.missing.push("file allowance is not enforced (no filesystem sandbox on this platform)".into());
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

/// Landlock for a worker that may read exactly one file. The filesystem rights of
/// Landlock ABI V1 (kernel 5.13+) are a hard requirement: if the kernel cannot enforce
/// them, nothing is restricted here and `false` is returned, so the caller keeps `openat`
/// denied. Newer filesystem rights are best-effort; TCP and scopes are applied in a
/// second ruleset so that their partial support does not hide whether the filesystem
/// part is enforced.
#[cfg(target_os = "linux")]
fn linux_landlock_with_file(path: &std::path::Path, r: &mut Report) -> bool {
    use landlock::{
        ABI, Access, AccessFs, AccessNet, CompatLevel, Compatible, PathBeneath, PathFd, Ruleset, RulesetAttr,
        RulesetCreatedAttr, RulesetStatus, Scope,
    };
    let fs = PathFd::new(path).map_err(|e| e.to_string()).and_then(|fd| {
        Ruleset::default()
            .set_compatibility(CompatLevel::HardRequirement)
            .handle_access(AccessFs::from_all(ABI::V1))
            .and_then(|rs| rs.set_compatibility(CompatLevel::BestEffort).handle_access(AccessFs::from_all(ABI::V6)))
            .and_then(|rs| rs.create())
            .and_then(|rs| {
                rs.set_compatibility(CompatLevel::HardRequirement).add_rule(PathBeneath::new(fd, AccessFs::ReadFile))
            })
            .and_then(|rs| rs.restrict_self())
            .map_err(|e| e.to_string())
    });
    let granted = match fs {
        Ok(st) if st.ruleset != RulesetStatus::NotEnforced => {
            r.applied.push("landlock (no filesystem except read access to one input file)".into());
            true
        }
        Ok(_) => {
            r.missing.push("landlock (not supported by this kernel)".into());
            false
        }
        Err(e) => {
            r.missing.push(format!("landlock filesystem ({e})"));
            false
        }
    };
    if !granted {
        return false;
    }
    let abi = ABI::V6;
    let net = Ruleset::default()
        .handle_access(AccessNet::from_all(abi))
        .and_then(|rs| rs.scope(Scope::from_all(abi)))
        .and_then(|rs| rs.create())
        .and_then(|rs| rs.restrict_self());
    match net {
        Ok(st) if st.ruleset == RulesetStatus::FullyEnforced => {
            r.applied.push("landlock (no TCP, signals and abstract sockets scoped)".into());
        }
        Ok(_) => r.missing.push("landlock network rules or scopes (kernel too old)".into()),
        Err(e) => r.missing.push(format!("landlock network rules or scopes ({e})")),
    }
    true
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
/// `open`/`openat` fail with `EACCES` even where Landlock is unavailable. Only with
/// `allow_open` (Landlock confines the worker to one read-only file) are they allowed,
/// and then only without write, create, truncate or tmpfile flags. Syscall numbers
/// with the x32 bit (`0x4000_0000`) are not on the list, so the x32 ABI is denied too.
#[cfg(target_os = "linux")]
fn allowed_syscalls(
    pid: u32,
    allow_open: bool,
) -> Result<std::collections::BTreeMap<i64, Vec<seccompiler::SeccompRule>>, String> {
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
    if !allow_open {
        // Listed so that the allow-list returns "allow" and the `EACCES` filter decides.
        m.insert(libc::SYS_openat, vec![]);
        #[cfg(target_arch = "x86_64")]
        m.insert(libc::SYS_open, vec![]);
    } else {
        let ro = (libc::O_ACCMODE | libc::O_CREAT | libc::O_TRUNC | libc::O_TMPFILE) as u64;
        let flags = |arg: u8| Cond::new(arg, Len::Dword, Op::MaskedEq(ro), 0).map_err(e).and_then(|c| rule(vec![c]));
        m.insert(libc::SYS_openat, vec![flags(2)?]);
        #[cfg(target_arch = "x86_64")]
        m.insert(libc::SYS_open, vec![flags(1)?]);
    }
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
fn linux_seccomp(allow_open: bool, r: &mut Report) {
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
    let mut filters = vec![single(libc::SYS_clone3, libc::ENOSYS)];
    if !allow_open {
        filters.push(single(libc::SYS_openat, libc::EACCES));
        #[cfg(target_arch = "x86_64")]
        filters.push(single(libc::SYS_open, libc::EACCES));
    }
    // Then the allow-list; anything not on it fails with EPERM.
    filters.push(
        allowed_syscalls(std::process::id(), allow_open)
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
        in_locked_child_with(test, &[], |_| probe())
    }

    /// Like [`in_locked_child`], with extra environment for the child. The path in
    /// `GUSSET_SANDBOX_FILE`, if set, becomes the read-only file allowance. The probe
    /// gets the lockdown report.
    fn in_locked_child_with(
        test: &str,
        envs: &[(&str, &std::path::Path)],
        probe: impl FnOnce(&Report),
    ) -> Option<(String, String)> {
        if std::env::var_os("GUSSET_SANDBOX_CHILD").is_some() {
            let read_file = std::env::var_os("GUSSET_SANDBOX_FILE").map(std::path::PathBuf::from);
            let rep = lockdown(&Limits { memory_bytes: 2 << 30, cpu_seconds: 30, read_file });
            eprintln!("{}", rep.to_text());
            probe(&rep);
            return None;
        }
        let exe = std::env::current_exe().unwrap();
        let mut cmd = std::process::Command::new(exe);
        cmd.args(["--exact", test, "--nocapture", "--test-threads=1"]).env("GUSSET_SANDBOX_CHILD", "1");
        for (k, v) in envs {
            cmd.env(k, v);
        }
        let out = cmd.output().unwrap();
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

    fn temp_dir(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("gusset-sandbox-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// The IGES allowance: exactly one file is readable, read-only; nothing else on the
    /// filesystem is; the report lists it. Where Landlock is unavailable the allowance
    /// must be refused (no silent open).
    #[test]
    fn read_file_allowance_opens_only_that_file() {
        use std::io::Write;
        let dir = temp_dir("allow");
        let allowed = dir.join("input.igs");
        let other = dir.join("other.txt");
        std::fs::write(&allowed, b"hello").unwrap();
        std::fs::write(&other, b"secret").unwrap();
        let envs: [(&str, &std::path::Path); 2] = [("GUSSET_SANDBOX_FILE", &allowed), ("GUSSET_SANDBOX_OTHER", &other)];
        let res = in_locked_child_with("tests::read_file_allowance_opens_only_that_file", &envs, |rep| {
            let get = |k: &str| std::path::PathBuf::from(std::env::var_os(k).unwrap());
            let (file, other) = (get("GUSSET_SANDBOX_FILE"), get("GUSSET_SANDBOX_OTHER"));
            println!("GRANTED={}", rep.read_file.as_deref() == Some(file.as_path()));
            println!("READ_ALLOWED={}", std::fs::read(&file).ok().as_deref() == Some(b"hello".as_slice()));
            println!("OTHER_BLOCKED={}", std::fs::read(&other).is_err());
            println!("WRITE_BLOCKED={}", std::fs::OpenOptions::new().write(true).open(&file).is_err());
            println!("APPEND_BLOCKED={}", std::fs::OpenOptions::new().append(true).open(&file).is_err());
            let new = other.with_file_name("new.txt");
            println!("CREATE_BLOCKED={}", std::fs::File::create(&new).and_then(|mut f| f.write_all(b"x")).is_err());
            println!("DIR_BLOCKED={}", std::fs::read_dir(other.parent().unwrap()).is_err());
            println!("NEW_ABSENT={}", !new.exists());
        });
        let _ = std::fs::remove_dir_all(&dir);
        let Some((stdout, stderr)) = res else { return };
        if stdout.contains("GRANTED=false") {
            assert!(
                stdout.contains("READ_ALLOWED=false") && stdout.contains("OTHER_BLOCKED=true"),
                "{stdout}\n{stderr}"
            );
            assert!(!stderr.contains("read-only file allowance"), "{stderr}");
            return;
        }
        for k in [
            "GRANTED",
            "READ_ALLOWED",
            "OTHER_BLOCKED",
            "WRITE_BLOCKED",
            "APPEND_BLOCKED",
            "CREATE_BLOCKED",
            "DIR_BLOCKED",
            "NEW_ABSENT",
        ] {
            assert!(stdout.contains(&format!("{k}=true")), "{k}: {stdout}\n{stderr}");
        }
        assert!(stderr.contains("read-only file allowance"), "the report must list the file: {stderr}");
    }

    /// Without the option `openat` stays denied (EACCES), whatever file is named.
    #[test]
    fn openat_stays_denied_without_the_option() {
        let dir = temp_dir("deny");
        let f = dir.join("input.igs");
        std::fs::write(&f, b"hello").unwrap();
        let res = in_locked_child_with(
            "tests::openat_stays_denied_without_the_option",
            &[("GUSSET_SANDBOX_TARGET", &f)],
            |rep| {
                let t = std::env::var_os("GUSSET_SANDBOX_TARGET").unwrap();
                let r = std::fs::read(t);
                let e = r.as_ref().err().and_then(std::io::Error::raw_os_error);
                println!(
                    "DENIED={} EACCES={} NO_ALLOWANCE={}",
                    r.is_err(),
                    e == Some(libc::EACCES),
                    rep.read_file.is_none()
                );
            },
        );
        let _ = std::fs::remove_dir_all(&dir);
        let Some((stdout, stderr)) = res else { return };
        assert!(stdout.contains("DENIED=true EACCES=true NO_ALLOWANCE=true"), "{stdout}\n{stderr}");
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
