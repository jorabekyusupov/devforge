//! Reviewed existing bind/switch and owned-backup restore only. State remains
//! manager-private; public choices contain identifiers, never roles or secrets.
use super::*;
use devforge_projects::{backup::Manifest, database::Engine};
use devforge_runtime::package::{Package, hash_file};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{cell::RefCell, io::Write};
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DatabaseActionState {
    action: String,
    project_id: String,
    revision: u64,
    root: PathBuf,
    identity: String,
    active: Option<Binding>,
    retained: Option<Binding>,
    target: Instance,
    data_root: PathBuf,
    data_root_identity: String,
    database_files_identity: String,
    package: Package,
    backup: Option<Manifest>,
    backup_archive_sha256: Option<String>,
    restore_tools: Option<Package>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DatabaseChoices {
    pub project_id: String,
    pub revision: u64,
    pub current_instance_id: Option<String>,
    pub retained_instance_ids: Vec<String>,
    pub backup_ids: Vec<String>,
}
struct HashWriter {
    hash: Sha256,
    remaining: usize,
}
impl Write for HashWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > self.remaining {
            return Err(std::io::Error::other("state limit"));
        }
        self.remaining -= bytes.len();
        self.hash.update(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
fn fingerprint(value: &impl Serialize) -> Result<String> {
    let mut writer = HashWriter {
        hash: Sha256::new(),
        remaining: 65536,
    };
    serde_json::to_writer(&mut writer, value).map_err(|_| error("database_state_limit"))?;
    Ok(writer
        .hash
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}
impl DatabaseActionState {
    pub fn fingerprint(&self) -> Result<String> {
        fingerprint(self)
    }
}
fn valid_id(id: &str) -> bool {
    id.len() == 32 && id.bytes().all(|b| b.is_ascii_hexdigit())
}
fn input(command: &Command) -> Result<(&str, &str, &str)> {
    match command {
        Command::SwitchDatabase {
            id,
            instance,
            consent: true,
        } if valid_id(id) && valid_id(instance) => Ok(("bind", id, instance)),
        Command::RestoreBackup {
            id,
            backup,
            consent: true,
        } if valid_id(id) && valid_id(backup) => Ok(("restore", id, backup)),
        _ => Err(error("database_action_unsupported")),
    }
}
fn policy(cancel: &Cancellation, check: &dyn Fn() -> Result<()>) -> Result<()> {
    if cancel.is_cancelled() {
        return Err(error("cancelled"));
    }
    check()
}
fn ordinary_directory(path: &Path) -> Result<PathBuf> {
    let meta = fs::symlink_metadata(path)?;
    if !meta.is_dir() || meta.file_type().is_symlink() {
        return Err(error("database_ownership_unverified"));
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if meta.file_attributes() & 0x400 != 0 {
            return Err(error("database_ownership_unverified"));
        }
    }
    Ok(fs::canonicalize(path)?)
}
fn read_state(
    projects: &devforge_projects::project::ProjectStore,
    catalog: &Catalog,
    command: &Command,
    check: &dyn Fn() -> Result<()>,
) -> Result<DatabaseActionState> {
    check()?;
    let (action, id, selected) = input(command)?;
    let project = projects.get(id)?;
    if project.id != id || !project.active {
        return Err(error("database_ownership_unverified"));
    }
    let root = ordinary_directory(&project.path)?;
    let identity = devforge_projects::discovery::identity(&root)?;
    if project.path != root || project.filesystem_identity.as_deref() != Some(identity.as_str()) {
        return Err(error("discovery_unavailable"));
    }
    let active = projects.binding(id)?;
    if active
        .as_ref()
        .is_some_and(|b| b.project_id != id || !valid_id(&b.instance_id) || !b.ready)
    {
        return Err(error("database_ownership_unverified"));
    }
    let instance_id = if action == "bind" {
        selected
    } else {
        active
            .as_ref()
            .filter(|b| b.ready)
            .ok_or_else(|| error("database_unbound"))?
            .instance_id
            .as_str()
    };
    let target = projects.instance(instance_id)?;
    if !valid_id(&target.id) || target.id != instance_id || !target.ready {
        return Err(error("database_precondition_missing"));
    }
    let package = catalog.get(&target.package)?.clone();
    if package.target != Target::current()?
        || package.variant != "server"
        || package.kind
            != if target.engine == Engine::Postgresql {
                "postgresql"
            } else {
                "mysql"
            }
    {
        return Err(error("database_precondition_missing"));
    }
    let database_root = projects.root().join("databases");
    ordinary_directory(&database_root)?;
    let data_root = ordinary_directory(&database_root.join(&target.id))?;
    let data_root_identity = devforge_projects::discovery::identity(&data_root)?;
    let database_files = ordinary_directory(&data_root.join("data"))?;
    let database_files_identity = devforge_projects::discovery::identity(&database_files)?;
    let retained = if action == "bind" {
        projects.retained_binding(id, instance_id)?
    } else {
        None
    };
    if retained
        .as_ref()
        .is_some_and(|b| b.project_id != id || b.instance_id != instance_id || !b.ready)
    {
        return Err(error("database_ownership_unverified"));
    }
    let (backup, backup_archive_sha256, restore_tools) = if action == "restore" {
        check()?;
        let backups = projects.root().join("backups");
        ordinary_directory(&backups)?;
        ordinary_directory(&backups.join(id))?;
        let path = backups.join(id).join(format!("{selected}.dfbackup"));
        let (manifest, _) = devforge_projects::backup::inspect(&path)?;
        if manifest.id != selected
            || manifest.project != id
            || manifest.engine != target.engine
            || manifest.version != package.version
        {
            return Err(devforge_projects::ProjectError::BackupUnsupported.into());
        }
        let digest = hash_file(&path)?;
        check()?;
        let tools = if target.engine == Engine::Postgresql {
            Some(
                catalog
                    .packages
                    .values()
                    .find(|p| {
                        p.kind == "postgresql-tools" && Some(p.target) == Target::current().ok()
                    })
                    .ok_or_else(|| error("database_precondition_missing"))?
                    .clone(),
            )
        } else {
            None
        };
        (Some(manifest), Some(digest), tools)
    } else {
        (None, None, None)
    };
    check()?;
    let state = DatabaseActionState {
        action: action.into(),
        project_id: id.into(),
        revision: project.revision,
        root,
        identity,
        active,
        retained,
        target,
        data_root,
        data_root_identity,
        database_files_identity,
        package,
        backup,
        backup_archive_sha256,
        restore_tools,
    };
    state.fingerprint()?;
    Ok(state)
}
fn verify_state(
    projects: &devforge_projects::project::ProjectStore,
    catalog: &Catalog,
    command: &Command,
    expected: &DatabaseActionState,
    own_candidate: Option<&Binding>,
    check: &dyn Fn() -> Result<()>,
) -> Result<()> {
    let mut current = read_state(projects, catalog, command, check)?;
    if expected.retained.is_none()
        && own_candidate.is_some_and(|own| {
            current.retained.as_ref().is_some_and(|retained| {
                retained.instance_id == own.instance_id
                    && retained.database == own.database
                    && retained.project_id == own.project_id
                    && retained.secret_ref == own.secret_ref
                    && retained.role == own.role
                    && retained.ready == own.ready
            })
        })
    {
        current.retained = None;
    }
    if current.fingerprint()? != expected.fingerprint()? {
        return Err(error("database_review_changed"));
    }
    Ok(())
}
fn cached(
    workspace: &Workspace,
    state: &DatabaseActionState,
    check: &dyn Fn() -> Result<()>,
) -> Result<()> {
    let mut descriptors = vec![&state.package];
    if let Some(tools) = &state.restore_tools {
        descriptors.push(tools);
    }
    for descriptor in descriptors {
        if descriptor.executables.is_empty() {
            return Err(error("database_precondition_missing"));
        }
        for tool in descriptor.executables.keys() {
            check()?;
            workspace
                .packages
                .executable(descriptor, tool)
                .map_err(|_| error("database_precondition_missing"))?;
            check()?;
        }
    }
    Ok(())
}
/// Advisory bounded snapshot fingerprint; live DatabaseActionState remains the
/// review/activation authority and hashes the actual owned backup archive too.
pub fn database_snapshot_fingerprint(snapshot: &Snapshot, command: &Command) -> Result<String> {
    let (action, id, selected) = input(command)?;
    if snapshot.projects.len() > 2048 || snapshot.databases.len() > 2048 {
        return Err(error("database_state_limit"));
    }
    let mut rows = snapshot.projects.iter().filter(|row| row.project.id == id);
    let row = rows
        .next()
        .ok_or_else(|| error("database_precondition_missing"))?;
    if rows.next().is_some() || row.database_history.len() > 2048 || row.backups.len() > 256 {
        return Err(error("database_state_limit"));
    }
    let instance_id = if action == "bind" {
        selected
    } else {
        row.database
            .as_ref()
            .ok_or_else(|| error("database_unbound"))?
            .instance_id
            .as_str()
    };
    let mut instances = snapshot
        .databases
        .iter()
        .filter(|db| db.instance.id == instance_id);
    let instance = instances
        .next()
        .ok_or_else(|| error("database_precondition_missing"))?;
    if instances.next().is_some() {
        return Err(error("database_state_limit"));
    }
    let retained = row
        .database_history
        .iter()
        .find(|b| b.ready && b.instance_id == instance_id);
    let backup = (action == "restore")
        .then(|| {
            row.backups
                .iter()
                .find(|b| b.id == selected && b.project == id)
        })
        .flatten();
    if action == "restore" && backup.is_none() {
        return Err(error("database_precondition_missing"));
    }
    fingerprint(&(
        action,
        id,
        selected,
        row.project.revision,
        &row.project.filesystem_identity,
        &row.state,
        &row.database,
        retained,
        &instance.instance,
        &instance.state,
        instance.users,
        backup,
    ))
}
impl Workspace {
    pub fn database_choices(&self, id: &str) -> Result<DatabaseChoices> {
        if !valid_id(id) {
            return Err(error("database_action_unsupported"));
        }
        let project = self.available_project(id)?;
        let metadata = self
            .projects
            .storage_metadata(devforge_projects::storage_metadata::Limits::default());
        if !metadata.complete {
            return Err(error("database_state_limit"));
        }
        let current_instance_id = self.projects.binding(id)?.map(|b| b.instance_id);
        let mut retained_instance_ids = metadata
            .retained_bindings
            .iter()
            .filter(|b| b.project_id == id && b.ready && valid_id(&b.instance_id))
            .map(|b| b.instance_id.clone())
            .collect::<Vec<_>>();
        retained_instance_ids.sort();
        retained_instance_ids.dedup();
        let backup_ids = self
            .projects
            .backup_metadata_bounded(id, 256)?
            .into_iter()
            .filter(|m| m.project == id)
            .map(|m| m.id)
            .collect();
        Ok(DatabaseChoices {
            project_id: id.into(),
            revision: project.revision,
            current_instance_id,
            retained_instance_ids,
            backup_ids,
        })
    }
    pub fn database_action_state(
        &self,
        command: &Command,
        check: &dyn Fn() -> Result<()>,
    ) -> Result<DatabaseActionState> {
        check()?;
        let (_, id, _) = input(command)?;
        self.require_stopped(id)?;
        let state = read_state(&self.projects, &self.catalog, command, check)?;
        cached(self, &state, check)?;
        check()?;
        Ok(state)
    }
    pub fn database_action_preflight(&self, command: &Command) -> Result<()> {
        self.database_action_state(command, &|| Ok(())).map(|_| ())
    }
    pub fn execute_database_action(
        &mut self,
        command: Command,
        expected: &DatabaseActionState,
        cancel: &Cancellation,
        check: &dyn Fn() -> Result<()>,
    ) -> Result<serde_json::Value> {
        let guarded = || policy(cancel, check);
        guarded()?;
        let (_, id, _) = input(&command)?;
        self.require_stopped(id)?;
        let current = self.database_action_state(&command, &guarded)?;
        if current.fingerprint()? != expected.fingerprint()? {
            return Err(error("database_review_changed"));
        }
        guarded()?;
        self.ensure_database(&expected.target.id)?;
        guarded()?;
        self.databases
            .get(&expected.target.id)
            .ok_or_else(|| error("database_precondition_missing"))?
            .validate_store_instance(&self.projects, &expected.target)?;
        verify_state(
            &self.projects,
            &self.catalog,
            &command,
            expected,
            None,
            &guarded,
        )?;
        match &command {
            Command::SwitchDatabase { id, instance, .. } => {
                if expected
                    .active
                    .as_ref()
                    .is_some_and(|b| b.instance_id == *instance)
                {
                    guarded()?;
                    return Ok(serde_json::json!({"switched":false}));
                }
                let binding = if let Some(retained) = &expected.retained {
                    guarded()?;
                    if !self
                        .projects
                        .credentials_for(retained.clone(), self.secrets.as_ref())?
                        .connection_test()?
                    {
                        return Err(error("database_unbound"));
                    }
                    retained.clone()
                } else {
                    guarded()?;
                    self.databases.get_mut(instance).unwrap().provision_staged(
                        &self.projects,
                        id,
                        self.secrets.as_ref(),
                    )?
                };
                guarded()?;
                verify_state(
                    &self.projects,
                    &self.catalog,
                    &command,
                    expected,
                    expected.retained.is_none().then_some(&binding),
                    &guarded,
                )?;
                self.databases
                    .get(instance)
                    .unwrap()
                    .validate_store_instance(&self.projects, &self.projects.instance(instance)?)?;
                self.projects.activate_binding(id, Some(&binding))?;
                Ok(serde_json::json!({"switched":true}))
            }
            Command::RestoreBackup { id, backup, .. } => {
                let original = RefCell::new(None);
                let callback = || {
                    guarded().map_err(|e| {
                        *original.borrow_mut() = Some(e);
                        devforge_projects::ProjectError::ExecutionFailed
                    })
                };
                let state_check = |manifest: &Manifest, active: &Binding, instance: &Instance| {
                    let verified = (|| {
                        if fingerprint(&expected.backup)? != fingerprint(&Some(manifest))?
                            || fingerprint(&expected.active)? != fingerprint(&Some(active))?
                            || fingerprint(&expected.target)? != fingerprint(instance)?
                        {
                            return Err(error("database_review_changed"));
                        }
                        verify_state(
                            &self.projects,
                            &self.catalog,
                            &command,
                            expected,
                            None,
                            &guarded,
                        )
                    })();
                    verified.map_err(|e| {
                        *original.borrow_mut() = Some(e);
                        devforge_projects::ProjectError::ExecutionFailed
                    })
                };
                let result = self.projects.restore_backup_checked(
                    id,
                    backup,
                    self.databases.get_mut(&expected.target.id).unwrap(),
                    &self.catalog,
                    &self.packages,
                    self.secrets.as_ref(),
                    cancel,
                    &callback,
                    &state_check,
                );
                result.map_err(|e| {
                    original.borrow_mut().take().unwrap_or_else(|| {
                        if cancel.is_cancelled() {
                            error("cancelled")
                        } else {
                            e.into()
                        }
                    })
                })?;
                Ok(serde_json::json!({"restored":true}))
            }
            _ => Err(error("database_action_unsupported")),
        }
    }
}
