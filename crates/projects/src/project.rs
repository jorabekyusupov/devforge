use crate::{ProjectError as E, Result, private_dir};
use devforge_runtime::package::{Package, Target};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    io::Read,
    path::{Path, PathBuf},
    time::Duration,
};

#[derive(Clone, Serialize, Deserialize)]
pub struct Catalog {
    pub packages: BTreeMap<String, Package>,
    #[serde(default)]
    pub superseded: BTreeMap<String, String>,
    #[serde(default)]
    pub defaults: BTreeMap<String, String>,
}
impl Catalog {
    pub fn bundled() -> Self {
        serde_json::from_str(include_str!("../../../runtime/catalog.json"))
            .expect("bundled reviewed catalog")
    }
    pub fn get(&self, id: &str) -> Result<&Package> {
        self.packages.get(id).ok_or(E::RuntimeUnavailable)
    }
    pub fn php_server(&self, cli: &Package) -> Result<&Package> {
        self.packages
            .values()
            .find(|p| {
                p.kind == "php"
                    && p.version == cli.version
                    && p.target == cli.target
                    && if cli.target == Target::WindowsX64 {
                        p.executables.contains_key("php-cgi")
                    } else {
                        p.variant == "fpm"
                    }
            })
            .ok_or(E::RuntimeUnavailable)
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub id: String,
    pub name: String,
    pub path: PathBuf,
    pub php: String,
    pub node: String,
    pub php_constraint: String,
    pub node_constraint: String,
    pub environment_names: Vec<String>,
    #[serde(default)]
    pub environment_references: BTreeMap<String, String>,
    #[serde(default)]
    pub mail: bool,
    #[serde(default)]
    pub worktree: Option<String>,
    #[serde(default)]
    pub debugger: devforge_debugger::Settings,
    pub revision: u64,
    pub active: bool,
    #[serde(default)]
    pub php_settings: crate::php_settings::PhpSettings,
    #[serde(default)]
    pub services: crate::cache::ServiceSelection,
}
#[derive(Debug, Clone, Serialize)]
pub struct Inspection {
    pub path: PathBuf,
    pub php_constraint: String,
    pub node_constraint: String,
    pub has_package_json: bool,
}
pub fn inspect(path: &Path) -> Result<Inspection> {
    let root = fs::canonicalize(path).map_err(|_| E::InvalidProject)?;
    if !root.is_dir() || !root.join("public/index.php").is_file() {
        return Err(E::InvalidProject);
    }
    let composer = read_json(&root.join("composer.json"))?;
    if !composer["require"]["laravel/framework"].is_string() {
        return Err(E::InvalidProject);
    }
    let php = constraint(&composer["require"], "php")?;
    composer_semver::Constraint::parse(&php).map_err(|_| E::UnsupportedConstraint)?;
    let has_package_json = root.join("package.json").exists();
    let node = if has_package_json {
        constraint(&read_json(&root.join("package.json"))?["engines"], "node")?
    } else {
        "*".into()
    };
    node.parse::<node_semver::Range>()
        .map_err(|_| E::UnsupportedConstraint)?;
    Ok(Inspection {
        path: root,
        php_constraint: php,
        node_constraint: node,
        has_package_json,
    })
}
fn constraint(object: &serde_json::Value, key: &str) -> Result<String> {
    match object.get(key) {
        None => Ok("*".into()),
        Some(v) => v
            .as_str()
            .filter(|s| !s.is_empty() && s.len() <= 4096)
            .map(String::from)
            .ok_or(E::InvalidManifest),
    }
}
fn read_json(path: &Path) -> Result<serde_json::Value> {
    let file = fs::File::open(path).map_err(|_| E::InvalidManifest)?;
    if file.metadata()?.len() > 2 * 1024 * 1024 {
        return Err(E::InvalidManifest);
    }
    let mut bytes = Vec::new();
    file.take(2 * 1024 * 1024 + 1).read_to_end(&mut bytes)?;
    serde_json::from_slice(&bytes).map_err(|_| E::InvalidManifest)
}
pub fn compatible(inspection: &Inspection, php: &Package, node: &Package) -> Result<()> {
    let target = Target::current()?;
    if php.kind != "php"
        || !php.executables.contains_key("php")
        || php.target != target
        || node.kind != "node"
        || node.target != target
    {
        return Err(E::RuntimeUnavailable);
    }
    let pversion = composer_semver::Version::parse(php.version.split('-').next().unwrap())
        .map_err(|_| E::RuntimeUnavailable)?;
    let php_req = composer_semver::Constraint::parse(&inspection.php_constraint)
        .map_err(|_| E::UnsupportedConstraint)?;
    if !php_req.matches(&pversion) {
        return Err(E::IncompatiblePhp);
    }
    let version: node_semver::Version = node
        .version
        .trim_start_matches('v')
        .parse()
        .map_err(|_| E::RuntimeUnavailable)?;
    let range: node_semver::Range = inspection
        .node_constraint
        .parse()
        .map_err(|_| E::UnsupportedConstraint)?;
    if !version.satisfies(&range) {
        return Err(E::IncompatibleNode);
    }
    Ok(())
}
pub struct ProjectStore {
    pub(crate) connection: Connection,
    pub(crate) root: PathBuf,
}
impl ProjectStore {
    pub fn open(root: impl AsRef<Path>) -> Result<Self> {
        fs::create_dir_all(root.as_ref())?;
        let root = fs::canonicalize(root)?;
        let marker = root.join(".devforge-project-store");
        if !marker.exists() {
            if fs::read_dir(&root)?.next().is_some() {
                return Err(E::StorageUnavailable);
            }
            fs::write(&marker, b"devforge-project-store-v1\n")?;
        }
        if fs::read(&marker)? != b"devforge-project-store-v1\n" {
            return Err(E::StorageUnavailable);
        }
        private_dir(&root)?;
        let connection = Connection::open(root.join("projects.sqlite3"))?;
        connection.busy_timeout(Duration::from_secs(3))?;
        connection.pragma_update(None, "foreign_keys", true)?;
        let version: u32 = connection.pragma_query_value(None, "user_version", |r| r.get(0))?;
        if version > 6 {
            return Err(E::StorageUnavailable);
        }
        if version == 0 {
            connection.execute_batch("BEGIN IMMEDIATE; CREATE TABLE projects(id TEXT PRIMARY KEY, path_key TEXT NOT NULL, active INTEGER NOT NULL, revision INTEGER NOT NULL, record TEXT NOT NULL); CREATE UNIQUE INDEX active_project_path ON projects(path_key) WHERE active=1; CREATE TABLE instances(id TEXT PRIMARY KEY,record TEXT NOT NULL); CREATE TABLE database_bindings(project_id TEXT PRIMARY KEY REFERENCES projects(id),instance_id TEXT NOT NULL REFERENCES instances(id),record TEXT NOT NULL); PRAGMA user_version=1; COMMIT;")?;
        }
        if version <= 1 {
            connection.execute_batch("BEGIN IMMEDIATE; CREATE TABLE database_binding_history(project_id TEXT NOT NULL REFERENCES projects(id),binding_key TEXT NOT NULL,record TEXT NOT NULL,PRIMARY KEY(project_id,binding_key)); CREATE TABLE cache_instances(id TEXT PRIMARY KEY,record TEXT NOT NULL); PRAGMA user_version=2; COMMIT;")?;
        }
        if version <= 2 {
            connection.pragma_update(None, "user_version", 3)?;
        }
        if version <= 3 {
            connection.pragma_update(None, "user_version", 4)?;
        }
        if version <= 4 {
            connection.pragma_update(None, "user_version", 5)?;
        }
        if version <= 5 {
            connection.pragma_update(None, "user_version", 6)?;
        }
        Ok(Self { connection, root })
    }
    pub fn root(&self) -> &Path {
        &self.root
    }
    pub(crate) fn operation_lock(&self) -> Result<fs::File> {
        let f = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(self.root.join("operation.lock"))?;
        f.try_lock().map_err(|_| E::InstanceBusy)?;
        Ok(f)
    }
    pub fn import(
        &self,
        path: &Path,
        name: &str,
        php: &str,
        node: &str,
        catalog: &Catalog,
    ) -> Result<Project> {
        let checked = inspect(path)?;
        compatible(&checked, catalog.get(php)?, catalog.get(node)?)?;
        validate_name(name)?;
        let key = path_key(&checked.path);
        if self
            .connection
            .query_row(
                "SELECT 1 FROM projects WHERE path_key=?1 AND active=1",
                [&key],
                |r| r.get::<_, i64>(0),
            )
            .optional()?
            .is_some()
        {
            return Err(E::DuplicateProject);
        }
        let project = Project {
            id: crate::id(),
            name: name.into(),
            path: checked.path,
            php: php.into(),
            node: node.into(),
            php_constraint: checked.php_constraint,
            node_constraint: checked.node_constraint,
            environment_names: Vec::new(),
            environment_references: Default::default(),
            mail: false,
            worktree: None,
            debugger: Default::default(),
            revision: 1,
            active: true,
            php_settings: Default::default(),
            services: Default::default(),
        };
        self.connection.execute("INSERT INTO projects VALUES(?1,?2,1,1,?3)",params![project.id,key,serde_json::to_string(&project).unwrap()]).map_err(|e|if matches!(e,rusqlite::Error::SqliteFailure(ref e,_) if e.code==rusqlite::ErrorCode::ConstraintViolation){E::DuplicateProject}else{E::StorageUnavailable})?;
        Ok(project)
    }
    pub fn list(&self) -> Result<Vec<Project>> {
        let mut query = self
            .connection
            .prepare("SELECT record FROM projects WHERE active=1 ORDER BY id")?;
        query
            .query_map([], |r| r.get::<_, String>(0))?
            .map(|row| serde_json::from_str(&row?).map_err(|_| E::StorageUnavailable))
            .collect()
    }
    pub fn get(&self, id: &str) -> Result<Project> {
        let value = self
            .connection
            .query_row(
                "SELECT record FROM projects WHERE id=?1 AND active=1",
                [id],
                |r| r.get::<_, String>(0),
            )
            .optional()?
            .ok_or(E::NotFound)?;
        serde_json::from_str(&value).map_err(|_| E::StorageUnavailable)
    }
    pub fn configure(
        &self,
        id: &str,
        revision: u64,
        name: &str,
        php: &str,
        node: &str,
        catalog: &Catalog,
    ) -> Result<Project> {
        validate_name(name)?;
        let mut p = self.get(id)?;
        let checked = inspect(&p.path)?;
        compatible(&checked, catalog.get(php)?, catalog.get(node)?)?;
        if p.debugger.mode != devforge_debugger::Mode::Off && p.php != php {
            return Err(E::DebugRuntimeChange);
        }
        p.name = name.into();
        p.php = php.into();
        p.node = node.into();
        p.php_constraint = checked.php_constraint;
        p.node_constraint = checked.node_constraint;
        self.save(&mut p, revision)?;
        Ok(p)
    }
    pub fn forget(&self, id: &str, revision: u64) -> Result<()> {
        let mut p = self.get(id)?;
        if p.worktree.is_some() {
            return Err(E::WorkspaceManaged);
        }
        p.active = false;
        self.save(&mut p, revision)
    }
    pub fn save_debugger(
        &self,
        id: &str,
        revision: u64,
        settings: devforge_debugger::Settings,
    ) -> Result<()> {
        let _lock = self.operation_lock()?;
        let mut p = self.get(id)?;
        settings
            .validate(&p.path)
            .map_err(|_| E::InvalidEnvironment)?;
        p.debugger = settings;
        self.save(&mut p, revision)
    }
    pub fn claim_worktree(&self, id: &str, revision: u64, worktree: &str) -> Result<()> {
        if worktree.len() != 32 || !worktree.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(E::InvalidProject);
        }
        let mut p = self.get(id)?;
        if p.worktree.is_some() {
            return Err(E::WorkspaceManaged);
        }
        p.worktree = Some(worktree.into());
        self.save(&mut p, revision)
    }
    pub fn retire_worktree(&self, id: &str, revision: u64, worktree: &str) -> Result<()> {
        let mut p = self.get(id)?;
        if p.worktree.as_deref() != Some(worktree) {
            return Err(E::WorkspaceManaged);
        }
        p.active = false;
        self.save(&mut p, revision)
    }
    pub(crate) fn save(&self, project: &mut Project, revision: u64) -> Result<()> {
        if project.revision != revision {
            return Err(E::StaleRevision);
        }
        project.revision = project
            .revision
            .checked_add(1)
            .filter(|v| *v <= i64::MAX as u64)
            .ok_or(E::StaleRevision)?;
        let count = self.connection.execute(
            "UPDATE projects SET record=?1,revision=?2,active=?3 WHERE id=?4 AND revision=?5",
            params![
                serde_json::to_string(project).unwrap(),
                project.revision as i64,
                project.active,
                project.id,
                revision as i64
            ],
        )?;
        if count != 1 {
            return Err(E::StaleRevision);
        }
        Ok(())
    }
}
fn path_key(path: &Path) -> String {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if let Ok(meta) = fs::metadata(path) {
            return format!("unix:{}:{}", meta.dev(), meta.ino());
        }
    }
    let text = path.to_string_lossy().into_owned();
    if cfg!(windows) {
        text.to_lowercase()
    } else {
        text
    }
}
pub fn validate_name(name: &str) -> Result<()> {
    if name.trim().is_empty() || name.len() > 200 || name.chars().any(char::is_control) {
        Err(E::InvalidProject)
    } else {
        Ok(())
    }
}
