//! Apple FFI: `sysctlbyname` reads and thread QoS classes.
#![allow(unsafe_code)]

use std::ffi::{CStr, c_void};
use std::io;

/// Read an integer sysctl (4- or 8-byte values).
pub(crate) fn sysctl_u64(name: &CStr) -> Option<u64> {
    let mut buf = [0_u8; 8];
    let mut len: libc::size_t = buf.len();
    // SAFETY: `name` is NUL-terminated; `buf` is a valid writable buffer of `len` bytes
    // and `len` is updated by the kernel to the number of bytes written (never more
    // than the buffer size; larger values fail with ENOMEM). No new value is set
    // (null pointer, length 0).
    let rc = unsafe {
        libc::sysctlbyname(name.as_ptr(), buf.as_mut_ptr().cast::<c_void>(), &raw mut len, std::ptr::null_mut(), 0)
    };
    if rc != 0 {
        return None;
    }
    match len {
        4 => {
            let b: [u8; 4] = buf.get(..4)?.try_into().ok()?;
            Some(u64::from(u32::from_ne_bytes(b)))
        }
        8 => Some(u64::from_ne_bytes(buf)),
        _ => None,
    }
}

/// Read a string sysctl (at most 256 bytes).
pub(crate) fn sysctl_string(name: &CStr) -> Option<String> {
    let mut buf = [0_u8; 256];
    let mut len: libc::size_t = buf.len();
    // SAFETY: as in `sysctl_u64`; the kernel writes at most `len` bytes into `buf`.
    let rc = unsafe {
        libc::sysctlbyname(name.as_ptr(), buf.as_mut_ptr().cast::<c_void>(), &raw mut len, std::ptr::null_mut(), 0)
    };
    if rc != 0 {
        return None;
    }
    crate::cpu::clean_string(buf.get(..len)?)
}

/// QoS classes we use for background threads.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Qos {
    Utility,
    Background,
}

/// Set the calling thread's QoS class (`pthread_set_qos_class_self_np`).
pub(crate) fn set_current_thread_qos(qos: Qos) -> io::Result<()> {
    let class = match qos {
        Qos::Utility => libc::qos_class_t::QOS_CLASS_UTILITY,
        Qos::Background => libc::qos_class_t::QOS_CLASS_BACKGROUND,
    };
    // SAFETY: affects only the calling thread; the class is a valid enum value and the
    // relative priority 0 is within the documented range [QOS_MIN_RELATIVE_PRIORITY, 0].
    let rc = unsafe { libc::pthread_set_qos_class_self_np(class, 0) };
    if rc == 0 { Ok(()) } else { Err(io::Error::from_raw_os_error(rc)) }
}
