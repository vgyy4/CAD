//! Win32 FFI used by `gusset-store`. This is the crate's only `unsafe` code.
//!
//! Every call passes NUL-terminated UTF-16 buffers that we own and that outlive the
//! call, or a handle borrowed from a live `std::fs::File`.

use std::ffi::OsStr;
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;

use windows_sys::Win32::Storage::FileSystem::{MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW};

fn wide(s: &OsStr) -> io::Result<Vec<u16>> {
    let mut v: Vec<u16> = s.encode_wide().collect();
    if v.contains(&0) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "path contains a NUL character"));
    }
    v.push(0);
    Ok(v)
}

/// Atomically replaces `to` with `from`, flushing the rename before returning.
pub(crate) fn replace_file(from: &Path, to: &Path) -> io::Result<()> {
    let f = wide(from.as_os_str())?;
    let t = wide(to.as_os_str())?;
    // SAFETY: `f` and `t` are valid NUL-terminated UTF-16 strings that live until the
    // call returns; MoveFileExW only reads them.
    let ok = unsafe { MoveFileExW(f.as_ptr(), t.as_ptr(), MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH) };
    if ok == 0 { Err(io::Error::last_os_error()) } else { Ok(()) }
}
