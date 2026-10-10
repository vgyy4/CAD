//! stdout → stderr redirection for protocol streams (see `crate::stdio`).
#![allow(unsafe_code)]

use std::fs::File;
use std::io;

#[cfg(any(target_os = "linux", target_os = "android", target_vendor = "apple"))]
pub(crate) fn take_stdout() -> io::Result<File> {
    use std::os::fd::AsFd;
    // Safe duplicate (F_DUPFD_CLOEXEC) of descriptor 1, owned by the returned File.
    let proto = File::from(io::stdout().as_fd().try_clone_to_owned()?);
    // SAFETY: dup2 only manipulates the process descriptor table. Descriptors 1 and 2 are
    // the standard streams; std never holds an owned handle to them, and `proto` (a
    // separate descriptor) is unaffected. Errors are reported through the return value.
    let r = unsafe { libc::dup2(libc::STDERR_FILENO, libc::STDOUT_FILENO) };
    if r < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(proto)
}

#[cfg(windows)]
pub(crate) fn take_stdout() -> io::Result<File> {
    use std::os::windows::io::AsHandle;
    use windows_sys::Win32::System::Console::{GetStdHandle, STD_ERROR_HANDLE, STD_OUTPUT_HANDLE, SetStdHandle};
    // Safe DuplicateHandle of the current standard output, owned by the returned File.
    let proto = File::from(io::stdout().as_handle().try_clone_to_owned()?);
    // SAFETY: GetStdHandle/SetStdHandle only read and replace the process's standard
    // handle slots. No ownership is transferred and nothing is closed: the stderr handle
    // stays owned by the process, and `proto` owns an independent duplicate of stdout.
    let ok = unsafe { SetStdHandle(STD_OUTPUT_HANDLE, GetStdHandle(STD_ERROR_HANDLE)) };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    // C/C++ code (OCCT's std::cout, printf) writes through the C runtime's descriptor 1,
    // which was bound at startup; point it at descriptor 2 as well.
    // SAFETY: _dup2 only manipulates the C runtime's descriptor table; 1 and 2 are the
    // standard descriptors. A failure (e.g. no console attached) is returned as -1.
    let r = unsafe { libc::dup2(2, 1) };
    if r < 0 {
        return Err(io::Error::other("could not redirect the C runtime's stdout to stderr"));
    }
    Ok(proto)
}

#[cfg(not(any(windows, target_os = "linux", target_os = "android", target_vendor = "apple")))]
pub(crate) fn take_stdout() -> io::Result<File> {
    Err(io::Error::new(io::ErrorKind::Unsupported, "stdout redirection is not implemented on this platform"))
}
