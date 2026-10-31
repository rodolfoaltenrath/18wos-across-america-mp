//! Keep an uncertainty marker alive IN THE TARGET, even if the launcher exits.
//! The handle contains no code/pointers and the OS reclaims it on process exit.
use super::Handle;
use anyhow::{Result, ensure};
use std::ptr;
use windows_sys::Win32::{
    Foundation::{
        DUPLICATE_CLOSE_SOURCE, DUPLICATE_SAME_ACCESS, DuplicateHandle, ERROR_ALREADY_EXISTS,
        ERROR_FILE_NOT_FOUND, GetLastError, HANDLE,
    },
    System::Threading::{CreateEventW, EVENT_MODIFY_STATE, GetCurrentProcess, OpenEventW},
};

pub(super) fn name(pid: u32, created: u64) -> Vec<u16> {
    format!("Local\\AcrossAmericaMP-Call-{pid}-{created:016x}")
        .encode_utf16()
        .chain(Some(0))
        .collect()
}
pub(super) fn ensure_clear(name: &[u16]) -> Result<()> {
    let event = Handle(unsafe { OpenEventW(EVENT_MODIFY_STATE, 0, name.as_ptr()) });
    let error = unsafe { GetLastError() };
    ensure!(
        event.0.is_null() && error == ERROR_FILE_NOT_FOUND,
        "remote call unresolved or guard unavailable; restart target before more control/unload operations"
    );
    Ok(())
}
pub(super) struct Lease {
    target: HANDLE,
    remote: HANDLE,
    _event: Handle,
    reclaim: bool,
}
impl Lease {
    pub(super) fn acquire(target: HANDLE, name: &[u16]) -> Result<Self> {
        let event = Handle(unsafe { CreateEventW(ptr::null(), 1, 0, name.as_ptr()) });
        let error = unsafe { GetLastError() };
        ensure!(!event.0.is_null(), "cannot create remote call guard");
        ensure!(
            error != ERROR_ALREADY_EXISTS,
            "remote call unresolved; restart target before more control/unload operations"
        );
        let mut remote = ptr::null_mut();
        ensure!(
            unsafe {
                DuplicateHandle(
                    GetCurrentProcess(),
                    event.0,
                    target,
                    &mut remote,
                    0,
                    0,
                    DUPLICATE_SAME_ACCESS,
                )
            } != 0,
            "cannot retain remote call guard in target"
        );
        Ok(Self {
            target,
            remote,
            _event: event,
            reclaim: true,
        })
    }
    pub(super) fn started(&mut self) {
        self.reclaim = false;
    }
    pub(super) fn completed(&mut self) {
        self.reclaim = true;
    }
}
impl Drop for Lease {
    fn drop(&mut self) {
        if self.reclaim {
            let mut local = ptr::null_mut();
            // Remove only our own target handle, never its thread or module.
            // Failure leaves the guard conservative until target exit.
            if unsafe {
                DuplicateHandle(
                    self.target,
                    self.remote,
                    GetCurrentProcess(),
                    &mut local,
                    0,
                    0,
                    DUPLICATE_CLOSE_SOURCE | DUPLICATE_SAME_ACCESS,
                )
            } != 0
            {
                drop(Handle(local));
            }
        }
    }
}
