use crate::{
    ProjectError as E, Result, private_dir,
    project::{Catalog, Project, ProjectStore, compatible, inspect},
};
use devforge_core::SecretStore;
use devforge_runtime::{
    Cancellation,
    package::{PackageStore, Target, VerifiedExecutable, VerifiedResource},
    process::{LogChunk, LogPolicy, Observed, ProcessSpec, Readiness, RestartPolicy, Supervisor},
};
use serde::Serialize;
use std::{collections::BTreeMap, ffi::OsString, path::PathBuf, time::Duration};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Php,
    PhpWeb,
    Node,
    Npm,
    Composer,
    Artisan,
    Queue,
    Git,
}
impl Tool {
    pub fn parse(s: &str) -> Result<Self> {
        match s {
            "php" => Ok(Self::Php),
            "php-web" => Ok(Self::PhpWeb),
            "node" => Ok(Self::Node),
            "npm" => Ok(Self::Npm),
            "composer" => Ok(Self::Composer),
            "artisan" => Ok(Self::Artisan),
            "queue" => Ok(Self::Queue),
            "git" => Ok(Self::Git),
            _ => Err(E::RuntimeUnavailable),
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::Php => "php",
            Self::PhpWeb => "php-web",
            Self::Node => "node",
            Self::Npm => "npm",
            Self::Composer => "composer",
            Self::Artisan => "artisan",
            Self::Queue => "queue",
            Self::Git => "git",
        }
    }
}
/// Contains credentials; deliberately not Debug/Serialize. Public diagnostics
/// return metadata only; the only log route passes through M2 redaction.
pub struct Context {
    pub(crate) project: Project,
    pub(crate) php: VerifiedExecutable,
    pub(crate) node: VerifiedExecutable,
    pub(crate) environment: BTreeMap<OsString, OsString>,
    pub(crate) sensitive: Vec<String>,
    pub(crate) ini: PathBuf,
    pub(crate) config: PathBuf,
}
impl Context {
    /// Serving owns only these runtime-derived values; no .env write occurs.
    pub fn web_origin(mut self, origin: &str) -> Result<Self> {
        if !(origin.starts_with("http://") || origin.starts_with("https://"))
            || origin.contains(['\r', '\n', '\0'])
        {
            return Err(E::InvalidEnvironment);
        }
        self.environment.insert("APP_URL".into(), origin.into());
        self.environment.remove(std::ffi::OsStr::new("ASSET_URL"));
        self.environment.insert(
            "SESSION_SECURE_COOKIE".into(),
            if origin.starts_with("https://") {
                "true"
            } else {
                "false"
            }
            .into(),
        );
        self.environment
            .insert("PHP_FCGI_MAX_REQUESTS".into(), "0".into());
        Ok(self)
    }
    pub fn frontend(mut self) -> Self {
        self.environment.retain(|name, _| {
            let key = name.to_string_lossy();
            !key.starts_with("DB_")
                && !key.starts_with("REDIS_")
                && !key.starts_with("MAIL_")
                && !key.starts_with("DEVFORGE_INSPECTOR_")
                && key != "DATABASE_URL"
                && key != "APP_KEY"
        });
        self
    }
    pub fn setup_sqlite(mut self, path: &std::path::Path) -> Result<Self> {
        let parent = std::fs::canonicalize(path.parent().ok_or(E::InvalidProject)?)?;
        if !parent.starts_with(&self.project.path) {
            return Err(E::InvalidProject);
        }
        self.environment
            .retain(|name, _| !name.to_string_lossy().starts_with("DB_") && name != "DATABASE_URL");
        self.environment
            .insert("DB_CONNECTION".into(), "sqlite".into());
        self.environment.insert(
            "DB_DATABASE".into(),
            dunce::simplified(path).as_os_str().to_owned(),
        );
        self.environment.insert("DB_URL".into(), "".into());
        self.environment.insert("DATABASE_URL".into(), "".into());
        Ok(self)
    }
    /// Dependency tooling does not require an already-installed Redis client.
    /// Scripts remain a caller policy; M10 invokes only fixed no-script commands.
    pub fn tooling(
        store: &ProjectStore,
        id: &str,
        catalog: &Catalog,
        packages: &PackageStore,
        secrets: &dyn SecretStore,
    ) -> Result<Self> {
        let mut project = store.get(id)?;
        project.services = Default::default();
        project.mail = false;
        Self::prepare(project, store, catalog, packages, secrets)
    }
    pub fn resolve(
        store: &ProjectStore,
        id: &str,
        catalog: &Catalog,
        packages: &PackageStore,
        secrets: &dyn SecretStore,
    ) -> Result<Self> {
        let project = store.get(id)?;
        let inspection = inspect(&project.path)?;
        compatible(
            &inspection,
            catalog.get(&project.php)?,
            catalog.get(&project.node)?,
        )?;
        Self::prepare(project, store, catalog, packages, secrets)
    }
    /// Unregistered tooling context: all configuration lives in the caller-owned
    /// temporary store, and no user's project/environment is imported or changed.
    pub fn bootstrap(
        store: &ProjectStore,
        path: &std::path::Path,
        php: &str,
        node: &str,
        catalog: &Catalog,
        packages: &PackageStore,
        secrets: &dyn SecretStore,
    ) -> Result<Self> {
        let path = std::fs::canonicalize(path).map_err(|_| E::InvalidProject)?;
        if !path.is_dir() {
            return Err(E::InvalidProject);
        }
        let project = Project {
            id: crate::id(),
            filesystem_identity: None,
            name: "Setup tooling".into(),
            path: path.clone(),
            php: php.into(),
            node: node.into(),
            php_constraint: "*".into(),
            node_constraint: "*".into(),
            environment_names: vec![],
            environment_references: Default::default(),
            mail: false,
            worktree: None,
            revision: 0,
            active: true,
            php_settings: Default::default(),
            services: Default::default(),
        };
        compatible(
            &crate::project::Inspection {
                path,
                php_constraint: "*".into(),
                node_constraint: "*".into(),
                has_package_json: false,
            },
            catalog.get(php)?,
            catalog.get(node)?,
        )?;
        Self::prepare(project, store, catalog, packages, secrets)
    }
    fn prepare(
        project: Project,
        store: &ProjectStore,
        catalog: &Catalog,
        packages: &PackageStore,
        secrets: &dyn SecretStore,
    ) -> Result<Self> {
        let id = project.id.as_str();
        let php = packages.executable(catalog.get(&project.php)?, "php")?;
        let node = packages.executable(catalog.get(&project.node)?, "node")?;
        let config = store.root.join("config").join(id);
        private_dir(&config)?;
        private_dir(&config.join("scan"))?;
        private_dir(&config.join("sessions"))?;
        let ini = config.join("php.ini");
        let mut text = format!(
            "; DevForge project {}\ndate.timezone=UTC\nsession.save_path={}\n",
            id,
            ini_path(&config.join("sessions"))?
        );
        text.push_str(&project.php_settings.ini()?);
        if cfg!(windows) {
            let ext = php.path().parent().unwrap().join("ext");
            text.push_str(&format!("extension_dir={}\n", ini_path(&ext)?));
            for name in [
                "openssl",
                "mbstring",
                "fileinfo",
                "curl",
                "pdo_pgsql",
                "pdo_mysql",
                "pdo_sqlite",
                "sqlite3",
                "intl",
                "sodium",
                "zip",
            ] {
                if ext.join(format!("php_{name}.dll")).is_file() {
                    text.push_str(&format!("extension={name}\n"));
                }
            }
        }
        // Stage to avoid partial reads by a simultaneously opened project command.
        let mut file = tempfile::NamedTempFile::new_in(&config)?;
        use std::io::Write;
        file.write_all(text.as_bytes())?;
        file.as_file().sync_all()?;
        file.persist(&ini).map_err(|_| E::StorageUnavailable)?;
        let mut environment = crate::environment::base();
        let mut sensitive = Vec::new();
        if let Some(key) = crate::env_file::app_key_secret(&project.path)? {
            sensitive.push(key.to_string());
        }
        for (key, value) in crate::environment::load(&project, secrets)? {
            if !value.is_empty() {
                sensitive.push(value.to_string());
            }
            environment.insert(key, value.to_string().into());
        }
        environment.insert(
            "PHPRC".into(),
            dunce::simplified(&ini).as_os_str().to_owned(),
        );
        environment.insert(
            "PHP_INI_SCAN_DIR".into(),
            dunce::simplified(&config.join("scan"))
                .as_os_str()
                .to_owned(),
        );
        for (key, dir) in [
            ("COMPOSER_HOME", config.join("composer")),
            ("COMPOSER_CACHE_DIR", store.root.join("cache/composer")),
            ("NPM_CONFIG_CACHE", config.join("npm-cache")),
        ] {
            private_dir(&dir)?;
            environment.insert(key.into(), dunce::simplified(&dir).as_os_str().to_owned());
        }
        let userconfig = config.join("npmrc");
        if !userconfig.exists() {
            std::fs::write(&userconfig, b"; DevForge project-local user config\n")?;
        }
        environment.insert(
            "NPM_CONFIG_USERCONFIG".into(),
            dunce::simplified(&userconfig).as_os_str().to_owned(),
        );
        if let Some(binding) = store.binding(id)? {
            if !binding.ready {
                return Err(E::DatabaseUnavailable);
            }
            let credentials = store.credentials(id, secrets)?;
            sensitive.push(credentials.password.to_string());
            for (key, value) in [
                ("DB_CONNECTION", credentials.engine.driver().to_owned()),
                ("DB_HOST", "127.0.0.1".to_owned()),
                ("DB_PORT", credentials.port.to_string()),
                ("DB_DATABASE", binding.database),
                ("DB_USERNAME", binding.role),
                ("DB_PASSWORD", credentials.password.to_string()),
                ("DB_URL", String::new()),
                ("DATABASE_URL", String::new()),
            ] {
                environment.insert(key.into(), value.into());
            }
        }
        crate::cache::apply_environment(
            store,
            &project,
            secrets,
            &mut environment,
            &mut sensitive,
        )?;
        crate::mail::apply(store, &project, secrets, &mut environment, &mut sensitive)?;
        let mut paths = vec![
            node.path().parent().unwrap().to_path_buf(),
            php.path().parent().unwrap().to_path_buf(),
        ];
        if cfg!(windows) {
            if let Some(root) = std::env::var_os("SystemRoot") {
                paths.push(PathBuf::from(root).join("System32"));
            }
        } else {
            paths.extend([
                PathBuf::from("/usr/bin"),
                PathBuf::from("/bin"),
                PathBuf::from("/usr/sbin"),
                PathBuf::from("/sbin"),
            ]);
        }
        if let Some(ambient) = std::env::var_os("PATH") {
            paths.extend(std::env::split_paths(&ambient).filter(|path| path.is_absolute()));
        }
        environment.insert(
            "PATH".into(),
            std::env::join_paths(paths.iter().map(|path| dunce::simplified(path)))
                .map_err(|_| E::InvalidEnvironment)?,
        );
        if let Some(session) =
            devforge_inspector::read_session(&store.root, id).map_err(|_| E::StorageUnavailable)?
        {
            sensitive.push(session.token.clone());
            environment.insert(
                "DEVFORGE_INSPECTOR_ENDPOINT".into(),
                session.endpoint.into(),
            );
            environment.insert("DEVFORGE_INSPECTOR_TOKEN".into(), session.token.into());
            environment.insert("DEVFORGE_INSPECTOR_PROJECT".into(), id.into());
        }
        Ok(Self {
            project,
            php,
            node,
            environment,
            sensitive,
            ini,
            config,
        })
    }
    pub fn project(&self) -> &Project {
        &self.project
    }
    pub fn php_path(&self) -> &std::path::Path {
        self.php.path()
    }
    pub fn node_path(&self) -> &std::path::Path {
        self.node.path()
    }
    pub fn ini_path(&self) -> &std::path::Path {
        &self.ini
    }
    pub fn invocation(
        mut self,
        tool: Tool,
        args: Vec<OsString>,
        catalog: &Catalog,
        packages: &PackageStore,
    ) -> Result<Invocation> {
        let mut prefix = Vec::new();
        let mut resources = Vec::new();
        let executable = match tool {
            Tool::Node | Tool::Npm | Tool::Git => {
                self.environment
                    .retain(|key, _| !key.to_string_lossy().starts_with("DEVFORGE_INSPECTOR_"));
                if tool == Tool::Npm {
                    let npm = if cfg!(windows) {
                        self.node
                            .path()
                            .parent()
                            .unwrap()
                            .join("node_modules/npm/bin/npm-cli.js")
                    } else {
                        self.node
                            .path()
                            .parent()
                            .unwrap()
                            .parent()
                            .unwrap()
                            .join("lib/node_modules/npm/bin/npm-cli.js")
                    };
                    if !npm.is_file() {
                        return Err(E::RuntimeUnavailable);
                    }
                    prefix.push(npm.into_os_string());
                }
                if tool == Tool::Git {
                    let executable = find_git()?;
                    // Git inherits M2's owned process group/Job through verified
                    // Node. No shell command or user-supplied JavaScript is used.
                    prefix.extend(["-e".into(),r#"const cp=require('node:child_process');const child=cp.spawn(process.argv[1],process.argv.slice(2),{stdio:'inherit',shell:false,windowsHide:true});child.on('error',()=>process.exit(127));child.on('exit',code=>process.exit(code===null?1:code));"#.into(),executable.into_os_string()]);
                    self.environment
                        .insert("GIT_TERMINAL_PROMPT".into(), "0".into());
                    self.environment
                        .insert("GCM_INTERACTIVE".into(), "never".into());
                    self.environment.insert(
                        "GIT_SSH_COMMAND".into(),
                        "ssh -o BatchMode=yes -o StrictHostKeyChecking=yes".into(),
                    );
                    if let Some(v) = std::env::var_os("SSH_AUTH_SOCK") {
                        self.environment.insert("SSH_AUTH_SOCK".into(), v);
                    }
                }
                self.node.clone()
            }
            Tool::PhpWeb => {
                let package = catalog.php_server(catalog.get(&self.project.php)?)?;
                prefix.extend([
                    "-c".into(),
                    dunce::simplified(&self.ini).as_os_str().to_owned(),
                ]);
                packages.executable(package, if cfg!(windows) { "php-cgi" } else { "php-fpm" })?
            }
            _ => {
                prefix.extend([
                    "-c".into(),
                    dunce::simplified(&self.ini).as_os_str().to_owned(),
                ]);
                match tool {
                    Tool::Composer => {
                        let version = catalog
                            .defaults
                            .get("composer")
                            .ok_or(E::RuntimeUnavailable)?;
                        let package = catalog
                            .packages
                            .values()
                            .find(|p| {
                                p.kind == "composer"
                                    && p.version == *version
                                    && p.target == Target::current().unwrap()
                            })
                            .ok_or(E::RuntimeUnavailable)?;
                        let resource = packages.resource(package, "composer")?;
                        resource.check()?;
                        prefix.push(resource.path().as_os_str().to_owned());
                        resources.push(resource);
                    }
                    Tool::Artisan | Tool::Queue => {
                        if !self.project.path.join("artisan").is_file() {
                            return Err(E::InvalidProject);
                        }
                        prefix.push("artisan".into());
                        if tool == Tool::Queue {
                            prefix.push("queue:work".into());
                        }
                    }
                    _ => {}
                }
                self.php.clone()
            }
        };
        prefix.extend(args);
        let spec = ProcessSpec {
            input: None,
            id: format!("exec-{}-{}", self.project.id, tool.name()),
            executable,
            args: prefix,
            directory: self.project.path.clone(),
            environment: self.environment.clone(),
            readiness: Readiness::SuccessfulExit,
            readiness_timeout: Duration::from_secs(300),
            stop_grace: Duration::from_secs(5),
            restart: RestartPolicy::default(),
            logs: LogPolicy::default(),
        };
        Ok(Invocation {
            context: self,
            spec,
            _resources: resources,
        })
    }
}
pub struct Invocation {
    context: Context,
    spec: ProcessSpec,
    _resources: Vec<VerifiedResource>,
}
#[derive(Serialize)]
pub struct ExecutionResult {
    pub success: bool,
    pub exit_code: Option<i32>,
    pub logs: Vec<LogChunk>,
    pub dropped_bytes: u64,
}
impl Invocation {
    pub fn id(&self) -> &str {
        &self.spec.id
    }
    pub fn start_service(&self, supervisor: &mut Supervisor, readiness: Readiness) -> Result<()> {
        let mut spec = self.spec.clone();
        spec.readiness = readiness;
        spec.restart = RestartPolicy {
            max_restarts: 2,
            backoff: Duration::from_millis(200),
        };
        supervisor.start_redacted(spec, self.context.sensitive.clone())?;
        Ok(())
    }
    pub fn executable_path(&self) -> &std::path::Path {
        self.spec.executable.path()
    }
    pub fn run(&self, logs: impl AsRef<std::path::Path>) -> Result<ExecutionResult> {
        self.run_cancellable(logs, &Cancellation::default())
    }
    pub fn run_cancellable(
        &self,
        logs: impl AsRef<std::path::Path>,
        cancel: &Cancellation,
    ) -> Result<ExecutionResult> {
        if cancel.is_cancelled() {
            return Err(E::ExecutionFailed);
        }
        let mut supervisor = Supervisor::open(logs)?;
        supervisor.start_redacted(self.spec.clone(), self.context.sensitive.clone())?;
        let until = std::time::Instant::now() + Duration::from_secs(310);
        let state = loop {
            let state = supervisor.snapshot(&self.spec.id)?;
            if matches!(
                state.observed,
                Observed::Succeeded | Observed::Failed | Observed::Stopped
            ) {
                break state;
            }
            if cancel.is_cancelled() || std::time::Instant::now() > until {
                supervisor.stop(&self.spec.id)?;
                return Err(E::ExecutionFailed);
            }
            std::thread::sleep(Duration::from_millis(25));
        };
        let (logs, dropped_bytes) = supervisor.logs(&self.spec.id)?;
        supervisor.stop(&self.spec.id)?;
        Ok(ExecutionResult {
            success: state.observed == Observed::Succeeded,
            exit_code: state.exit_code,
            logs,
            dropped_bytes,
        })
    }
}
pub fn find_git() -> Result<PathBuf> {
    let name = if cfg!(windows) { "git.exe" } else { "git" };
    std::env::var_os("PATH")
        .and_then(|path| {
            std::env::split_paths(&path)
                .filter(|p| p.is_absolute())
                .map(|p| p.join(name))
                .find(|p| p.is_file())
        })
        .and_then(|p| dunce::canonicalize(p).ok())
        .ok_or(E::RuntimeUnavailable)
}
fn ini_path(path: &std::path::Path) -> Result<String> {
    let text = dunce::simplified(path).to_string_lossy().replace('\\', "/");
    if text.contains(['"', '\n', '\r']) {
        return Err(E::InvalidEnvironment);
    }
    Ok(format!("\"{text}\""))
}
