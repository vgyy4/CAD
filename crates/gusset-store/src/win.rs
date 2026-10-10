//! Win32 FFI used by `gusset-store`. This is the crate's only `unsafe` code.
//!
//! Every call passes NUL-terminated UTF-16 buffers that we own and that outlive the
//! call, or a handle borrowed from a live `std::fs::File`.

use std::ffi::OsStr;
use std::fs::File;
use std::io;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::os::windows::io::AsRawHandle;
use std::path::Path;

use windows_sys::Win32::Foundation::HANDLE;
use windows_sys::Win32::Storage::FileSystem::{
    BY_HANDLE_FILE_INFORMATION, GetDriveTypeW, GetFileInformationByHandle, GetVolumeInformationW, GetVolumePathNameW,
    MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
};

/// `DRIVE_REMOTE` from `WinBase.h` (`GetDriveTypeW` result for network drives).
const DRIVE_REMOTE: u32 = 4;

fn wide(s: &OsStr) -> io::Result<Vec<u16>> {
    let mut v: Vec<u16> = s.encode_wide().collect();
    if v.contains(&0) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "path contains a NUL character"));
    }
    v.push(0);
    Ok(v)
}

fn from_wide_buf(buf: &[u16]) -> std::ffi::OsString {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    std::ffi::OsString::from_wide(buf.get(..end).unwrap_or(&[]))
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

/// The (volume serial, file index) pair identifying the file behind `file`.
pub(crate) fn file_id(file: &File) -> io::Result<(u32, u64)> {
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: the handle is owned by `file`, which is borrowed for the duration of the
    // call, so it is open and valid; `info` is a properly aligned, writable struct of
    // the type the API expects.
    let ok = unsafe { GetFileInformationByHandle(file.as_raw_handle() as HANDLE, &mut info) };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok((info.dwVolumeSerialNumber, (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow)))
}

/// Volume facts for the risky-storage classifier: (is a remote drive, file-system name).
pub(crate) fn volume_info(path: &Path) -> io::Result<(bool, Option<String>)> {
    let p = wide(path.as_os_str())?;
    let mut root = vec![0u16; 1024];
    let len = u32::try_from(root.len()).unwrap_or(u32::MAX);
    // SAFETY: `p` is a valid NUL-terminated string; `root` is a writable buffer whose
    // length in u16 units is passed as `len`.
    let ok = unsafe { GetVolumePathNameW(p.as_ptr(), root.as_mut_ptr(), len) };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    let root = wide(&from_wide_buf(&root))?;
    // SAFETY: `root` is a valid NUL-terminated string that outlives the call.
    let drive_type = unsafe { GetDriveTypeW(root.as_ptr()) };
    let mut fs_name = vec![0u16; 64];
    let fs_len = u32::try_from(fs_name.len()).unwrap_or(0);
    // SAFETY: `root` is valid and NUL-terminated; the only output buffer requested is
    // `fs_name`, whose capacity is passed as `fs_len`; all other out-pointers are null,
    // which the API documents as "not requested".
    let ok = unsafe {
        GetVolumeInformationW(
            root.as_ptr(),
            std::ptr::null_mut(),
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            fs_name.as_mut_ptr(),
            fs_len,
        )
    };
    let fs = (ok != 0).then(|| from_wide_buf(&fs_name).to_string_lossy().into_owned());
    Ok((drive_type == DRIVE_REMOTE, fs))
}
