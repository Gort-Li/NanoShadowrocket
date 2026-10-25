//! Windows Job Object：应用进程退出（包括崩溃）时一并结束 sidecar。
//!
//! 只依赖子进程句柄，不需要额外权限；`AssignProcessToJobObject` 失败时
//! 退化为普通的 `Child::kill`，不影响主流程。

#[cfg(windows)]
use std::os::windows::io::AsRawHandle;
#[cfg(windows)]
use std::process::Child;

#[cfg(windows)]
use windows::core::PCWSTR;
#[cfg(windows)]
use windows::Win32::Foundation::{CloseHandle, HANDLE};
#[cfg(windows)]
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
    SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};

#[cfg(windows)]
pub struct JobHandle(HANDLE);

#[cfg(windows)]
unsafe impl Send for JobHandle {}
#[cfg(windows)]
unsafe impl Sync for JobHandle {}

#[cfg(windows)]
impl JobHandle {
    pub fn assign(child: &Child) -> Option<Self> {
        unsafe {
            let job = CreateJobObjectW(None, PCWSTR::null()).ok()?;
            let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            let ok = SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                &info as *const _ as *const _,
                std::mem::size_of_val(&info) as u32,
            );
            if ok.is_err() {
                let _ = CloseHandle(job);
                return None;
            }
            let process = HANDLE(child.as_raw_handle() as _);
            if AssignProcessToJobObject(job, process).is_err() {
                let _ = CloseHandle(job);
                return None;
            }
            Some(Self(job))
        }
    }
}

#[cfg(windows)]
impl Drop for JobHandle {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}
