//! Sandboxing for the out-of-process import worker (ADR 0008 §4).
//!
//! - [`lockdown`] runs *inside* the worker before it reads untrusted bytes:
//!   - Linux: resource limits, Landlock (no filesystem and, where the kernel supports
//!     it, no TCP), and a seccomp filter that refuses process creation, sockets,
//!     ptrace and similar syscalls.
//!   - macOS: resource limits only (the sandbox profile is a known gap, see Report).
//! - [`contain_child`] runs in the *parent* on Windows: it puts the worker in a job
//!   object with a memory cap and kill-on-close, so a hostile file can at most crash or
//!   exhaust its own worker.
//!
//! Everything degrades visibly: [`Report`] lists what was and was not applied, and the
//! app shows it in Diagnostics.

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
    use landlock::{ABI, Access, AccessFs, AccessNet, Ruleset, RulesetAttr, RulesetStatus};
    let abi = ABI::V5;
    let result = Ruleset::default()
        .handle_access(AccessFs::from_all(abi))
        .and_then(|rs| rs.handle_access(AccessNet::from_all(abi)))
        .and_then(|rs| rs.create())
        .and_then(|rs| rs.restrict_self());
    match result {
        Ok(status) => match status.ruleset {
            RulesetStatus::FullyEnforced => r.applied.push("landlock (no filesystem, no TCP)".into()),
            RulesetStatus::PartiallyEnforced => {
                r.applied.push("landlock (partial: older kernel)".into());
                r.missing.push("landlock network rules (kernel too old)".into());
            }
            RulesetStatus::NotEnforced => r.missing.push("landlock (not supported by this kernel)".into()),
        },
        Err(e) => r.missing.push(format!("landlock ({e})")),
    }
}

#[cfg(target_os = "linux")]
fn linux_seccomp(r: &mut Report) {
    use seccompiler::{BpfProgram, SeccompAction, SeccompFilter, TargetArch};
    use std::collections::BTreeMap;
    // Deny-list: anything that would let a compromised parser reach outside the
    // worker. (An allow-list would be stricter but OCCT's syscall surface varies by
    // libc and version; Landlock + rlimits cover filesystem and resources.)
    let denied: &[i64] = &[
        libc::SYS_execve,
        libc::SYS_execveat,
        libc::SYS_fork,
        libc::SYS_vfork,
        libc::SYS_socket,
        libc::SYS_socketpair,
        libc::SYS_connect,
        libc::SYS_bind,
        libc::SYS_listen,
        libc::SYS_accept,
        libc::SYS_accept4,
        libc::SYS_ptrace,
        libc::SYS_process_vm_readv,
        libc::SYS_process_vm_writev,
        libc::SYS_mount,
        libc::SYS_umount2,
        libc::SYS_pivot_root,
        libc::SYS_chroot,
        libc::SYS_setns,
        libc::SYS_unshare,
        libc::SYS_kexec_load,
        libc::SYS_init_module,
        libc::SYS_finit_module,
        libc::SYS_bpf,
        libc::SYS_perf_event_open,
        libc::SYS_keyctl,
        libc::SYS_add_key,
        libc::SYS_request_key,
    ];
    let arch = if cfg!(target_arch = "x86_64") {
        TargetArch::x86_64
    } else if cfg!(target_arch = "aarch64") {
        TargetArch::aarch64
    } else {
        r.missing.push("seccomp (unsupported architecture)".into());
        return;
    };
    let rules: BTreeMap<i64, Vec<seccompiler::SeccompRule>> = denied.iter().map(|&s| (s, vec![])).collect();
    let filter = match SeccompFilter::new(rules, SeccompAction::Allow, SeccompAction::Errno(libc::EPERM as u32), arch) {
        Ok(f) => f,
        Err(e) => {
            r.missing.push(format!("seccomp ({e})"));
            return;
        }
    };
    let prog: Result<BpfProgram, _> = filter.try_into();
    match prog.map_err(|e| e.to_string()).and_then(|p| seccompiler::apply_filter(&p).map_err(|e| e.to_string())) {
        Ok(()) => r.applied.push("seccomp (no exec, sockets, ptrace)".into()),
        Err(e) => r.missing.push(format!("seccomp ({e})")),
    }
}

/// Parent-side containment of a spawned worker. On Windows this assigns the child to a
/// job object (memory cap, single process, killed when the guard drops). Elsewhere it
/// is a no-op guard (the worker sandboxes itself).
pub fn contain_child(child: &std::process::Child, limits: &Limits) -> Result<ChildGuard, String> {
    #[cfg(windows)]
    {
        win::contain(child, limits).map(|job| ChildGuard { _job: Some(job) })
    }
    #[cfg(not(windows))]
    {
        let _ = (child, limits);
        Ok(ChildGuard { _job: None })
    }
}

/// Keeps the containment alive; dropping it kills the worker on Windows.
#[derive(Debug)]
pub struct ChildGuard {
    #[cfg(windows)]
    _job: Option<win::Job>,
    #[cfg(not(windows))]
    _job: Option<()>,
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    /// Lockdown is irreversible, so it is tested in a child process: re-run this test
    /// binary with an env marker and check what the locked-down child can still do.
    #[test]
    fn lockdown_blocks_exec_and_files() {
        if std::env::var_os("GUSSET_SANDBOX_CHILD").is_some() {
            let rep = lockdown(&Limits { memory_bytes: 2 << 30, cpu_seconds: 30 });
            eprintln!("{}", rep.to_text());
            let exec = std::process::Command::new("/bin/true").status();
            let file = std::fs::read("/etc/hostname").or_else(|_| std::fs::read("/etc/passwd"));
            println!("EXEC_BLOCKED={} FILE_BLOCKED={}", exec.is_err(), file.is_err());
            return;
        }
        let exe = std::env::current_exe().unwrap();
        let out = std::process::Command::new(exe)
            .args(["--exact", "tests::lockdown_blocks_exec_and_files", "--nocapture", "--test-threads=1"])
            .env("GUSSET_SANDBOX_CHILD", "1")
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&out.stdout);
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(stdout.contains("EXEC_BLOCKED=true"), "stdout: {stdout}\nstderr: {stderr}");
        // Landlock needs kernel ≥ 5.13; containers may lack it. Only assert when applied.
        if stderr.contains("landlock (no filesystem") || stderr.contains("landlock (partial") {
            assert!(stdout.contains("FILE_BLOCKED=true"), "stdout: {stdout}\nstderr: {stderr}");
        }
    }
}
