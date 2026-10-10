//! Low-priority / efficiency-QoS background threads.
//!
//! UI and interactive rebuilds run at normal priority. Speculative and background work
//! (pre-tessellation, thumbnails, autosave, indexing) calls
//! [`lower_current_thread_priority`] or is started with [`spawn_background`], so the OS
//! may schedule it on efficiency cores and save power.
//!
//! | Platform | [`BackgroundClass::Low`] | [`BackgroundClass::Idle`] |
//! |---|---|---|
//! | Linux | nice 10 (`setpriority` on the thread id) | nice 19 |
//! | macOS | QoS `UTILITY` | QoS `BACKGROUND` (also throttles I/O) |
//! | Windows | EcoQoS + `THREAD_PRIORITY_BELOW_NORMAL` | EcoQoS + `THREAD_PRIORITY_LOWEST` |
//! | other | unsupported (thread runs at normal priority) | same |

use std::io;
use std::thread::{Builder, JoinHandle};

/// How far to lower a background thread.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackgroundClass {
    /// Work the user is waiting for indirectly (autosave, pre-tessellation of the open
    /// document).
    Low,
    /// Purely speculative work (thumbnails, indexing, cache warm-up).
    Idle,
}

/// What [`lower_current_thread_priority`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PriorityOutcome {
    /// Priority/QoS was lowered with the named mechanism.
    Applied(&'static str),
    /// The thread was already at or below the requested priority; nothing changed.
    AlreadyLow,
    /// No mechanism on this platform; the thread keeps normal priority.
    Unsupported,
}

/// Lower the calling thread's scheduling priority / QoS. Best effort, never raises
/// priority.
///
/// # Errors
/// The OS error if the platform call failed. Callers normally log and continue.
pub fn lower_current_thread_priority(class: BackgroundClass) -> io::Result<PriorityOutcome> {
    imp::lower(class)
}

/// Spawn a named thread that lowers its own priority before running `f`. A failure to
/// lower the priority is not fatal: `f` still runs (at normal priority).
///
/// # Errors
/// Thread creation errors from [`std::thread::Builder::spawn`].
pub fn spawn_background<F, T>(name: &str, class: BackgroundClass, f: F) -> io::Result<JoinHandle<T>>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    Builder::new().name(name.to_owned()).spawn(move || {
        let _ = lower_current_thread_priority(class);
        f()
    })
}

#[cfg(target_os = "linux")]
mod imp {
    use super::{BackgroundClass, PriorityOutcome};
    use crate::sys::linux;
    use std::io;

    pub(super) fn lower(class: BackgroundClass) -> io::Result<PriorityOutcome> {
        let target = match class {
            BackgroundClass::Low => 10,
            BackgroundClass::Idle => 19,
        };
        if linux::current_thread_nice()? >= target {
            return Ok(PriorityOutcome::AlreadyLow);
        }
        linux::set_current_thread_nice(target)?;
        Ok(PriorityOutcome::Applied(if target == 10 { "nice 10" } else { "nice 19" }))
    }
}

#[cfg(target_vendor = "apple")]
mod imp {
    use super::{BackgroundClass, PriorityOutcome};
    use crate::sys::apple::{Qos, set_current_thread_qos};
    use std::io;

    pub(super) fn lower(class: BackgroundClass) -> io::Result<PriorityOutcome> {
        let (qos, name) = match class {
            BackgroundClass::Low => (Qos::Utility, "QOS_CLASS_UTILITY"),
            BackgroundClass::Idle => (Qos::Background, "QOS_CLASS_BACKGROUND"),
        };
        set_current_thread_qos(qos)?;
        Ok(PriorityOutcome::Applied(name))
    }
}

#[cfg(windows)]
mod imp {
    use super::{BackgroundClass, PriorityOutcome};
    use std::io;

    pub(super) fn lower(class: BackgroundClass) -> io::Result<PriorityOutcome> {
        let eco = crate::sys::windows::set_current_thread_eco(class == BackgroundClass::Idle)?;
        Ok(PriorityOutcome::Applied(if eco { "EcoQoS + lowered priority" } else { "lowered priority" }))
    }
}

#[cfg(not(any(target_os = "linux", target_vendor = "apple", windows)))]
mod imp {
    use super::{BackgroundClass, PriorityOutcome};
    use std::io;

    pub(super) fn lower(_class: BackgroundClass) -> io::Result<PriorityOutcome> {
        Ok(PriorityOutcome::Unsupported)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn background_thread_runs_and_returns() {
        let h = spawn_background("gusset-test-bg", BackgroundClass::Low, || {
            let again = lower_current_thread_priority(BackgroundClass::Low);
            (std::thread::current().name().map(str::to_owned), again)
        });
        let (name, again) = h.and_then(|h| h.join().map_err(|_| io::Error::other("panicked"))).unwrap();
        assert_eq!(name.as_deref(), Some("gusset-test-bg"));
        // Second call on the same thread must not fail.
        let again = again.unwrap();
        #[cfg(target_os = "linux")]
        assert_eq!(again, PriorityOutcome::AlreadyLow);
        let _ = again;
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_nice_is_raised_on_the_thread_only() {
        let before = crate::sys::linux::current_thread_nice().unwrap();
        let h = std::thread::spawn(|| {
            let out = lower_current_thread_priority(BackgroundClass::Idle).unwrap();
            (out, crate::sys::linux::current_thread_nice().unwrap())
        });
        let (out, nice) = h.join().unwrap();
        assert!(nice >= 19, "nice {nice}, outcome {out:?}");
        // The spawning thread is unaffected.
        assert_eq!(crate::sys::linux::current_thread_nice().unwrap(), before);
    }
}
