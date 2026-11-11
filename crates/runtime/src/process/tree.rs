use crate::{Result, RuntimeError};
use std::{
    io::Write,
    process::{Child, Command},
    time::{Duration, Instant},
};

#[cfg(unix)]
pub struct Tree {
    pub child: Child,
    finished: bool,
    #[cfg(target_os = "macos")]
    watchdog: Option<Child>,
}
#[cfg(unix)]
impl Tree {
    pub fn spawn(command: &mut Command) -> Result<Self> {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
        #[allow(unused_mut)]
        let mut child = command.spawn().map_err(|_| RuntimeError::SpawnFailed)?;
        #[cfg(target_os = "macos")]
        let watchdog = match Command::new("/bin/sh")
            .args([
                "-c",
                "IFS= read -r released && exit 0; /bin/kill -KILL -- \"-$1\" 2>/dev/null",
                "devforge-owner-watch",
                &child.id().to_string(),
            ])
            .env_clear()
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .process_group(0)
            .spawn()
        {
            Ok(guard) => Some(guard),
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(RuntimeError::SpawnFailed);
            }
        };
        Ok(Self {
            child,
            finished: false,
            #[cfg(target_os = "macos")]
            watchdog,
        })
    }
    pub fn exited(&mut self) -> Result<bool> {
        // WNOWAIT retains the root PID until its group has been cleaned. Calling
        // Child::try_wait first would reap it and permit a dangerous PGID reuse.
        let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
        let result = unsafe {
            libc::waitid(
                libc::P_PID,
                self.child.id(),
                &mut info,
                libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
            )
        };
        if result != 0 {
            return Err(RuntimeError::Io);
        }
        Ok(unsafe { info.si_pid() } != 0)
    }
    fn signal(&self, signal: i32) -> Result<()> {
        let result = unsafe { libc::kill(-(self.child.id() as i32), signal) };
        if result == 0 {
            return Ok(());
        }
        let error = std::io::Error::last_os_error().raw_os_error();
        if error == Some(libc::ESRCH) {
            return Ok(());
        }
        // Darwin returns EPERM for a group containing only zombies. Confirm
        // member states rather than hiding a genuine live-process denial.
        #[cfg(target_os = "macos")]
        if error == Some(libc::EPERM) && no_live_group_members(self.child.id()) {
            return Ok(());
        }
        Err(RuntimeError::Io)
    }
    pub fn finish(&mut self, grace: Duration) -> Result<Option<i32>> {
        self.child.stdin.take().map(|mut s| s.write_all(b"quit\n"));
        self.signal(libc::SIGTERM)?;
        let deadline = Instant::now() + grace;
        while !self.exited()? && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        self.signal(libc::SIGKILL)?;
        let code = self.child.wait()?.code();
        self.finished = true;
        #[cfg(target_os = "macos")]
        if let Some(mut guard) = self.watchdog.take() {
            if let Some(mut pipe) = guard.stdin.take() {
                let _ = pipe.write_all(b"released\n");
            }
            let _ = guard.wait();
        }
        Ok(code)
    }
}

#[cfg(target_os = "macos")]
fn no_live_group_members(group: u32) -> bool {
    let mut pids = [0i32; 16384];
    let bytes = unsafe {
        libc::proc_listpids(
            2,
            group,
            pids.as_mut_ptr().cast(),
            std::mem::size_of_val(&pids) as i32,
        )
    };
    if bytes < 0 || bytes as usize >= std::mem::size_of_val(&pids) {
        return false;
    }
    for pid in pids
        .iter()
        .take(bytes as usize / std::mem::size_of::<i32>())
        .filter(|p| **p > 0)
    {
        let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
        let size = unsafe {
            libc::proc_pidinfo(
                *pid,
                libc::PROC_PIDTBSDINFO,
                0,
                &mut info as *mut _ as *mut _,
                std::mem::size_of_val(&info) as i32,
            )
        };
        if size == 0 && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH) {
            continue;
        }
        if size as usize != std::mem::size_of_val(&info) || info.pbi_status != libc::SZOMB {
            return false;
        }
    }
    true
}

