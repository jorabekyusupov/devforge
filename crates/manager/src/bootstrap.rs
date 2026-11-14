//! Private cold-GUI bootstrap channel. Never a normal IPC issuance tool.
//!
//! Capture only at the explicit private-launch entry. Capturing validates a real
//! inherited pipe, retains a noninheritable duplicate, and redirects stdout to
//! NUL immediately. Credential generation happens later, after actual owner,
//! Workspace actor and listener readiness have been combined into ReadyPermit.
use crate::{
    actor::ActorHandle,
    auth::{BootstrapCredential, DesktopAuth, NativeGuiPolicy},
    transport::ListenerReady,
};
use sha2::{Digest, Sha256};
use std::{
    fmt,
    fs::File,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::ChildStdout,
    time::{Duration, Instant},
};
use zeroize::Zeroizing;

const MAGIC: &[u8; 8] = b"DFGBOOT\0";
const VERSION: u16 = 1;
pub const FRAME_BYTES: usize = 138;
pub const MAX_WAIT: Duration = Duration::from_secs(30);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BootstrapError {
    PipeRequired,
    InvalidFrame,
    IoUnavailable,
    TimedOut,
    RootMismatch,
    NotReady,
    InvalidTimeout,
    AuthUnavailable,
}
impl fmt::Display for BootstrapError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::PipeRequired => "bootstrap_pipe_required",
            Self::InvalidFrame => "bootstrap_invalid_frame",
            Self::IoUnavailable => "bootstrap_io_unavailable",
            Self::TimedOut => "bootstrap_timeout",
            Self::RootMismatch => "bootstrap_root_mismatch",
            Self::NotReady => "bootstrap_not_ready",
            Self::InvalidTimeout => "bootstrap_invalid_timeout",
            Self::AuthUnavailable => "bootstrap_auth_unavailable",
        })
    }
}
impl std::error::Error for BootstrapError {}
pub type Result<T> = std::result::Result<T, BootstrapError>;

/// Private in-process proof. Only the actual post-bind listener callback and a
/// successfully initialized, healthy actor can construct it; no wire decoder.
pub struct ReadyPermit {
    root: PathBuf,
}
impl ReadyPermit {
    pub(crate) fn from_ready(listener: ListenerReady, actor: &ActorHandle) -> Result<Self> {
        let root = actor
            .bootstrap_ready_root()
            .map_err(|_| BootstrapError::NotReady)?;
        if root != listener.endpoint().root() {
            return Err(BootstrapError::RootMismatch);
        }
        Ok(Self {
            root: root.to_owned(),
        })
    }
    pub(crate) fn root(&self) -> &Path {
        &self.root
    }
}

/// Hash native canonical path bytes. Not a secret, token, or authorization claim.
pub(crate) fn root_fingerprint(root: &Path) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(b"devforge.private-bootstrap.root.v1\0");
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        hash.update(root.as_os_str().as_bytes());
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        for unit in root.as_os_str().encode_wide() {
            hash.update(unit.to_le_bytes());
        }
    }
    hash.finalize().into()
}

pub struct BootstrapWriter {
    pipe: File,
}
impl fmt::Debug for BootstrapWriter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("BootstrapWriter([PRIVATE PIPE])")
    }
}
impl BootstrapWriter {
    /// Process-global standard-handle change: private manager launch entry only,
    /// before any stdout logging/children. Does not generate a credential.
    pub fn capture_inherited_stdout() -> Result<Self> {
        Ok(Self {
            pipe: capture_stdout_pipe()?,
        })
    }
    /// Crate-private controller readiness callback only. Consumes both the writer
    /// and permit, writes one bounded zeroizing frame, then closes the pipe.
    pub(crate) fn issue_once(
        mut self,
        auth: &mut DesktopAuth,
        permit: ReadyPermit,
        policy: NativeGuiPolicy,
        now: Instant,
    ) -> Result<()> {
        let credential = auth
            .issue_private_bootstrap(permit, policy, now)
            .map_err(|_| BootstrapError::AuthUnavailable)?;
        let frame = encode(&credential);
        self.pipe
            .write_all(frame.as_slice())
            .map_err(|_| BootstrapError::IoUnavailable)?;
        self.pipe.flush().map_err(|_| BootstrapError::IoUnavailable)
    }
}
fn encode(credential: &BootstrapCredential) -> Zeroizing<[u8; FRAME_BYTES]> {
    let mut frame = Zeroizing::new([0u8; FRAME_BYTES]);
    frame[..8].copy_from_slice(MAGIC);
    frame[8..10].copy_from_slice(&VERSION.to_be_bytes());
    frame[10..42].copy_from_slice(&credential.root);
    frame[42..74].copy_from_slice(&credential.instance);
    frame[74..].copy_from_slice(credential.token.as_bytes());
    frame
}
fn decode(frame: &[u8], root: &Path) -> Result<BootstrapCredential> {
    if frame.len() != FRAME_BYTES || &frame[..8] != MAGIC || frame[8..10] != VERSION.to_be_bytes() {
        return Err(BootstrapError::InvalidFrame);
    }
    let supplied_root: [u8; 32] = frame[10..42]
        .try_into()
        .map_err(|_| BootstrapError::InvalidFrame)?;
    if supplied_root != root_fingerprint(root) {
        return Err(BootstrapError::RootMismatch);
    }
    let instance = frame[42..74]
        .try_into()
        .map_err(|_| BootstrapError::InvalidFrame)?;
    let token = std::str::from_utf8(&frame[74..]).map_err(|_| BootstrapError::InvalidFrame)?;
    BootstrapCredential::from_wire(token, supplied_root, instance)
        .map_err(|_| BootstrapError::InvalidFrame)
}

