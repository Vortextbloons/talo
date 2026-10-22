//! Power callbacks work without a window. Never block the Windows power notification thread.
use std::sync::atomic::{AtomicBool, Ordering};
static INTERRUPTED: AtomicBool = AtomicBool::new(false);
static SUSPENDED: AtomicBool = AtomicBool::new(false);
pub fn is_suspended() -> bool {
    SUSPENDED.load(Ordering::SeqCst)
}
pub fn take_interrupt() -> bool {
    INTERRUPTED.swap(false, Ordering::SeqCst)
}
#[cfg(windows)]
mod windows {
    use super::*;
    use std::ffi::c_void;
    use windows_sys::Win32::System::Power::*;
    pub struct Registration {
        handle: *mut c_void,
        _parameters: Box<DEVICE_NOTIFY_SUBSCRIBE_PARAMETERS>,
    }
    unsafe impl Send for Registration {}
    unsafe impl Sync for Registration {}
    unsafe extern "system" fn callback(_: *const c_void, kind: u32, _: *const c_void) -> u32 {
        if matches!(kind, 4 | 7 | 18) {
            SUSPENDED.store(kind == 4, Ordering::SeqCst);
            INTERRUPTED.store(true, Ordering::SeqCst);
        }
        0
    }
    impl Registration {
        pub fn new() -> std::io::Result<Self> {
            let mut parameters = Box::new(DEVICE_NOTIFY_SUBSCRIBE_PARAMETERS {
                Callback: Some(callback),
                Context: std::ptr::null_mut(),
            });
            let mut handle = std::ptr::null_mut();
            let error = unsafe {
                PowerRegisterSuspendResumeNotification(
                    2,
                    (&mut *parameters) as *mut _ as _,
                    &mut handle,
                )
            };
            if error != 0 {
                return Err(std::io::Error::from_raw_os_error(error as i32));
            }
            Ok(Self {
                handle,
                _parameters: parameters,
            })
        }
    }
    impl Drop for Registration {
        fn drop(&mut self) {
            unsafe {
                PowerUnregisterSuspendResumeNotification(self.handle as isize);
            }
        }
    }
}
#[cfg(windows)]
pub use windows::Registration;
