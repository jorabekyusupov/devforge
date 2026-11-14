//! Read-only removal review. No deletion authority, free-space promise or apply token.
use crate::{
    Workspace,
    storage::{Inventory, Reference, StorageLimits, open_metadata_file},
};
use devforge_projects::{database::Engine, project::Project, storage_metadata};
use devforge_runtime::package::{Package, RemovalLeaseObservation, RemovalLeaseState, Target};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Write},
    path::{Component, Path},
};
#[derive(Clone, Copy)]
pub struct PreviewLimits {
    pub storage: StorageLimits,
    pub registry: storage_metadata::Limits,
    pub max_references: usize,
}
impl Default for PreviewLimits {
    fn default() -> Self {
        Self {
            storage: StorageLimits::default(),
            registry: storage_metadata::Limits::default(),
            max_references: 128,
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PreviewState {
    Blocked,
    ReviewCandidate,
    Unknown,
}
#[derive(Debug, Serialize)]
pub struct RuntimeCleanupPreview {
    pub schema: u32,
    pub action: &'static str,
    pub package_id: String,
    pub state: PreviewState,
    pub descriptor_sha256: Option<String>,
    pub references: Vec<Reference>,
    pub lease: RemovalLeaseObservation,
    pub logical_bytes: Option<u64>,
    pub size_semantics: &'static str,
    pub snapshot_semantics: &'static str,
    pub apply_semantics: &'static str,
    pub problems: Vec<&'static str>,
}
struct HashWriter {
    hash: Sha256,
    left: usize,
}
impl Write for HashWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > self.left {
            return Err(std::io::Error::other("descriptor limit"));
        }
        self.left -= bytes.len();
        self.hash.update(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
fn registry_id(id: &str) -> bool {
    id.len() == 32 && id.bytes().all(|b| b.is_ascii_hexdigit())
}
fn descriptor_digest(package: &Package) -> Option<String> {
    let mut writer = HashWriter {
        hash: Sha256::new(),
        left: 16384,
    };
    serde_json::to_writer(&mut writer, package).ok()?;
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
fn runtime_receipt_metadata(root: &Path, package: &Package) -> Option<()> {
    let relative = Path::new("runtimes/packages")
        .join(package.key().ok()?)
        .join(".devforge-receipt.json");
    let mut path = root.to_owned();
    for part in relative.components() {
        let Component::Normal(name) = part else {
            return None;
        };
        path.push(name);
        if linked(&fs::symlink_metadata(&path).ok()?) {
            return None;
        }
    }
    let mut bytes = Vec::new();
    open_metadata_file(&path, 262144)?
        .take(262145)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() > 262144 {
        return None;
    }
    let value: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    if value["schema"] != 1
        || serde_json::from_value::<Package>(value["package"].clone())
            .ok()
            .as_ref()
            != Some(package)
        || !value["hashes"].is_object()
    {
        return None;
    }
    for (names, field) in [
        (&package.executables, "hashes"),
        (&package.resources, "resource_hashes"),
    ] {
        for name in names.keys() {
            let digest = value[field][name].as_str()?;
            if digest.len() != 64 || !digest.bytes().all(|b| b.is_ascii_hexdigit()) {
                return None;
            }
        }
    }
    Some(())
}
fn worktree_receipt(root: &Path, id: &str) -> Option<crate::worktrees::Record> {
    if id.len() != 32 || !id.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let relative = Path::new("worktrees").join(id).join("receipt.json");
    let mut path = root.to_owned();
    for part in relative.components() {
        let Component::Normal(name) = part else {
            return None;
        };
        path.push(name);
        if linked(&fs::symlink_metadata(&path).ok()?) {
            return None;
        }
    }
    let mut bytes = Vec::new();
    open_metadata_file(&path, 262144)?
        .take(262145)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() > 262144 {
        return None;
    }
    let record: crate::worktrees::Record = serde_json::from_slice(&bytes).ok()?;
    (record.id == id && [1, 2].contains(&record.schema)).then_some(record)
}
impl RuntimeCleanupPreview {
    fn problem(&mut self, code: &'static str) {
        if !self.problems.contains(&code) {
            self.problems.push(code);
        }
    }
    fn selected(
        &mut self,
        selected: &str,
        kind: &'static str,
        id: &str,
        workspace: &Workspace,
        target: &Package,
        limit: usize,
    ) {
        if id.len() > 200
            || id.is_empty()
            || !id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
        {
            self.problem("reference_identity_unverified");
            return;
        }
        let Ok(package) = workspace.catalog.get(selected) else {
            self.problem("selected_package_unresolved");
            return;
        };
        let same_key = match (package.key(), target.key()) {
            (Ok(selected_key), Ok(target_key)) => selected_key == target_key,
            _ => {
                self.problem("selected_package_key_unverified");
                return;
            }
        };
        if same_key {
            if package != target {
                self.problem("physical_package_descriptor_collision_unverified");
            }
            let item = Reference {
                kind,
                id: id.into(),
            };
            if !self.references.contains(&item) {
                if self.references.len() >= limit {
                    self.problem("reference_limit");
                } else {
                    self.references.push(item);
                }
            }
        }
    }
    fn project(
        &mut self,
        project: &Project,
        workspace: &Workspace,
        target: &Package,
        limit: usize,
    ) {
        if !registry_id(&project.id) {
            self.problem("project_registry_identity_unverified");
            return;
        }
        let kind = if project.active {
            "project_runtime_selection"
        } else {
            "inactive_project_runtime_selection"
        };
        for selected in [&project.php, &project.node, &project.composer] {
            self.selected(selected, kind, &project.id, workspace, target, limit);
        }
        match workspace
            .catalog
            .get(&project.php)
            .and_then(|php| workspace.catalog.php_server(php))
        {
            Ok(web) => {
                if let Some((id, _)) = workspace.catalog.packages.iter().find(|(_, p)| *p == web) {
                    self.selected(
                        id,
                        "project_web_runtime_selection",
                        &project.id,
                        workspace,
                        target,
                        limit,
                    );
                }
            }
            Err(_) => self.problem("project_web_runtime_unresolved"),
        }
    }
}
impl Workspace {
    /// Bounded best-effort metadata and instantaneous lease observation. A
    /// candidate must be reviewed and fully revalidated by a future apply path.
    pub fn runtime_cleanup_preview(
        &self,
        package_id: &str,
        limits: PreviewLimits,
    ) -> RuntimeCleanupPreview {
        let mut out = RuntimeCleanupPreview {
            schema: 1,
            action: "remove_runtime_preview",
            package_id: String::new(),
            state: PreviewState::Unknown,
            descriptor_sha256: None,
            references: vec![],
            lease: RemovalLeaseObservation {
                state: RemovalLeaseState::Unknown,
                reason: "not_observed",
            },
            logical_bytes: None,
            size_semantics: "logical_apparent_bytes_not_allocated_or_freeable_hardlinks_and_sparse_files_unattributed",
            snapshot_semantics: "bounded_non_atomic_metadata_and_instantaneous_kernel_lease_observation",
            apply_semantics: "not_authorization_no_apply_token_future_apply_must_relock_and_revalidate_catalog_receipt_references",
            problems: vec![],
        };
        if package_id.is_empty()
            || package_id.len() > 200
            || !package_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
            || limits.max_references == 0
            || limits.max_references > 512
        {
            out.problem("invalid_preview_input");
            return out;
        }
        out.package_id = package_id.into();
        let Ok(target) = self.catalog.get(package_id) else {
            out.problem("unknown_catalog_package");
            return out;
        };
        out.descriptor_sha256 = descriptor_digest(target);
        if out.descriptor_sha256.is_none() {
            out.problem("descriptor_limit");
        }
        if target.key().is_err() || Some(target.target) != Target::current().ok() {
            out.problem("package_target_or_identity_unverified");
        }
        let inventory: Inventory = self.storage_inventory(limits.storage);
        for problem in &inventory.problems {
            out.problem(problem);
        }
        if !inventory.registry_complete {
            out.problem("active_registry_coverage_incomplete");
        }
        if inventory.resources.iter().any(|r| r.ownership == "unknown") {
            out.problem("inventory_ownership_unknown");
        }
        let runtime = inventory.resources.iter().find(|r| {
            r.category == "runtime_package"
                && self.catalog.get(&r.resource_id).is_ok_and(|p| p == target)
        });
        match runtime {
            Some(resource)
                if resource.ownership == "package_receipt" && resource.measurement_complete =>
            {
                out.logical_bytes = resource.logical_bytes
            }
            _ => out.problem("installed_package_ownership_or_measurement_unverified"),
        }
        if runtime_receipt_metadata(&self.root, target).is_none() {
            out.problem("runtime_receipt_metadata_unverified");
        }
        let metadata = self
            .projects
            .storage_metadata_all_registered(limits.registry);
        if !metadata.complete {
            out.problem("all_registered_reference_coverage_incomplete");
        }
        for problem in metadata.problems {
            out.problem(problem);
        }
        for project in &metadata.projects {
            out.project(project, self, target, limits.max_references);
        }
        for instance in &metadata.instances {
            if !registry_id(&instance.id) {
                out.problem("instance_registry_identity_unverified");
                continue;
            }
            out.selected(
                &instance.package,
                "registered_database_instance",
                &instance.id,
                self,
                target,
                limits.max_references,
            );
            // Existing backup code resolves a current-target PostgreSQL tool package.
            if instance.engine == Engine::Postgresql && target.kind == "postgresql-tools" {
                out.selected(
                    package_id,
                    "database_restore_tools_dependency",
                    &instance.id,
                    self,
                    target,
                    limits.max_references,
                );
            }
        }
        for binding in metadata
            .active_bindings
            .iter()
            .chain(metadata.retained_bindings.iter())
        {
            if !registry_id(&binding.project_id) || !registry_id(&binding.instance_id) {
                out.problem("generation_registry_identity_unverified");
                continue;
            }
            let Some(instance) = metadata
                .instances
                .iter()
                .find(|i| i.id == binding.instance_id)
            else {
                out.problem("retained_generation_instance_unresolved");
                continue;
            };
            if !metadata.projects.iter().any(|p| p.id == binding.project_id) {
                out.problem("retained_generation_project_unresolved");
            }
            let key = Sha256::digest(
                format!(
                    "{}/{}/{}",
                    binding.project_id, binding.instance_id, binding.database
                )
                .as_bytes(),
            )
            .iter()
            .take(16)
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
            out.selected(
                &instance.package,
                if binding.ready {
                    "database_generation_reference"
                } else {
                    "pending_database_generation_reference"
                },
                &key,
                self,
                target,
                limits.max_references,
            );
        }
        for cache in &metadata.caches {
            if !registry_id(&cache.id) {
                out.problem("cache_registry_identity_unverified");
                continue;
            }
            for id in [&cache.package, &cache.runtime] {
                out.selected(
                    id,
                    "registered_cache_instance",
                    &cache.id,
                    self,
                    target,
                    limits.max_references,
                );
            }
        }
        if target.kind == "caddy"
            && !metadata.projects.is_empty()
            && self
                .catalog
                .defaults
                .get("caddy")
                .is_some_and(|v| v == &target.version)
        {
            out.selected(
                package_id,
                "registered_project_proxy_default",
                "workspace",
                self,
                target,
                limits.max_references,
            );
        }
        if target.kind == "php-debug" {
            out.problem("debug_provider_reference_coverage_unverified");
        }
        for resource in &inventory.resources {
            if resource.category != "worktree_payload" {
                continue;
            }
            if resource.ownership != "worktree_receipt_and_source_registry" {
                out.problem("worktree_reference_coverage_unverified");
                continue;
            }
            let Some(record) = worktree_receipt(&self.root, &resource.resource_id) else {
                out.problem("worktree_receipt_unverified");
                continue;
            };
            if !metadata
                .projects
                .iter()
                .any(|p| p.id == record.input.source_id && p.path == record.source_path)
            {
                out.problem("worktree_source_registry_unverified");
                continue;
            }
            match Target::current()
                .map_err(devforge_projects::ProjectError::from)
                .and_then(|target| record.configuration.verify_pins(&self.catalog, target))
            {
                Ok(packages) => {
                    for id in packages.values() {
                        out.selected(
                            id,
                            "retained_worktree_configuration",
                            &record.id,
                            self,
                            target,
                            limits.max_references,
                        );
                    }
                }
                Err(_) => out.problem("worktree_configuration_unresolved"),
            }
        }
        out.lease = self.packages.observe_removal_lease(target);
        out.references.sort();
        out.state = if !out.references.is_empty() || out.lease.state == RemovalLeaseState::Held {
            PreviewState::Blocked
        } else if !out.problems.is_empty()
            || out.lease.state != RemovalLeaseState::UnheldAtObservation
        {
            PreviewState::Unknown
        } else {
            PreviewState::ReviewCandidate
        };
        out
    }
}
