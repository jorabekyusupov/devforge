//! Project-scoped application services; all blocking I/O belongs on a worker.
pub mod backup;
pub mod cache;
pub mod database;
pub mod database_store;
pub mod discovery;
pub mod env_file;
pub mod environment;
pub mod execution;
pub mod mail;
pub mod manifest;
pub mod mysql_service;
pub mod php_settings;
pub mod project;
pub mod terminal;
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectError {
    InvalidProject,
    InvalidManifest,
    UnsupportedConstraint,
    IncompatiblePhp,
    IncompatibleNode,
    DuplicateProject,
    NotFound,
    StaleRevision,
    WorkspaceManaged,
    StorageUnavailable,
    SecretUnavailable,
    InvalidEnvironment,
    MailUnavailable,
    RuntimeUnavailable,
    ExecutionFailed,
    DatabaseUnavailable,
    DatabaseConflict,
    InstanceBusy,
    RecoveryRequired,
    BackupDestinationExists,
    InvalidBackup,
    BackupUnsupported,
    InvalidEnvFile,
    EnvFileChanged,
    TerminalUnavailable,
    AdministratorUnsupported,
}
impl std::fmt::Display for ProjectError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            serde_json::to_value(self).unwrap().as_str().unwrap()
        )
    }
}
impl std::error::Error for ProjectError {}
impl From<std::io::Error> for ProjectError {
    fn from(_: std::io::Error) -> Self {
        Self::StorageUnavailable
    }
}
impl From<rusqlite::Error> for ProjectError {
    fn from(_: rusqlite::Error) -> Self {
        Self::StorageUnavailable
    }
}
impl From<devforge_runtime::RuntimeError> for ProjectError {
    fn from(_: devforge_runtime::RuntimeError) -> Self {
        Self::RuntimeUnavailable
    }
}
pub type Result<T> = std::result::Result<T, ProjectError>;
pub(crate) fn id() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}
pub(crate) fn password() -> zeroize::Zeroizing<String> {
    zeroize::Zeroizing::new(format!("{}{}", id(), id()))
}
pub(crate) fn private_dir(path: &std::path::Path) -> Result<()> {
    std::fs::create_dir_all(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

#[cfg(test)]
mod localization_tests {
    #[test]
    fn project_errors_have_all_initial_catalogs() {
        for data in [
            include_str!("../../../shared/locales/en.json"),
            include_str!("../../../shared/locales/ru.json"),
            include_str!("../../../shared/locales/uz-Latn.json"),
        ] {
            let catalog: serde_json::Value = serde_json::from_str(data).unwrap();
            for code in [
                super::ProjectError::InvalidProject,
                super::ProjectError::InvalidManifest,
                super::ProjectError::UnsupportedConstraint,
                super::ProjectError::IncompatiblePhp,
                super::ProjectError::IncompatibleNode,
                super::ProjectError::DuplicateProject,
                super::ProjectError::NotFound,
                super::ProjectError::StaleRevision,
                super::ProjectError::WorkspaceManaged,
                super::ProjectError::StorageUnavailable,
                super::ProjectError::SecretUnavailable,
                super::ProjectError::InvalidEnvironment,
                super::ProjectError::MailUnavailable,
                super::ProjectError::RuntimeUnavailable,
                super::ProjectError::ExecutionFailed,
                super::ProjectError::DatabaseUnavailable,
                super::ProjectError::DatabaseConflict,
                super::ProjectError::InstanceBusy,
                super::ProjectError::RecoveryRequired,
                super::ProjectError::InvalidBackup,
                super::ProjectError::BackupUnsupported,
                super::ProjectError::BackupDestinationExists,
                super::ProjectError::InvalidEnvFile,
                super::ProjectError::EnvFileChanged,
                super::ProjectError::TerminalUnavailable,
                super::ProjectError::AdministratorUnsupported,
            ] {
                assert!(
                    catalog["project"]["errors"][code.to_string()]
                        .as_str()
                        .is_some_and(|s| !s.is_empty())
                );
            }
        }
    }
}

pub mod workspace_sqlite;
