use crate::{
    ProjectError as E, Result, private_dir,
    project::{Catalog, Project, ProjectStore, compatible, inspect},
};
use devforge_core::SecretStore;
use devforge_runtime::{
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
        if cfg!(windows) {
            let ext = php.path().parent().unwrap().join("ext");
            text.push_str(&format!("extension_dir={}\n", ini_path(&ext)?));
            for name in [
                "openssl",
                "mbstring",
                "fileinfo",
                "curl",
                "pdo_pgsql",
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
                ("DB_CONNECTION", "pgsql".to_owned()),
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
        self,
        tool: Tool,
        args: Vec<OsString>,
        catalog: &Catalog,
        packages: &PackageStore,
    ) -> Result<Invocation> {
        let mut prefix = Vec::new();
        let mut resources = Vec::new();
        let executable = match tool {
            Tool::Node | Tool::Npm => {
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
    pub fn executable_path(&self) -> &std::path::Path {
        self.spec.executable.path()
    }
    pub fn run(&self, logs: impl AsRef<std::path::Path>) -> Result<ExecutionResult> {
        let mut supervisor = Supervisor::open(logs)?;
        supervisor.start_redacted(self.spec.clone(), self.context.sensitive.clone())?;
        let state = supervisor.wait_terminal(&self.spec.id, Duration::from_secs(310))?;
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
fn ini_path(path: &std::path::Path) -> Result<String> {
    let text = dunce::simplified(path).to_string_lossy().replace('\\', "/");
    if text.contains(['"', '\n', '\r']) {
        return Err(E::InvalidEnvironment);
    }
    Ok(format!("\"{text}\""))
}
