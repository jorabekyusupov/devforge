//! Read-only apparent-size inventory. This is not a cleanup planner or a disk
//! allocation/free-space estimate. No payload, credential or project-source reads.
use crate::Workspace;
use devforge_projects::{database::Binding, project::Project};
use devforge_runtime::package::Package;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Read,
    path::{Component, Path, PathBuf},
};

#[derive(Clone, Copy)]
pub struct StorageLimits {
    pub max_entries: usize,
    pub max_resources: usize,
    pub max_depth: usize,
}
impl Default for StorageLimits {
    fn default() -> Self {
        Self {
            max_entries: 20_000,
            max_resources: 512,
            max_depth: 24,
        }
    }
}
#[derive(Debug, Clone, Serialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct Reference {
    pub kind: &'static str,
    pub id: String,
}
#[derive(Debug, Serialize)]
pub struct Resource {
    pub category: &'static str,
    pub resource_id: String,
    pub project_id: Option<String>,
    pub workspace_id: Option<String>,
    pub ownership: &'static str,
    pub logical_bytes: Option<u64>,
    pub files: u64,
    pub directories: u64,
    pub measurement_complete: bool,
    pub active_refs: Vec<Reference>,
    pub reference_coverage: &'static str,
    pub retained_reason: &'static str,
    pub problems: Vec<&'static str>,
}
#[derive(Debug, Serialize)]
pub struct Inventory {
    pub schema: u32,
    pub size_semantics: &'static str,
    pub scope_semantics: &'static str,
    pub registry_complete: bool,
    pub snapshot_semantics: &'static str,
    pub logical_bytes_observed: u64,
    pub entries_examined: usize,
    pub registry_rows_examined: usize,
    pub registry_bytes_read: usize,
    pub measurement_complete: bool,
    pub resources: Vec<Resource>,
    pub problems: Vec<&'static str>,
}
fn valid_id(id: &str) -> bool {
    id.len() == 32 && id.bytes().all(|b| b.is_ascii_hexdigit())
}
fn opaque(value: &str) -> String {
    Sha256::digest(value.as_bytes())
        .iter()
        .take(16)
        .map(|b| format!("{b:02x}"))
        .collect()
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
/// Final-component protection only. Ancestor checks and the inventory remain a
/// non-atomic observation; this does not implement an adversarial openat sandbox.
fn open_metadata_file(path: &Path, limit: u64) -> Option<fs::File> {
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT);
    }
    let file = options.open(path).ok()?;
    let metadata = file.metadata().ok()?;
    if !metadata.is_file() || linked(&metadata) || metadata.len() > limit {
        return None;
    }
    Some(file)
}

