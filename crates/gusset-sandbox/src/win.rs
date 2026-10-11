//! Windows job-object containment (FFI; the only `unsafe` in this crate).

#![allow(unsafe_code)]

use crate::Limits;
use std::os::windows::io::AsRawHandle;
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_ACTIVE_PROCESS,
    JOB_OBJECT_LIMIT_DIE_ON_UNHANDLED_EXCEPTION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOB_OBJECT_LIMIT_PROCESS_MEMORY,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation, SetInformationJobObject,
    TerminateJobObject,
};

/// An owned job object handle; closing it kills the contained worker.
#[derive(Debug)]
pub(crate) struct Job(HANDLE);

// SAFETY: a job handle is a kernel object handle usable from any thread.
unsafe impl Send for Job {}

impl Drop for Job {
    fn drop(&mut self) {
        // SAFETY: the handle was created by CreateJobObjectW and is closed exactly once.
        unsafe { CloseHandle(self.0) };
    }
}

impl Job {
    /// Terminates every process in the job now.
    pub(crate) fn terminate(&self) {
        // SAFETY: the handle is a live job handle owned by `self`.
        unsafe { TerminateJobObject(self.0, 1) };
    }
}

pub(crate) fn contain(child: &std::process::Child, limits: &Limits) -> Result<Job, String> {
    // SAFETY: null attributes and name create an anonymous job with default security.
    let h = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
    if h.is_null() {
        return Err("CreateJobObjectW failed".into());
    }
    let job = Job(h);
    // SAFETY: zero-initialising a plain C struct of integers is valid.
    let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
    info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
        | JOB_OBJECT_LIMIT_PROCESS_MEMORY
        | JOB_OBJECT_LIMIT_ACTIVE_PROCESS
        | JOB_OBJECT_LIMIT_DIE_ON_UNHANDLED_EXCEPTION;
    info.BasicLimitInformation.ActiveProcessLimit = 1;
    info.ProcessMemoryLimit = usize::try_from(limits.memory_bytes).unwrap_or(usize::MAX);
    // SAFETY: `info` is a valid, correctly sized struct for this information class.
    let ok = unsafe {
        SetInformationJobObject(
            job.0,
            JobObjectExtendedLimitInformation,
            std::ptr::from_ref(&info).cast(),
            u32::try_from(std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>()).unwrap_or(0),
        )
    };
    if ok == 0 {
        return Err("SetInformationJobObject failed".into());
    }
    let ph = child.as_raw_handle() as HANDLE;
    // SAFETY: both handles are valid for the duration of the call.
    let ok = unsafe { AssignProcessToJobObject(job.0, ph) };
    if ok == 0 {
        return Err("AssignProcessToJobObject failed".into());
    }
    Ok(job)
}
