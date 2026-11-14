//! Native desktop-only report composition. No external observer tool, alternate
//! store, caller-selected limits, runtime probe or destructive action is added.
use crate::actor::ROOT_SUBJECT;
use devforge_control::Id;
use devforge_workspace::{
    Workspace, catalog::OfflineLimits, cleanup_preview::PreviewLimits,
    environment_report::ReportLimits, storage::StorageLimits,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::OnceLock;

/// Same-account desktop transport authenticates the caller before this local
/// helper is invoked. It is not a standalone authorization boundary.
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ReportReadRequest {
    StorageInventory {},
    RuntimeCleanupPreview { package_id: String },
    OfflineReadiness { project_id: Id },
    EnvironmentReport { project_id: Id },
}
/// Globals have a fixed receipt namespace; clients never choose a project to
/// disguise a whole-installation inventory/cleanup preview as a scoped read.
pub fn root_subject() -> &'static Id {
    static ROOT: OnceLock<Id> = OnceLock::new();
    ROOT.get_or_init(|| Id::new(ROOT_SUBJECT).expect("fixed desktop root subject"))
}
impl ReportReadRequest {
    pub fn subject(&self) -> &Id {
        match self {
            Self::StorageInventory {} | Self::RuntimeCleanupPreview { .. } => root_subject(),
            Self::OfflineReadiness { project_id } | Self::EnvironmentReport { project_id } => {
                project_id
            }
        }
    }
    pub fn valid(&self) -> bool {
        match self {
            Self::StorageInventory {} => true,
            Self::RuntimeCleanupPreview { package_id } => {
                !package_id.is_empty()
                    && package_id.len() <= 200
                    && !matches!(package_id.as_str(), "." | "..")
                    && package_id
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
            }
            Self::OfflineReadiness { project_id } | Self::EnvironmentReport { project_id } => {
                project_id.as_str() != ROOT_SUBJECT
            }
        }
    }
    /// Returns the exact published library object as JSON. The enclosing native
    /// facade must use its existing bounded, ephemeral ReadImage/receipt path;
    /// never persist this value in an operation result or advertise it to MCP.
    pub fn execute(&self, workspace: &Workspace) -> devforge_workspace::Result<Value> {
        if !self.valid() {
            return Err(devforge_workspace::Error {
                scope: "workspace",
                code: "invalid_report_read".into(),
                project_id: None,
            });
        }
        let encoded = match self {
            Self::StorageInventory {} => {
                serde_json::to_value(workspace.storage_inventory(StorageLimits::default()))
            }
            Self::RuntimeCleanupPreview { package_id } => serde_json::to_value(
                workspace.runtime_cleanup_preview(package_id, PreviewLimits::default()),
            ),
            Self::OfflineReadiness { project_id } => serde_json::to_value(
                workspace.offline_readiness(project_id.as_str(), OfflineLimits::default())?,
            ),
            Self::EnvironmentReport { project_id } => serde_json::to_value(
                workspace.environment_report(project_id.as_str(), ReportLimits::default())?,
            ),
        };
        encoded.map_err(|_| devforge_workspace::Error {
            scope: "workspace",
            code: "report_serialization_failed".into(),
            project_id: None,
        })
    }
}