struct ResourceKey {
    category: &'static str,
    id: String,
    project: Option<String>,
    workspace: Option<String>,
}
struct Scan<'a> {
    root: &'a Path,
    limits: StorageLimits,
    out: Inventory,
    issue_serial: usize,
}
impl<'a> Scan<'a> {
    fn problem(&mut self, code: &'static str) {
        self.issue_serial = self.issue_serial.saturating_add(1);
        self.out.measurement_complete = false;
        if !self.out.problems.contains(&code) {
            self.out.problems.push(code);
        }
    }
    fn step(&mut self) -> bool {
        if self.out.entries_examined >= self.limits.max_entries {
            self.problem("entry_limit");
            false
        } else {
            self.out.entries_examined += 1;
            true
        }
    }
    /// Check every ancestor, not only the leaf, so a linked category root is refused.
    fn metadata(&self, relative: &Path) -> Result<fs::Metadata, &'static str> {
        let mut current = self.root.to_owned();
        for component in relative.components() {
            let Component::Normal(name) = component else {
                return Err("invalid_owned_path");
            };
            current.push(name);
            let meta = fs::symlink_metadata(&current).map_err(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    "missing"
                } else {
                    "metadata_unavailable"
                }
            })?;
            if linked(&meta) {
                return Err("link_not_followed");
            }
        }
        fs::symlink_metadata(current).map_err(|_| "metadata_unavailable")
    }
    fn children(&mut self, relative: &Path) -> Vec<PathBuf> {
        match self.metadata(relative) {
            Err("missing") => return vec![],
            Err(code) => {
                self.problem(code);
                return vec![];
            }
            Ok(m) if !m.is_dir() => {
                self.problem("expected_directory");
                return vec![];
            }
            Ok(_) => {}
        }
        let Ok(entries) = fs::read_dir(self.root.join(relative)) else {
            self.problem("directory_unavailable");
            return vec![];
        };
        let mut out = vec![];
        for entry in entries {
            if !self.step() {
                break;
            }
            match entry {
                Ok(e) => out.push(relative.join(e.file_name())),
                Err(_) => self.problem("directory_unavailable"),
            }
        }
        out.sort();
        out
    }
    fn metadata_bytes(&mut self, relative: &Path, limit: u64) -> Option<Vec<u8>> {
        if !self.step() {
            return None;
        }
        let metadata = self.metadata(relative).ok()?;
        if !metadata.is_file() || metadata.len() > limit {
            return None;
        }
        let file = open_metadata_file(&self.root.join(relative), limit)?;
        let mut bytes = Vec::new();
        file.take(limit + 1).read_to_end(&mut bytes).ok()?;
        (bytes.len() as u64 <= limit).then_some(bytes)
    }
    fn marker(&mut self, relative: &str, expected: &[u8]) -> bool {
        self.metadata_bytes(Path::new(relative), 128).as_deref() == Some(expected)
    }
    fn resource(
        &mut self,
        key: ResourceKey,
        ownership: &'static str,
        reason: &'static str,
        refs: Vec<Reference>,
        path: Option<&Path>,
    ) {
        let ResourceKey {
            category,
            id,
            project,
            workspace,
        } = key;
        if self.out.resources.len() >= self.limits.max_resources {
            self.problem("resource_limit");
            return;
        }
        let mut r = Resource {
            category,
            resource_id: id,
            project_id: project,
            workspace_id: workspace,
            ownership,
            logical_bytes: None,
            files: 0,
            directories: 0,
            measurement_complete: true,
            active_refs: refs,
            reference_coverage: "active_registry_and_current_workspace_only_other_leases_and_archived_refs_unknown",
            retained_reason: reason,
            problems: vec![],
        };
        if r.active_refs.len() > 128 {
            r.active_refs.truncate(128);
            r.problems.push("reference_limit");
            r.measurement_complete = false;
        }
        if let Some(path) = path {
            if ownership == "unknown" {
                r.problems.push("ownership_unverified_not_traversed");
                r.measurement_complete = false;
            } else {
                r.logical_bytes = Some(0);
                self.measure(path, 0, &mut r);
            }
        } else {
            r.problems.push("shared_physical_storage_size_unattributed");
            r.measurement_complete = false;
        }
        if !r.measurement_complete {
            self.out.measurement_complete = false;
        }
        if let Some(bytes) = r.logical_bytes {
            if let Some(total) = self.out.logical_bytes_observed.checked_add(bytes) {
                self.out.logical_bytes_observed = total;
            } else {
                self.problem("size_overflow");
                r.problems.push("size_overflow");
            }
        }
        self.out.resources.push(r);
    }
    fn measure(&mut self, path: &Path, depth: usize, r: &mut Resource) {
        if !self.step() {
            r.measurement_complete = false;
            r.problems.push("entry_limit");
            return;
        }
        match self.metadata(path) {
            Err(code) => {
                r.measurement_complete = false;
                if !r.problems.contains(&code) {
                    r.problems.push(code);
                }
            }
            Ok(m) if m.is_file() => {
                r.files += 1;
                if let Some(bytes) = r.logical_bytes.and_then(|bytes| bytes.checked_add(m.len())) {
                    r.logical_bytes = Some(bytes);
                } else {
                    r.measurement_complete = false;
                    r.problems.push("size_overflow");
                }
            }
            Ok(m) if m.is_dir() => {
                r.directories += 1;
                if depth >= self.limits.max_depth {
                    r.measurement_complete = false;
                    r.problems.push("depth_limit");
                    return;
                }
                let issue_serial = self.issue_serial;
                let children = self.children(path);
                if issue_serial != self.issue_serial {
                    r.measurement_complete = false;
                    r.problems.push("directory_scan_incomplete");
                }
                for child in children {
                    self.measure(&child, depth + 1, r);
                    if self.out.entries_examined >= self.limits.max_entries {
                        r.measurement_complete = false;
                        break;
                    }
                }
            }
            Ok(_) => {
                r.measurement_complete = false;
                r.problems.push("special_file_not_followed");
            }
        }
    }
    fn unknown(&mut self, category: &'static str, path: &Path) {
        self.resource(
            ResourceKey {
                category,
                id: opaque(&path.to_string_lossy()),
                project: None,
                workspace: None,
            },
            "unknown",
            "unrecognized_ownership",
            vec![],
            Some(path),
        );
    }
}
fn reference(kind: &'static str, id: &str) -> Reference {
    Reference {
        kind,
        id: id.into(),
    }
}
impl Workspace {
    /// Does not create directories, acquire runtime leases, query databases or read
    /// secrets. Registry references are observations, never cleanup authorization.
    pub fn storage_inventory(&self, limits: StorageLimits) -> Inventory {
        let mut scan = Scan {
            root: &self.root,
            limits,
            issue_serial: 0,
            out: Inventory {
                schema: 1,
                size_semantics: "logical_apparent_file_lengths_not_allocated_or_freeable_hardlinks_may_repeat_sparse_files_may_overstate",
                scope_semantics: "registered_application_namespaces_not_entire_data_root_or_user_projects",
                registry_complete: false,
                snapshot_semantics: "bounded_best_effort_non_atomic_filesystem_observation",
                logical_bytes_observed: 0,
                entries_examined: 0,
                registry_rows_examined: 0,
                registry_bytes_read: 0,
                measurement_complete: true,
                resources: vec![],
                problems: vec![],
            },
        };
        if limits.max_entries == 0
            || limits.max_entries > 100_000
            || limits.max_resources == 0
            || limits.max_resources > 2048
            || limits.max_depth == 0
            || limits.max_depth > 32
        {
            scan.problem("invalid_limits");
            return scan.out;
        }
        let metadata =
            self.projects
                .storage_metadata(devforge_projects::storage_metadata::Limits {
                    max_rows: limits.max_resources,
                    ..Default::default()
                });
        scan.out.registry_complete = metadata.complete;
        scan.out.registry_rows_examined = metadata.rows_examined;
        scan.out.registry_bytes_read = metadata.bytes_read;
        for problem in metadata.problems {
            scan.problem(problem);
        }
        let projects: Vec<Project> = metadata.projects;
        let instances = metadata.instances;
        let caches = metadata.caches;
        let active_bindings = metadata.active_bindings;
        let retained_bindings = metadata.retained_bindings;
        let by_project: BTreeMap<_, _> = projects
            .iter()
            .filter(|p| valid_id(&p.id))
            .map(|p| (p.id.clone(), p))
            .collect();
        let mut runtime_refs: BTreeMap<String, BTreeSet<Reference>> = BTreeMap::new();
        let mut instance_refs: BTreeMap<String, Vec<Reference>> = BTreeMap::new();
        let mut generations: BTreeMap<(String, String, String), (Binding, bool)> = BTreeMap::new();
        for project in projects.iter().take(limits.max_resources) {
            if !valid_id(&project.id) {
                scan.problem("invalid_project_registry_id");
                continue;
            }
            for package in [&project.php, &project.node, &project.composer] {
                runtime_refs
                    .entry(package.clone())
                    .or_default()
                    .insert(reference("project_runtime_selection", &project.id));
            }
            if let Ok(php) = self.catalog.get(&project.php)
                && let Ok(web) = self.catalog.php_server(php)
                && let Some((id, _)) = self.catalog.packages.iter().find(|(_, p)| *p == web)
            {
                runtime_refs
                    .entry(id.clone())
                    .or_default()
                    .insert(reference("project_web_runtime_selection", &project.id));
            }
        }
        for binding in active_bindings {
            instance_refs
                .entry(binding.instance_id.clone())
                .or_default()
                .push(reference("active_database_binding", &binding.project_id));
            generations.insert(
                (
                    binding.project_id.clone(),
                    binding.instance_id.clone(),
                    binding.database.clone(),
                ),
                (binding, true),
            );
        }
        for binding in retained_bindings {
            generations
                .entry((
                    binding.project_id.clone(),
                    binding.instance_id.clone(),
                    binding.database.clone(),
                ))
                .or_insert((binding, false));
        }
        if projects.len() > limits.max_resources {
            scan.problem("registry_limit");
        }
        if scan.marker(
            "runtimes/.devforge-runtime-store",
            b"devforge-runtime-store-v1\n",
        ) {
            for path in scan.children(Path::new("runtimes/packages")) {
                let receipt = path.join(".devforge-receipt.json");
                let package = scan
                    .metadata_bytes(&receipt, 262144)
                    .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
                    .and_then(|value| {
                        if value["schema"] != 1 {
                            return None;
                        }
                        serde_json::from_value::<Package>(value["package"].clone()).ok()
                    });
                let package = package.filter(|p| {
                    p.key().ok().is_some_and(|key| {
                        path.file_name().and_then(|n| n.to_str()) == Some(key.as_str())
                    })
                });
                if let Some(package) = package {
                    let catalog_id = self
                        .catalog
                        .packages
                        .iter()
                        .find(|(_, p)| *p == &package)
                        .map(|(id, _)| id.clone());
                    let Some(id) = catalog_id else {
                        scan.unknown("runtime_package", &path);
                        continue;
                    };
                    let refs = runtime_refs
                        .get(&id)
                        .cloned()
                        .unwrap_or_default()
                        .into_iter()
                        .collect();
                    scan.resource(
                        ResourceKey {
                            category: "runtime_package",
                            id,
                            project: None,
                            workspace: None,
                        },
                        "package_receipt",
                        "retained_package_other_process_leases_unknown",
                        refs,
                        Some(&path),
                    );
                } else {
                    scan.unknown("runtime_package", &path);
                }
            }
        } else {
            scan.problem("runtime_store_ownership_unverified");
        }
        if scan.marker(
            "projects/.devforge-project-store",
            b"devforge-project-store-v1\n",
        ) {
            let known: BTreeMap<_, _> = instances
                .iter()
                .filter(|i| valid_id(&i.id))
                .map(|i| (i.id.clone(), i))
                .collect();
            for instance in &instances {
                let mut refs = instance_refs.get(&instance.id).cloned().unwrap_or_default();
                refs.push(reference("registered_database_instance", &instance.id));
                runtime_refs
                    .entry(instance.package.clone())
                    .or_default()
                    .extend(refs);
            }
            for path in scan.children(Path::new("projects/databases")) {
                let id = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or_default();
                if let Some(instance) = known.get(id) {
                    scan.resource(
                        ResourceKey {
                            category: "database_instance",
                            id: id.into(),
                            project: None,
                            workspace: None,
                        },
                        "instance_registry",
                        if instance.ready {
                            "owned_instance_data_preserved"
                        } else {
                            "pending_initialization_data_preserved"
                        },
                        instance_refs.get(id).cloned().unwrap_or_default(),
                        Some(&path),
                    );
                } else {
                    scan.unknown("database_instance", &path);
                }
            }
            for ((project, instance, database), (_, active)) in
                generations.into_iter().take(limits.max_resources)
            {
                let id = opaque(&format!("{project}/{instance}/{database}"));
                scan.resource(
                    ResourceKey {
                        category: "database_generation",
                        id,
                        project: Some(project.clone()),
                        workspace: None,
                    },
                    "binding_registry",
                    if active {
                        "active_binding"
                    } else {
                        "retained_generation"
                    },
                    if active {
                        vec![reference("active_database_binding", &project)]
                    } else {
                        vec![]
                    },
                    None,
                );
            }
            for cache in &caches {
                for package in [&cache.package, &cache.runtime] {
                    runtime_refs
                        .entry(package.clone())
                        .or_default()
                        .insert(reference("registered_cache_instance", &cache.id));
                }
            }
            let caches: BTreeMap<_, _> = caches
                .iter()
                .filter(|c| valid_id(&c.id))
                .map(|c| (c.id.clone(), c))
                .collect();
            for path in scan.children(Path::new("projects/cache-services")) {
                let id = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or_default();
                if let Some(cache) = caches.get(id) {
                    let refs = by_project
                        .get(&cache.project_id)
                        .filter(|p| p.services.redis_instance.as_deref() == Some(id))
                        .map(|p| vec![reference("active_cache_selection", &p.id)])
                        .unwrap_or_default();
                    scan.resource(
                        ResourceKey {
                            category: "cache_instance",
                            id: id.into(),
                            project: Some(cache.project_id.clone()),
                            workspace: None,
                        },
                        "cache_registry",
                        "owned_cache_data_preserved",
                        refs,
                        Some(&path),
                    );
                } else {
                    scan.unknown("cache_instance", &path);
                }
            }
            for (base, category, reason) in [
                (
                    "projects/backups",
                    "backups",
                    "retained_backup_payloads_not_validated",
                ),
                (
                    "projects/debugging",
                    "debug_profiles",
                    "retained_debug_artifacts",
                ),
                (
                    "projects/config",
                    "project_configuration",
                    "project_configuration_preserved",
                ),
                ("queue-logs", "project_logs", "retained_logs"),
            ] {
                for path in scan.children(Path::new(base)) {
                    let id = path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or_default();
                    if by_project.contains_key(id) {
                        scan.resource(
                            ResourceKey {
                                category,
                                id: id.into(),
                                project: Some(id.into()),
                                workspace: None,
                            },
                            "active_project_namespace",
                            reason,
                            vec![reference("registered_project", id)],
                            Some(&path),
                        );
                    } else {
                        scan.unknown(category, &path);
                    }
                }
            }
            for path in scan.children(Path::new("mail")) {
                let id = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or_default();
                if valid_id(id)
                    && scan
                        .metadata_bytes(&path.join(".devforge-inbox"), 128)
                        .as_deref()
                        == Some(b"devforge-inbox-v1\n")
                {
                    let refs = by_project
                        .contains_key(id)
                        .then(|| reference("registered_project", id))
                        .into_iter()
                        .collect();
                    scan.resource(
                        ResourceKey {
                            category: "mail",
                            id: id.into(),
                            project: Some(id.into()),
                            workspace: None,
                        },
                        "mail_store_marker",
                        "retained_mail",
                        refs,
                        Some(&path),
                    );
                } else {
                    scan.unknown("mail", &path);
                }
            }
        } else {
            scan.out.registry_complete = false;
            scan.problem("project_store_ownership_unverified");
        }
        for path in scan.children(Path::new("worktrees")) {
            if path.file_name().and_then(|n| n.to_str()) == Some("operation.lock") {
                continue;
            }
            let id = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default();
            let receipt = scan
                .metadata_bytes(&path.join("receipt.json"), 262144)
                .and_then(|bytes| serde_json::from_slice::<crate::worktrees::Record>(&bytes).ok());
            let known = valid_id(id)
                && receipt.as_ref().is_some_and(|r| {
                    r.id == id
                        && [1, 2].contains(&r.schema)
                        && r.project_id.as_ref().is_none_or(|p| valid_id(p))
                        && by_project
                            .get(&r.input.source_id)
                            .is_some_and(|source| source.path == r.source_path)
                });
            if known {
                let record = receipt.unwrap();
                let refs = record
                    .project_id
                    .as_ref()
                    .filter(|p| by_project.contains_key(*p))
                    .map(|p| vec![reference("registered_project", p)])
                    .unwrap_or_default();
                scan.resource(
                    ResourceKey {
                        category: "worktree_payload",
                        id: id.into(),
                        project: record.project_id,
                        workspace: Some(id.into()),
                    },
                    "worktree_receipt_and_source_registry",
                    "owned_workspace_or_retained_archive",
                    refs,
                    Some(&path),
                );
            } else {
                scan.unknown("worktree_payload", &path);
            }
        }
        for (path, category) in [
            ("workspace.json", "workspace_metadata"),
            ("projects/projects.sqlite3", "project_registry_metadata"),
            ("projects/projects.sqlite3-wal", "project_registry_metadata"),
            ("projects/projects.sqlite3-shm", "project_registry_metadata"),
            ("preferences.sqlite3", "preferences_metadata"),
            ("inspector", "inspection_artifacts"),
            ("projects/client-install-logs", "tooling_logs"),
        ] {
            match scan.metadata(Path::new(path)) {
                Err("missing") => {}
                _ => scan.resource(
                    ResourceKey {
                        category,
                        id: opaque(path),
                        project: None,
                        workspace: None,
                    },
                    "application_namespace",
                    "application_metadata_or_artifacts",
                    vec![],
                    Some(Path::new(path)),
                ),
            }
        }
        // Merge references after DB/cache metadata is known; process leases and
        // archived project records remain deliberately uninspected/unknown.
        for resource in &mut scan.out.resources {
            if !scan.out.registry_complete {
                resource.reference_coverage =
                    "partial_registry_metadata_missing_references_unknown";
                resource.problems.push("reference_registry_incomplete");
            }
            if resource.category == "runtime_package"
                && let Some(refs) = runtime_refs.get(&resource.resource_id)
            {
                resource.active_refs.extend(refs.iter().cloned());
                resource.active_refs.sort();
                resource.active_refs.dedup();
                if resource.active_refs.len() > 128 {
                    resource.active_refs.truncate(128);
                    resource.problems.push("reference_limit");
                    resource.measurement_complete = false;
                    scan.out.measurement_complete = false;
                }
            }
        }
        scan.out
            .resources
            .sort_by(|a, b| (a.category, &a.resource_id).cmp(&(b.category, &b.resource_id)));
        scan.out
    }
}

#[cfg(test)]
mod metadata_open_tests {
    use super::*;
    #[test]
    fn metadata_handle_refuses_nonregular_oversized_and_final_component_links() {
        let temp = tempfile::tempdir().unwrap();
        let ordinary = temp.path().join("metadata");
        fs::write(&ordinary, b"marker").unwrap();
        assert!(open_metadata_file(&ordinary, 6).is_some());
        assert!(open_metadata_file(&ordinary, 5).is_none());
        assert!(open_metadata_file(temp.path(), 128).is_none());
        #[cfg(unix)]
        {
            let link = temp.path().join("linked-marker");
            std::os::unix::fs::symlink(&ordinary, &link).unwrap();
            // Exercise the actual no-follow open directly, without relying on
            // the earlier path metadata checks to detect a final-component link.
            assert!(open_metadata_file(&link, 128).is_none());
        }
    }
}
