//! Dedicated manager launch, deliberately independent of runtime::process::Tree.
//! A caller must resolve the trusted installed executable, then wait for an
//! authenticated endpoint handshake; successful spawn alone is not readiness.
use crate::{Error, Result};
use std::{
    collections::BTreeMap,
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
};

pub struct LaunchSpec {
    pub executable: PathBuf,
    pub arguments: Vec<OsString>,
    pub data_root: PathBuf,
    /// Explicit trusted composition-root environment; never supplied by IPC.
    pub environment: BTreeMap<OsString, OsString>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Capability {
    Detached,
    /// Windows ancestry may forbid breakaway. Spawn must prove it, otherwise
    /// RestrictedJob is returned and the user starts the manager independently.
    RequiresJobBreakaway,
}
pub fn capability() -> Result<Capability> {
    #[cfg(unix)]
    {
        Ok(Capability::Detached)
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::{
            JobObjects::IsProcessInJob, Threading::GetCurrentProcess,
        };
        let mut in_job = 0;
        if unsafe { IsProcessInJob(GetCurrentProcess(), std::ptr::null_mut(), &mut in_job) } == 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        Ok(if in_job != 0 {
            Capability::RequiresJobBreakaway
        } else {
            Capability::Detached
        })
    }
    #[cfg(not(any(unix, windows)))]
    {
        Err(Error::UnsupportedPlatform)
    }
}
pub fn spawn_detached(spec: &LaunchSpec) -> Result<Child> {
    if !spec.executable.is_absolute()
        || !spec.data_root.is_absolute()
        || spec.arguments.len() > 64
        || spec.environment.len() > 64
    {
        return Err(Error::InvalidLaunch);
    }
    reject_link(&spec.executable, false)?;
    reject_link(&spec.data_root, true)?;
    let executable = fs::canonicalize(&spec.executable)?;
    let root = fs::canonicalize(&spec.data_root)?;
    crate::transport::os::validate_root(&root)?;
    let mut command = Command::new(executable);
    command
        .args(&spec.arguments)
        .current_dir(&root)
        .env_clear()
        .envs(&spec.environment)
        .env("DEVFORGE_DATA_DIR", &root)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // setsid is async-signal-safe; no allocation, locks or Rust formatting
        // occurs between fork and exec. No inherited parent-pipe watchdog.
        unsafe {
            command.pre_exec(|| {
                if libc::setsid() == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        Ok(command.spawn()?)
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        use windows_sys::Win32::{
            Foundation::ERROR_ACCESS_DENIED,
            System::Threading::{
                CREATE_BREAKAWAY_FROM_JOB, CREATE_NEW_PROCESS_GROUP, DETACHED_PROCESS,
            },
        };
        let ancestry = capability()?;
        command.creation_flags(
            CREATE_BREAKAWAY_FROM_JOB | CREATE_NEW_PROCESS_GROUP | DETACHED_PROCESS,
        );
        command.spawn().map_err(|e| {
            if ancestry == Capability::RequiresJobBreakaway
                && e.raw_os_error() == Some(ERROR_ACCESS_DENIED as i32)
            {
                Error::RestrictedJob
            } else {
                Error::Io(e)
            }
        })
    }
    #[cfg(not(any(unix, windows)))]
    {
        Err(Error::UnsupportedPlatform)
    }
}
fn reject_link(path: &Path, directory: bool) -> Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink()
        || (directory && !metadata.is_dir())
        || (!directory && !metadata.is_file())
    {
        return Err(Error::InvalidLaunch);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;
        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(Error::InvalidLaunch);
        }
    }
    Ok(())
}
