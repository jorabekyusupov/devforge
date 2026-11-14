use crate::{Endpoint, Error, PeerIdentity, Result};
use std::{
    fs::{self, File, OpenOptions},
    os::{
        fd::AsRawFd,
        unix::fs::{DirBuilderExt, FileTypeExt, MetadataExt, OpenOptionsExt},
    },
    path::Path,
};
pub(crate) type Stream = tokio::net::UnixStream;
pub(crate) struct Listener {
    listener: tokio::net::UnixListener,
}
pub(crate) fn current_user() -> Result<String> {
    Ok(unsafe { libc::geteuid() }.to_string())
}
pub(crate) fn validate_root(path: &Path) -> Result<()> {
    let meta = fs::symlink_metadata(path)?;
    if meta.uid() != unsafe { libc::geteuid() } || meta.mode() & 0o022 != 0 {
        return Err(Error::UnsafeEndpoint);
    }
    Ok(())
}
pub(crate) fn validate_private_dir(path: &Path) -> Result<()> {
    let meta = fs::symlink_metadata(path)?;
    if !meta.is_dir()
        || meta.file_type().is_symlink()
        || meta.uid() != unsafe { libc::geteuid() }
        || meta.mode() & 0o077 != 0
    {
        return Err(Error::UnsafeEndpoint);
    }
    Ok(())
}
pub(crate) fn ensure_private_dir(path: &Path) -> Result<()> {
    match fs::DirBuilder::new().mode(0o700).create(path) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(e.into()),
    }
    validate_private_dir(path)
}
pub(crate) fn secure_lock_options(options: &mut OpenOptions) {
    options
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
}
pub(crate) fn validate_lock(file: &File) -> Result<()> {
    let meta = file.metadata()?;
    if !meta.is_file()
        || meta.uid() != unsafe { libc::geteuid() }
        || meta.mode() & 0o077 != 0
        || meta.nlink() != 1
    {
        return Err(Error::UnsafeEndpoint);
    }
    Ok(())
}
fn check_socket(path: &Path) -> Result<()> {
    let meta = fs::symlink_metadata(path)?;
    if !meta.file_type().is_socket() || meta.uid() != unsafe { libc::geteuid() } {
        return Err(Error::UnsafeEndpoint);
    }
    Ok(())
}
impl Listener {
    pub(crate) fn bind(endpoint: &Endpoint) -> Result<Self> {
        if fs::symlink_metadata(&endpoint.address).is_ok() {
            check_socket(&endpoint.address)?;
            // Caller still owns ManagerLease: no live owner can use this path.
            fs::remove_file(&endpoint.address)?;
        }
        Ok(Self {
            listener: tokio::net::UnixListener::bind(&endpoint.address)?,
        })
    }
    pub(crate) async fn accept(&mut self) -> Result<Stream> {
        Ok(self.listener.accept().await?.0)
    }
}
pub(crate) async fn connect(endpoint: &Endpoint) -> Result<Stream> {
    check_socket(&endpoint.address)?;
    Ok(Stream::connect(&endpoint.address).await?)
}
pub(crate) fn peer(stream: &Stream, _server: bool) -> Result<PeerIdentity> {
    #[cfg(target_os = "macos")]
    let (user, pid) = {
        let mut uid: libc::uid_t = 0;
        let mut gid: libc::gid_t = 0;
        if unsafe { libc::getpeereid(stream.as_raw_fd(), &mut uid, &mut gid) } != 0 {
            return Err(Error::UnauthorizedPeer);
        }
        let mut pid: libc::pid_t = 0;
        let mut length = std::mem::size_of::<libc::pid_t>() as libc::socklen_t;
        if unsafe {
            libc::getsockopt(
                stream.as_raw_fd(),
                libc::SOL_LOCAL,
                libc::LOCAL_PEERPID,
                (&mut pid as *mut libc::pid_t).cast(),
                &mut length,
            )
        } != 0
            || length as usize != std::mem::size_of::<libc::pid_t>()
            || pid <= 0
        {
            return Err(Error::UnauthorizedPeer);
        }
        (uid, Some(pid as u32))
    };
    #[cfg(target_os = "linux")]
    let (user, pid) = {
        let mut credentials: libc::ucred = unsafe { std::mem::zeroed() };
        let mut length = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
        if unsafe {
            libc::getsockopt(
                stream.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_PEERCRED,
                (&mut credentials as *mut libc::ucred).cast(),
                &mut length,
            )
        } != 0
            || length as usize != std::mem::size_of::<libc::ucred>()
        {
            return Err(Error::UnauthorizedPeer);
        }
        (credentials.uid, u32::try_from(credentials.pid).ok())
    };
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    return Err(Error::UnsupportedPlatform);
    if user != unsafe { libc::geteuid() } {
        return Err(Error::UnauthorizedPeer);
    }
    Ok(PeerIdentity {
        user: user.to_string(),
        process_id: pid,
    })
}
pub(crate) fn cleanup(endpoint: &Endpoint) -> Result<()> {
    match fs::symlink_metadata(&endpoint.address) {
        Ok(_) => {
            check_socket(&endpoint.address)?;
            fs::remove_file(&endpoint.address)?;
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.into()),
    }
    Ok(())
}