#[cfg(windows)]
pub struct Tree {
    pub child: Child,
    job: windows_sys::Win32::Foundation::HANDLE,
}
#[cfg(windows)]
unsafe impl Send for Tree {}
#[cfg(windows)]
impl Tree {
    pub fn spawn(command: &mut Command) -> Result<Self> {
        use std::os::windows::{io::AsRawHandle, process::CommandExt};
        use windows_sys::Win32::{
            Foundation::*,
            System::{Diagnostics::ToolHelp::*, JobObjects::*, Threading::*},
        };
        // Spawn suspended: no child/grandchild can escape before job assignment.
        let job = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        if job.is_null() {
            return Err(RuntimeError::SpawnFailed);
        }
        let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        if unsafe {
            SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                &limits as *const _ as *const _,
                std::mem::size_of_val(&limits) as u32,
            )
        } == 0
        {
            unsafe { CloseHandle(job) };
            return Err(RuntimeError::SpawnFailed);
        }
        command.creation_flags(CREATE_SUSPENDED | CREATE_NO_WINDOW);
        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(_) => {
                unsafe { CloseHandle(job) };
                return Err(RuntimeError::SpawnFailed);
            }
        };
        let assigned =
            unsafe { AssignProcessToJobObject(job, child.as_raw_handle() as HANDLE) } != 0;
        // Stable std::Child does not expose the primary thread handle. Enumerate
        // only this suspended PID's threads, open one, and resume after assignment.
        let resumed = assigned
            && unsafe {
                let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0);
                if snapshot == INVALID_HANDLE_VALUE {
                    false
                } else {
                    let mut entry: THREADENTRY32 = std::mem::zeroed();
                    entry.dwSize = std::mem::size_of_val(&entry) as u32;
                    let mut next = Thread32First(snapshot, &mut entry);
                    let mut ok = false;
                    while next != 0 {
                        if entry.th32OwnerProcessID == child.id() {
                            let thread = OpenThread(THREAD_SUSPEND_RESUME, 0, entry.th32ThreadID);
                            if !thread.is_null() {
                                ok = ResumeThread(thread) == 1;
                                CloseHandle(thread);
                            }
                            break;
                        }
                        next = Thread32Next(snapshot, &mut entry);
                    }
                    CloseHandle(snapshot);
                    ok
                }
            };
        if !resumed {
            let _ = child.kill();
            let _ = child.wait();
            unsafe { CloseHandle(job) };
            return Err(RuntimeError::SpawnFailed);
        }
        Ok(Self { child, job })
    }
    pub fn exited(&mut self) -> Result<bool> {
        Ok(self.child.try_wait()?.is_some())
    }
    pub fn finish(&mut self, grace: Duration) -> Result<Option<i32>> {
        use windows_sys::Win32::System::JobObjects::TerminateJobObject;
        // Headless GUI apps do not share a console with children. A stdin quit
        // request is the cooperative protocol; unsupported runtimes fall back to
        // terminating this owned job after grace, never taskkill/name matching.
        self.child.stdin.take().map(|mut s| s.write_all(b"quit\n"));
        let deadline = Instant::now() + grace;
        while !self.exited()? && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        if unsafe { TerminateJobObject(self.job, 1) } == 0 {
            return Err(RuntimeError::Io);
        }
        Ok(self.child.wait()?.code())
    }
}
impl Drop for Tree {
    fn drop(&mut self) {
        // Best-effort last-resort cleanup on any early-return path. Successful
        // finish has already waited; don't signal a reaped Unix PID a second time.
        #[cfg(windows)]
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.job);
        }
        #[cfg(unix)]
        if !self.finished {
            let _ = self.signal(libc::SIGKILL);
            #[cfg(target_os = "macos")]
            if let Some(mut guard) = self.watchdog.take() {
                if let Some(mut pipe) = guard.stdin.take() {
                    let _ = pipe.write_all(b"released\n");
                }
                let _ = guard.wait();
            }
            let _ = self.child.wait();
        }
    }
}
