//! Metadata-only offline readiness; never downloads, resolves secrets, starts tools,
//! acquires runtime leases or replaces a selected package with a catalog default.
use crate::{Result, Workspace, error};
use devforge_projects::{project::Project, storage_metadata};
use devforge_runtime::package::{Package, Target};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Write},
    path::{Component, Path},
};
#[derive(Clone, Copy)]
pub struct OfflineLimits {
    pub max_requirements: usize,
    pub max_registry_rows: usize,
    pub max_receipt_bytes: usize,
    pub max_total_receipt_bytes: usize,
}
impl Default for OfflineLimits {
    fn default() -> Self {
        Self {
            max_requirements: 32,
            max_registry_rows: 2048,
            max_receipt_bytes: 262144,
            max_total_receipt_bytes: 2 * 1024 * 1024,
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Availability {
    Available,
    Missing,
    Unsupported,
    Unverified,
}
#[derive(Debug, Serialize)]
pub struct Requirement {
    pub role: &'static str,
    pub package_id: Option<String>,
    pub selection_basis: &'static str,
    pub state: Availability,
    pub reason: &'static str,
    pub version: Option<String>,
    pub archive_sha256: Option<String>,
    pub superseded_by: Option<String>,
    pub verification: &'static str,
}
#[derive(Debug, Serialize)]
pub struct Prerequisite {
    pub code: &'static str,
    pub state: Availability,
    pub reason: &'static str,
}
#[derive(Debug, Serialize)]
pub struct OfflineReport {
    pub schema: u32,
    pub project_id: String,
    pub project_revision: Option<u64>,
    pub scope: &'static str,
    pub snapshot_semantics: &'static str,
    pub catalog_fingerprint: Option<String>,
    pub catalog_provenance: &'static str,
    pub managed_artifacts_available: bool,
    pub offline_ready: Option<bool>,
    pub requirements: Vec<Requirement>,
    pub prerequisites: Vec<Prerequisite>,
    pub problems: Vec<&'static str>,
    pub registry_complete: bool,
    pub receipt_bytes_read: usize,
}
struct BoundedHash {
    hash: Sha256,
    remaining: usize,
}
impl Write for BoundedHash {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > self.remaining {
            return Err(std::io::Error::other("catalog fingerprint limit"));
        }
        self.remaining -= bytes.len();
        self.hash.update(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
fn fingerprint(catalog: &devforge_projects::project::Catalog) -> Option<String> {
    if catalog.packages.len() > 512 || catalog.defaults.len() > 64 || catalog.superseded.len() > 512
    {
        return None;
    }
    let mut writer = BoundedHash {
        hash: Sha256::new(),
        remaining: 1024 * 1024,
    };
    serde_json::to_writer(&mut writer, catalog).ok()?;
    Some(
        writer
            .hash
            .finalize()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect(),
    )
}
fn linked(meta: &fs::Metadata) -> bool {
    if meta.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if meta.file_attributes() & 0x400 != 0 {
            return true;
        }
    }
    false
}
fn metadata(root: &Path, relative: &Path) -> std::result::Result<fs::Metadata, &'static str> {
    if relative.as_os_str().len() > 4096 {
        return Err("path_limit");
    }
    let mut path = root.to_owned();
    let mut count = 0;
    for part in relative.components() {
        count += 1;
        if count > 32 {
            return Err("path_limit");
        }
        let Component::Normal(name) = part else {
            return Err("invalid_relative_path");
        };
        path.push(name);
        let m = fs::symlink_metadata(&path).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                "missing"
            } else {
                "metadata_unavailable"
            }
        })?;
        if linked(&m) {
            return Err("link_not_followed");
        }
    }
    fs::symlink_metadata(path).map_err(|_| "metadata_unavailable")
}
struct Reader<'a> {
    root: &'a Path,
    limits: OfflineLimits,
    bytes: usize,
}
impl Reader<'_> {
    fn read(&mut self, relative: &Path) -> std::result::Result<Vec<u8>, &'static str> {
        let m = metadata(self.root, relative)?;
        if !m.is_file() {
            return Err("metadata_not_regular");
        }
        let cap = self.limits.max_receipt_bytes.min(
            self.limits
                .max_total_receipt_bytes
                .saturating_sub(self.bytes),
        );
        if m.len() > cap as u64 {
            return Err("receipt_byte_limit");
        }
        let mut file = crate::storage::open_metadata_file(&self.root.join(relative), cap as u64)
            .ok_or("metadata_open_unverified")?;
        let expected = file
            .metadata()
            .map_err(|_| "metadata_open_unverified")?
            .len();
        let mut bytes = vec![];
        (&mut file)
            .take(cap as u64)
            .read_to_end(&mut bytes)
            .map_err(|_| "metadata_read_unavailable")?;
        self.bytes = self.bytes.saturating_add(bytes.len());
        let after = file
            .metadata()
            .map_err(|_| "metadata_open_unverified")?
            .len();
        if bytes.len() as u64 != expected || after != expected || expected != m.len() {
            return Err("receipt_changed_during_read");
        }
        Ok(bytes)
    }
    fn package(
        &mut self,
        catalog: &devforge_projects::project::Catalog,
        role: &'static str,
        id: &str,
        basis: &'static str,
    ) -> Requirement {
        let mut result = Requirement {
            role,
            package_id: (id.len() <= 256).then(|| id.into()),
            selection_basis: basis,
            state: Availability::Unverified,
            reason: "selected_pin_not_in_catalog",
            version: None,
            archive_sha256: None,
            superseded_by: None,
            verification: "not_verified",
        };
        if id.len() > 256 {
            result.reason = "selected_pin_limit";
            return result;
        }
        let Ok(package) = catalog.get(id) else {
            return result;
        };
        if package.executables.len() + package.resources.len() > 64 {
            result.reason = "descriptor_entry_limit";
            return result;
        }
        let role_matches = match role {
            "php" | "php_web" => package.kind == "php",
            "node" => package.kind == "node",
            "composer" => package.kind == "composer",
            "proxy" => package.kind == "caddy",
            "database" => ["mysql", "postgresql"].contains(&package.kind.as_str()),
            "cache" => package.kind == "garnet",
            "cache_runtime" => package.kind == "dotnet",
            _ => false,
        };
        if !role_matches {
            result.reason = "selected_package_role_mismatch";
            return result;
        }
        let Ok(key) = package.key() else {
            result.reason = "invalid_package_descriptor";
            return result;
        };
        result.version = Some(package.version.clone());
        result.archive_sha256 = Some(package.sha256.clone());
        result.superseded_by = catalog
            .superseded
            .get(id)
            .filter(|id| id.len() <= 256)
            .cloned();
        if Some(package.target) != Target::current().ok() {
            result.state = Availability::Unsupported;
            result.reason = "foreign_target_selection";
            return result;
        }
        let relative = Path::new("runtimes/packages").join(key);
        let bytes = match self.read(&relative.join(".devforge-receipt.json")) {
            Ok(bytes) => bytes,
            Err("missing") => {
                result.state = Availability::Missing;
                result.reason = "package_receipt_missing";
                return result;
            }
            Err(reason) => {
                result.reason = reason;
                return result;
            }
        };
        let receipt: serde_json::Value = match serde_json::from_slice(&bytes) {
            Ok(value) => value,
            Err(_) => {
                result.reason = "invalid_package_receipt";
                return result;
            }
        };
        let recorded = serde_json::from_value::<Package>(receipt["package"].clone()).ok();
        if receipt["schema"] != 1 || recorded.as_ref() != Some(package) {
            result.reason = "receipt_catalog_mismatch";
            return result;
        }
        for (name, file) in package.executables.iter().chain(package.resources.iter()) {
            let hash_key = if package.executables.contains_key(name) {
                "hashes"
            } else {
                "resource_hashes"
            };
            let valid_hash = receipt[hash_key][name].as_str().is_some_and(|hash| {
                hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit())
            });
            if !valid_hash {
                result.reason = "receipt_file_hash_missing";
                return result;
            }
            match metadata(self.root, &relative.join(file)) {
                Ok(m) if m.is_file() => {}
                Ok(_) => {
                    result.reason = "package_file_not_regular";
                    return result;
                }
                Err("missing") => {
                    result.state = Availability::Missing;
                    result.reason = "package_file_missing";
                    return result;
                }
                Err(reason) => {
                    result.reason = reason;
                    return result;
                }
            }
        }
        result.state = Availability::Available;
        result.reason = "receipt_and_regular_files_present";
        result.verification =
            "receipt_match_and_file_metadata_only_payload_hash_and_execution_unverified";
        result
    }
}
impl OfflineReport {
    fn problem(&mut self, code: &'static str) {
        if !self.problems.contains(&code) {
            self.problems.push(code);
        }
    }
    fn unknown(&mut self, role: &'static str, reason: &'static str) {
        self.requirements.push(Requirement {
            role,
            package_id: None,
            selection_basis: "unresolved",
            state: Availability::Unverified,
            reason,
            version: None,
            archive_sha256: None,
            superseded_by: None,
            verification: "not_verified",
        });
    }
}
fn prerequisite(
    project: &Project,
    relative: &str,
    directory: bool,
    code: &'static str,
) -> Prerequisite {
    let result = metadata(&project.path, Path::new(relative));
    let (state, reason) = match result {
        Ok(m) if m.is_dir() == directory && (directory || m.is_file()) => (
            Availability::Unverified,
            "present_metadata_only_completeness_unverified",
        ),
        Ok(_) => (Availability::Unverified, "unexpected_file_type"),
        Err("missing") => (Availability::Missing, "not_present"),
        Err(code) => (Availability::Unverified, code),
    };
    Prerequisite {
        code,
        state,
        reason,
    }
}
impl Workspace {
    pub fn offline_readiness(&self, id: &str, limits: OfflineLimits) -> Result<OfflineReport> {
        if !devforge_inspector::valid_id(id) {
            return Err(error("invalid_project"));
        }
        if limits.max_requirements == 0
            || limits.max_requirements > 64
            || limits.max_registry_rows == 0
            || limits.max_registry_rows > 4096
            || limits.max_receipt_bytes == 0
            || limits.max_receipt_bytes > 262144
            || limits.max_total_receipt_bytes == 0
            || limits.max_total_receipt_bytes > 2 * 1024 * 1024
        {
            return Err(error("offline_limits_invalid"));
        }
        let mut report = OfflineReport {
            schema: 1,
            project_id: id.into(),
            project_revision: None,
            scope: "selected_managed_runtime_artifacts_and_project_metadata_presence_only",
            snapshot_semantics: "bounded_non_atomic_no_hostile_filesystem_sandbox",
            catalog_fingerprint: fingerprint(&self.catalog),
            catalog_provenance: "current_workspace_catalog_fingerprint_not_remote_signature_or_publisher_revalidation",
            managed_artifacts_available: false,
            offline_ready: None,
            requirements: vec![],
            prerequisites: vec![],
            problems: vec![],
            registry_complete: false,
            receipt_bytes_read: 0,
        };
        if report.catalog_fingerprint.is_none() {
            report.problem("catalog_fingerprint_limit");
            return Ok(report);
        }
        let snapshot = self.projects.storage_metadata(storage_metadata::Limits {
            max_rows: limits.max_registry_rows,
            ..Default::default()
        });
        report.registry_complete = snapshot.complete;
        for problem in snapshot.problems {
            report.problem(problem);
        }
        let Some(project) = snapshot.projects.iter().find(|p| p.id == id) else {
            if report.registry_complete {
                return Err(devforge_projects::ProjectError::NotFound.into());
            }
            report.problem("project_metadata_incomplete");
            return Ok(report);
        };
        report.project_revision = Some(project.revision);
        let mut selected: Vec<(&'static str, String, &'static str)> = vec![
            ("php", project.php.clone(), "persisted_project_pin"),
            ("node", project.node.clone(), "persisted_project_pin"),
            (
                "composer",
                project.composer.clone(),
                "persisted_project_pin",
            ),
        ];
        if let Ok(php) = self.catalog.get(&project.php) {
            if let Ok(server) = self.catalog.php_server(php) {
                if let Some((id, _)) = self.catalog.packages.iter().find(|(_, p)| *p == server) {
                    selected.push(("php_web", id.clone(), "catalog_pair_for_persisted_php"));
                }
            } else {
                report.unknown("php_web", "matching_web_runtime_not_in_catalog");
            }
        } else {
            report.unknown("php_web", "selected_php_pin_unknown");
        }
        // Serving currently resolves this provider from the reviewed catalog
        // default. Mark that basis explicitly; this is not a persisted project pin.
        if let Some((id, _)) = self.catalog.packages.iter().find(|(_, p)| {
            p.kind == "caddy"
                && Some(&p.version) == self.catalog.defaults.get("caddy")
                && Some(p.target) == Target::current().ok()
        }) {
            selected.push((
                "proxy",
                id.clone(),
                "current_catalog_default_not_project_pin",
            ));
        } else {
            report.unknown("proxy", "default_proxy_unresolved");
        }
        if let Some(binding) = snapshot.active_bindings.iter().find(|b| b.project_id == id) {
            if let Some(instance) = snapshot
                .instances
                .iter()
                .find(|i| i.id == binding.instance_id)
            {
                selected.push((
                    "database",
                    instance.package.clone(),
                    "persisted_database_instance_pin",
                ));
                if !instance.ready || !binding.ready {
                    report.problem("database_initialization_unverified");
                }
            } else {
                report.unknown("database", "database_instance_metadata_unavailable");
            }
        }
        if project.services.uses_redis() {
            if let Some(cache) = snapshot
                .caches
                .iter()
                .find(|c| Some(c.id.as_str()) == project.services.redis_instance.as_deref())
            {
                selected.push((
                    "cache",
                    cache.package.clone(),
                    "persisted_cache_instance_pin",
                ));
                selected.push((
                    "cache_runtime",
                    cache.runtime.clone(),
                    "persisted_cache_runtime_pin",
                ));
                if !cache.ready {
                    report.problem("cache_initialization_unverified");
                }
            } else {
                report.unknown("cache", "cache_instance_metadata_unavailable");
            }
        }
        if project.debugger.mode != devforge_debugger::Mode::Off {
            report.unknown(
                "debugger",
                "optional_debugger_provider_not_checked_in_artifact_scope",
            );
        }
        let mut reader = Reader {
            root: &self.root,
            limits,
            bytes: 0,
        };
        let owned = match reader.read(Path::new("runtimes/.devforge-runtime-store")) {
            Ok(bytes) => bytes == b"devforge-runtime-store-v1\n",
            Err(reason) => {
                report.problem(reason);
                false
            }
        };
        for (role, package, basis) in selected {
            if report.requirements.len() >= limits.max_requirements {
                report.problem("requirement_limit");
                break;
            }
            if owned {
                report
                    .requirements
                    .push(reader.package(&self.catalog, role, &package, basis));
            } else {
                report.unknown(role, "runtime_store_ownership_unverified");
            }
        }
        if report.requirements.len() > limits.max_requirements {
            report.requirements.truncate(limits.max_requirements);
            report.problem("requirement_limit");
        }
        report.receipt_bytes_read = reader.bytes;
        let root_safe = project.path.as_os_str().len() <= 4096
            && fs::symlink_metadata(&project.path).is_ok_and(|m| m.is_dir() && !linked(&m))
            && fs::canonicalize(&project.path).is_ok_and(|path| path == project.path)
            && project
                .filesystem_identity
                .as_ref()
                .is_some_and(|expected| {
                    devforge_projects::discovery::identity(&project.path)
                        .is_ok_and(|actual| &actual == expected)
                });
        if root_safe {
            report.prerequisites.push(prerequisite(
                project,
                "vendor/autoload.php",
                false,
                "composer_dependencies",
            ));
            report.prerequisites.push(prerequisite(
                project,
                ".env",
                false,
                "local_environment_file",
            ));
            match metadata(&project.path, Path::new("package.json")) {
                Ok(m) if m.is_file() => report.prerequisites.push(prerequisite(
                    project,
                    "node_modules",
                    true,
                    "node_dependencies",
                )),
                Err("missing") => {}
                _ => report.problem("node_manifest_metadata_unverified"),
            }
        } else {
            report.problem("project_root_identity_unverified_no_project_metadata_visited");
        }
        report.problem("secret_availability_not_queried");
        report.problem("minimum_os_and_provider_vendor_support_not_evaluated");
        report.problem("dependency_cache_payload_integrity_and_service_readiness_unverified");
        report.managed_artifacts_available = report.registry_complete
            && !report.problems.contains(&"requirement_limit")
            && !report.requirements.is_empty()
            && report
                .requirements
                .iter()
                .all(|r| r.state == Availability::Available);
        if report
            .requirements
            .iter()
            .any(|r| matches!(r.state, Availability::Missing | Availability::Unsupported))
        {
            report.offline_ready = Some(false);
        }
        report.requirements.sort_by(|a, b| a.role.cmp(b.role));
        Ok(report)
    }
}
