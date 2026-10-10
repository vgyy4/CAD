//! Windows FFI: EcoQoS / thread priority and installed RAM.
#![allow(unsafe_code)]

use std::io;

use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
use windows_sys::Win32::System::Threading::{
    GetCurrentThread, SetThreadInformation, SetThreadPriority, THREAD_POWER_THROTTLING_CURRENT_VERSION,
    THREAD_POWER_THROTTLING_EXECUTION_SPEED, THREAD_POWER_THROTTLING_STATE, THREAD_PRIORITY_BELOW_NORMAL,
    THREAD_PRIORITY_LOWEST, ThreadPowerThrottling,
};

/// Total physical RAM via `GlobalMemoryStatusEx`.
pub(crate) fn total_ram_bytes() -> Option<u64> {
    let mut st = MEMORYSTATUSEX {
        dwLength: u32::try_from(std::mem::size_of::<MEMORYSTATUSEX>()).ok()?,
        ..MEMORYSTATUSEX::default()
    };
    // SAFETY: `st` is a valid, writable MEMORYSTATUSEX with `dwLength` set as the API
    // requires; the pointer does not outlive the call.
    let ok = unsafe { GlobalMemoryStatusEx(&raw mut st) };
    (ok != 0).then_some(st.ullTotalPhys)
}

/// Opt the calling thread into EcoQoS (execution-speed throttling; Windows 10 1709+,
/// effective on Windows 11) and lower its priority.
///
/// Returns `Ok(true)` if EcoQoS was applied, `Ok(false)` if only the priority could be
/// lowered (older Windows).
pub(crate) fn set_current_thread_eco(idle: bool) -> io::Result<bool> {
    let state = THREAD_POWER_THROTTLING_STATE {
        Version: THREAD_POWER_THROTTLING_CURRENT_VERSION,
        ControlMask: THREAD_POWER_THROTTLING_EXECUTION_SPEED,
        StateMask: THREAD_POWER_THROTTLING_EXECUTION_SPEED,
    };
    let size = u32::try_from(std::mem::size_of::<THREAD_POWER_THROTTLING_STATE>()).map_err(io::Error::other)?;
    // SAFETY: GetCurrentThread returns a pseudo-handle for the calling thread that needs
    // no closing. `state` is a valid THREAD_POWER_THROTTLING_STATE and `size` is its
    // exact size, as ThreadPowerThrottling requires; the pointer is only read during
    // the call.
    let eco =
        unsafe { SetThreadInformation(GetCurrentThread(), ThreadPowerThrottling, (&raw const state).cast(), size) }
            != 0;
    let prio = if idle { THREAD_PRIORITY_LOWEST } else { THREAD_PRIORITY_BELOW_NORMAL };
    // SAFETY: pseudo-handle of the calling thread and a documented priority constant.
    let ok = unsafe { SetThreadPriority(GetCurrentThread(), prio) } != 0;
    if ok { Ok(eco) } else { Err(io::Error::last_os_error()) }
}
