use crate::{
    ProjectError as E, Result, private_dir,
    project::{Catalog, Project, ProjectStore, compatible, inspect},
};
use devforge_core::SecretStore;
use devforge_runtime::{
    Cancellation,
    package::{PackageStore, VerifiedExecutable, VerifiedResource},
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
    debug_runtime: Option<devforge_debugger::provider::Runtime>,
    debug_launch: Option<devforge_debugger::session::Launch>,
}
/// Only the selected project's metadata, copied during one short read transaction.
/// No SQLite read lock survives into OS credential access or child execution.
pub(crate) struct TerminalFacts {
    project: Project,
    database: Option<(crate::database::Binding, crate::database::Instance)>,
    cache: Option<crate::cache::CacheInstance>,
}
impl TerminalFacts {
    pub(crate) fn capture(store: &ProjectStore, id: &str) -> Result<Self> {
        use rusqlite::OptionalExtension;
        fn record<T: serde::de::DeserializeOwned>(
            store: &ProjectStore,
            table: &str,
            column: &str,
            id: &str,
            active: bool,
        ) -> Result<Option<T>> {
            // Identifiers below are fixed internal callers; values stay bound.
            let sql = format!(
                "SELECT CASE WHEN octet_length(record)<=262144 THEN record END FROM {table} WHERE {column}=?1{}",
                if active { " AND active=1" } else { "" }
            );
            let raw: Option<Option<String>> = store
                .connection
                .query_row(&sql, [id], |r| r.get(0))
                .optional()?;
            raw.map(|value| {
                serde_json::from_str(&value.ok_or(E::StorageUnavailable)?)
                    .map_err(|_| E::StorageUnavailable)
            })
            .transpose()
        }
        if id.len() != 32 || !id.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(E::InvalidProject);
        }
        let transaction = store.connection.unchecked_transaction()?;
        let project: Project = record(store, "projects", "id", id, true)?.ok_or(E::NotFound)?;
        if project.id != id {
            return Err(E::InvalidProject);
        }
        let binding: Option<crate::database::Binding> =
            record(store, "database_bindings", "project_id", id, false)?;
        let database = binding
            .map(|binding| {
                let instance: crate::database::Instance =
                    record(store, "instances", "id", &binding.instance_id, false)?
                        .ok_or(E::DatabaseUnavailable)?;
                if binding.project_id != project.id || instance.id != binding.instance_id {
                    return Err(E::DatabaseConflict);
                }
                Ok((binding, instance))
            })
            .transpose()?;
        let cache = if project.services.uses_redis() {
            Some(
                record(
                    store,
                    "cache_instances",
                    "id",
                    project
                        .services
                        .redis_instance
                        .as_deref()
                        .ok_or(E::DatabaseUnavailable)?,
                    false,
                )?
                .ok_or(E::DatabaseUnavailable)?,
            )
        } else {
            None
        };
        transaction.commit()?;
        Ok(Self {
            project,
            database,
            cache,
        })
    }
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
                && !key.starts_with("XDEBUG_")
                && key != "PHP_IDE_CONFIG"
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
        project.debugger = Default::default();
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
        let composer = catalog.default_composer()?;
        Self::bootstrap_selection(
            store,
            path,
            (php, node, &composer),
            catalog,
            packages,
            secrets,
        )
    }
    /// Credential-free tooling for a registered project. Preserve its runtime pins,
    /// but do not reuse its identity, environment references or service bindings.
    pub fn bootstrap_for_project(
        store: &ProjectStore,
        project: &Project,
        catalog: &Catalog,
        packages: &PackageStore,
        secrets: &dyn SecretStore,
    ) -> Result<Self> {
        Self::bootstrap_selection(
            store,
            &project.path,
            (&project.php, &project.node, &project.composer),
            catalog,
            packages,
            secrets,
        )
    }
    fn bootstrap_selection(
        store: &ProjectStore,
        path: &std::path::Path,
        runtimes: (&str, &str, &str),
        catalog: &Catalog,
        packages: &PackageStore,
        secrets: &dyn SecretStore,
    ) -> Result<Self> {
        let (php, node, composer) = runtimes;
        catalog.composer(composer)?;
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
            composer: composer.into(),
            php_constraint: "*".into(),
            node_constraint: "*".into(),
            environment_names: vec![],
            environment_references: Default::default(),
            mail: false,
            worktree: None,
            debugger: Default::default(),
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
        let config = store.root.join("config").join(&project.id);
        Self::prepare_in(project, store, catalog, packages, secrets, config, None)
    }
    pub(crate) fn resolve_isolated(
        facts: &TerminalFacts,
        store: &ProjectStore,
        catalog: &Catalog,
        packages: &PackageStore,
        secrets: &dyn SecretStore,
        config: PathBuf,
    ) -> Result<Self> {
        let project = facts.project.clone();
        let inspection = inspect(&project.path)?;
        if inspection.path != project.path {
            return Err(E::InvalidProject);
        }
        if let Some(identity) = &project.filesystem_identity
            && crate::discovery::identity(&project.path)? != *identity
        {
            return Err(E::InvalidProject);
        }
        compatible(
            &inspection,
            catalog.get(&project.php)?,
            catalog.get(&project.node)?,
        )?;
        Self::prepare_in(
            project,
            store,
            catalog,
            packages,
            secrets,
            config,
            Some(facts),
        )
    }
    fn prepare_in(
        project: Project,
        store: &ProjectStore,
        catalog: &Catalog,
        packages: &PackageStore,
        secrets: &dyn SecretStore,
        config: PathBuf,
        frozen: Option<&TerminalFacts>,
    ) -> Result<Self> {
        let id = project.id.as_str();
        let base = catalog.get(&project.php)?;
        let debug_runtime = if project.debugger.mode != devforge_debugger::Mode::Off {
            Some(
                (if frozen.is_some() {
                    devforge_debugger::provider::resolve_existing
                } else {
                    devforge_debugger::provider::resolve
                })(
                    &store.root,
                    project
                        .debugger
                        .runtime
                        .as_deref()
                        .ok_or(E::RuntimeUnavailable)?,
                    base.version.split('-').next().unwrap(),
                    packages,
                    secrets,
                )
                .map_err(|_| E::RuntimeUnavailable)?,
            )
        } else {
            None
        };
        let debug_launch = if debug_runtime.is_some() {
            let directory = store.root.join("debugging");
            if std::fs::symlink_metadata(&directory)
                .is_ok_and(|m| !m.is_dir() || m.file_type().is_symlink())
                || (directory.exists() && std::fs::canonicalize(&directory)? != directory)
            {
                return Err(E::StorageUnavailable);
            }
            devforge_debugger::session::read(&directory.join(id), id)
                .map_err(|_| E::StorageUnavailable)?
        } else {
            None
        };
        if debug_launch.as_ref().is_some_and(|s| {
            Some(s.runtime.as_str()) != project.debugger.runtime.as_deref()
                || s.mode != project.debugger.mode
        }) {
            return Err(E::RuntimeUnavailable);
        }
        let php = if let Some(runtime) = &debug_runtime {
            if let Some(php) = &runtime.php {
                php.clone()
            } else {
                packages.executable(base, "php")?
            }
        } else {
            packages.executable(base, "php")?
        };
        let node = packages.executable(catalog.get(&project.node)?, "node")?;
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
        if let Some(debug) = &debug_runtime {
            text.push_str(&format!("zend_extension={}\nxdebug.discover_client_host=0\nxdebug.start_with_request=trigger\nxdebug.use_compression=0\nxdebug.profiler_output_name=cachegrind.%p.%u.out\nxdebug.log_level=0\n",ini_path(debug.extension.path())?));
            if let Some(s) = &debug_launch {
                text.push_str(&format!("xdebug.mode={}\nxdebug.trigger_value={}\nxdebug.idekey={}\nxdebug.output_dir={}\n",if s.mode==devforge_debugger::Mode::Debug{"debug"}else{"profile"},s.key,s.key,ini_path(&s.output)?));
                if let Some(port) = s.port {
                    text.push_str(&format!(
                        "xdebug.client_host=127.0.0.1\nxdebug.client_port={port}\n"
                    ));
                }
            } else {
                text.push_str("xdebug.mode=off\n");
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
        if let Some(s) = &debug_launch {
            sensitive.push(s.key.clone());
        }
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
        let tooling = store.root.join("config").join(id);
        for (key, dir) in [
            ("COMPOSER_HOME", tooling.join("composer")),
            ("COMPOSER_CACHE_DIR", store.root.join("cache/composer")),
            ("NPM_CONFIG_CACHE", tooling.join("npm-cache")),
        ] {
            private_dir(&dir)?;
            environment.insert(key.into(), dunce::simplified(&dir).as_os_str().to_owned());
        }
        let userconfig = tooling.join("npmrc");
        match std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&userconfig)
        {
            Ok(mut file) => file.write_all(b"; DevForge project-local user config\n")?,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error.into()),
        }
        environment.insert(
            "NPM_CONFIG_USERCONFIG".into(),
            dunce::simplified(&userconfig).as_os_str().to_owned(),
        );
        let binding = match frozen {
            Some(facts) => facts.database.as_ref().map(|(binding, _)| binding.clone()),
            None => store.binding(id)?,
        };
        if let Some(binding) = binding {
            if !binding.ready {
                return Err(E::DatabaseUnavailable);
            }
            let credentials = if let Some(facts) = frozen {
                let (_, instance) = facts.database.as_ref().ok_or(E::DatabaseUnavailable)?;
                crate::database::Credentials {
                    engine: instance.engine,
                    root: store.root.join("databases").join(&instance.id),
                    binding: binding.clone(),
                    port: instance.port,
                    password: zeroize::Zeroizing::new(
                        secrets
                            .get(&binding.secret_ref)
                            .map_err(|_| E::SecretUnavailable)?
                            .ok_or(E::SecretUnavailable)?,
                    ),
                }
            } else {
                store.credentials(id, secrets)?
            };
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
        if let Some(facts) = frozen {
            crate::cache::apply_captured_environment(
                &project,
                facts.cache.as_ref(),
                secrets,
                &mut environment,
                &mut sensitive,
            )?;
        } else {
            crate::cache::apply_environment(
                store,
                &project,
                secrets,
                &mut environment,
                &mut sensitive,
            )?;
        }
        crate::mail::apply(store, &project, secrets, &mut environment, &mut sensitive)?;
        if debug_runtime.is_some() {
            environment.retain(|k, _| {
                !k.to_string_lossy().starts_with("XDEBUG_") && k != "PHP_IDE_CONFIG"
            });
        }
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
            debug_runtime,
            debug_launch,
        })
    }
    pub fn debug_trigger(mut self) -> Result<Self> {
        let launch = self.debug_launch.as_ref().ok_or(E::RuntimeUnavailable)?;
        self.environment
            .insert("XDEBUG_TRIGGER".into(), launch.key.clone().into());
        Ok(self)
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
                if let Some(web) = self.debug_runtime.as_ref().and_then(|d| d.web.as_ref()) {
                    web.clone()
                } else {
                    packages
                        .executable(package, if cfg!(windows) { "php-cgi" } else { "php-fpm" })?
                }
            }
            _ => {
                prefix.extend([
                    "-c".into(),
                    dunce::simplified(&self.ini).as_os_str().to_owned(),
                ]);
                match tool {
                    Tool::Composer => {
                        let package = catalog.composer(&self.project.composer)?;
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
        let profile_budget =
            if matches!(tool, Tool::Php | Tool::PhpWeb | Tool::Artisan | Tool::Queue) {
                self.debug_launch
                    .as_ref()
                    .filter(|s| s.mode == devforge_debugger::Mode::Profile)
                    .map(|s| devforge_runtime::process::FileBudget {
                        directory: s.output.clone(),
                        prefix: "cachegrind.".into(),
                        file_bytes: 32 * 1024 * 1024,
                        total_bytes: 128 * 1024 * 1024,
                        files: 25,
                    })
            } else {
                None
            };
        Ok(Invocation {
            context: self,
            spec,
            _resources: resources,
            profile_budget,
        })
    }
}
pub struct Invocation {
    context: Context,
    spec: ProcessSpec,
    _resources: Vec<VerifiedResource>,
    profile_budget: Option<devforge_runtime::process::FileBudget>,
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
        if let Some(budget) = &self.profile_budget {
            supervisor.attach_file_budget(&self.spec.id, budget.clone())?;
        }
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
        if let Some(budget) = &self.profile_budget {
            supervisor.attach_file_budget(&self.spec.id, budget.clone())?;
        }
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

#[cfg(test)]
mod composer_binding_tests {
    use super::*;
    use devforge_core::ErrorCode;
    use devforge_runtime::package::{Archive, Limits, Package, Target, hash_file};
    use std::{fs, path::Path};
    struct Secrets;
    impl SecretStore for Secrets {
        fn get(&self, _: &str) -> std::result::Result<Option<String>, ErrorCode> {
            Ok(None)
        }
        fn set(&self, _: &str, _: &str) -> std::result::Result<(), ErrorCode> {
            Ok(())
        }
        fn delete(&self, _: &str) -> std::result::Result<(), ErrorCode> {
            Ok(())
        }
    }
    fn install(root: &Path, packages: &PackageStore, kind: &str, version: &str) -> Package {
        let archive = root.join(format!("{kind}-{version}.fixture"));
        if kind == "composer" {
            fs::write(&archive, format!("inert {kind} {version} fixture")).unwrap();
        } else {
            fs::copy(std::env::current_exe().unwrap(), &archive).unwrap();
        }
        let name = if kind == "composer" {
            "composer.phar"
        } else {
            kind
        };
        let p = Package {
            kind: kind.into(),
            version: version.into(),
            variant: "fixture".into(),
            target: Target::current().unwrap(),
            url: "https://example.invalid/fixture".into(),
            sha256: hash_file(&archive).unwrap(),
            archive: Archive::File { path: name.into() },
            executables: if kind == "composer" {
                BTreeMap::new()
            } else {
                BTreeMap::from([(kind.into(), name.into())])
            },
            resources: if kind == "composer" {
                BTreeMap::from([("composer".into(), name.into())])
            } else {
                BTreeMap::new()
            },
        };
        packages
            .install_cached(&p, &archive, &Cancellation::default(), |_| {})
            .unwrap();
        p
    }
    #[test]
    fn invocation_and_terminal_select_persisted_composer_after_default_changes() {
        let temp = tempfile::tempdir().unwrap();
        let packages = PackageStore::open(temp.path().join("packages"), Limits::default()).unwrap();
        let php = install(temp.path(), &packages, "php", "8.5.12");
        let node = install(temp.path(), &packages, "node", "24.22.2");
        let first = install(temp.path(), &packages, "composer", "2.11.2");
        let second = install(temp.path(), &packages, "composer", "2.99.0");
        let mut catalog = Catalog {
            packages: BTreeMap::from([
                ("php".into(), php),
                ("node".into(), node),
                ("first".into(), first.clone()),
                ("second".into(), second),
            ]),
            superseded: BTreeMap::new(),
            defaults: BTreeMap::from([("composer".into(), "2.11.2".into())]),
        };
        let root = temp.path().join("repo");
        fs::create_dir_all(root.join("public")).unwrap();
        fs::write(root.join("public/index.php"), "<?php").unwrap();
        fs::write(
            root.join("composer.json"),
            r#"{"require":{"php":"^8.3","laravel/framework":"^13"}}"#,
        )
        .unwrap();
        let store = ProjectStore::open(temp.path().join("meta")).unwrap();
        let project = store
            .import(&root, "Pinned", "php", "node", &catalog)
            .unwrap();
        catalog.defaults.insert("composer".into(), "2.99.0".into());
        let resource = packages.resource(&first, "composer").unwrap();
        let invocation = Context::resolve(&store, &project.id, &catalog, &packages, &Secrets)
            .unwrap()
            .invocation(
                Tool::Composer,
                vec!["--version".into()],
                &catalog,
                &packages,
            )
            .unwrap();
        assert!(
            invocation
                .spec
                .args
                .contains(&resource.path().as_os_str().to_owned())
        );
        assert!(
            packages.remove(&first).is_err(),
            "selected resource remains leased"
        );
        struct DenySecrets;
        impl SecretStore for DenySecrets {
            fn get(&self, _: &str) -> std::result::Result<Option<String>, ErrorCode> {
                panic!("bootstrap must not read registered secrets")
            }
            fn set(&self, _: &str, _: &str) -> std::result::Result<(), ErrorCode> {
                panic!("bootstrap must not write registered secrets")
            }
            fn delete(&self, _: &str) -> std::result::Result<(), ErrorCode> {
                panic!("bootstrap must not delete registered secrets")
            }
        }
        let mut credentialed_project = project.clone();
        credentialed_project.environment_names = vec!["SHARED_TOKEN".into()];
        credentialed_project
            .environment_references
            .insert("SHARED_TOKEN".into(), "private-ref".into());
        credentialed_project.mail = true;
        // Prove the explicit bootstrap does not need a catalog default at all.
        catalog.defaults.remove("composer");
        let tooling = Context::bootstrap_for_project(
            &store,
            &credentialed_project,
            &catalog,
            &packages,
            &DenySecrets,
        )
        .unwrap();
        assert_ne!(tooling.project.id, project.id);
        assert!(tooling.project.environment_names.is_empty());
        assert!(
            !tooling
                .environment
                .contains_key(std::ffi::OsStr::new("SHARED_TOKEN"))
        );
        assert!(!tooling.project.mail);
        let tooling = tooling
            .invocation(Tool::Composer, vec!["require".into()], &catalog, &packages)
            .unwrap();
        assert!(
            tooling
                .spec
                .args
                .contains(&resource.path().as_os_str().to_owned())
        );
        let mut context =
            Context::resolve(&store, &project.id, &catalog, &packages, &Secrets).unwrap();
        let bin = crate::terminal::prepare(&mut context, &catalog, &packages).unwrap();
        let shim = fs::read_to_string(bin.join(if cfg!(windows) {
            "composer.cmd"
        } else {
            "composer"
        }))
        .unwrap();
        assert!(
            shim.contains(
                &dunce::simplified(resource.path())
                    .to_string_lossy()
                    .to_string()
            )
        );
    }
}
