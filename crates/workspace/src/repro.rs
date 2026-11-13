//! Repository-to-local reconciliation; independent of desktop presentation.
//! Setup has a durable stage journal, not an illusory cross-filesystem/SQL transaction.
use super::*;
use devforge_projects::{
    cache::ServiceSelection,
    env_file::EnvPlan,
    environment,
    manifest::{self, Manifest},
};
use sha2::{Digest, Sha256};
use std::{fs::File, io::Write};
use zeroize::Zeroizing;
fn hash(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
fn code(value: &str) -> Error {
    error(value)
}
fn root(path: &Path) -> Result<PathBuf> {
    let root = fs::canonicalize(path)?;
    if !root.is_dir() || root.to_str().is_none() {
        return Err(code("manifest_invalid"));
    }
    Ok(root)
}
fn scope(path: &Path) -> String {
    hash(path.to_string_lossy().as_bytes())
}
fn secret_key(path: &Path, reference: &str) -> String {
    format!("manifest/{}/secret/{reference}", scope(path))
}
fn optional(path: &Path, max: u64) -> Result<Option<Vec<u8>>> {
    if fs::symlink_metadata(path).is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound) {
        return Ok(None);
    }
    Ok(Some(manifest::read_file(path, max)?))
}
fn inputs(path: &Path) -> Result<BTreeMap<String, Option<String>>> {
    [
        "composer.json",
        "composer.lock",
        "package.json",
        "package-lock.json",
        ".env.example",
    ]
    .iter()
    .map(|name| {
        Ok((
            name.to_string(),
            optional(&path.join(name), 8 * 1024 * 1024)?.map(|b| hash(&b)),
        ))
    })
    .collect()
}
fn env_hash(path: &Path) -> Result<String> {
    Ok(hash(
        &serde_json::to_vec(&optional(&path.join(".env"), 1024 * 1024)?).unwrap(),
    ))
}
fn atomic(path: &Path, data: &[u8]) -> Result<()> {
    let mut f = tempfile::NamedTempFile::new_in(path.parent().unwrap())?;
    f.write_all(data)?;
    f.as_file().sync_all()?;
    f.persist(path).map_err(|_| code("storage_unavailable"))?;
    Ok(())
}
#[derive(Clone, Serialize, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct Journal {
    pub schema: u32,
    pub project_id: Option<String>,
    pub stage: String,
    pub manifest: String,
    pub database: Option<String>,
    pub redis: Option<String>,
    pub completed: bool,
    pub last_error: Option<String>,
}
#[derive(Serialize, Clone)]
pub struct SecretStatus {
    pub environment: String,
    pub reference: String,
    pub available: bool,
    pub version: Option<String>,
    pub existing_project: bool,
}
#[derive(Serialize, Clone)]
pub struct Plan {
    pub schema: u32,
    pub root: PathBuf,
    pub target: Target,
    pub project_id: Option<String>,
    pub packages: BTreeMap<String, String>,
    pub downloads: Vec<String>,
    pub secrets: Vec<SecretStatus>,
    pub env: Vec<(String, String)>,
    pub write_env: bool,
    pub database_action: String,
    pub environment_file: serde_json::Value,
    pub settings: serde_json::Value,
    pub blockers: Vec<String>,
    pub state_hash: String,
    pub digest: String,
}
impl Workspace {
    pub(crate) fn repro_dir(&self, path: &Path) -> Result<PathBuf> {
        let parent = self.root.join("manifests");
        for dir in [&parent, &parent.join(scope(path))] {
            if !dir.exists() {
                fs::create_dir(dir)?;
            }
            if !fs::symlink_metadata(dir)?.is_dir()
                || fs::symlink_metadata(dir)?.file_type().is_symlink()
                || fs::canonicalize(dir)? != *dir
            {
                return Err(code("manifest_invalid"));
            }
        }
        Ok(parent.join(scope(path)))
    }
    pub(crate) fn repro_lock(&self, path: &Path) -> Result<File> {
        let path = self.repro_dir(path)?.join("setup.lock");
        if fs::symlink_metadata(&path).is_ok_and(|m| !m.is_file() || m.file_type().is_symlink()) {
            return Err(code("manifest_invalid"));
        }
        let f = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)?;
        f.try_lock().map_err(|_| code("manifest_busy"))?;
        Ok(f)
    }
    fn project_at(&self, path: &Path) -> Result<Option<Project>> {
        Ok(self.projects.list()?.into_iter().find(|p| p.path == path))
    }
    fn document(&self, path: &Path) -> Result<(Manifest, Vec<u8>)> {
        let bytes = manifest::read_file(&path.join("devforge.yaml"), 65536)?;
        let m = Manifest::parse(&bytes).map_err(|_| code("manifest_invalid"))?;
        self.validate_site(
            self.project_at(path)?
                .as_ref()
                .map_or("", |p| p.id.as_str()),
            &m.project.domain,
        )?;
        Ok((m, bytes))
    }
    pub fn manifest_status(&self, path: &Path) -> Result<Journal> {
        let path = root(path)?;
        let p = self.repro_dir(&path)?.join("journal.json");
        match optional(&p, 65536)? {
            Some(b) => {
                let j: Journal =
                    serde_json::from_slice(&b).map_err(|_| code("storage_unavailable"))?;
                if j.schema != 1 {
                    return Err(code("storage_unavailable"));
                }
                Ok(j)
            }
            None => Ok(Journal {
                schema: 1,
                stage: "not_started".into(),
                ..Default::default()
            }),
        }
    }
    fn save_journal(&self, path: &Path, j: &Journal) -> Result<()> {
        atomic(
            &self.repro_dir(path)?.join("journal.json"),
            &serde_json::to_vec_pretty(j).unwrap(),
        )
    }
    pub(crate) fn manifest_secret_revision(&self, path: &Path, reference: &str) -> Result<String> {
        if !manifest::alias(reference) {
            return Err(code("manifest_secret_invalid"));
        }
        self.secrets
            .get(&format!("{}/revision", secret_key(path, reference)))
            .map_err(|_| code("manifest_secret_unavailable"))?
            .ok_or_else(|| code("manifest_secret_missing"))
    }
    pub(crate) fn manifest_secret_value(
        &self,
        path: &Path,
        reference: &str,
    ) -> Result<Zeroizing<String>> {
        if !manifest::alias(reference) {
            return Err(code("manifest_secret_invalid"));
        }
        self.secrets
            .get(&secret_key(path, reference))
            .map_err(|_| code("manifest_secret_unavailable"))?
            .map(Zeroizing::new)
            .ok_or_else(|| code("manifest_secret_missing"))
    }
    pub fn manifest_secret(&self, path: &Path, reference: &str, value: &str) -> Result<()> {
        let path = root(path)?;
        if !manifest::alias(reference) || value.len() > 4096 || value.contains('\0') {
            return Err(code("manifest_secret_invalid"));
        }
        let _lock = self.repro_lock(&path)?;
        let key = secret_key(&path, reference);
        let version_key = format!("{key}/revision");
        let old = self
            .secrets
            .get(&key)
            .map_err(|_| code("manifest_secret_unavailable"))?
            .map(Zeroizing::new);
        self.secrets
            .set(&key, value)
            .map_err(|_| code("manifest_secret_unavailable"))?;
        if self
            .secrets
            .set(&version_key, &devforge_serving::id())
            .is_err()
        {
            match old {
                Some(old) => self.secrets.set(&key, &old),
                None => self.secrets.delete(&key),
            }
            .map_err(|_| code("manifest_secret_unavailable"))?;
            return Err(code("manifest_secret_unavailable"));
        }
        Ok(())
    }
    pub fn manifest_definition(&self, id: &str) -> Result<Manifest> {
        let p = self.projects.get(id)?;
        let site = self
            .sites
            .sites()?
            .into_iter()
            .find(|s| s.project_id == id)
            .ok_or_else(|| code("setup_site_missing"))?;
        let composer = self
            .catalog
            .packages
            .values()
            .find(|p| p.kind == "composer" && Some(p.target) == Target::current().ok())
            .ok_or_else(|| code("manifest_runtime_unavailable"))?;
        let database = self
            .projects
            .binding(id)?
            .map(|b| self.projects.instance(&b.instance_id))
            .transpose()?
            .map(|i| {
                Ok::<_, Error>(manifest::Database {
                    engine: i.engine,
                    version: self.catalog.get(&i.package)?.version.clone(),
                })
            })
            .transpose()?;
        let redis = p
            .services
            .redis_instance
            .as_deref()
            .map(|i| self.projects.cache_instance(i))
            .transpose()?
            .map(|i| {
                Ok::<_, Error>(manifest::Redis {
                    version: self.catalog.get(&i.package)?.version.clone(),
                    dotnet: self.catalog.get(&i.runtime)?.version.clone(),
                })
            })
            .transpose()?;
        let env = p
            .environment_names
            .iter()
            .filter(|n| manifest::environment_name(n).is_ok())
            .map(|name| {
                let alias = p
                    .environment_references
                    .get(name)
                    .and_then(|r| r.strip_prefix(&format!("manifest/{}/secret/", scope(&p.path))))
                    .filter(|v| manifest::alias(v))
                    .map(str::to_string)
                    .unwrap_or_else(|| {
                        let n = name.to_lowercase();
                        if n.len() <= 64 {
                            n
                        } else {
                            format!("{}-{}", &n[..40], &hash(name.as_bytes())[..12])
                        }
                    });
                (name.clone(), alias)
            })
            .collect();
        let mut m = Manifest {
            schema: 1,
            mail: Some(p.mail),
            project: manifest::Project {
                name: p.name.clone(),
                domain: site.domain,
                https: site.https,
                vite: site.vite,
                queue: self
                    .preferences
                    .development
                    .get(id)
                    .is_some_and(|d| d.queue),
            },
            runtimes: manifest::Runtimes {
                php: self
                    .catalog
                    .get(&p.php)?
                    .version
                    .split('-')
                    .next()
                    .unwrap()
                    .into(),
                node: self
                    .catalog
                    .get(&p.node)?
                    .version
                    .trim_start_matches('v')
                    .into(),
                composer: composer.version.clone(),
            },
            database,
            redis,
            services: manifest::Drivers {
                cache: p.services.cache,
                queue: p.services.queue,
            },
            php_settings: p.php_settings.clone(),
            environment: env,
            setup: Default::default(),
            pins: Default::default(),
        };
        m.pin_all(&self.catalog)?;
        Ok(m)
    }
    pub fn manifest_export(&self, id: &str, destination: Option<&Path>) -> Result<Manifest> {
        self.require_stopped(id)?;
        let p = self.projects.get(id)?;
        let m = self.manifest_definition(id)?;
        let destination = destination
            .map(Path::to_path_buf)
            .unwrap_or_else(|| p.path.join("devforge.yaml"));
        let mut f = tempfile::NamedTempFile::new_in(
            destination
                .parent()
                .ok_or_else(|| code("manifest_invalid"))?,
        )?;
        f.write_all(m.yaml()?.as_bytes())?;
        f.as_file().sync_all()?;
        f.persist_noclobber(destination)
            .map_err(|_| code("manifest_file_exists"))?;
        Ok(m)
    }
    pub fn manifest_init(&self, path: &Path, php: &str, node: &str, composer: &str) -> Result<()> {
        let path = root(path)?;
        let name = path
            .file_name()
            .ok_or_else(|| code("manifest_invalid"))?
            .to_string_lossy()
            .to_string();
        let slug = name
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
            .collect::<String>()
            .to_ascii_lowercase();
        let slug = if slug.is_empty() {
            "project"
        } else {
            slug.as_str()
        };
        let mut m = Manifest {
            schema: 1,
            mail: None,
            project: manifest::Project {
                name,
                domain: format!("{slug}.devforge.test"),
                https: false,
                vite: false,
                queue: false,
            },
            runtimes: manifest::Runtimes {
                php: php.into(),
                node: node.into(),
                composer: composer.into(),
            },
            database: None,
            redis: None,
            services: Default::default(),
            php_settings: Default::default(),
            environment: Default::default(),
            setup: Default::default(),
            pins: Default::default(),
        };
        m.validate()?;
        self.validate_site(
            self.project_at(&path)?
                .as_ref()
                .map_or("", |p| p.id.as_str()),
            &m.project.domain,
        )?;
        m.pin_all(&self.catalog)?;
        let mut f = tempfile::NamedTempFile::new_in(&path)?;
        f.write_all(m.yaml()?.as_bytes())?;
        f.persist_noclobber(path.join("devforge.yaml"))
            .map_err(|_| code("manifest_file_exists"))?;
        Ok(())
    }
    fn secret_status(
        &self,
        path: &Path,
        m: &Manifest,
        p: Option<&Project>,
    ) -> Result<Vec<SecretStatus>> {
        m.environment
            .iter()
            .map(|(name, reference)| {
                let key = secret_key(path, reference);
                let value = self
                    .secrets
                    .get(&key)
                    .map_err(|_| code("manifest_secret_unavailable"))?
                    .map(Zeroizing::new);
                let version = self
                    .secrets
                    .get(&format!("{key}/revision"))
                    .map_err(|_| code("manifest_secret_unavailable"))?;
                let existing =
                    p.is_some_and(|p| p.environment_names.contains(name)) && value.is_none();
                let fallback = if existing {
                    self.secrets
                        .get(
                            p.unwrap()
                                .environment_references
                                .get(name)
                                .unwrap_or(&environment::key(p.unwrap(), name)),
                        )
                        .map_err(|_| code("manifest_secret_unavailable"))?
                        .map(Zeroizing::new)
                } else {
                    None
                };
                let selected = value.as_ref().or(fallback.as_ref());
                if let Some(v) = selected {
                    environment::validate(name, v)?;
                }
                Ok(SecretStatus {
                    environment: name.clone(),
                    reference: reference.clone(),
                    available: selected.is_some() && (existing || version.is_some()),
                    version,
                    existing_project: existing,
                })
            })
            .collect()
    }
    pub fn manifest_plan(&self, path: &Path, write_env: bool) -> Result<Plan> {
        let path = root(path)?;
        let _lock = self.repro_lock(&path)?;
        self.plan(&path, write_env)
    }
    fn plan(&self, path: &Path, write_env: bool) -> Result<Plan> {
        let (m, bytes) = self.document(path)?;
        let p = self.project_at(path)?;
        let target = Target::current()?;
        let packages = m
            .verify_pins(&self.catalog, target)
            .map_err(|_| code("manifest_runtime_unavailable"))?;
        compatible(
            &inspect(path)?,
            self.catalog.get(&packages["php"])?,
            self.catalog.get(&packages["node"])?,
        )?;
        let secrets = self.secret_status(path, &m, p.as_ref())?;
        let mut blockers = vec![];
        if secrets.iter().any(|s| !s.available) {
            blockers.push("manifest_secret_missing".into());
        }
        let redis_driver = m.services.cache == devforge_projects::cache::CacheDriver::Redis
            || m.services.queue == devforge_projects::cache::QueueDriver::Redis;
        if redis_driver
            && !m.setup.predis
            && !path.join("vendor/predis/predis/src/Client.php").is_file()
            && !m.setup.composer
        {
            blockers.push("manifest_redis_client_missing".into());
        }
        if p.as_ref()
            .is_some_and(|p| self.sessions.contains_key(&p.id))
        {
            blockers.push("setup_stop_first".into());
        }
        let binding = p
            .as_ref()
            .map(|p| self.projects.binding(&p.id))
            .transpose()?
            .flatten();
        let instance = binding
            .as_ref()
            .map(|b| self.projects.instance(&b.instance_id))
            .transpose()?;
        let database_action = match (&m.database, &instance) {
            (None, None) => "project_default",
            (None, Some(_)) => "unbind_preserve_data",
            (Some(_), Some(i)) if packages.get("database") == Some(&i.package) => "reuse",
            (Some(_), _) => "create_or_reuse_retained_database",
        }
        .into();
        let mut keys = m.environment.keys().cloned().collect::<Vec<_>>();
        if m.database.is_some() {
            keys.extend(
                [
                    "DB_CONNECTION",
                    "DB_HOST",
                    "DB_PORT",
                    "DB_DATABASE",
                    "DB_USERNAME",
                    "DB_PASSWORD",
                    "DB_URL",
                    "DATABASE_URL",
                ]
                .map(String::from),
            );
        }
        if m.services.cache != devforge_projects::cache::CacheDriver::Project {
            keys.extend(["CACHE_STORE".into(), "CACHE_DRIVER".into()]);
        }
        if m.services.queue != devforge_projects::cache::QueueDriver::Project {
            keys.push("QUEUE_CONNECTION".into());
        }
        if redis_driver {
            keys.extend(
                [
                    "REDIS_CLIENT",
                    "REDIS_HOST",
                    "REDIS_PORT",
                    "REDIS_USERNAME",
                    "REDIS_PASSWORD",
                    "REDIS_DB",
                    "REDIS_CACHE_DB",
                    "REDIS_PREFIX",
                    "CACHE_PREFIX",
                    "REDIS_URL",
                ]
                .map(String::from),
            );
        }
        if m.setup.generate_key {
            keys.push("APP_KEY".into());
        }
        if m.mail.unwrap_or_else(|| p.as_ref().is_some_and(|p| p.mail)) {
            keys.extend(devforge_projects::mail::KEYS.iter().map(|s| s.to_string()));
        }
        keys.sort();
        keys.dedup();
        let mut env = EnvPlan::preview(path, &keys)?;
        if m.setup.generate_key {
            let keep = EnvPlan::application_status(path)?.1;
            for (name, action) in &mut env {
                if name == "APP_KEY" {
                    *action = if keep {
                        "preserve_existing"
                    } else {
                        "generate_if_empty_redacted"
                    }
                    .into();
                }
            }
        }
        if write_env {
            for s in &secrets {
                if s.available {
                    let p = p.as_ref();
                    let key = if s.existing_project {
                        p.unwrap()
                            .environment_references
                            .get(&s.environment)
                            .cloned()
                            .unwrap_or_else(|| environment::key(p.unwrap(), &s.environment))
                    } else {
                        secret_key(path, &s.reference)
                    };
                    let v = Zeroizing::new(
                        self.secrets
                            .get(&key)
                            .map_err(|_| code("manifest_secret_unavailable"))?
                            .ok_or_else(|| code("manifest_secret_missing"))?,
                    );
                    if v.contains(['\r', '\n']) {
                        blockers.push("manifest_env_multiline".into());
                    }
                }
            }
        }
        let downloads = packages
            .values()
            .filter(|id| {
                let package = self.catalog.get(id).unwrap();
                if let Some(name) = package.executables.keys().next() {
                    self.packages.executable(package, name).is_err()
                } else {
                    self.packages
                        .resource(package, package.resources.keys().next().unwrap())
                        .is_err()
                }
            })
            .cloned()
            .collect();
        let state = serde_json::json!({"document":hash(&bytes),"project":p,"binding":binding,"site":self.sites.sites()?,"env":env_hash(path)?,"composer":optional(&path.join("composer.json"),2*1024*1024)?.map(|b|hash(&b)),"composer_lock":optional(&path.join("composer.lock"),8*1024*1024)?.map(|b|hash(&b)),"package":optional(&path.join("package.json"),2*1024*1024)?.map(|b|hash(&b)),"package_lock":optional(&path.join("package-lock.json"),8*1024*1024)?.map(|b|hash(&b)),"env_example":optional(&path.join(".env.example"),1024*1024)?.map(|b|hash(&b)),"journal":self.manifest_status(path)?});
        let mut plan = Plan {
            schema: 1,
            root: path.into(),
            target,
            project_id: p.map(|p| p.id),
            packages,
            downloads,
            secrets,
            env,
            write_env,
            database_action,
            environment_file: serde_json::json!({"exists":path.join(".env").exists(),"create_from_example":write_env&&m.setup.create_env&&!path.join(".env").exists(),"generate_key_if_empty":write_env&&m.setup.generate_key,"values_redacted":true}),
            settings: serde_json::json!({"project":m.project,"php_settings":m.php_settings,"services":m.services,"setup":m.setup,"scripts_enabled":false,"plugins_enabled":false,"mail":m.mail}),
            blockers,
            state_hash: hash(&serde_json::to_vec(&state).unwrap()),
            digest: String::new(),
        };
        plan.digest = hash(&serde_json::to_vec(&plan).unwrap());
        Ok(plan)
    }
    pub fn manifest_setup(
        &mut self,
        path: &Path,
        review: &str,
        write_env: bool,
        trusted: bool,
        cancel: &Cancellation,
    ) -> Result<Journal> {
        let path = root(path)?;
        let _lock = self.repro_lock(&path)?;
        if !trusted {
            return Err(code("setup_trust_required"));
        }
        let plan = self.plan(&path, write_env)?;
        if plan.digest != review {
            return Err(code("manifest_review_changed"));
        }
        if !plan.blockers.is_empty() {
            return Err(code(&plan.blockers[0]));
        }
        let mut session_guard = plan
            .project_id
            .as_deref()
            .map(|id| self.sites.project_mutation_guard(id))
            .transpose()?;
        let (m, bytes) = self.document(&path)?;
        let source = hash(&bytes);
        let mut journal = self.manifest_status(&path)?;
        journal.schema = 1;
        journal.manifest = source.clone();
        journal.completed = false;
        journal.last_error = None;
        let mut expected_env = env_hash(&path)?;
        let reviewed_inputs = inputs(&path)?;
        let result = (|| {
            journal.stage = "download".into();
            self.save_journal(&path, &journal)?;
            for id in plan.packages.values() {
                if cancel.is_cancelled() {
                    return Err(code("setup_cancelled"));
                }
                let p = self.catalog.get(id)?;
                self.packages.download(p, cancel, |_| {})?;
            }
            if hash(&manifest::read_file(&path.join("devforge.yaml"), 65536)?) != source
                || env_hash(&path)? != expected_env
                || inputs(&path)? != reviewed_inputs
            {
                return Err(code("manifest_review_changed"));
            }
            if cancel.is_cancelled() {
                return Err(code("setup_cancelled"));
            }
            journal.stage = "register".into();
            self.save_journal(&path, &journal)?;
            let id = if let Some(p) = self.project_at(&path)? {
                self.require_stopped(&p.id)?;
                p.id
            } else {
                self.projects
                    .import(
                        &path,
                        &m.project.name,
                        &plan.packages["php"],
                        &plan.packages["node"],
                        &self.catalog,
                    )?
                    .id
            };
            if session_guard.is_none() {
                session_guard = Some(self.sites.project_mutation_guard(&id)?);
            }
            journal.project_id = Some(id.clone());
            self.save_journal(&path, &journal)?;
            let p = self.projects.get(&id)?;
            self.projects.configure(
                &id,
                p.revision,
                &m.project.name,
                &plan.packages["php"],
                &plan.packages["node"],
                &self.catalog,
            )?;
            let p = self.projects.get(&id)?;
            devforge_projects::php_settings::save(
                &self.projects,
                &id,
                p.revision,
                m.php_settings.clone(),
            )?;
            self.sites.configure(
                &self.projects,
                Site {
                    project_id: id.clone(),
                    domain: m.project.domain.clone(),
                    https: m.project.https,
                    vite: m.project.vite,
                },
            )?;
            self.preferences.development.insert(
                id.clone(),
                Development {
                    queue: m.project.queue,
                },
            );
            self.save()?;
            for s in &plan.secrets {
                if !s.existing_project {
                    let p = self.projects.get(&id)?;
                    environment::link(
                        &self.projects,
                        self.secrets.as_ref(),
                        &id,
                        p.revision,
                        &s.environment,
                        &secret_key(&path, &s.reference),
                    )?;
                }
            }
            journal.stage = "environment".into();
            self.save_journal(&path, &journal)?;
            if write_env
                && let Some(env) = EnvPlan::application(
                    &self.projects,
                    &id,
                    m.setup.create_env,
                    m.setup.generate_key,
                )?
            {
                if env.original_hash() != expected_env {
                    return Err(code("manifest_review_changed"));
                }
                env.apply_if_changed(&self.projects)?;
                expected_env = env_hash(&path)?;
            }
            journal.stage = "database".into();
            self.save_journal(&path, &journal)?;
            if let Some(package) = plan.packages.get("database") {
                let active = self
                    .projects
                    .binding(&id)?
                    .map(|b| self.projects.instance(&b.instance_id))
                    .transpose()?;
                let instance = if let Some(i) = active.filter(|i| i.package == *package) {
                    i.id
                } else if let Some(i) = journal
                    .database
                    .as_ref()
                    .and_then(|i| self.projects.instance(i).ok())
                    .filter(|i| i.ready && i.package == *package)
                {
                    i.id
                } else {
                    self.projects
                        .initialize_instance(
                            package,
                            0,
                            &self.catalog,
                            &self.packages,
                            self.secrets.as_ref(),
                        )?
                        .id
                };
                journal.database = Some(instance.clone());
                self.save_journal(&path, &journal)?;
                self.switch_database(&id, &instance, true)?;
            } else {
                self.projects.activate_binding(&id, None)?;
            }
            journal.stage = "dependencies".into();
            self.save_journal(&path, &journal)?;
            if m.setup.predis && !path.join("vendor/predis/predis/src/Client.php").is_file() {
                self.install_redis_client(&id, true, cancel)?;
            }
            for (enabled, tool, args) in [
                (
                    m.setup.composer,
                    Tool::Composer,
                    vec![
                        "install",
                        "--no-interaction",
                        "--prefer-dist",
                        "--no-plugins",
                        "--no-scripts",
                    ],
                ),
                (
                    m.setup.npm && path.join("package.json").exists(),
                    Tool::Npm,
                    vec![
                        if path.join("package-lock.json").exists() {
                            "ci"
                        } else {
                            "install"
                        },
                        "--ignore-scripts",
                        "--no-audit",
                        "--no-fund",
                    ],
                ),
            ] {
                if enabled {
                    let context = Context::tooling(
                        &self.projects,
                        &id,
                        &self.catalog,
                        &self.packages,
                        self.secrets.as_ref(),
                    )?;
                    let context = if tool == Tool::Npm {
                        context.frontend()
                    } else {
                        context
                    };
                    let result = context
                        .invocation(
                            tool,
                            args.into_iter().map(Into::into).collect(),
                            &self.catalog,
                            &self.packages,
                        )?
                        .run_cancellable(self.projects.root().join("manifest-tool-logs"), cancel)?;
                    if !result.success {
                        return Err(code("manifest_dependency_failed"));
                    }
                }
            }
            if hash(&manifest::read_file(&path.join("devforge.yaml"), 65536)?) != source {
                return Err(code("manifest_review_changed"));
            }
            journal.stage = "services".into();
            self.save_journal(&path, &journal)?;
            let redis = if let Some(package) = plan.packages.get("redis") {
                let current = self
                    .projects
                    .get(&id)?
                    .services
                    .redis_instance
                    .and_then(|i| self.projects.cache_instance(&i).ok())
                    .filter(|i| {
                        i.ready
                            && i.project_id == id
                            && i.package == *package
                            && Some(&i.runtime) == plan.packages.get("dotnet")
                    });
                let instance = if let Some(i) = current {
                    i.id
                } else if let Some(i) = journal
                    .redis
                    .as_ref()
                    .and_then(|i| self.projects.cache_instance(i).ok())
                    .filter(|i| {
                        i.ready
                            && i.project_id == id
                            && i.package == *package
                            && Some(&i.runtime) == plan.packages.get("dotnet")
                    })
                {
                    i.id
                } else {
                    self.projects
                        .initialize_cache(
                            &id,
                            package,
                            &plan.packages["dotnet"],
                            &self.catalog,
                            &self.packages,
                            self.secrets.as_ref(),
                        )?
                        .id
                };
                journal.redis = Some(instance.clone());
                self.save_journal(&path, &journal)?;
                Some(instance)
            } else {
                None
            };
            let p = self.projects.get(&id)?;
            self.projects.save_services(
                &id,
                p.revision,
                ServiceSelection {
                    cache: m.services.cache,
                    queue: m.services.queue,
                    redis_instance: redis,
                },
            )?;
            if let Some(enabled) = m.mail {
                self.configure_mail(&id, enabled)?;
            } else if self.projects.get(&id)?.mail {
                self.ensure_mail(&id)?;
            }
            if cancel.is_cancelled() {
                return Err(code("setup_cancelled"));
            }
            if write_env {
                journal.stage = "env_write".into();
                self.save_journal(&path, &journal)?;
                let env = EnvPlan::profile(
                    &self.projects,
                    &id,
                    self.secrets.as_ref(),
                    &m.environment.keys().cloned().collect::<Vec<_>>(),
                )?;
                if env.original_hash() != expected_env {
                    return Err(code("manifest_review_changed"));
                }
                env.apply_if_changed(&self.projects)?;
            }
            journal.stage = "complete".into();
            journal.completed = true;
            self.save_journal(&path, &journal)?;
            self.refresh_runtimes()?;
            Ok(journal.clone())
        })();
        if let Err(mut e) = result {
            journal.last_error = Some(e.code.clone());
            let _ = self.save_journal(&path, &journal);
            e.project_id = journal.project_id.clone();
            return Err(e);
        }
        result
    }
}
