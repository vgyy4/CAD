//! Windows FFI: EcoQoS / thread priority, installed RAM and DLL search-path hardening.
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

/// Restricts the default DLL search path to the application directory, System32 and
/// user-added directories, and removes the current directory from it.
pub(crate) fn restrict_dll_search() -> Result<(), String> {
    use windows_sys::Win32::System::LibraryLoader::{
        LOAD_LIBRARY_SEARCH_DEFAULT_DIRS, SetDefaultDllDirectories, SetDllDirectoryW,
    };
    // SAFETY: takes only a flags value; affects later LoadLibrary* calls in this process.
    let ok = unsafe { SetDefaultDllDirectories(LOAD_LIBRARY_SEARCH_DEFAULT_DIRS) };
    if ok == 0 {
        return Err(format!("SetDefaultDllDirectories failed: {}", io::Error::last_os_error()));
    }
    let empty: [u16; 1] = [0];
    // SAFETY: `empty` is a valid NUL-terminated UTF-16 string that outlives the call; an
    // empty string removes the current directory from the search path.
    let ok = unsafe { SetDllDirectoryW(empty.as_ptr()) };
    if ok == 0 {
        return Err(format!("SetDllDirectoryW failed: {}", io::Error::last_os_error()));
    }
    Ok(())
}
