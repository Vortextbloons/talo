//! Kill the full managed process tree when the native host exits or crashes.
use std::{io, os::windows::io::AsRawHandle};
use windows_sys::Win32::{
    Foundation::{CloseHandle, HANDLE},
    System::JobObjects::*,
};
pub struct Job(HANDLE);
unsafe impl Send for Job {}
impl Job {
    pub fn new() -> io::Result<Self> {
        unsafe {
            let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if job.is_null() {
                return Err(io::Error::last_os_error());
            }
            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            if SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                &info as *const _ as _,
                std::mem::size_of_val(&info) as u32,
            ) == 0
            {
                CloseHandle(job);
                return Err(io::Error::last_os_error());
            }
            Ok(Self(job))
        }
    }
    pub fn assign(&self, child: &std::process::Child) -> io::Result<()> {
        unsafe {
            if AssignProcessToJobObject(self.0, child.as_raw_handle() as _) == 0 {
                Err(io::Error::last_os_error())
            } else {
                Ok(())
            }
        }
    }
}
impl Drop for Job {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}
