//! Linux FFI: per-thread nice value.
#![allow(unsafe_code)]

use std::io;

/// Current thread's nice value, from `/proc/thread-self/stat` (field 19).
pub(crate) fn current_thread_nice() -> io::Result<i32> {
    let stat = std::fs::read_to_string("/proc/thread-self/stat")?;
    parse_stat_nice(&stat).ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "unparsable /proc stat"))
}

/// Field 19 (`nice`) of a `/proc/<pid>/stat` line. The command name (field 2) may
/// contain spaces and parentheses, so parsing starts after the last `)`.
pub(crate) fn parse_stat_nice(stat: &str) -> Option<i32> {
    let (_, rest) = stat.rsplit_once(')')?;
    // rest starts with field 3 (state); nice is field 19 -> index 16.
    rest.split_whitespace().nth(16)?.parse().ok()
}

/// Set the calling thread's nice value. On Linux, `setpriority(PRIO_PROCESS, tid, n)`
/// affects only the thread `tid` (documented in `setpriority(2)`, "BUGS": the
/// per-thread behavior is a Linux deviation from POSIX). Raising the nice value
/// (lowering priority) needs no privileges.
pub(crate) fn set_current_thread_nice(nice: i32) -> io::Result<()> {
    // SAFETY: `gettid` has no preconditions and cannot fail.
    let tid = unsafe { libc::gettid() };
    let tid = libc::id_t::try_from(tid).map_err(|_| io::Error::other("negative tid"))?;
    // SAFETY: plain syscall wrapper with integer arguments only; `tid` is the calling
    // thread's own id, so it cannot refer to another process's thread.
    let rc = unsafe { libc::setpriority(libc::PRIO_PROCESS, tid, nice) };
    if rc == 0 { Ok(()) } else { Err(io::Error::last_os_error()) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_nice_with_odd_comm() {
        let line = "1234 (my (odd) prog) S 1 1234 1234 0 -1 4194560 100 0 0 0 1 2 0 0 20 5 1 0 100 1000 10";
        assert_eq!(parse_stat_nice(line), Some(5));
        assert_eq!(parse_stat_nice("garbage"), None);
    }

    #[test]
    fn reads_own_nice() {
        assert!(current_thread_nice().is_ok());
    }
}
