//! Loopback serving and narrowly scoped system integration. Blocking APIs belong
//! on workers. The privileged helper never launches project code or reads keys.
pub mod authority;
mod config;
mod fastcgi;
pub mod hosts;
mod hot;
pub mod manager;
pub mod site;
pub mod system;
use serde::{Deserialize, Serialize};
use std::{fs, io::Write, path::Path};
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServingError {
    InvalidDomain,
    DomainConflict,
    InvalidPublicRoot,
    InvalidConfiguration,
    StorageUnavailable,
    SecretUnavailable,
    AuthorityInvalid,
    CertificateExpired,
    PermissionRequired,
    HostsChanged,
    HostsConflict,
    TrustFailed,
    RuntimeUnavailable,
    PortUnavailable,
    StartFailed,
    ProbeFailed,
    ViteUnavailable,
    HotFileConflict,
    ProjectUnavailable,
    ElevatedProcessUnsupported,
}
impl std::fmt::Display for ServingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            serde_json::to_value(self).unwrap().as_str().unwrap()
        )
    }
}
impl std::error::Error for ServingError {}
impl From<std::io::Error> for ServingError {
    fn from(_: std::io::Error) -> Self {
        Self::StorageUnavailable
    }
}
impl From<devforge_projects::ProjectError> for ServingError {
    fn from(_: devforge_projects::ProjectError) -> Self {
        Self::ProjectUnavailable
    }
}
impl From<devforge_runtime::RuntimeError> for ServingError {
    fn from(_: devforge_runtime::RuntimeError) -> Self {
        Self::RuntimeUnavailable
    }
}
pub type Result<T> = std::result::Result<T, ServingError>;
pub fn id() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}
pub fn hash(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
pub(crate) fn private_dir(path: &Path) -> Result<()> {
    fs::create_dir_all(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}
pub(crate) fn atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path.parent().ok_or(ServingError::StorageUnavailable)?;
    private_dir(parent)?;
    let mut f = tempfile::NamedTempFile::new_in(parent)?;
    f.write_all(bytes)?;
    f.as_file().sync_all()?;
    f.persist(path)
        .map_err(|_| ServingError::StorageUnavailable)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn serving_errors_have_all_initial_catalogs() {
        for data in [
            include_str!("../../../shared/locales/en.json"),
            include_str!("../../../shared/locales/ru.json"),
            include_str!("../../../shared/locales/uz-Latn.json"),
        ] {
            let catalog: serde_json::Value = serde_json::from_str(data).unwrap();
            for error in [
                ServingError::InvalidDomain,
                ServingError::DomainConflict,
                ServingError::InvalidPublicRoot,
                ServingError::InvalidConfiguration,
                ServingError::StorageUnavailable,
                ServingError::SecretUnavailable,
                ServingError::AuthorityInvalid,
                ServingError::CertificateExpired,
                ServingError::PermissionRequired,
                ServingError::HostsChanged,
                ServingError::HostsConflict,
                ServingError::TrustFailed,
                ServingError::RuntimeUnavailable,
                ServingError::PortUnavailable,
                ServingError::StartFailed,
                ServingError::ProbeFailed,
                ServingError::ViteUnavailable,
                ServingError::HotFileConflict,
                ServingError::ProjectUnavailable,
                ServingError::ElevatedProcessUnsupported,
            ] {
                assert!(
                    catalog["serving"]["errors"][error.to_string()]
                        .as_str()
                        .is_some_and(|s| !s.is_empty())
                );
            }
        }
    }
}
