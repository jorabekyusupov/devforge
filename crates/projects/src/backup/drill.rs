//! Restore verification without connection activation or implicit deletion.
//! Successful drill candidates remain non-selectable pending generations; a
//! later reviewed cleanup must establish ownership/references before deletion.
use super::*;
use crate::database::Instance;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DrillOutcome {
    Provisioning,
    Passed,
    Failed,
    Cancelled,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DrillGeneration {
    pub instance_id: String,
    pub database: String,
    /// False includes partially created or unknown resources, not proof of absence.
    pub restore_verified: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DrillReceipt {
    pub schema: u32,
    pub id: String,
    pub project_id: String,
    pub project_revision: u64,
    pub backup_id: String,
    pub backup_sha256: String,
    pub engine: Engine,
    pub version: String,
    pub created: u64,
    pub outcome: DrillOutcome,
    pub error: Option<E>,
    pub generation: DrillGeneration,
    pub activation_performed: bool,
    pub cleanup: DrillCleanup,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DrillCleanup {
    /// No automatic DROP, role/credential removal or backup deletion occurs.
    RetainedRequiresReview,
}
struct Plan {
    manifest: Manifest,
    active: Binding,
    instance: Instance,
    revision: u64,
}
fn same_binding(a: &Binding, b: &Binding) -> bool {
    a.project_id == b.project_id
        && a.instance_id == b.instance_id
        && a.database == b.database
        && a.role == b.role
        && a.secret_ref == b.secret_ref
        && a.ready == b.ready
}
fn ordinary_directory(meta: &fs::Metadata) -> bool {
    if !meta.is_dir() || meta.file_type().is_symlink() {
        return false;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if meta.file_attributes() & 0x400 != 0 {
            return false;
        }
    }
    true
}
fn owned_directory(path: &Path, owner: &[u8], create: bool) -> Result<()> {
    let fresh = match fs::symlink_metadata(path) {
        Ok(meta) => {
            if !ordinary_directory(&meta) {
                return Err(E::StorageUnavailable);
            }
            false
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound && create => {
            fs::create_dir(path)?;
            if !ordinary_directory(&fs::symlink_metadata(path)?) {
                return Err(E::StorageUnavailable);
            }
            true
        }
        Err(_) => return Err(E::StorageUnavailable),
    };
    let marker = path.join(".devforge-restore-drill-owner");
    if fresh {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
        }
        let mut file = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&marker)?;
        file.write_all(owner)?;
        file.sync_all()?;
    }
    if crate::manifest::read_file(&marker, 64).map_err(|_| E::StorageUnavailable)? != owner {
        return Err(E::StorageUnavailable);
    }
    Ok(())
}
impl ProjectStore {
    fn drill_plan(
        &self,
        project: &str,
        backup: &str,
        revision: u64,
        catalog: &Catalog,
        cancel: &Cancellation,
    ) -> Result<(Plan, zip::ZipArchive<File>)> {
        if cancel.is_cancelled() {
            return Err(E::ExecutionFailed);
        }
        if !valid_id(project) || !valid_id(backup) {
            return Err(E::InvalidBackup);
        }
        let record = self.get(project)?;
        if record.revision != revision {
            return Err(E::StaleRevision);
        }
        let (manifest, archive) = inspect(&self.backup_path(project, backup)?)?;
        let active = self
            .binding(project)?
            .filter(|b| b.ready)
            .ok_or(E::DatabaseUnavailable)?;
        let instance = self.instance(&active.instance_id)?;
        if instance.engine != manifest.engine
            || catalog.get(&instance.package)?.version != manifest.version
        {
            return Err(E::BackupUnsupported);
        }
        Ok((
            Plan {
                manifest,
                active,
                instance,
                revision,
            },
            archive,
        ))
    }
    fn drill_root(&self, project: &str, create: bool) -> Result<PathBuf> {
        if !valid_id(project) {
            return Err(E::InvalidBackup);
        }
        self.get(project)?;
        let store_meta = fs::symlink_metadata(&self.root)?;
        if !ordinary_directory(&store_meta) {
            return Err(E::StorageUnavailable);
        }
        let root = self.root.join("restore-drills");
        owned_directory(&root, b"devforge-restore-drills-v1\n", create)?;
        let child = root.join(project);
        owned_directory(&child, project.as_bytes(), create)?;
        Ok(child)
    }
    fn persist_drill(&self, receipt: &DrillReceipt) -> Result<()> {
        let root = self.drill_root(&receipt.project_id, true)?;
        let bytes = serde_json::to_vec(receipt).map_err(|_| E::StorageUnavailable)?;
        if bytes.len() > 16384 || !valid_id(&receipt.id) {
            return Err(E::StorageUnavailable);
        }
        let mut file = tempfile::NamedTempFile::new_in(&root)?;
        file.write_all(&bytes)?;
        file.as_file().sync_all()?;
        file.persist(root.join(format!("{}.json", receipt.id)))
            .map_err(|_| E::StorageUnavailable)?;
        Ok(())
    }
    /// Caller supplies the existing owned server. This method never starts or
    /// stops a server/project and never calls activate_binding. A checked fresh
    /// candidate is retained, but is NOT eligible for ordinary binding reuse.
    #[allow(
        clippy::too_many_arguments,
        reason = "Explicit owned restore execution boundary"
    )]
    pub fn restore_drill(
        &self,
        project: &str,
        backup: &str,
        revision: u64,
        server: &mut RunningDatabase,
        catalog: &Catalog,
        packages: &PackageStore,
        secrets: &dyn SecretStore,
        cancel: &Cancellation,
    ) -> Result<DrillReceipt> {
        let (plan, mut archive) = self.drill_plan(project, backup, revision, catalog, cancel)?;
        server.validate_store_instance(self, &plan.instance)?;
        let directory = tempfile::tempdir_in(self.backup_root(project)?)?;
        let payload = directory.path().join("payload");
        let mut output = File::create(&payload)?;
        std::io::copy(
            &mut archive.by_name("payload").map_err(|_| E::InvalidBackup)?,
            &mut output,
        )?;
        output.sync_all()?;
        drop(output);
        if hash(&payload)? != plan.manifest.sha256
            || fs::metadata(&payload)?.len() != plan.manifest.bytes
        {
            return Err(E::InvalidBackup);
        }
        if cancel.is_cancelled() {
            return Err(E::ExecutionFailed);
        }
        if self.get(project)?.revision != plan.revision
            || !self
                .binding(project)?
                .is_some_and(|active| same_binding(&active, &plan.active))
        {
            return Err(E::StaleRevision);
        }
        let candidate = self.staged_binding(project, &plan.instance.id, secrets)?;
        let mut receipt = DrillReceipt {
            schema: 1,
            id: crate::id(),
            project_id: project.into(),
            project_revision: revision,
            backup_id: backup.into(),
            backup_sha256: plan.manifest.sha256,
            engine: plan.instance.engine,
            version: plan.manifest.version,
            created: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            outcome: DrillOutcome::Provisioning,
            error: None,
            generation: DrillGeneration {
                instance_id: candidate.instance_id.clone(),
                database: candidate.database.clone(),
                restore_verified: false,
            },
            activation_performed: false,
            cleanup: DrillCleanup::RetainedRequiresReview,
        };
        self.persist_drill(&receipt)?;
        let result = (|| {
            if cancel.is_cancelled() {
                return Err(E::ExecutionFailed);
            }
            let ready = server.provision_prepared_staged(self, project, candidate, secrets)?;
            self.restore_staged_payload(
                &ready,
                &plan.instance,
                &payload,
                catalog,
                packages,
                secrets,
                cancel,
            )
        })();
        match result {
            Ok(()) => {
                receipt.outcome = DrillOutcome::Passed;
                receipt.generation.restore_verified = true;
            }
            Err(error) => {
                receipt.outcome = if cancel.is_cancelled() {
                    DrillOutcome::Cancelled
                } else {
                    DrillOutcome::Failed
                };
                receipt.error = Some(error);
            }
        }
        // Keep history ready=false even after verification. The candidate must
        // never replace the user's retained connection through generic reuse.
        self.persist_drill(&receipt)?;
        Ok(receipt)
    }
    pub fn restore_drill_receipt(&self, project: &str, id: &str) -> Result<DrillReceipt> {
        let root = self.drill_root(project, false)?;
        if !valid_id(id) {
            return Err(E::InvalidBackup);
        }
        let path = root.join(format!("{id}.json"));
        let meta = fs::symlink_metadata(&path)?;
        if !meta.is_file() || meta.file_type().is_symlink() || meta.len() > 16384 {
            return Err(E::StorageUnavailable);
        }
        let bytes = crate::manifest::read_file(&path, 16384).map_err(|_| E::StorageUnavailable)?;
        let receipt: DrillReceipt =
            serde_json::from_slice(&bytes).map_err(|_| E::StorageUnavailable)?;
        if receipt.schema != 1
            || receipt.project_id != project
            || receipt.id != id
            || receipt.activation_performed
        {
            return Err(E::StorageUnavailable);
        }
        Ok(receipt)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::Project;
    fn fixture() -> (tempfile::TempDir, ProjectStore, Catalog, Project, Manifest) {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("project");
        fs::create_dir_all(path.join("public")).unwrap();
        fs::write(path.join("public/index.php"), b"<?php").unwrap();
        fs::write(
            path.join("composer.json"),
            br#"{"require":{"php":"^8.3","laravel/framework":"^13"}}"#,
        )
        .unwrap();
        let catalog = Catalog::bundled();
        let target = devforge_runtime::package::Target::current().unwrap();
        let select = |kind: &str, tool: &str| {
            catalog
                .packages
                .iter()
                .find(|(_, p)| {
                    p.kind == kind && p.target == target && p.executables.contains_key(tool)
                })
                .unwrap()
                .0
                .clone()
        };
        let store = ProjectStore::open(root.path().join("metadata")).unwrap();
        let project = store
            .import(
                &path,
                "Drill fixture",
                &select("php", "php"),
                &select("node", "node"),
                &catalog,
            )
            .unwrap();
        let package = select("postgresql", "initdb");
        let instance = Instance {
            engine: Engine::Postgresql,
            id: crate::id(),
            package: package.clone(),
            port: 32123,
            admin: "fixture".into(),
            secret_ref: "fixture/admin-secret-must-not-leak".into(),
            ready: true,
        };
        store.save_instance(&instance).unwrap();
        let binding = Binding {
            project_id: project.id.clone(),
            instance_id: instance.id,
            database: "fixture_active".into(),
            role: "fixture_role".into(),
            secret_ref: "fixture/password-must-not-leak".into(),
            ready: true,
        };
        store.activate_binding(&project.id, Some(&binding)).unwrap();
        let payload = root.path().join("inert-backup-payload");
        fs::write(
            &payload,
            b"not a real PostgreSQL dump: metadata fixture only",
        )
        .unwrap();
        let backup = store
            .publish_payload(
                &project.id,
                &payload,
                Engine::Postgresql,
                &catalog.get(&package).unwrap().version,
            )
            .unwrap();
        (root, store, catalog, project, backup)
    }
    #[test]
    fn preflight_rejects_stale_cancelled_corrupt_or_wrong_engine_without_staging() {
        let (_root, store, catalog, project, backup) = fixture();
        let cancel = Cancellation::default();
        assert_eq!(
            store
                .drill_plan(
                    &project.id,
                    &backup.id,
                    project.revision + 1,
                    &catalog,
                    &cancel
                )
                .err(),
            Some(E::StaleRevision)
        );
        let cancelled = Cancellation::default();
        cancelled.cancel();
        assert_eq!(
            store
                .drill_plan(
                    &project.id,
                    &backup.id,
                    project.revision,
                    &catalog,
                    &cancelled
                )
                .err(),
            Some(E::ExecutionFailed)
        );
        let mut wrong = catalog.clone();
        let active = store.binding(&project.id).unwrap().unwrap();
        let instance = store.instance(&active.instance_id).unwrap();
        wrong.packages.get_mut(&instance.package).unwrap().version = "different-version".into();
        assert_eq!(
            store
                .drill_plan(&project.id, &backup.id, project.revision, &wrong, &cancel)
                .err(),
            Some(E::BackupUnsupported)
        );
        fs::write(
            store.backup_path(&project.id, &backup.id).unwrap(),
            b"corrupt",
        )
        .unwrap();
        assert_eq!(
            store
                .drill_plan(&project.id, &backup.id, project.revision, &catalog, &cancel)
                .err(),
            Some(E::InvalidBackup)
        );
        assert!(store.binding_history(&project.id).unwrap().is_empty());
        assert!(same_binding(
            &active,
            &store.binding(&project.id).unwrap().unwrap()
        ));
        assert!(!store.root().join("restore-drills").exists());
    }
    #[test]
    fn receipt_is_bounded_sanitized_and_never_authorizes_activation() {
        let (_root, store, catalog, project, backup) = fixture();
        let (plan, _) = store
            .drill_plan(
                &project.id,
                &backup.id,
                project.revision,
                &catalog,
                &Cancellation::default(),
            )
            .unwrap();
        let receipt = DrillReceipt {
            schema: 1,
            id: crate::id(),
            project_id: project.id.clone(),
            project_revision: project.revision,
            backup_id: backup.id,
            backup_sha256: backup.sha256,
            engine: plan.instance.engine,
            version: plan.manifest.version,
            created: 1,
            outcome: DrillOutcome::Failed,
            error: Some(E::DatabaseUnavailable),
            generation: DrillGeneration {
                instance_id: plan.instance.id,
                database: "new_candidate_may_be_partial".into(),
                restore_verified: false,
            },
            activation_performed: false,
            cleanup: DrillCleanup::RetainedRequiresReview,
        };
        store.persist_drill(&receipt).unwrap();
        let bytes = serde_json::to_vec(&receipt).unwrap();
        let text = std::str::from_utf8(&bytes).unwrap();
        assert!(!text.contains("secret_ref"));
        assert!(!text.contains("must-not-leak"));
        assert!(!text.contains("fixture_role"));
        let loaded = store
            .restore_drill_receipt(&project.id, &receipt.id)
            .unwrap();
        assert_eq!(loaded.outcome, DrillOutcome::Failed);
        assert_eq!(loaded.cleanup, DrillCleanup::RetainedRequiresReview);
        let mut foreign = receipt.clone();
        foreign.activation_performed = true;
        store.persist_drill(&foreign).unwrap();
        assert_eq!(
            store.restore_drill_receipt(&project.id, &receipt.id).err(),
            Some(E::StorageUnavailable)
        );
        let mut oversized = receipt.clone();
        oversized.version = "x".repeat(16384);
        assert_eq!(store.persist_drill(&oversized), Err(E::StorageUnavailable));
        assert!(same_binding(
            &plan.active,
            &store.binding(&project.id).unwrap().unwrap()
        ));
    }
    #[derive(Default)]
    struct MemorySecrets {
        values: std::sync::Mutex<std::collections::BTreeMap<String, String>>,
        cancel_next_get: std::sync::Mutex<Option<Cancellation>>,
    }
    impl SecretStore for MemorySecrets {
        fn get(&self, key: &str) -> std::result::Result<Option<String>, devforge_core::ErrorCode> {
            if let Some(cancel) = self.cancel_next_get.lock().unwrap().take() {
                cancel.cancel();
            }
            Ok(self.values.lock().unwrap().get(key).cloned())
        }
        fn set(&self, key: &str, value: &str) -> std::result::Result<(), devforge_core::ErrorCode> {
            self.values.lock().unwrap().insert(key.into(), value.into());
            Ok(())
        }
        fn delete(&self, key: &str) -> std::result::Result<(), devforge_core::ErrorCode> {
            self.values.lock().unwrap().remove(key);
            Ok(())
        }
    }
    fn copy_cache(source: &Path, destination: &Path, boundary: &Path) {
        let actual = fs::canonicalize(source).unwrap();
        assert!(
            actual.starts_with(boundary),
            "cache link escaped reviewed package store"
        );
        if actual.is_dir() {
            fs::create_dir_all(destination).unwrap();
            for entry in fs::read_dir(actual).unwrap() {
                let entry = entry.unwrap();
                copy_cache(
                    &entry.path(),
                    &destination.join(entry.file_name()),
                    boundary,
                );
            }
        } else {
            fs::copy(actual, destination).unwrap();
        }
    }
    fn sql(store: &ProjectStore, project: &str, secrets: &dyn SecretStore, statement: &str) {
        let c = store.credentials(project, secrets).unwrap();
        match c.engine {
            Engine::Postgresql => c.connect().unwrap().batch_execute(statement).unwrap(),
            Engine::Mysql => {
                use mysql::prelude::Queryable;
                c.mysql().unwrap().query_drop(statement).unwrap();
            }
        }
    }
    fn canary(store: &ProjectStore, binding: Binding, secrets: &dyn SecretStore) -> i64 {
        let c = store.credentials_for(binding, secrets).unwrap();
        match c.engine {
            Engine::Postgresql => c
                .connect()
                .unwrap()
                .query_one("SELECT value FROM drill_canary", &[])
                .unwrap()
                .get(0),
            Engine::Mysql => {
                use mysql::prelude::Queryable;
                c.mysql()
                    .unwrap()
                    .query_first::<i64, _>("SELECT value FROM drill_canary")
                    .unwrap()
                    .unwrap()
            }
        }
    }
    #[test]
    #[ignore = "explicit isolated native PostgreSQL cached-store fixture; no downloads"]
    fn native_postgresql_drill_preserves_live_binding_and_records_all_outcomes() {
        native(Engine::Postgresql);
    }
    #[test]
    #[ignore = "explicit isolated native MySQL cached-store fixture; no downloads"]
    fn native_mysql_drill_preserves_live_binding_and_records_all_outcomes() {
        native(Engine::Mysql);
    }
    fn native(engine: Engine) {
        let source = fs::canonicalize(
            std::env::var_os("DEVFORGE_DRILL_RUNTIME_STORE")
                .expect("explicit reviewed cache required"),
        )
        .unwrap();
        let root = tempfile::tempdir().unwrap();
        let catalog = Catalog::bundled();
        let target = devforge_runtime::package::Target::current().unwrap();
        let available = |kind: &str| {
            catalog
                .packages
                .iter()
                .find(|(_, p)| {
                    p.kind == kind
                        && p.target == target
                        && source.join("packages").join(p.key().unwrap()).is_dir()
                })
                .map(|(id, p)| (id.clone(), p.clone()))
                .expect("current catalog native package absent from reviewed cache")
        };
        let (package_id, package) = available(if engine == Engine::Postgresql {
            "postgresql"
        } else {
            "mysql"
        });
        let packages = PackageStore::open(
            root.path().join("runtimes"),
            devforge_runtime::package::Limits::default(),
        )
        .unwrap();
        let mut selected = vec![package.clone()];
        if engine == Engine::Postgresql {
            selected.push(available("postgresql-tools").1);
        }
        for package in &selected {
            copy_cache(
                &source.join("packages").join(package.key().unwrap()),
                &root
                    .path()
                    .join("runtimes/packages")
                    .join(package.key().unwrap()),
                &source,
            );
            // Local copy only: verifies descriptor/receipt/executable SHA before use.
            packages
                .executable(
                    package,
                    if package.kind == "postgresql-tools" {
                        "pg_restore"
                    } else if engine == Engine::Mysql {
                        "mysqld"
                    } else {
                        "initdb"
                    },
                )
                .unwrap();
        }
        let path = root.path().join("native project Ўзбек");
        fs::create_dir_all(path.join("public")).unwrap();
        fs::write(path.join("public/index.php"), b"<?php").unwrap();
        fs::write(
            path.join("composer.json"),
            br#"{"require":{"php":"^8.3","laravel/framework":"^13"}}"#,
        )
        .unwrap();
        let select = |kind: &str, tool: &str| {
            catalog
                .packages
                .iter()
                .find(|(_, p)| {
                    p.kind == kind && p.target == target && p.executables.contains_key(tool)
                })
                .unwrap()
                .0
                .clone()
        };
        let store = ProjectStore::open(root.path().join("metadata")).unwrap();
        let project = store
            .import(
                &path,
                "Live binding drill",
                &select("php", "php"),
                &select("node", "node"),
                &catalog,
            )
            .unwrap();
        let secrets = MemorySecrets::default();
        let instance = store
            .initialize_instance(&package_id, 0, &catalog, &packages, &secrets)
            .unwrap();
        let mut server =
            RunningDatabase::start(&store, &instance.id, &catalog, &packages, &secrets).unwrap();
        let active = server.provision(&store, &project, &secrets).unwrap();
        sql(
            &store,
            &project.id,
            &secrets,
            "CREATE TABLE drill_canary(value BIGINT NOT NULL)",
        );
        sql(
            &store,
            &project.id,
            &secrets,
            "INSERT INTO drill_canary(value) VALUES (701)",
        );
        let cancel = Cancellation::default();
        let backup = store
            .create_backup(&project.id, &catalog, &packages, &secrets, &cancel)
            .unwrap();
        let original_hash = hash(&store.backup_path(&project.id, &backup.id).unwrap()).unwrap();
        // Live source has diverged from backup. A drill must not rewind it.
        sql(
            &store,
            &project.id,
            &secrets,
            "UPDATE drill_canary SET value=902",
        );
        let process_before = server.status().unwrap();
        let other_instance = store
            .initialize_instance(&package_id, 0, &catalog, &packages, &secrets)
            .unwrap();
        let mut wrong_server =
            RunningDatabase::start(&store, &other_instance.id, &catalog, &packages, &secrets)
                .unwrap();
        let history_before = store.binding_history(&project.id).unwrap().len();
        assert_eq!(
            store
                .restore_drill(
                    &project.id,
                    &backup.id,
                    project.revision,
                    &mut wrong_server,
                    &catalog,
                    &packages,
                    &secrets,
                    &cancel
                )
                .err(),
            Some(E::DatabaseConflict)
        );
        assert_eq!(
            store
                .restore_backup(
                    &project.id,
                    &backup.id,
                    &mut wrong_server,
                    &catalog,
                    &packages,
                    &secrets,
                    &cancel
                )
                .err(),
            Some(E::DatabaseConflict)
        );
        assert_eq!(
            store.binding_history(&project.id).unwrap().len(),
            history_before
        );
        wrong_server.stop().unwrap();
        let mut drifted = instance.clone();
        drifted.port = instance.port.saturating_add(1);
        store.save_instance(&drifted).unwrap();
        assert_eq!(
            store
                .restore_drill(
                    &project.id,
                    &backup.id,
                    project.revision,
                    &mut server,
                    &catalog,
                    &packages,
                    &secrets,
                    &cancel
                )
                .err(),
            Some(E::DatabaseConflict)
        );
        store.save_instance(&instance).unwrap();
        assert_eq!(
            store.binding_history(&project.id).unwrap().len(),
            history_before
        );
        let passed = store
            .restore_drill(
                &project.id,
                &backup.id,
                project.revision,
                &mut server,
                &catalog,
                &packages,
                &secrets,
                &cancel,
            )
            .unwrap();
        assert_eq!(passed.outcome, DrillOutcome::Passed);
        assert!(passed.generation.restore_verified);
        assert!(!passed.activation_performed);
        assert!(same_binding(
            &active,
            &store.binding(&project.id).unwrap().unwrap()
        ));
        assert_eq!(canary(&store, active.clone(), &secrets), 902);
        let staged = store
            .binding_history(&project.id)
            .unwrap()
            .into_iter()
            .find(|b| b.database == passed.generation.database)
            .unwrap();
        assert!(
            !staged.ready,
            "drill must never become generic retained connection"
        );
        assert_eq!(canary(&store, staged, &secrets), 701);
        assert!(
            store
                .retained_binding(&project.id, &instance.id)
                .unwrap()
                .is_none()
        );
        let invalid = root.path().join("invalid logical dump");
        fs::write(&invalid, b"NOT A VALID DATABASE DUMP;").unwrap();
        let bad = store
            .publish_payload(&project.id, &invalid, engine, &package.version)
            .unwrap();
        let failed = store
            .restore_drill(
                &project.id,
                &bad.id,
                project.revision,
                &mut server,
                &catalog,
                &packages,
                &secrets,
                &cancel,
            )
            .unwrap();
        assert_eq!(failed.outcome, DrillOutcome::Failed);
        assert!(!failed.generation.restore_verified);
        assert!(failed.error.is_some());
        let interrupted = Cancellation::default();
        *secrets.cancel_next_get.lock().unwrap() = Some(interrupted.clone());
        let cancelled = store
            .restore_drill(
                &project.id,
                &backup.id,
                project.revision,
                &mut server,
                &catalog,
                &packages,
                &secrets,
                &interrupted,
            )
            .unwrap();
        assert_eq!(cancelled.outcome, DrillOutcome::Cancelled);
        assert!(!cancelled.generation.restore_verified);
        for receipt in [&passed, &failed, &cancelled] {
            assert_eq!(
                store
                    .restore_drill_receipt(&project.id, &receipt.id)
                    .unwrap()
                    .outcome,
                receipt.outcome
            );
            assert_eq!(receipt.cleanup, DrillCleanup::RetainedRequiresReview);
        }
        assert!(same_binding(
            &active,
            &store.binding(&project.id).unwrap().unwrap()
        ));
        assert_eq!(canary(&store, active.clone(), &secrets), 902);
        assert_eq!(
            hash(&store.backup_path(&project.id, &backup.id).unwrap()).unwrap(),
            original_hash
        );
        let process_after = server.status().unwrap();
        assert_eq!(process_before.pid, process_after.pid);
        // Direct consumer regression: ordinary restore still explicitly activates.
        let restored = store
            .restore_backup(
                &project.id,
                &backup.id,
                &mut server,
                &catalog,
                &packages,
                &secrets,
                &cancel,
            )
            .unwrap();
        assert_ne!(restored.database, active.database);
        assert!(same_binding(
            &restored,
            &store.binding(&project.id).unwrap().unwrap()
        ));
        assert_eq!(canary(&store, restored, &secrets), 701);
        server.stop().unwrap();
        println!(
            "{}",
            serde_json::json!({"suite":"restore_drill", "engine":engine,"version":package.version,"target":target,"passed_restore_verified":true,"failed_and_cancelled_receipts":true,"active_binding_and_live_canary_preserved":true,"backup_hash_preserved":true,"server_pid_unchanged":true,"wrong_server_refused_before_staging":true,"legacy_restore_activation":true,"cleanup":"retained_requires_review_in_disposable_instance","native_credentials":false})
        );
    }
    #[cfg(unix)]
    #[test]
    fn receipt_directories_reject_links_and_unmarked_existing_directories() {
        use std::os::unix::{fs::PermissionsExt, fs::symlink};
        let (root, store, _catalog, project, _backup) = fixture();
        let outside = root.path().join("outside");
        fs::create_dir(&outside).unwrap();
        fs::write(outside.join("canary"), b"unchanged").unwrap();
        fs::set_permissions(&outside, fs::Permissions::from_mode(0o755)).unwrap();
        let top = store.root().join("restore-drills");
        symlink(&outside, &top).unwrap();
        assert_eq!(
            store.drill_root(&project.id, true),
            Err(E::StorageUnavailable)
        );
        assert_eq!(
            store.drill_root(&project.id, false),
            Err(E::StorageUnavailable)
        );
        assert!(!outside.join(&project.id).exists());
        fs::remove_file(&top).unwrap();
        fs::create_dir(&top).unwrap();
        assert_eq!(
            store.drill_root(&project.id, true),
            Err(E::StorageUnavailable)
        );
        fs::remove_dir(&top).unwrap();
        let child = store.drill_root(&project.id, true).unwrap();
        fs::remove_dir_all(&child).unwrap();
        symlink(&outside, &child).unwrap();
        assert_eq!(
            store.drill_root(&project.id, true),
            Err(E::StorageUnavailable)
        );
        assert_eq!(
            store.drill_root(&project.id, false),
            Err(E::StorageUnavailable)
        );
        assert_eq!(fs::read(outside.join("canary")).unwrap(), b"unchanged");
        assert_eq!(
            fs::metadata(&outside).unwrap().permissions().mode() & 0o777,
            0o755
        );
        assert_eq!(fs::read_dir(&outside).unwrap().count(), 1);
    }
}
