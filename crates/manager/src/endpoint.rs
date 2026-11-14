use crate::{Error, Result};
use devforge_platform::distribution::OperationLease;
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
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
    lease: Arc<OwnerLease>,
}
/// Logical ownership must release the OS lock explicitly. A fork/duplicated
/// descriptor can retain an open-file-description after our File closes, even
/// with CLOEXEC (the child may not have reached exec yet).
struct ManagerLock(File);
impl Drop for ManagerLock {
    fn drop(&mut self) {
        let _ = self.0.unlock();
    }
}
struct OwnerLease {
    _lock: ManagerLock,
    _operation: OperationLease,
    workspace_claimed: AtomicBool,
}
/// Transitional legacy CLI exclusion only. This is not a manager owner and does
/// not hold OperationLease: legacy modules retain their original shared/exclusive
/// policies, including config setup's exclusive lease.
pub struct LegacyExclusion {
    _lock: ManagerLock,
}
impl LegacyExclusion {
    pub fn acquire(root: &Path) -> Result<Self> {
        let (_, _, lock) = acquire_manager_lock(root)?;
        Ok(Self { _lock: lock })
    }
}
fn acquire_manager_lock(root: &Path) -> Result<(PathBuf, PathBuf, ManagerLock)> {
    let root = root_path(root)?;
    let directory = root.join("control");
    crate::transport::os::ensure_private_dir(&directory)?;
    let mut options = OpenOptions::new();
    options.create(true).truncate(false).read(true).write(true);
    crate::transport::os::secure_lock_options(&mut options);
    let lock_path = directory.join("manager.lock");
    if let Ok(meta) = fs::symlink_metadata(&lock_path)
        && (!meta.is_file() || meta.file_type().is_symlink())
    {
        return Err(Error::UnsafeEndpoint);
    }
    let lock = options.open(lock_path)?;
    crate::transport::os::validate_lock(&lock)?;
    lock.try_lock().map_err(|error| match error {
        std::fs::TryLockError::WouldBlock => Error::Busy,
        std::fs::TryLockError::Error(error) => Error::Io(error),
    })?;
    Ok((root, directory, ManagerLock(lock)))
}
/// One-use in-process authority to construct this owner's Workspace worker.
/// It cannot be cloned or decoded from wire input. The worker retains its OS
/// leases until Workspace cleanup finishes, even after the Server/Owner drops.
pub struct WorkspacePermit {
    endpoint: Endpoint,
    _lease: Arc<OwnerLease>,
}
impl WorkspacePermit {
    pub fn root(&self) -> &Path {
        self.endpoint.root()
    }
    pub fn endpoint(&self) -> &Endpoint {
        &self.endpoint
    }
}
impl Owner {
    pub fn acquire(root: &Path) -> Result<Self> {
        let (root, directory, lock) = acquire_manager_lock(root)?;
        let endpoint = Endpoint::resolved(root.clone(), directory)?;
        let operation = OperationLease::acquire(&root).map_err(|e| {
            if e.kind() == std::io::ErrorKind::WouldBlock {
                Error::Busy
            } else {
                Error::Io(e)
            }
        })?;
        Ok(Self {
            endpoint,
            lease: Arc::new(OwnerLease {
                _lock: lock,
                _operation: operation,
                workspace_claimed: AtomicBool::new(false),
            }),
        })
    }
    pub fn endpoint(&self) -> &Endpoint {
        &self.endpoint
    }
    pub fn claim_workspace(&self) -> Result<WorkspacePermit> {
        if self.lease.workspace_claimed.swap(true, Ordering::AcqRel) {
            return Err(Error::Busy);
        }
        Ok(WorkspacePermit {
            endpoint: self.endpoint.clone(),
            _lease: self.lease.clone(),
        })
    }
}

#[cfg(all(test, unix))]
mod lock_release_tests {
    use super::*;

    #[test]
    fn legacy_exclusion_releases_lock_with_duplicate_descriptor_alive() {
        let root = tempfile::Builder::new()
            .prefix("df-lease-")
            .tempdir_in("/tmp")
            .unwrap();
        let legacy = LegacyExclusion::acquire(root.path()).unwrap();
        let inherited = legacy._lock.0.try_clone().unwrap();
        drop(legacy);
        let next = LegacyExclusion::acquire(root.path()).unwrap();
        drop(next);
        drop(inherited);
    }

    #[test]
    fn final_workspace_permit_releases_owner_lock_with_duplicate_descriptor_alive() {
        let root = tempfile::Builder::new()
            .prefix("df-lease-")
            .tempdir_in("/tmp")
            .unwrap();
        let owner = Owner::acquire(root.path()).unwrap();
        let inherited = owner.lease._lock.0.try_clone().unwrap();
        let permit = owner.claim_workspace().unwrap();
        drop(owner);
        // The worker must keep ownership and installer protection until cleanup.
        assert!(matches!(Owner::acquire(root.path()), Err(Error::Busy)));
        assert!(OperationLease::exclusive(root.path()).is_err());
        drop(permit);
        let next = Owner::acquire(root.path()).unwrap();
        drop(next);
        drop(inherited);
    }
}
