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
    spawn_command(prepare_command(spec)?)
}
fn prepare_command(spec: &LaunchSpec) -> Result<Command> {
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
    Ok(command)
}
fn spawn_command(mut command: Command) -> Result<Child> {
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

/// A known newly spawned, not-yet-bootstrapped child. Dropping this closes its
/// reader but NEVER blindly kills a possibly ready owner. Use wait_ready with an
/// authenticated root/PID readiness probe for explicit failed-launch cleanup.
pub struct PendingBootstrapChild {
    root: PathBuf,
    child: Child,
    pipe: Option<std::process::ChildStdout>,
}
/// A successful private frame is not perpetual liveness. Redeem through the
/// authenticated owner endpoint; orderly shutdown is through owner IPC, not kill.
pub struct BootstrappedChild {
    child: Child,
    credential: crate::auth::BootstrapCredential,
}
/// Credential-free monitor retained by a native launcher after redemption.
pub struct ReadyChild {
    child: Child,
}
impl ReadyChild {
    pub fn process_id(&self) -> u32 {
        self.child.id()
    }
    pub fn wait_for_exit(mut self) -> std::io::Result<std::process::ExitStatus> {
        self.child.wait()
    }
}
impl BootstrappedChild {
    pub fn process_id(&self) -> u32 {
        self.child.id()
    }
    pub fn credential(&self) -> &crate::auth::BootstrapCredential {
        &self.credential
    }
    /// Split once so the native SDK can erase the redeemed bootstrap secret
    /// while retaining a credential-free handle for orderly exit/reaping.
    pub fn into_parts(self) -> (crate::auth::BootstrapCredential, ReadyChild) {
        (self.credential, ReadyChild { child: self.child })
    }
    /// Optional launcher-worker reaping after orderly authenticated owner quit.
    /// This does not send a signal or stop a ready manager.
    pub fn wait_for_exit(mut self) -> std::io::Result<std::process::ExitStatus> {
        self.child.wait()
    }
}
impl PendingBootstrapChild {
    pub fn process_id(&self) -> u32 {
        self.child.id()
    }
    /// Blocking launch-worker API. `ready_owner` must perform a bounded,
    /// authenticated root+process-ID probe, never trust a request flag. On an
    /// unknown/error probe outcome no kill occurs. Only this NEW child's handle
    /// can be cleaned up, never an already running owner discovered separately.
    pub fn wait_ready<F>(
        mut self,
        timeout: std::time::Duration,
        ready_owner: F,
    ) -> crate::bootstrap::Result<BootstrappedChild>
    where
        F: FnOnce(u32) -> crate::bootstrap::Result<bool>,
    {
        let pipe = self
            .pipe
            .take()
            .ok_or(crate::bootstrap::BootstrapError::InvalidFrame)?;
        let result = crate::bootstrap::read_child_pipe(pipe, &self.root, timeout);
        match result {
            Ok(credential) => Ok(BootstrappedChild {
                child: self.child,
                credential,
            }),
            Err(error) => {
                if !ready_owner(self.child.id())? {
                    if self
                        .child
                        .try_wait()
                        .map_err(|_| crate::bootstrap::BootstrapError::IoUnavailable)?
                        .is_none()
                        && self.child.kill().is_err()
                        && self
                            .child
                            .try_wait()
                            .map_err(|_| crate::bootstrap::BootstrapError::IoUnavailable)?
                            .is_none()
                    {
                        // A concurrently exited child can make kill fail;
                        // reap that exit below. An actually unkillable live
                        // child must not turn this worker into an unbounded wait.
                        return Err(crate::bootstrap::BootstrapError::IoUnavailable);
                    }
                    self.child
                        .wait()
                        .map_err(|_| crate::bootstrap::BootstrapError::IoUnavailable)?;
                }
                Err(error)
            }
        }
    }
}

/// Trusted installed absolute executable + caller's expected publisher/package
/// digest are mandatory. No shell, renderer input, token argument/environment,
/// temporary token file or native credential store is used. Old spawn_detached
/// retains its null-standard-handle behavior.
pub fn spawn_with_private_bootstrap(
    spec: &LaunchSpec,
    expected_sha256: &[u8; 32],
) -> Result<PendingBootstrapChild> {
    use sha2::{Digest, Sha256};
    use std::io::Read;
    let mut command = prepare_command(spec)?;
    let mut file = fs::File::open(&spec.executable)?;
    if file.metadata()?.len() > 512 * 1024 * 1024 {
        return Err(Error::InvalidLaunch);
    }
    let mut hash = Sha256::new();
    let mut bytes = [0u8; 65536];
    let mut total = 0u64;
    loop {
        let count = file.read(&mut bytes)?;
        if count == 0 {
            break;
        }
        total += count as u64;
        if total > 512 * 1024 * 1024 {
            return Err(Error::InvalidLaunch);
        }
        hash.update(&bytes[..count]);
    }
    if hash.finalize().as_slice() != expected_sha256 {
        return Err(Error::InvalidLaunch);
    }
    command.stdout(Stdio::piped());
    let mut child = spawn_command(command)?;
    let pipe = child.stdout.take().ok_or(Error::InvalidLaunch)?;
    Ok(PendingBootstrapChild {
        child,
        pipe: Some(pipe),
        root: fs::canonicalize(&spec.data_root)?,
    })
}