/// Read one fixed frame followed by EOF from the newly spawned child's actual
/// pipe. There is no blocking reader thread to leak on timeout; the owned reader
/// closes on every return. No frame/token is logged or converted to normal JSON.
pub fn read_child_pipe(
    mut pipe: ChildStdout,
    endpoint_root: &Path,
    timeout: Duration,
) -> Result<BootstrapCredential> {
    if timeout.is_zero() || timeout > MAX_WAIT {
        return Err(BootstrapError::InvalidTimeout);
    }
    let root = std::fs::canonicalize(endpoint_root).map_err(|_| BootstrapError::RootMismatch)?;
    let deadline = Instant::now()
        .checked_add(timeout)
        .ok_or(BootstrapError::InvalidTimeout)?;
    let mut bytes = Zeroizing::new([0u8; FRAME_BYTES + 1]);
    let mut used = 0;
    loop {
        if Instant::now() >= deadline {
            return Err(BootstrapError::TimedOut);
        }
        if !pipe_readable(&pipe)? {
            std::thread::sleep(Duration::from_millis(2));
            continue;
        }
        match pipe.read(&mut bytes[used..]) {
            Ok(0) => return decode(&bytes[..used], &root),
            Ok(count) => {
                used += count;
                if used > FRAME_BYTES {
                    return Err(BootstrapError::InvalidFrame);
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => {
                return decode(&bytes[..used], &root);
            }
            Err(_) => return Err(BootstrapError::IoUnavailable),
        }
    }
}

#[cfg(unix)]
fn capture_stdout_pipe() -> Result<File> {
    use std::os::fd::FromRawFd;
    let mut metadata: libc::stat = unsafe { std::mem::zeroed() };
    if unsafe { libc::fstat(libc::STDOUT_FILENO, &mut metadata) } != 0
        || metadata.st_mode & libc::S_IFMT != libc::S_IFIFO
    {
        return Err(BootstrapError::PipeRequired);
    }
    let private = unsafe { libc::fcntl(libc::STDOUT_FILENO, libc::F_DUPFD_CLOEXEC, 3) };
    if private < 0 {
        return Err(BootstrapError::IoUnavailable);
    }
    let private = unsafe { File::from_raw_fd(private) };
    let null = std::fs::OpenOptions::new()
        .write(true)
        .open("/dev/null")
        .map_err(|_| BootstrapError::IoUnavailable)?;
    use std::os::fd::AsRawFd;
    if unsafe { libc::dup2(null.as_raw_fd(), libc::STDOUT_FILENO) } == -1 {
        return Err(BootstrapError::IoUnavailable);
    }
    Ok(private)
}
#[cfg(unix)]
fn pipe_readable(pipe: &ChildStdout) -> Result<bool> {
    use std::os::fd::AsRawFd;
    let mut item = libc::pollfd {
        fd: pipe.as_raw_fd(),
        events: libc::POLLIN | libc::POLLHUP,
        revents: 0,
    };
    let result = unsafe { libc::poll(&mut item, 1, 0) };
    if result < 0 {
        return Err(BootstrapError::IoUnavailable);
    }
    if item.revents & (libc::POLLERR | libc::POLLNVAL) != 0 {
        return Err(BootstrapError::IoUnavailable);
    }
    Ok(item.revents & (libc::POLLIN | libc::POLLHUP) != 0)
}
#[cfg(all(windows, target_env = "msvc"))]
#[link(name = "ucrt")]
unsafe extern "C" {
    fn _open_osfhandle(handle: isize, flags: i32) -> i32;
    fn _get_osfhandle(fd: i32) -> isize;
    fn _dup2(from: i32, to: i32) -> i32;
    fn _close(fd: i32) -> i32;
}
#[cfg(all(windows, target_env = "msvc"))]
fn capture_stdout_pipe() -> Result<File> {
    use std::os::windows::io::{FromRawHandle, IntoRawHandle};
    use windows_sys::Win32::{
        Foundation::{CloseHandle, DUPLICATE_SAME_ACCESS, DuplicateHandle, INVALID_HANDLE_VALUE},
        Storage::FileSystem::{FILE_TYPE_PIPE, GetFileType},
        System::{
            Console::{GetStdHandle, STD_OUTPUT_HANDLE, SetStdHandle},
            Threading::GetCurrentProcess,
        },
    };
    let stdout = unsafe { GetStdHandle(STD_OUTPUT_HANDLE) };
    if stdout.is_null()
        || stdout == INVALID_HANDLE_VALUE
        || unsafe { GetFileType(stdout) } != FILE_TYPE_PIPE
        || unsafe { _get_osfhandle(1) } != stdout as isize
    {
        // Private launch is a fresh MSVC process. Diverged/uninitialized CRT
        // stdout is not adopted or repaired as an authority-bearing channel.
        return Err(BootstrapError::PipeRequired);
    }
    let mut private = std::ptr::null_mut();
    if unsafe {
        DuplicateHandle(
            GetCurrentProcess(),
            stdout,
            GetCurrentProcess(),
            &mut private,
            0,
            0,
            DUPLICATE_SAME_ACCESS,
        )
    } == 0
    {
        return Err(BootstrapError::IoUnavailable);
    }
    let private = unsafe { File::from_raw_handle(private) };
    let null = std::fs::OpenOptions::new()
        .write(true)
        .open("NUL")
        .map_err(|_| BootstrapError::IoUnavailable)?
        .into_raw_handle();
    // Ownership transfers to the CRT descriptor only on successful open.
    const O_WRONLY_BINARY_NOINHERIT: i32 = 0x0001 | 0x8000 | 0x0080;
    let null_fd = unsafe { _open_osfhandle(null as isize, O_WRONLY_BINARY_NOINHERIT) };
    if null_fd == -1 {
        unsafe {
            CloseHandle(null);
        }
        return Err(BootstrapError::IoUnavailable);
    }
    // _dup2 closes the old CRT fd1/pipe and installs a live NUL descriptor. Do
    // NOT CloseHandle(stdout) afterward: its value could already be reused.
    if unsafe { _dup2(null_fd, 1) } == -1 {
        unsafe {
            _close(null_fd);
        }
        return Err(BootstrapError::IoUnavailable);
    }
    let nul_stdout = unsafe { _get_osfhandle(1) };
    let redirected = nul_stdout != -1
        && nul_stdout != -2
        && unsafe { SetStdHandle(STD_OUTPUT_HANDLE, nul_stdout as _) } != 0;
    unsafe {
        _close(null_fd);
    }
    if !redirected {
        // Fail private entry before actor/children; clear any stale standard
        // handle best-effort. CRT fd1 itself remains valid NUL until exit.
        unsafe {
            SetStdHandle(STD_OUTPUT_HANDLE, std::ptr::null_mut());
        }
        return Err(BootstrapError::IoUnavailable);
    }
    // CRT owns fd1's NUL for process lifetime. Only the noninheritable duplicate
    // pipe is returned; dropping it cannot free/reuse standard stdout as a DB.
    Ok(private)
}
#[cfg(all(windows, not(target_env = "msvc")))]
fn capture_stdout_pipe() -> Result<File> {
    Err(BootstrapError::PipeRequired)
}
#[cfg(windows)]
fn pipe_readable(pipe: &ChildStdout) -> Result<bool> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::{
        Foundation::{ERROR_BROKEN_PIPE, GetLastError},
        System::Pipes::PeekNamedPipe,
    };
    let mut available = 0;
    if unsafe {
        PeekNamedPipe(
            pipe.as_raw_handle(),
            std::ptr::null_mut(),
            0,
            std::ptr::null_mut(),
            &mut available,
            std::ptr::null_mut(),
        )
    } == 0
    {
        return if unsafe { GetLastError() } == ERROR_BROKEN_PIPE {
            Ok(true)
        } else {
            Err(BootstrapError::IoUnavailable)
        };
    }
    Ok(available > 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Owner, PeerIdentity, Server, StopSignal,
        actor::WorkspaceActor,
        auth::{AuthError, Policy},
    };
    use devforge_core::{ErrorCode, SecretStore};
    use std::sync::Arc;
    struct DenyStore;
    impl SecretStore for DenyStore {
        fn get(&self, _: &str) -> std::result::Result<Option<String>, ErrorCode> {
            panic!("no native-store probe in private bootstrap")
        }
        fn set(&self, _: &str, _: &str) -> std::result::Result<(), ErrorCode> {
            panic!("no native-store write in private bootstrap")
        }
        fn delete(&self, _: &str) -> std::result::Result<(), ErrorCode> {
            panic!("no native-store delete in private bootstrap")
        }
    }
    fn peer() -> PeerIdentity {
        PeerIdentity {
            user: "fixture-owner".into(),
            process_id: Some(71),
        }
    }
    fn with_actual_ready(f: impl FnOnce(&mut DesktopAuth, ReadyPermit)) {
        let mut builder = tempfile::Builder::new();
        builder.prefix("df-ready-");
        #[cfg(unix)]
        let container = builder.tempdir_in("/tmp").unwrap();
        #[cfg(not(unix))]
        let container = builder.tempdir().unwrap();
        // A hosted Windows administrator's default TempDir owner can be the
        // Administrators SID rather than TokenUser. Create only our fresh child
        // with the production private-directory adapter; do not weaken ACL checks.
        let root = container.path().join("data");
        crate::transport::os::ensure_private_dir(&root).unwrap();
        let owner = Owner::acquire(&root).unwrap();
        let actor =
            WorkspaceActor::start(owner.claim_workspace().unwrap(), Arc::new(DenyStore)).unwrap();
        let handle = actor.handle();
        let mut auth =
            DesktopAuth::new(owner.endpoint().clone(), peer().user, Policy::default()).unwrap();
        let stop = StopSignal::default();
        let ready_stop = stop.clone();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime
            .block_on(Server::new(owner).run_async_ready(
                stop,
                move |listener| {
                    let permit = ReadyPermit::from_ready(listener, &handle).unwrap();
                    f(&mut auth, permit);
                    ready_stop.request();
                    Ok(())
                },
                |_, _| async { Ok(serde_json::Value::Null) },
            ))
            .unwrap();
        actor.shutdown().unwrap();
    }
    #[test]
    fn actual_ready_bootstrap_expiry_is_short_and_cannot_be_redeemed_late() {
        with_actual_ready(|auth, permit| {
            let now = Instant::now();
            let credential = auth
                .issue_private_bootstrap(permit, NativeGuiPolicy::default(), now)
                .unwrap();
            assert_eq!(
                auth.redeem_private_bootstrap(&peer(), &credential, now + Duration::from_secs(31))
                    .unwrap_err(),
                AuthError::BootstrapUnavailable
            );
            assert_eq!(
                auth.redeem_private_bootstrap(&peer(), &credential, now + Duration::from_secs(32))
                    .unwrap_err(),
                AuthError::BootstrapUnavailable
            );
        });
    }
    #[test]
    fn private_bootstrap_foreign_peer_does_not_consume_and_revoke_removes_pending_context() {
        with_actual_ready(|auth, permit| {
            let now = Instant::now();
            let credential = auth
                .issue_private_bootstrap(permit, NativeGuiPolicy::default(), now)
                .unwrap();
            let foreign = PeerIdentity {
                user: "foreign".into(),
                process_id: Some(71),
            };
            assert_eq!(
                auth.redeem_private_bootstrap(&foreign, &credential, now)
                    .unwrap_err(),
                AuthError::Unauthorized
            );
            auth.revoke_all();
            assert_eq!(
                auth.redeem_private_bootstrap(&peer(), &credential, now)
                    .unwrap_err(),
                AuthError::BootstrapUnavailable
            );
        });
    }
}
