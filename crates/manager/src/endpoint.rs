use crate::{Error, Result};
use devforge_platform::distribution::OperationLease;
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    path::{Path, PathBuf},
};

#[derive(Debug, Clone)]
pub struct Endpoint {
    pub(crate) root: PathBuf,
    pub(crate) directory: PathBuf,
    pub(crate) address: PathBuf,
}
impl Endpoint {
    /// Resolve an existing private endpoint. This never accepts a socket or pipe
    /// address from an untrusted request and never creates or repairs paths.
    pub fn existing(root: &Path) -> Result<Self> {
        let root = root_path(root)?;
        let directory = root.join("control");
        crate::transport::os::validate_private_dir(&directory)?;
        Self::resolved(root, directory)
    }
    fn resolved(root: PathBuf, directory: PathBuf) -> Result<Self> {
        // Hash OS-native path bytes, not a lossy Unicode representation.
        #[cfg(unix)]
        let identity = {
            use std::os::unix::ffi::OsStrExt;
            root.as_os_str().as_bytes().to_vec()
        };
        #[cfg(windows)]
        let identity = {
            use std::os::windows::ffi::OsStrExt;
            root.as_os_str()
                .encode_wide()
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>()
        };
        #[cfg(not(any(unix, windows)))]
        let identity = return Err(Error::UnsupportedPlatform);
        let digest = Sha256::digest(&identity)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        #[cfg(unix)]
        let address = {
            use std::os::unix::ffi::OsStrExt;
            let address = directory.join("manager.sock");
            // Darwin sockaddr_un has 104 bytes including its trailing NUL.
            if address.as_os_str().as_bytes().len() > 103 {
                return Err(Error::UnsafeEndpoint);
            }
            address
        };
        #[cfg(windows)]
        let address = PathBuf::from(format!(
            r"\\.\pipe\DevForge-{}-{}",
            crate::transport::os::current_user_digest()?,
            &digest[..24]
        ));
        #[cfg(unix)]
        let _ = digest;
        Ok(Self {
            root,
            directory,
            address,
        })
    }
    pub fn root(&self) -> &Path {
        &self.root
    }
    /// For diagnostics only. Construct clients using this Endpoint, not a path.
    pub fn address(&self) -> &Path {
        &self.address
    }
    pub(crate) fn validate(&self) -> Result<()> {
        let current = Self::existing(&self.root)?;
        if current.directory != self.directory || current.address != self.address {
            return Err(Error::UnsafeEndpoint);
        }
        Ok(())
    }
}
fn root_path(root: &Path) -> Result<PathBuf> {
    if !root.is_absolute() {
        return Err(Error::UnsafeEndpoint);
    }
    let metadata = fs::symlink_metadata(root)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(Error::UnsafeEndpoint);
    }
    crate::transport::os::validate_root(root)?;
    Ok(fs::canonicalize(root)?)
}

/// Singleton lifetime and installer protection. Desktop AppLease remains a
/// separate GUI singleton; a headless owner also blocks installer replacement.
pub struct Owner {
    endpoint: Endpoint,
    _lock: File,
    _operation: OperationLease,
}
impl Owner {
    pub fn acquire(root: &Path) -> Result<Self> {
        let root = root_path(root)?;
        let directory = root.join("control");
        crate::transport::os::ensure_private_dir(&directory)?;
        let endpoint = Endpoint::resolved(root.clone(), directory.clone())?;
        let mut options = OpenOptions::new();
        options.create(true).truncate(false).read(true).write(true);
        crate::transport::os::secure_lock_options(&mut options);
        let lock_path = directory.join("manager.lock");
        if let Ok(meta) = fs::symlink_metadata(&lock_path)
            && (!meta.is_file() || meta.file_type().is_symlink())
        {
            return Err(Error::UnsafeEndpoint);
        }
        let lock = options.open(&lock_path)?;
        crate::transport::os::validate_lock(&lock)?;
        lock.try_lock().map_err(|e| match e {
            std::fs::TryLockError::WouldBlock => Error::Busy,
            std::fs::TryLockError::Error(e) => Error::Io(e),
        })?;
        let operation = OperationLease::acquire(&root).map_err(|e| {
            if e.kind() == std::io::ErrorKind::WouldBlock {
                Error::Busy
            } else {
                Error::Io(e)
            }
        })?;
        Ok(Self {
            endpoint,
            _lock: lock,
            _operation: operation,
        })
    }
    pub fn endpoint(&self) -> &Endpoint {
        &self.endpoint
    }
}
