//! Unix process FFI for the worker sandbox: parent-death signal, process-group kill and
//! non-reaping exit checks. The only `unsafe` on Unix in this crate (ADR 0008).

#![allow(unsafe_code)]

use std::io;

/// Asks the kernel to SIGKILL this process when its parent dies (Linux). Returns false
/// if the parent is already gone (we were reparented before the call took effect).
#[cfg(target_os = "linux")]
pub(crate) fn die_with_parent() -> io::Result<bool> {
    // SAFETY: PR_SET_PDEATHSIG takes a signal number and ignores the remaining args.
    let rc = unsafe { libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL as libc::c_ulong, 0, 0, 0) };
    if rc != 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: getppid has no preconditions.
    let ppid = unsafe { libc::getppid() };
    Ok(ppid != 1)
}

/// SIGKILLs every process in process group `pgid`. A missing group is not an error.
pub(crate) fn kill_group(pgid: u32) -> io::Result<()> {
    let pgid = libc::pid_t::try_from(pgid).map_err(io::Error::other)?;
    if pgid <= 1 {
        return Err(io::Error::other("refusing to signal process group <= 1"));
    }
    // SAFETY: killpg only takes integers; a positive pgid never addresses "all processes".
    let rc = unsafe { libc::killpg(pgid, libc::SIGKILL) };
    if rc != 0 {
        let e = io::Error::last_os_error();
        if e.raw_os_error() != Some(libc::ESRCH) {
            return Err(e);
        }
    }
    Ok(())
}

/// Whether child `pid` has exited, *without* reaping it: while the zombie exists its pid
/// (and so its process-group id) cannot be reused, so a later [`kill_group`] is safe.
#[cfg(target_os = "linux")]
pub(crate) fn exited_without_reaping(pid: u32) -> io::Result<bool> {
    let pid = libc::id_t::from(pid);
    // SAFETY: an all-zero siginfo_t is a valid (empty) value of this plain C struct.
    let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
    // SAFETY: `info` is a valid, writable siginfo_t for the duration of the call.
    let rc = unsafe { libc::waitid(libc::P_PID, pid, &raw mut info, libc::WEXITED | libc::WNOHANG | libc::WNOWAIT) };
    if rc != 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: waitid filled `info` (or left it zeroed with WNOHANG); si_pid reads the
    // field of the SIGCHLD layout, which is the layout waitid uses.
    Ok(unsafe { info.si_pid() } != 0)
}
