//! Shared daily-workspace orchestration. No Tauri or UI types, no credentials in DTOs.
pub mod debugging;
mod mail;
pub mod repro;
pub mod worktrees;
pub use devforge_inspector::logs::Filter as LogFilter;
mod services;
pub mod setup;
mod system_setup;
use devforge_core::SecretStore;
use devforge_projects::{
    database::{Binding, Instance, RunningDatabase},
    execution::{Context, Invocation, Tool},
    project::{Catalog, Project, ProjectStore, compatible, inspect},
    terminal,
};
use devforge_runtime::{
    Cancellation,
    package::{Limits, PackageStore, Target},
    process::{Observed, Readiness, Supervisor},
};
use devforge_serving::{
    manager::{Options, Serving, SiteStatus},
    site::{Site, SiteStore},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};
#[derive(Debug, Clone, Serialize)]
pub struct Error {
    pub scope: &'static str,
    pub code: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.scope, self.code)
    }
}
impl std::error::Error for Error {}
impl From<devforge_projects::ProjectError> for Error {
    fn from(e: devforge_projects::ProjectError) -> Self {
        Self {
            scope: "project",
            code: e.to_string(),
            project_id: None,
        }
    }
}
impl From<devforge_serving::ServingError> for Error {
    fn from(e: devforge_serving::ServingError) -> Self {
        Self {
            scope: "serving",
            code: e.to_string(),
            project_id: None,
        }
    }
}
impl From<devforge_runtime::RuntimeError> for Error {
    fn from(e: devforge_runtime::RuntimeError) -> Self {
        Self {
            scope: "runtime",
            code: e.to_string(),
            project_id: None,
        }
    }
}
impl From<std::io::Error> for Error {
    fn from(_: std::io::Error) -> Self {
        error("storage_unavailable")
    }
}
fn error(code: &str) -> Error {
    Error {
        scope: "workspace",
        code: code.into(),
        project_id: None,
    }
}
pub type Result<T> = std::result::Result<T, Error>;
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Development {
    pub queue: bool,
}
#[derive(Serialize, Deserialize, Default)]
struct Preferences {
    schema: u32,
    development: BTreeMap<String, Development>,
    #[serde(default)]
    navigation: BTreeMap<String, setup::Navigation>,
}
#[derive(Clone, Serialize)]
pub struct Runtime {
    pub problem: Option<String>,
    pub id: String,
    pub kind: String,
    pub version: String,
    pub variant: String,
    pub installed: bool,
}
#[derive(Clone, Serialize)]
pub struct Row {
    pub setup_owned: bool,
    pub navigation: setup::Navigation,
    pub php_diagnostics: Option<serde_json::Value>,
    pub processes: Vec<(String, devforge_runtime::process::Snapshot)>,
    pub project: Project,
    pub site: Option<Site>,
    pub development: Development,
    pub state: String,
    pub serving: Option<SiteStatus>,
    pub queue_state: Option<String>,
    pub database: Option<Binding>,
    pub backups: Vec<devforge_projects::backup::Manifest>,
    pub database_history: Vec<Binding>,
}
#[derive(Clone, Serialize)]
pub struct Database {
    pub instance: Instance,
    pub state: String,
    pub users: usize,
}
#[derive(Clone, Serialize)]
pub struct Log {
    pub project_id: Option<String>,
    pub process: String,
    pub stream: String,
    pub text: String,
}
#[derive(Clone, Serialize, Default)]
pub struct Snapshot {
    pub worktrees: Vec<worktrees::View>,
    pub starter_constraint: String,
    pub projects: Vec<Row>,
    pub runtimes: Vec<Runtime>,
    pub databases: Vec<Database>,
    pub caches: Vec<services::Cache>,
    pub logs: Vec<Log>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    DebugInstall {
        id: String,
        trusted: bool,
    },
    DebugConfigure {
        id: String,
        revision: u64,
        settings: devforge_debugger::Settings,
        consent: bool,
    },
    DebugBreakpoints {
        id: String,
        revision: u64,
        breakpoints: Vec<devforge_debugger::Breakpoint>,
    },
    DebugControl {
        id: String,
        control: debugging::Control,
    },
    DebugRequest {
        id: String,
        path: String,
    },
    DebugProfileDelete {
        id: String,
        profile: String,
        consent: bool,
    },
    DebugProfileExport {
        id: String,
        profile: String,
        path: PathBuf,
    },
    WorktreeOpen {
        id: String,
    },
    WorktreePlan {
        input: worktrees::Input,
    },
    WorktreeCreate {
        input: worktrees::Input,
        review: String,
        trusted: bool,
    },
    WorktreeCleanupPlan {
        id: String,
    },
    WorktreeCleanup {
        id: String,
        review: String,
        consent: bool,
    },
    CreateProject {
        input: setup::CreateInput,
    },
    PrepareProject {
        id: String,
        options: setup::PrepareOptions,
    },
    PhpDiagnostics {
        id: String,
    },
    PhpSettings {
        id: String,
        revision: u64,
        settings: devforge_projects::php_settings::PhpSettings,
    },
    Favorite {
        id: String,
        favorite: bool,
    },
    Visit {
        id: String,
    },
    HttpsPreview {
        id: String,
    },
    HttpsExport {
        id: String,
    },
    HttpsApply {
        id: String,
        ticket: String,
        digest: String,
        consent: bool,
    },
    InspectorInstall {
        id: String,
    },
    InspectorCapture {
        id: String,
        enabled: bool,
    },
    InspectorClear {
        id: String,
    },
    InspectorProbe {
        id: String,
    },
    RefreshRuntimes,
    Import {
        path: PathBuf,
        name: String,
        php: String,
        node: String,
        domain: String,
        https: bool,
        vite: bool,
        queue: bool,
    },
    Configure {
        id: String,
        revision: u64,
        name: String,
        php: String,
        node: String,
        domain: String,
        https: bool,
        vite: bool,
        queue: bool,
    },
    Forget {
        id: String,
        revision: u64,
    },
    MailConfigure {
        id: String,
        enabled: bool,
    },
    MailDelete {
        id: String,
        message: Option<String>,
        consent: bool,
    },
    MailSave {
        id: String,
        message: String,
        index: usize,
        path: PathBuf,
    },
    Start {
        id: String,
    },
    Stop {
        id: String,
    },
    Restart {
        id: String,
    },
    Install {
        package: String,
    },
    RemoveRuntime {
        package: String,
    },
    CreateDatabase {
        package: String,
    },
    BindDatabase {
        id: String,
        instance: String,
    },
    TestDatabase {
        id: String,
    },
    ActivateDatabaseGeneration {
        id: String,
        instance: String,
        database: String,
        consent: bool,
    },
    UnbindDatabase {
        id: String,
        consent: bool,
    },
    InstallRedisClient {
        id: String,
        consent: bool,
    },
    SwitchDatabase {
        id: String,
        instance: String,
        consent: bool,
    },
    StartDatabase {
        instance: String,
    },
    CreateCache {
        id: String,
        package: String,
    },
    StartCache {
        instance: String,
    },
    StopCache {
        instance: String,
    },
    ConfigureServices {
        id: String,
        revision: u64,
        selection: devforge_projects::cache::ServiceSelection,
    },
    CreateBackup {
        id: String,
    },
    ExportBackup {
        id: String,
        backup: String,
        path: PathBuf,
    },
    ImportBackup {
        id: String,
        path: PathBuf,
    },
    RestoreBackup {
        id: String,
        backup: String,
        consent: bool,
    },
    StopDatabase {
        instance: String,
    },
}
struct Session {
    web: Serving,
    queue: Option<Invocation>,
    supervisor: Supervisor,
}
impl Session {
    fn stop(&mut self) -> Result<()> {
        if let Some(q) = self.queue.take() {
            self.supervisor.stop(q.id())?;
        }
        self.web.stop()?;
        Ok(())
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}
pub struct Workspace {
    pub progress: Arc<Mutex<Option<setup::Progress>>>,
    diagnostics: BTreeMap<String, serde_json::Value>,
    root: PathBuf,
    pub projects: ProjectStore,
    pub sites: SiteStore,
    pub packages: PackageStore,
    pub catalog: Catalog,
    secrets: Arc<dyn SecretStore + Send + Sync>,
    sessions: BTreeMap<String, Session>,
    databases: BTreeMap<String, RunningDatabase>,
    caches: BTreeMap<String, devforge_projects::cache::RunningCache>,
    preferences: Preferences,
    runtime_cache: Vec<Runtime>,
    history: Vec<Log>,
    inspector: Option<devforge_inspector::Collector>,
    mail: BTreeMap<String, devforge_inspector::mail::MailServer>,
    php_capabilities: BTreeMap<String, serde_json::Value>,
    debugging: BTreeMap<String, devforge_debugger::session::Activation>,
}
impl Workspace {
    pub fn open(
        root: impl AsRef<Path>,
        secrets: Arc<dyn SecretStore + Send + Sync>,
    ) -> Result<Self> {
        fs::create_dir_all(root.as_ref())?;
        let root = fs::canonicalize(root)?;
        let pref = root.join("workspace.json");
        let preferences = if pref.exists() {
            let p: Preferences = serde_json::from_slice(&fs::read(&pref)?)
                .map_err(|_| error("storage_unavailable"))?;
            if p.schema != 1 {
                return Err(error("storage_unavailable"));
            }
            p
        } else {
            Preferences {
                schema: 1,
                ..Default::default()
            }
        };
        let mut w = Self {
            progress: Arc::new(Mutex::new(None)),
            diagnostics: BTreeMap::new(),
            projects: ProjectStore::open(root.join("projects"))?,
            sites: SiteStore::open(root.join("serving"))?,
            packages: PackageStore::open(root.join("runtimes"), Limits::default())?,
            catalog: Catalog::bundled(),
            secrets,
            sessions: BTreeMap::new(),
            databases: BTreeMap::new(),
            caches: BTreeMap::new(),
            preferences,
            root,
            runtime_cache: vec![],
            history: vec![],
            inspector: None,
            mail: BTreeMap::new(),
            php_capabilities: BTreeMap::new(),
            debugging: BTreeMap::new(),
        };
        w.refresh_runtimes()?;
        Ok(w)
    }
    fn save(&self) -> Result<()> {
        use std::io::Write;
        let mut file = tempfile::NamedTempFile::new_in(&self.root)?;
        file.write_all(&serde_json::to_vec_pretty(&self.preferences).unwrap())?;
        file.as_file().sync_all()?;
        file.persist(self.root.join("workspace.json"))
            .map_err(|_| error("storage_unavailable"))?;
        Ok(())
    }
    pub fn refresh_runtimes(&mut self) -> Result<()> {
        let target = Target::current()?;
        self.runtime_cache = self
            .catalog
            .packages
            .iter()
            .filter(|(_, p)| p.target == target)
            .map(|(id, p)| {
                let result = if let Some(name) = p.executables.keys().next() {
                    self.packages.executable(p, name).map(|_| ())
                } else if let Some(name) = p.resources.keys().next() {
                    self.packages.resource(p, name).map(|_| ())
                } else {
                    Err(devforge_runtime::RuntimeError::InvalidPackage)
                };
                let installed = result.is_ok();
                let problem = result
                    .err()
                    .filter(|e| *e != devforge_runtime::RuntimeError::NotInstalled)
                    .map(|e| e.to_string());
                Runtime {
                    id: id.clone(),
                    kind: p.kind.clone(),
                    version: p.version.clone(),
                    variant: p.variant.clone(),
                    installed,
                    problem,
                }
            })
            .collect();
        Ok(())
    }
    fn validate_site(&self, id: &str, domain: &str) -> Result<()> {
        devforge_serving::site::domain(domain)?;
        let site = Site {
            project_id: id.into(),
            domain: domain.into(),
            https: false,
            vite: false,
        };
        devforge_serving::site::domain(&site.vite_domain())?;
        if self
            .sites
            .sites()?
            .iter()
            .filter(|s| s.project_id != id)
            .any(|s| s.names().iter().any(|n| site.names().contains(n)))
        {
            return Err(devforge_serving::ServingError::DomainConflict.into());
        }
        Ok(())
    }
    pub fn execute(
        &mut self,
        command: Command,
        cancel: &Cancellation,
    ) -> Result<serde_json::Value> {
        match command {
            Command::DebugInstall { id, trusted } => self.debug_install(&id, trusted, cancel),
            Command::DebugConfigure {
                id,
                revision,
                settings,
                consent,
            } => self.debug_configure(&id, revision, settings, consent),
            Command::DebugBreakpoints {
                id,
                revision,
                breakpoints,
            } => self.debug_breakpoints(&id, revision, breakpoints),
            Command::DebugControl { id, control } => self.debug_control(&id, control),
            Command::DebugRequest { id, path } => self.debug_request(&id, &path),
            Command::DebugProfileDelete {
                id,
                profile,
                consent,
            } => self.debug_profile_delete(&id, &profile, consent),
            Command::DebugProfileExport { id, profile, path } => {
                self.debug_profile_export(&id, &profile, &path)
            }
            Command::WorktreeOpen { id } => self.worktree_open(&id),
            Command::WorktreePlan { input } => {
                Ok(serde_json::to_value(self.worktree_plan(&input)?).unwrap())
            }
            Command::WorktreeCreate {
                input,
                review,
                trusted,
            } => Ok(
                serde_json::to_value(self.worktree_create(input, &review, trusted, cancel)?)
                    .unwrap(),
            ),
            Command::WorktreeCleanupPlan { id } => {
                Ok(serde_json::to_value(self.worktree_cleanup_plan(&id)?).unwrap())
            }
            Command::WorktreeCleanup {
                id,
                review,
                consent,
            } => Ok(
                serde_json::to_value(self.worktree_cleanup(&id, &review, consent, cancel)?)
                    .unwrap(),
            ),
            Command::MailConfigure { id, enabled } => self.configure_mail(&id, enabled),
            Command::MailDelete {
                id,
                message,
                consent,
            } => self.mail_delete(&id, message.as_deref(), consent),
            Command::MailSave {
                id,
                message,
                index,
                path,
            } => self.mail_save(&id, &message, index, &path),
            Command::ActivateDatabaseGeneration {
                id,
                instance,
                database,
                consent,
            } => self.activate_generation(&id, &instance, &database, consent),
            Command::UnbindDatabase { id, consent } => {
                self.require_stopped(&id)?;
                if !consent {
                    return Err(error("service_consent_required"));
                }
                self.projects.activate_binding(&id, None)?;
                Ok(serde_json::json!({"unbound":true}))
            }
            Command::InstallRedisClient { id, consent } => {
                self.install_redis_client(&id, consent, cancel)
            }
            Command::SwitchDatabase {
                id,
                instance,
                consent,
            } => self.switch_database(&id, &instance, consent),
            Command::StartDatabase { instance } => {
                self.ensure_database(&instance)?;
                Ok(serde_json::json!({"started":true}))
            }
            Command::CreateCache { id, package } => self.create_cache(&id, &package, cancel),
            Command::StartCache { instance } => {
                self.ensure_cache(&instance)?;
                Ok(serde_json::json!({"started":true}))
            }
            Command::StopCache { instance } => self.stop_cache(&instance),
            Command::ConfigureServices {
                id,
                revision,
                selection,
            } => {
                self.require_stopped(&id)?;
                self.projects.save_services(&id, revision, selection)?;
                Ok(serde_json::json!({"saved":true}))
            }
            Command::CreateBackup { id } => self.create_backup(&id, cancel),
            Command::ExportBackup { id, backup, path } => {
                self.projects.export_backup(&id, &backup, &path)?;
                Ok(serde_json::json!({"exported":true}))
            }
            Command::ImportBackup { id, path } => {
                let m = self.projects.import_backup(&id, &path)?;
                Ok(serde_json::to_value(m).unwrap())
            }
            Command::RestoreBackup {
                id,
                backup,
                consent,
            } => self.restore_backup(&id, &backup, consent, cancel),
            Command::CreateProject { input } => self.create_project(input, cancel),
            Command::PrepareProject { id, options } => self.prepare_project(&id, &options, cancel),
            Command::PhpDiagnostics { id } => self.php_diagnostics(&id, cancel),
            Command::PhpSettings {
                id,
                revision,
                settings,
            } => self.save_php_settings(&id, revision, settings, cancel),
            Command::Favorite { id, favorite } => {
                self.navigation(&id, Some(favorite), false)?;
                Ok(serde_json::json!({"saved":true}))
            }
            Command::Visit { id } => {
                self.navigation(&id, None, true)?;
                Ok(serde_json::json!({"saved":true}))
            }
            Command::HttpsPreview { id } => self.https_preview(&id),
            Command::HttpsExport { id } => self.https_export(&id),
            Command::HttpsApply {
                id,
                ticket,
                digest,
                consent,
            } => self.https_apply(&id, &ticket, &digest, consent),
            Command::InspectorInstall { id } => {
                if self.projects.get(&id)?.mail {
                    self.ensure_mail(&id)?;
                }
                if self.sessions.contains_key(&id) {
                    return Err(error("inspection_stop_first"));
                }
                let project = self.projects.get(&id)?;
                devforge_inspector::materialize(&project.path)
                    .map_err(|_| error("inspection_package_conflict"))?;
                for (tool, args) in [
                    (
                        Tool::Composer,
                        vec![
                            "config",
                            "repositories.devforge-inspector",
                            r#"{"type":"path","url":".devforge/laravel-inspector","options":{"symlink":false}}"#,
                        ],
                    ),
                    (
                        Tool::Composer,
                        vec![
                            "require",
                            "--dev",
                            "devforge/laravel-inspector:0.2.0",
                            "--no-scripts",
                            "--no-plugins",
                            "--no-interaction",
                        ],
                    ),
                    (
                        Tool::Composer,
                        vec![
                            "reinstall",
                            "devforge/laravel-inspector",
                            "--no-scripts",
                            "--no-plugins",
                            "--no-interaction",
                        ],
                    ),
                    (Tool::Artisan, vec!["package:discover", "--no-interaction"]),
                ] {
                    if cancel.is_cancelled() {
                        return Err(error("inspection_install_failed"));
                    }
                    let result = Context::resolve(
                        &self.projects,
                        &id,
                        &self.catalog,
                        &self.packages,
                        self.secrets.as_ref(),
                    )?
                    .invocation(
                        tool,
                        args.into_iter().map(Into::into).collect(),
                        &self.catalog,
                        &self.packages,
                    )?
                    .run(self.projects.root().join("inspection-install-logs"))?;
                    if !result.success {
                        return Err(error("inspection_install_failed"));
                    }
                }
                Ok(serde_json::json!({"installed":true}))
            }
            Command::InspectorCapture { id, enabled } => {
                let project = self.projects.get(&id)?;
                if enabled
                    && !project
                        .path
                        .join("vendor/devforge/laravel-inspector/src/InspectorServiceProvider.php")
                        .is_file()
                {
                    return Err(error("inspection_package_missing"));
                }
                let running = self.sessions.contains_key(&id);
                if running {
                    self.stop(&id)?;
                }
                if enabled && self.inspector.is_none() {
                    self.inspector = Some(
                        devforge_inspector::Collector::open(
                            &self.root.join("inspector"),
                            self.projects.root(),
                        )
                        .map_err(|_| error("inspection_unavailable"))?,
                    );
                }
                if let Some(inspector) = self.inspector.as_mut() {
                    if enabled {
                        inspector.enable(&id)
                    } else {
                        inspector.disable(&id)
                    }
                    .map_err(|_| error("inspection_unavailable"))?;
                }
                if running {
                    self.start(&id)?;
                }
                Ok(serde_json::json!({"enabled":enabled}))
            }
            Command::InspectorClear { id } => {
                self.projects.get(&id)?;
                if self.inspector.is_none() {
                    self.inspector = Some(
                        devforge_inspector::Collector::open(
                            &self.root.join("inspector"),
                            self.projects.root(),
                        )
                        .map_err(|_| error("inspection_unavailable"))?,
                    );
                }
                self.inspector
                    .as_ref()
                    .unwrap()
                    .clear(&id)
                    .map_err(|_| error("inspection_unavailable"))?;
                Ok(serde_json::json!({"cleared":true}))
            }
            Command::InspectorProbe { id } => {
                if self.projects.get(&id)?.mail {
                    self.ensure_mail(&id)?;
                }
                let result=Context::resolve(&self.projects,&id,&self.catalog,&self.packages,self.secrets.as_ref())?
                    .invocation(Tool::Php,vec!["-r".into(),r#"echo json_encode(['loaded'=>extension_loaded('xdebug'),'version'=>phpversion('xdebug')?:null,'mode'=>ini_get('xdebug.mode')?:null,'php'=>PHP_VERSION]);"#.into()],&self.catalog,&self.packages)?
                    .run(self.projects.root().join("inspection-probe-logs"))?;
                if !result.success {
                    return Err(error("inspection_probe_failed"));
                }
                let text = result
                    .logs
                    .iter()
                    .filter(|l| l.stream == "stdout")
                    .map(|l| l.text.as_str())
                    .collect::<String>();
                let value: serde_json::Value =
                    serde_json::from_str(&text).map_err(|_| error("inspection_probe_failed"))?;
                self.php_capabilities.insert(id, value.clone());
                Ok(value)
            }
            Command::RefreshRuntimes => {
                self.refresh_runtimes()?;
                Ok(serde_json::json!({"refreshed":true}))
            }
            Command::Import {
                path,
                name,
                php,
                node,
                domain,
                https,
                vite,
                queue,
            } => {
                self.validate_site("", &domain)?;
                let p = self
                    .projects
                    .import(&path, &name, &php, &node, &self.catalog)?;
                self.sites.configure(
                    &self.projects,
                    Site {
                        project_id: p.id.clone(),
                        domain,
                        https,
                        vite,
                    },
                )?;
                self.preferences
                    .development
                    .insert(p.id.clone(), Development { queue });
                self.save()?;
                self.navigation(&p.id, None, true)?;
                Ok(serde_json::json!({"project_id":p.id}))
            }
            Command::Configure {
                id,
                revision,
                name,
                php,
                node,
                domain,
                https,
                vite,
                queue,
            } => {
                let p = self.projects.get(&id)?;
                if p.revision != revision {
                    return Err(devforge_projects::ProjectError::StaleRevision.into());
                }
                self.validate_site(&id, &domain)?;
                compatible(
                    &inspect(&p.path)?,
                    self.catalog.get(&php)?,
                    self.catalog.get(&node)?,
                )?;
                let running = self.sessions.contains_key(&id);
                self.stop(&id)?;
                self.projects
                    .configure(&id, revision, &name, &php, &node, &self.catalog)?;
                self.sites.configure(
                    &self.projects,
                    Site {
                        project_id: id.clone(),
                        domain,
                        https,
                        vite,
                    },
                )?;
                self.preferences
                    .development
                    .insert(id.clone(), Development { queue });
                self.save()?;
                if running {
                    self.start(&id)?;
                }
                Ok(serde_json::json!({"saved":true}))
            }
            Command::Forget { id, revision } => {
                if self.managed_worktree(&id)? {
                    return Err(error("worktree_cleanup_required"));
                }
                self.stop(&id)?;
                self.mail.remove(&id);
                self.projects.forget(&id, revision)?;
                self.sites.remove(&id)?;
                self.preferences.development.remove(&id);
                self.preferences.navigation.remove(&id);
                self.diagnostics.remove(&id);
                self.save()?;
                Ok(serde_json::json!({"data_preserved":true}))
            }
            Command::Start { id } => {
                self.start(&id)?;
                Ok(serde_json::json!({"started":true}))
            }
            Command::Stop { id } => {
                self.stop(&id)?;
                Ok(serde_json::json!({"stopped":true}))
            }
            Command::Restart { id } => {
                self.stop(&id)?;
                self.start(&id)?;
                Ok(serde_json::json!({"restarted":true}))
            }
            Command::Install { package } => {
                let p = self.catalog.get(&package)?.clone();
                self.packages.download(&p, cancel, |_| {})?;
                self.refresh_runtimes()?;
                Ok(serde_json::json!({"installed":true}))
            }
            Command::RemoveRuntime { package } => {
                self.packages.remove(self.catalog.get(&package)?)?;
                self.refresh_runtimes()?;
                Ok(serde_json::json!({"removed":true}))
            }
            Command::CreateDatabase { package } => {
                let i = self.projects.initialize_instance(
                    &package,
                    0,
                    &self.catalog,
                    &self.packages,
                    self.secrets.as_ref(),
                )?;
                Ok(serde_json::to_value(i).unwrap())
            }
            Command::BindDatabase { id, instance } => {
                if self.sessions.contains_key(&id) {
                    return Err(error("stop_before_database_change"));
                }
                self.ensure_database(&instance)?;
                let p = self.projects.get(&id)?;
                self.databases
                    .get_mut(&instance)
                    .ok_or_else(|| error("database_external"))?
                    .provision(&self.projects, &p, self.secrets.as_ref())?;
                Ok(serde_json::json!({"bound":true}))
            }
            Command::TestDatabase { id } => {
                let b = self
                    .projects
                    .binding(&id)?
                    .ok_or_else(|| error("database_unbound"))?;
                self.ensure_database(&b.instance_id)?;
                let connected = self
                    .projects
                    .credentials(&id, self.secrets.as_ref())?
                    .connection_test()?;
                Ok(serde_json::json!({"connected":connected}))
            }
            Command::StopDatabase { instance } => {
                if self.users(&instance) > 0 {
                    return Err(error("database_in_use"));
                }
                if let Some(mut server) = self.databases.remove(&instance) {
                    server.stop()?;
                }
                Ok(serde_json::json!({"stopped":true}))
            }
        }
    }
    fn users(&self, id: &str) -> usize {
        self.sessions
            .keys()
            .filter(|p| {
                self.projects
                    .binding(p)
                    .ok()
                    .flatten()
                    .is_some_and(|b| b.instance_id == id)
            })
            .count()
    }
    fn ensure_database(&mut self, id: &str) -> Result<()> {
        if !self.databases.contains_key(id) {
            let server = RunningDatabase::start(
                &self.projects,
                id,
                &self.catalog,
                &self.packages,
                self.secrets.as_ref(),
            )?;
            self.databases.insert(id.into(), server);
        }
        Ok(())
    }
    pub fn start(&mut self, id: &str) -> Result<()> {
        if self.sessions.contains_key(id) {
            return Err(error("already_running"));
        }
        let project = self.projects.get(id)?;
        if project.mail {
            self.ensure_mail(id)?;
        }
        self.ensure_debugging(id)?;
        if project.services.uses_redis() {
            self.ensure_cache(
                project
                    .services
                    .redis_instance
                    .as_deref()
                    .ok_or_else(|| error("database_unbound"))?,
            )?;
        }
        if let Some(binding) = self.projects.binding(id)? {
            self.ensure_database(&binding.instance_id)?;
        }
        let web = Serving::start_selected(
            &self.sites,
            &self.projects,
            &self.catalog,
            &self.packages,
            self.secrets.as_ref(),
            Options::default(),
            Some(&[id.into()]),
        )?;
        let mut session = Session {
            web,
            queue: None,
            supervisor: Supervisor::open(self.root.join("queue-logs").join(id))?,
        };
        if self
            .preferences
            .development
            .get(id)
            .is_some_and(|d| d.queue)
        {
            let dir = self.root.join("queue");
            fs::create_dir_all(&dir)?;
            let path = dir.join(format!("{}.php", devforge_serving::id()));
            fs::write(&path, QUEUE_WRAPPER)?;
            let p = self.projects.get(id)?;
            let nonce = devforge_serving::id();
            let origin = session.web.status().sites[0].preferred_url.clone();
            let invocation = Context::resolve(
                &self.projects,
                id,
                &self.catalog,
                &self.packages,
                self.secrets.as_ref(),
            )?
            .web_origin(&origin)?
            .invocation(
                Tool::Php,
                vec![
                    dunce::simplified(&path).as_os_str().into(),
                    dunce::simplified(&p.path).as_os_str().into(),
                    nonce.clone().into(),
                ],
                &self.catalog,
                &self.packages,
            )?;
            invocation.start_service(
                &mut session.supervisor,
                Readiness::Output {
                    stream: "stdout",
                    needle: format!("DEVFORGE_QUEUE_READY:{nonce}"),
                },
            )?;
            let queue_id = invocation.id().to_owned();
            session.queue = Some(invocation);
            let until = Instant::now() + Duration::from_secs(20);
            loop {
                let status = session.supervisor.snapshot(&queue_id)?;
                if status.observed == Observed::Ready {
                    break;
                }
                if status.observed == Observed::Failed || Instant::now() >= until {
                    self.history
                        .extend(
                            session
                                .supervisor
                                .logs(&queue_id)?
                                .0
                                .into_iter()
                                .map(|c| Log {
                                    project_id: Some(id.into()),
                                    process: "queue".into(),
                                    stream: c.stream.into(),
                                    text: c.text,
                                }),
                        );
                    return Err(error("queue_unavailable"));
                }
                thread::sleep(Duration::from_millis(25));
            }
        }
        self.sessions.insert(id.into(), session);
        self.navigation(id, None, true)?;
        Ok(())
    }
    pub fn stop(&mut self, id: &str) -> Result<()> {
        if let Some(mut session) = self.sessions.remove(id) {
            let mut logs = session
                .web
                .logs()
                .into_iter()
                .map(|(process, chunk)| Log {
                    project_id: Some(id.into()),
                    process,
                    stream: chunk.stream.into(),
                    text: chunk.text,
                })
                .collect::<Vec<_>>();
            if let Some(q) = &session.queue {
                logs.extend(session.supervisor.logs(q.id())?.0.into_iter().map(|c| Log {
                    project_id: Some(id.into()),
                    process: "queue".into(),
                    stream: c.stream.into(),
                    text: c.text,
                }));
            }
            self.history.extend(logs);
            if self.history.len() > 500 {
                self.history.drain(..self.history.len() - 500);
            }
            session.stop()?;
        }
        self.debugging.remove(id);
        Ok(())
    }
    pub fn shutdown(&mut self) -> Result<()> {
        let ids: Vec<_> = self.sessions.keys().cloned().collect();
        for id in ids {
            self.stop(&id)?;
        }
        for (_, mut server) in std::mem::take(&mut self.databases) {
            server.stop()?;
        }
        for (_, mut server) in std::mem::take(&mut self.caches) {
            server.stop()?;
        }
        self.mail.clear();
        self.debugging.clear();
        self.inspector.take();
        Ok(())
    }
    pub fn inspector_view(&self, id: &str) -> Result<serde_json::Value> {
        self.inspector_context(id, None)
    }
    pub fn inspector_context(&self, id: &str, context: Option<&str>) -> Result<serde_json::Value> {
        let p = self.projects.get(id)?;
        let mut capture = if let Some(i) = &self.inspector {
            i.view(id)
        } else {
            devforge_inspector::read(&self.root.join("inspector"), id)
        }
        .map_err(|_| error("inspection_unavailable"))?;
        if let Some(context) = context {
            capture.events =
                devforge_inspector::read_context(&self.root.join("inspector"), id, context)
                    .map_err(|_| error("inspection_unavailable"))?;
        }
        Ok(
            serde_json::json!({"profiles":devforge_inspector::profile::analyze(&capture.events),"capture":capture,"companion_installed":p.path.join("vendor/devforge/laravel-inspector/src/InspectorServiceProvider.php").is_file(),"php_capability":self.php_capabilities.get(id)}),
        )
    }
    pub fn snapshot(&self) -> Result<Snapshot> {
        let sites = self.sites.sites()?;
        let mut logs = self.history.clone();
        let mut rows = vec![];
        for project in self.projects.list()? {
            let session = self.sessions.get(&project.id);
            let serving = session.and_then(|s| s.web.status().sites.into_iter().next());
            let processes = session
                .map(|s| s.web.process_snapshots())
                .unwrap_or_default();
            let queue_state = session.and_then(|s| {
                s.queue.as_ref().map(|q| {
                    s.supervisor
                        .snapshot(q.id())
                        .map(|s| format!("{:?}", s.observed).to_lowercase())
                        .unwrap_or_else(|_| "failed".into())
                })
            });
            let state = if session.is_none() {
                "stopped"
            } else if processes
                .iter()
                .any(|(_, s)| s.observed == Observed::Failed)
                || queue_state.as_deref() == Some("failed")
            {
                "failed"
            } else if session.is_some_and(|s| s.web.status().running)
                && serving.as_ref().is_some_and(|s| {
                    s.php_ready
                        && (!sites
                            .iter()
                            .any(|site| site.project_id == project.id && site.vite)
                            || s.vite_ready)
                })
            {
                "running"
            } else {
                "starting"
            };
            if let Some(session) = session {
                for (process, chunk) in session.web.logs() {
                    logs.push(Log {
                        project_id: Some(project.id.clone()),
                        process,
                        stream: chunk.stream.into(),
                        text: chunk.text,
                    });
                }
                if let Some(q) = &session.queue {
                    for chunk in session.supervisor.logs(q.id())?.0 {
                        logs.push(Log {
                            project_id: Some(project.id.clone()),
                            process: "queue".into(),
                            stream: chunk.stream.into(),
                            text: chunk.text,
                        });
                    }
                }
            }
            let database = self.projects.binding(&project.id)?;
            rows.push(Row {
                setup_owned: self
                    .projects
                    .root()
                    .join("setup-owned")
                    .join(format!("{}.json", project.id))
                    .is_file(),
                navigation: self
                    .preferences
                    .navigation
                    .get(&project.id)
                    .cloned()
                    .unwrap_or_default(),
                php_diagnostics: self.diagnostics.get(&project.id).cloned(),
                processes,
                site: sites.iter().find(|s| s.project_id == project.id).cloned(),
                development: self
                    .preferences
                    .development
                    .get(&project.id)
                    .cloned()
                    .unwrap_or_default(),
                state: state.into(),
                serving,
                queue_state,
                backups: self.projects.backups(&project.id)?,
                database_history: self.projects.binding_history(&project.id)?,
                database,
                project,
            });
        }
        let databases = self
            .projects
            .instances()?
            .into_iter()
            .map(|instance| {
                let server = self.databases.get(&instance.id);
                if let Some(server) = server {
                    for chunk in server.logs() {
                        logs.push(Log {
                            project_id: None,
                            process: format!("{}:{}", instance.engine.driver(), instance.id),
                            stream: chunk.stream.into(),
                            text: chunk.text,
                        });
                    }
                }
                let state = server
                    .and_then(|s| s.status())
                    .map(|s| format!("{:?}", s.observed).to_lowercase())
                    .unwrap_or_else(|| "stopped".into());
                Database {
                    users: self.users(&instance.id),
                    instance,
                    state,
                }
            })
            .collect();
        // Per-process buffers are bounded; cap IPC output as well.
        let mut bytes = 0;
        logs.reverse();
        logs.retain(|log| {
            bytes += log.text.len();
            bytes <= 256 * 1024
        });
        logs.truncate(500);
        logs.reverse();
        Ok(Snapshot {
            worktrees: self.worktrees()?,
            starter_constraint: self
                .catalog
                .defaults
                .get("laravel")
                .cloned()
                .unwrap_or_default(),
            projects: rows,
            runtimes: self.runtime_cache.clone(),
            databases,
            caches: self.cache_snapshot()?,
            logs,
        })
    }
    pub fn open_target(&mut self, id: &str, target: &str) -> Result<()> {
        self.open_target_inner(id, target)?;
        self.navigation(id, None, true)
    }
    fn open_target_inner(&self, id: &str, target: &str) -> Result<()> {
        let project = self.projects.get(id)?;
        if target == "terminal" {
            let mut context = Context::resolve(
                &self.projects,
                id,
                &self.catalog,
                &self.packages,
                self.secrets.as_ref(),
            )?;
            terminal::prepare(&mut context, &self.catalog, &self.packages)?;
            return terminal::open(&self.projects, id, &self.root).map_err(Into::into);
        }
        let url = match target {
            "browser" => {
                self.sessions
                    .get(id)
                    .map(|s| s.web.status())
                    .filter(|s| s.running)
                    .and_then(|s| s.sites.into_iter().next())
                    .ok_or_else(|| error("not_running"))?
                    .preferred_url
            }
            "editor" => format!(
                "vscode://file{}",
                url::Url::from_directory_path(dunce::simplified(&project.path))
                    .map_err(|_| error("open_failed"))?
                    .path()
            ),
            "folder" => url::Url::from_directory_path(dunce::simplified(&project.path))
                .map_err(|_| error("open_failed"))?
                .into(),
            _ => return Err(error("open_failed")),
        };
        open_url(&url)
    }
}
impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}
fn open_url(url: &str) -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        if std::process::Command::new("/usr/bin/open")
            .arg(url)
            .status()?
            .success()
        {
            return Ok(());
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        let u: Vec<u16> = std::ffi::OsStr::new(url)
            .encode_wide()
            .chain(Some(0))
            .collect();
        let verb: Vec<u16> = "open".encode_utf16().chain(Some(0)).collect();
        let result = unsafe {
            windows_sys::Win32::UI::Shell::ShellExecuteW(
                std::ptr::null_mut(),
                verb.as_ptr(),
                u.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                1,
            )
        };
        if result as isize > 32 {
            return Ok(());
        }
    }
    Err(error("open_failed"))
}
const QUEUE_WRAPPER: &str = r#"<?php
$root=$argv[1]; $token=$argv[2]; chdir($root);
require $root.'/vendor/autoload.php'; $app=require $root.'/bootstrap/app.php';
$kernel=$app->make(Illuminate\Contracts\Console\Kernel::class);$kernel->bootstrap();
$app->make('queue')->connection()->size();
fwrite(STDOUT,'DEVFORGE_QUEUE_READY:'.$token.PHP_EOL);fflush(STDOUT);
exit($kernel->handle(new Symfony\Component\Console\Input\ArgvInput(['artisan','queue:work','--sleep=1','--tries=1','--timeout=60','--no-interaction']),new Symfony\Component\Console\Output\ConsoleOutput()));
"#;
#[cfg(feature = "test-support")]
pub mod testing;
