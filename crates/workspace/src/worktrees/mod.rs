//! Per-branch projects. Receipts outlive failures; cleanup archives files and retains databases.
use super::*;
use devforge_projects::{env_file::EnvPlan, manifest::Manifest};
use sha2::{Digest, Sha256};
use std::{ffi::OsString, io::Write};
mod git;
mod lifecycle;
mod remote;
fn digest(value: &impl Serialize) -> String {
    Sha256::digest(serde_json::to_vec(value).unwrap())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
fn valid_id(value: &str) -> bool {
    value.len() == 32 && value.bytes().all(|b| b.is_ascii_hexdigit())
}
fn atomic(path: &Path, value: &impl Serialize) -> Result<()> {
    let mut file = tempfile::NamedTempFile::new_in(path.parent().unwrap())?;
    file.write_all(&serde_json::to_vec_pretty(value).unwrap())?;
    file.as_file().sync_all()?;
    file.persist(path)
        .map_err(|_| error("storage_unavailable"))?;
    Ok(())
}
fn ordinary(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|m| m.is_dir() && !m.file_type().is_symlink())
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DatabaseMode {
    Empty,
    Clone,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Input {
    pub source_id: String,
    pub name: String,
    pub branch: String,
    pub base: String,
    pub new_branch: bool,
    pub database: DatabaseMode,
    pub dependencies: bool,
    pub copy_environment: bool,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Record {
    #[serde(default)]
    pub isolated: bool,
    pub schema: u32,
    pub id: String,
    pub input: Input,
    pub source_path: PathBuf,
    pub common_dir: PathBuf,
    pub git_dir: Option<PathBuf>,
    pub commit: String,
    pub domain: String,
    pub review_digest: String,
    pub project_id: Option<String>,
    pub stage: String,
    pub last_error: Option<String>,
    pub configuration: Manifest,
}
#[derive(Clone, Serialize)]
pub struct View {
    pub files_archived: bool,
    pub record: Record,
    pub path: PathBuf,
    pub retained_path: PathBuf,
}
#[derive(Serialize)]
pub struct Plan {
    pub isolated: bool,
    pub submodule_sources: String,
    pub input: Input,
    pub commit: String,
    pub domain: String,
    pub source_dirty: bool,
    pub configuration: Manifest,
    pub packages: BTreeMap<String, String>,
    pub source_database: Option<Binding>,
    pub sqlite: bool,
    pub environment_names: Vec<String>,
    pub blockers: Vec<String>,
    pub digest: String,
}
#[derive(Serialize)]
pub struct Cleanup {
    pub workspace: View,
    pub dirty: bool,
    pub branch: String,
    pub head: String,
    pub project_revision: Option<u64>,
    pub databases: Vec<Binding>,
    pub blockers: Vec<String>,
    pub digest: String,
}
impl Workspace {
    pub fn worktree_open(&self, id: &str) -> Result<serde_json::Value> {
        let record = self.worktree_record(id)?;
        let dir = self.worktree_dir(id)?;
        let path = if record.stage == "removed"
            || (!dir.join("tree").exists() && dir.join("retained").exists())
        {
            dir.join("retained")
        } else {
            dir.join("tree")
        };
        if !ordinary(&path) || fs::canonicalize(&path)? != path {
            return Err(error("worktree_ownership"));
        }
        open_url(
            url::Url::from_directory_path(dunce::simplified(&path))
                .map_err(|_| error("open_failed"))?
                .as_str(),
        )?;
        Ok(serde_json::json!({"opened":true}))
    }
    fn worktree_root(&self) -> Result<PathBuf> {
        let path = self.root.join("worktrees");
        if !path.exists() {
            fs::create_dir(&path)?;
        }
        if !ordinary(&path) || fs::canonicalize(&path)? != path {
            return Err(error("worktree_ownership"));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o700))?;
        }
        Ok(path)
    }
    fn worktree_lock(&self) -> Result<fs::File> {
        let path = self.worktree_root()?.join("operation.lock");
        if fs::symlink_metadata(&path).is_ok_and(|m| !m.is_file() || m.file_type().is_symlink()) {
            return Err(error("worktree_ownership"));
        }
        let f = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(path)?;
        f.try_lock().map_err(|_| error("worktree_busy"))?;
        Ok(f)
    }
    fn worktree_dir(&self, id: &str) -> Result<PathBuf> {
        if !valid_id(id) {
            return Err(error("worktree_ownership"));
        }
        let path = self.worktree_root()?.join(id);
        if !ordinary(&path) || fs::canonicalize(&path)? != path {
            return Err(error("worktree_ownership"));
        }
        Ok(path)
    }
    fn worktree_record(&self, id: &str) -> Result<Record> {
        let path = self.worktree_dir(id)?.join("receipt.json");
        let record: Record =
            serde_json::from_slice(&devforge_projects::manifest::read_file(&path, 262144)?)
                .map_err(|_| error("worktree_ownership"))?;
        if record.schema != 1 || record.id != id || !git::branch(&record.input.branch) {
            return Err(error("worktree_ownership"));
        }
        Ok(record)
    }
    fn save_worktree(&self, record: &Record) -> Result<()> {
        atomic(&self.worktree_dir(&record.id)?.join("receipt.json"), record)
    }
    pub fn worktrees(&self) -> Result<Vec<View>> {
        let mut out = Vec::new();
        for item in fs::read_dir(self.worktree_root()?)? {
            let item = item?;
            let id = item.file_name().to_string_lossy().to_string();
            if !valid_id(&id) {
                continue;
            }
            let record = self.worktree_record(&id)?;
            let dir = self.worktree_dir(&id)?;
            out.push(View {
                files_archived: ordinary(&dir.join("retained")),
                record,
                path: dir.join("tree"),
                retained_path: dir.join("retained"),
            });
            if out.len() > 1000 {
                return Err(error("worktree_ownership"));
            }
        }
        out.sort_by(|a, b| a.record.input.name.cmp(&b.record.input.name));
        Ok(out)
    }
    pub(crate) fn managed_worktree(&self, project: &str) -> Result<bool> {
        Ok(self.worktrees()?.iter().any(|v| {
            v.record.stage != "removed" && v.record.project_id.as_deref() == Some(project)
        }))
    }
    pub fn worktree_plan(&self, input: &Input) -> Result<Plan> {
        let source = self.projects.get(&input.source_id)?;
        devforge_projects::project::validate_name(&input.name)?;
        if input.name.chars().count() > 120
            || !git::branch(&input.branch)
            || !(input.base == "HEAD" || git::branch(&input.base))
        {
            return Err(error("worktree_branch_invalid"));
        }
        git::text(
            &self.root,
            &source.path,
            &["check-ref-format", &format!("refs/heads/{}", input.branch)],
        )?;
        let top = git::text(&self.root, &source.path, &["rev-parse", "--show-toplevel"])?;
        if fs::canonicalize(top)? != source.path {
            return Err(error("worktree_repository_root"));
        }
        let common = git::common(&self.root, &source.path)?;
        let configuration = self.manifest_definition(&source.id)?;
        let packages = configuration.verify_pins(&self.catalog, Target::current()?)?;
        let exists = git::text(
            &self.root,
            &source.path,
            &[
                "show-ref",
                "--verify",
                "--hash",
                &format!("refs/heads/{}", input.branch),
            ],
        )
        .ok();
        let mut blockers = vec![];
        if source.services.uses_redis() && !input.dependencies {
            blockers.push("worktree_dependencies_required".into());
        }
        if input.new_branch && exists.is_some() {
            blockers.push("worktree_branch_exists".into());
        }
        if !input.new_branch && exists.is_none() {
            blockers.push("worktree_branch_missing".into());
        }
        if !input.new_branch {
            let listed = git::read(
                &self.root,
                &source.path,
                &["worktree", "list", "--porcelain", "-z"],
            )?;
            if listed
                .split(|b| *b == 0)
                .any(|line| line == format!("branch refs/heads/{}", input.branch).as_bytes())
            {
                blockers.push("worktree_branch_in_use".into());
            }
        }
        let reference = if input.new_branch {
            git::text(
                &self.root,
                &source.path,
                &[
                    "show-ref",
                    "--verify",
                    "--hash",
                    &format!("refs/heads/{}", input.base),
                ],
            )
            .unwrap_or_else(|_| input.base.clone())
        } else {
            exists.unwrap_or_else(|| "HEAD".into())
        };
        let commit = git::oid(&self.root, &source.path, &reference)?;
        let isolated = git::submodules(&self.root, &source.path, Some(&commit))?;
        if isolated {
            blockers.retain(|code| code != "worktree_branch_in_use");
        }
        let submodule_sources = if isolated {
            git::text(
                &self.root,
                &source.path,
                &["show", &format!("{commit}:.gitmodules")],
            )?
        } else {
            String::new()
        };
        let slug = input
            .branch
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() {
                    c.to_ascii_lowercase()
                } else {
                    '-'
                }
            })
            .collect::<String>();
        let label = slug.trim_matches('-').chars().take(28).collect::<String>();
        let domain = format!(
            "{}-{}.devforge.test",
            if label.is_empty() {
                "workspace"
            } else {
                &label
            },
            &digest(&(source.id.clone(), input.branch.clone()))[..8]
        );
        self.validate_site("", &domain)?;
        let source_database = self.projects.binding(&source.id)?;
        let sqlite = source_database.is_none();
        if input.database == DatabaseMode::Clone {
            if self.sessions.contains_key(&source.id) {
                blockers.push("worktree_stop_source".into());
            }
            if sqlite {
                let file = devforge_projects::workspace_sqlite::location(&source.path)
                    .map_err(|_| error("worktree_sqlite_scope"))?;
                if !file.is_file() {
                    blockers.push("worktree_sqlite_scope".into());
                }
                // Only the conventional, DevForge-initialized local SQLite profile is inferred.
                let receipt = self
                    .projects
                    .root()
                    .join("setup-owned")
                    .join(format!("{}.json", source.id));
                let owned = devforge_projects::manifest::read_file(&receipt, 65536)
                    .ok()
                    .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
                    .is_some_and(|v| v["sqlite_started"] == true || v["sqlite_complete"] == true);
                if !owned
                    || !devforge_projects::workspace_sqlite::conventional_profile(&source.path)
                        .unwrap_or(false)
                {
                    blockers.push("worktree_sqlite_scope".into());
                }
            }
        }
        let environment_names = source
            .environment_names
            .iter()
            .filter(|n| *n != "APP_KEY" && !n.starts_with("MAIL_") && !n.starts_with("REDIS_"))
            .cloned()
            .collect();
        let mut plan = Plan {
            isolated,
            submodule_sources,
            input: input.clone(),
            commit,
            domain,
            source_dirty: !git::status(&self.root, &source.path)?.is_empty(),
            configuration,
            packages,
            source_database,
            sqlite,
            environment_names,
            blockers,
            digest: String::new(),
        };
        plan.digest = digest(&(&plan, &source, &common));
        Ok(plan)
    }
    fn worktree_git(
        &self,
        common: &Path,
        args: Vec<OsString>,
        cancel: &Cancellation,
    ) -> Result<()> {
        let mut all = vec![format!("--git-dir={}", dunce::simplified(common).display()).into()];
        all.extend(args);
        git::run(&self.root, &self.root, all, true, cancel)?;
        Ok(())
    }
    fn worktree_identity(&self, record: &Record, path: &Path) -> Result<()> {
        if record.isolated {
            if !ordinary(path)
                || fs::canonicalize(path)? != path
                || !ordinary(&path.join(".git"))
                || git::common(&self.root, path)? != path.join(".git")
            {
                return Err(error("worktree_ownership"));
            }
            return Ok(());
        }
        if !ordinary(path)
            || fs::canonicalize(path)? != path
            || !fs::symlink_metadata(path.join(".git"))
                .is_ok_and(|m| m.is_file() && !m.file_type().is_symlink())
        {
            return Err(error("worktree_ownership"));
        }
        if git::common(&self.root, path)? != record.common_dir {
            return Err(error("worktree_ownership"));
        }
        let admin = fs::canonicalize(git::text(
            &self.root,
            path,
            &["rev-parse", "--absolute-git-dir"],
        )?)?;
        if record.git_dir.as_ref().is_some_and(|p| p != &admin)
            || admin.parent() != Some(record.common_dir.join("worktrees").as_path())
        {
            return Err(error("worktree_ownership"));
        }
        let target = git::text(&self.root, path, &["rev-parse", "--show-toplevel"])?;
        if fs::canonicalize(target)? != path {
            return Err(error("worktree_ownership"));
        }
        Ok(())
    }
    pub fn worktree_create(
        &mut self,
        input: Input,
        review: &str,
        trusted: bool,
        cancel: &Cancellation,
    ) -> Result<Record> {
        let _lock = self.worktree_lock()?;
        if !trusted {
            return Err(error("setup_trust_required"));
        }
        let plan = self.worktree_plan(&input)?;
        if plan.digest != review {
            return Err(error("worktree_review_changed"));
        }
        if let Some(code) = plan.blockers.first() {
            return Err(error(code));
        }
        if cancel.is_cancelled() {
            return Err(error("setup_cancelled"));
        }
        let source = self.projects.get(&input.source_id)?;
        let _source_guard = if input.database == DatabaseMode::Clone {
            Some(self.sites.project_mutation_guard(&source.id)?)
        } else {
            None
        };
        let common_dir = git::common(&self.root, &source.path)?;
        let id = devforge_serving::id();
        let dir = self.worktree_root()?.join(&id);
        fs::create_dir(&dir)?;
        let mut configuration = plan.configuration;
        configuration.project.name = input.name.clone();
        configuration.project.domain = plan.domain.clone();
        configuration.environment.retain(|name, _| {
            input.copy_environment
                && name != "APP_KEY"
                && !name.starts_with("MAIL_")
                && !name.starts_with("REDIS_")
        });
        let mut record = Record {
            isolated: plan.isolated,
            schema: 1,
            id,
            input,
            source_path: source.path.clone(),
            common_dir,
            git_dir: None,
            commit: plan.commit,
            domain: plan.domain,
            review_digest: plan.digest,
            project_id: None,
            stage: "reserved".into(),
            last_error: None,
            configuration,
        };
        self.save_worktree(&record)?;
        let result = self.build_worktree(
            &mut record,
            &source,
            &plan.packages,
            &plan.source_database,
            plan.sqlite,
            cancel,
        );
        if let Err(mut e) = result {
            record.stage = "failed".into();
            record.last_error = Some(e.code.clone());
            self.save_worktree(&record)?;
            e.project_id = record.project_id;
            return Err(e);
        }
        Ok(record)
    }
    fn worktree_stage(&self, record: &mut Record, stage: &str) -> Result<()> {
        record.stage = stage.into();
        self.save_worktree(record)?;
        if let Ok(mut p) = self.progress.lock() {
            *p = Some(setup::Progress {
                stage: format!("worktree_{stage}"),
                project_id: record.project_id.clone(),
            });
        }
        Ok(())
    }
    fn build_worktree(
        &mut self,
        record: &mut Record,
        source: &Project,
        packages: &BTreeMap<String, String>,
        source_database: &Option<Binding>,
        sqlite: bool,
        cancel: &Cancellation,
    ) -> Result<()> {
        let result = (|| {
            for id in packages.values() {
                self.packages
                    .download(self.catalog.get(id)?, cancel, |_| {})?;
            }
            if self.worktree_plan(&record.input)?.digest != record.review_digest {
                return Err(error("worktree_review_changed"));
            }
            let path = self.worktree_dir(&record.id)?.join("tree");
            let mut args = vec!["worktree".into(), "add".into()];
            if record.input.new_branch {
                args.extend(["-b".into(), record.input.branch.clone().into()]);
            }
            args.extend([
                "--".into(),
                dunce::simplified(&path).as_os_str().into(),
                if record.input.new_branch {
                    record.commit.clone()
                } else {
                    record.input.branch.clone()
                }
                .into(),
            ]);
            self.worktree_stage(record, "checkout")?;
            if record.isolated {
                git::run(
                    &self.root,
                    &self.root,
                    vec![
                        "clone".into(),
                        "--no-hardlinks".into(),
                        "--no-checkout".into(),
                        "--".into(),
                        dunce::simplified(&source.path).as_os_str().into(),
                        dunce::simplified(&path).as_os_str().into(),
                    ],
                    true,
                    cancel,
                )?;
                git::run(
                    &self.root,
                    &path,
                    vec![
                        "checkout".into(),
                        "-B".into(),
                        record.input.branch.clone().into(),
                        record.commit.clone().into(),
                        "--".into(),
                    ],
                    true,
                    cancel,
                )?;
                git::run(
                    &self.root,
                    &path,
                    vec![
                        "-c".into(),
                        "protocol.allow=never".into(),
                        "-c".into(),
                        "protocol.https.allow=always".into(),
                        "-c".into(),
                        "protocol.ssh.allow=always".into(),
                        "-c".into(),
                        "protocol.file.allow=always".into(),
                        "submodule".into(),
                        "update".into(),
                        "--init".into(),
                        "--recursive".into(),
                    ],
                    true,
                    cancel,
                )?;
            } else {
                self.worktree_git(&record.common_dir, args, cancel)?;
            }
            self.worktree_identity(record, &path)?;
            record.git_dir = Some(fs::canonicalize(git::text(
                &self.root,
                &path,
                &["rev-parse", "--absolute-git-dir"],
            )?)?);
            self.save_worktree(record)?;
            if git::attached_branch(&self.root, &path)? != record.input.branch {
                return Err(error("worktree_review_changed"));
            }
            if git::oid(&self.root, &path, "HEAD")? != record.commit {
                return Err(error("worktree_review_changed"));
            }
            self.worktree_stage(record, "register")?;
            let checked = inspect(&path)?;
            compatible(
                &checked,
                self.catalog.get(&source.php)?,
                self.catalog.get(&source.node)?,
            )?;
            // A committed .env can embed shared credentials; never consume it silently.
            if path.join(".env").exists() {
                return Err(error("worktree_committed_env"));
            }
            let child = self.projects.import(
                &path,
                &record.input.name,
                &source.php,
                &source.node,
                &self.catalog,
            )?;
            record.project_id = Some(child.id.clone());
            self.save_worktree(record)?;
            self.projects
                .claim_worktree(&child.id, child.revision, &record.id)?;
            let id = &child.id;
            self.sites.configure(
                &self.projects,
                Site {
                    project_id: id.clone(),
                    domain: record.domain.clone(),
                    https: record.configuration.project.https,
                    vite: record.configuration.project.vite,
                },
            )?;
            devforge_projects::php_settings::save(
                &self.projects,
                id,
                self.projects.get(id)?.revision,
                record.configuration.php_settings.clone(),
            )?;
            self.preferences.development.insert(
                id.clone(),
                Development {
                    queue: record.configuration.project.queue,
                },
            );
            self.save()?;
            if let Some(env) = EnvPlan::application(&self.projects, id, true, true)? {
                env.apply(&self.projects)?;
            }
            if record.input.copy_environment {
                for name in &source.environment_names {
                    if name == "APP_KEY" || name.starts_with("MAIL_") || name.starts_with("REDIS_")
                    {
                        continue;
                    }
                    let key = source
                        .environment_references
                        .get(name)
                        .cloned()
                        .unwrap_or_else(|| devforge_projects::environment::key(source, name));
                    let value = zeroize::Zeroizing::new(
                        self.secrets
                            .get(&key)
                            .map_err(|_| error("manifest_secret_unavailable"))?
                            .ok_or_else(|| error("manifest_secret_missing"))?,
                    );
                    let p = self.projects.get(id)?;
                    devforge_projects::environment::set(
                        &self.projects,
                        self.secrets.as_ref(),
                        id,
                        p.revision,
                        name,
                        &value,
                    )?;
                }
            }
            self.worktree_stage(record, "database")?;
            if digest(&self.projects.binding(&source.id)?) != digest(source_database) {
                return Err(error("worktree_review_changed"));
            }
            if let Some(binding) = source_database {
                self.ensure_database(&binding.instance_id)?;
                self.databases
                    .get_mut(&binding.instance_id)
                    .unwrap()
                    .provision(&self.projects, &child, self.secrets.as_ref())?;
                if record.input.database == DatabaseMode::Clone {
                    self.require_stopped(&source.id)?;
                    let backup = self.create_backup(&source.id, cancel)?;
                    if digest(&self.projects.binding(&source.id)?) != digest(source_database) {
                        return Err(error("worktree_review_changed"));
                    }
                    let temp = tempfile::tempdir_in(self.worktree_dir(&record.id)?)?;
                    let archive = temp.path().join("clone.devforge-db");
                    self.projects.export_backup(
                        &source.id,
                        backup["id"]
                            .as_str()
                            .ok_or_else(|| error("worktree_database_failed"))?,
                        &archive,
                    )?;
                    let imported = self.projects.import_backup(id, &archive)?;
                    self.restore_backup(id, &imported.id, true, cancel)?;
                }
            } else if sqlite {
                let database = devforge_projects::workspace_sqlite::location(&path)
                    .map_err(|_| error("worktree_sqlite_scope"))?;
                if record.input.database == DatabaseMode::Clone {
                    devforge_projects::workspace_sqlite::snapshot(
                        &devforge_projects::workspace_sqlite::location(&source.path)?,
                        &database,
                    )?;
                } else {
                    fs::OpenOptions::new()
                        .create_new(true)
                        .write(true)
                        .open(&database)?;
                }
                // Reserved DB_* overrides live in .env, not the generic secret namespace.
                EnvPlan::overrides(
                    &self.projects,
                    id,
                    BTreeMap::from([
                        ("DB_CONNECTION".into(), "sqlite".into()),
                        (
                            "DB_DATABASE".into(),
                            dunce::simplified(&database).display().to_string(),
                        ),
                        ("DB_URL".into(), "(null)".into()),
                        ("DATABASE_URL".into(), "(null)".into()),
                    ]),
                )?
                .apply(&self.projects)?;
                let receipts = self.projects.root().join("setup-owned");
                fs::create_dir_all(&receipts)?;
                atomic(
                    &receipts.join(format!("{id}.json")),
                    &serde_json::json!({"sqlite_started":true,"sqlite_complete":false}),
                )?;
            }
            self.worktree_stage(record, "prepare")?;
            if path.join("composer.json").is_file() {
                let contents = devforge_projects::manifest::read_file(
                    &path.join("composer.json"),
                    1024 * 1024,
                )?;
                let json: serde_json::Value =
                    serde_json::from_slice(&contents).map_err(|_| error("worktree_git_failed"))?;
                if json["require-dev"]["devforge/laravel-inspector"].is_string() {
                    devforge_inspector::materialize(&path)
                        .map_err(|_| error("inspection_package_conflict"))?;
                }
            }
            self.prepare_project(
                id,
                &setup::PrepareOptions {
                    composer: record.input.dependencies,
                    npm: record.input.dependencies,
                    create_env: true,
                    generate_key: true,
                    run_scripts: false,
                    initialize_sqlite: false,
                    trusted: true,
                },
                cancel,
            )?;
            if let Some(redis) = packages.get("redis") {
                let cache = self.projects.initialize_cache(
                    id,
                    redis,
                    &packages["dotnet"],
                    &self.catalog,
                    &self.packages,
                    self.secrets.as_ref(),
                )?;
                let p = self.projects.get(id)?;
                self.projects.save_services(
                    id,
                    p.revision,
                    devforge_projects::cache::ServiceSelection {
                        cache: record.configuration.services.cache,
                        queue: record.configuration.services.queue,
                        redis_instance: Some(cache.id),
                    },
                )?;
            } else {
                let p = self.projects.get(id)?;
                self.projects.save_services(
                    id,
                    p.revision,
                    devforge_projects::cache::ServiceSelection {
                        cache: record.configuration.services.cache,
                        queue: record.configuration.services.queue,
                        redis_instance: None,
                    },
                )?;
            }
            if record.configuration.mail == Some(true) {
                self.configure_mail(id, true)?;
            }
            EnvPlan::profile(
                &self.projects,
                id,
                self.secrets.as_ref(),
                &[], // Copied custom secrets remain in independent SecretStore entries.
            )?
            .apply_if_changed(&self.projects)?;
            if cancel.is_cancelled() {
                return Err(error("setup_cancelled"));
            }
            atomic(
                &self.worktree_dir(&record.id)?.join("configuration.json"),
                &record.configuration,
            )?;
            self.worktree_stage(record, "ready")?;
            Ok(())
        })();
        if let Ok(mut p) = self.progress.lock() {
            *p = None;
        }
        result
    }
    pub fn worktree_cleanup_plan(&self, id: &str) -> Result<Cleanup> {
        let mut record = self.worktree_record(id)?;
        let dir = self.worktree_dir(id)?;
        let path = dir.join("tree");
        let mut blockers = Vec::new();
        let mut dirty = false;
        let mut head = String::new();
        let mut branch = record.input.branch.clone();
        if record.stage == "removed" {
            blockers.push("worktree_removed".into());
        }
        let retained = dir.join("retained");
        let inspection = if path.exists() {
            Some(path.clone())
        } else if (record.isolated || record.git_dir.as_ref().is_some_and(|p| p.exists()))
            && ordinary(&retained)
        {
            Some(retained.clone())
        } else {
            None
        };
        if let Some(inspection) = inspection {
            self.worktree_identity(&record, &inspection)?;
            record.git_dir = Some(fs::canonicalize(git::text(
                &self.root,
                &inspection,
                &["rev-parse", "--absolute-git-dir"],
            )?)?);
            if !record.isolated
                && (git::submodules(&self.root, &inspection, None)?
                    || record
                        .git_dir
                        .as_ref()
                        .is_some_and(|p| p.join("modules").exists()))
            {
                blockers.push("worktree_submodules_unsupported".into());
            }
            dirty = !git::status(&self.root, &inspection)?.is_empty();
            if dirty {
                blockers.push("worktree_dirty".into());
            }
            head = git::text(&self.root, &inspection, &["rev-parse", "HEAD"])?;
            branch =
                git::attached_branch(&self.root, &inspection).unwrap_or_else(|_| "HEAD".into());
            if branch == "HEAD" {
                blockers.push("worktree_detached".into());
            }
        } else if record.git_dir.is_some() && !ordinary(&retained) {
            blockers.push("worktree_ownership".into());
        }
        if record.project_id.is_none() {
            record.project_id = self
                .projects
                .list()?
                .into_iter()
                .find(|p| p.path == path)
                .map(|p| p.id);
        }
        if self.projects.list()?.iter().any(|p| {
            Some(p.id.as_str()) != record.project_id.as_deref()
                && (p.path.starts_with(&path) || p.path.starts_with(&retained))
        }) {
            blockers.push("worktree_nested_project".into());
        }
        let project = match record.project_id.as_deref().map(|id| self.projects.get(id)) {
            Some(Ok(p)) => Some(p),
            Some(Err(devforge_projects::ProjectError::NotFound)) | None => None,
            Some(Err(e)) => return Err(e.into()),
        };
        if project.as_ref().is_some_and(|p| {
            p.path != path || p.worktree.as_deref().is_some_and(|w| w != record.id)
        }) {
            return Err(error("worktree_ownership"));
        }
        if record
            .git_dir
            .as_ref()
            .is_some_and(|p| p.join("locked").exists())
        {
            blockers.push("worktree_locked".into());
        }
        let mut databases = project
            .as_ref()
            .map(|p| self.projects.binding_history(&p.id))
            .transpose()?
            .unwrap_or_default();
        if let Some(p) = &project
            && let Some(binding) = self.projects.binding(&p.id)?
            && !databases
                .iter()
                .any(|b| b.database == binding.database && b.instance_id == binding.instance_id)
        {
            databases.push(binding);
        }
        let mut plan = Cleanup {
            workspace: View {
                files_archived: ordinary(&retained),
                record,
                path,
                retained_path: dir.join("retained"),
            },
            dirty,
            branch,
            head,
            project_revision: project.map(|p| p.revision),
            databases,
            blockers,
            digest: String::new(),
        };
        plan.digest = digest(&plan);
        Ok(plan)
    }
    pub fn worktree_cleanup(
        &mut self,
        id: &str,
        review: &str,
        consent: bool,
        cancel: &Cancellation,
    ) -> Result<Record> {
        let _lock = self.worktree_lock()?;
        if !consent {
            return Err(error("service_consent_required"));
        }
        let plan = self.worktree_cleanup_plan(id)?;
        if plan.digest != review {
            return Err(error("worktree_review_changed"));
        }
        if let Some(code) = plan.blockers.first() {
            return Err(error(code));
        }
        if cancel.is_cancelled() {
            return Err(error("setup_cancelled"));
        }
        let mut record = plan.workspace.record;
        if let Some(project) = &record.project_id {
            self.stop(project)?;
        }
        let _session_guard = record
            .project_id
            .as_deref()
            .map(|id| self.sites.project_mutation_guard(id))
            .transpose()?;
        if let Some(project) = &record.project_id {
            self.mail.remove(project);
            let caches = self
                .projects
                .cache_instances()?
                .into_iter()
                .filter(|c| &c.project_id == project)
                .map(|c| c.id)
                .collect::<Vec<_>>();
            for cache in caches {
                self.stop_cache(&cache)?;
            }
        }
        let path = plan.workspace.path;
        let retained = plan.workspace.retained_path;
        self.worktree_stage(&mut record, "archiving")?;
        let result: Result<Record> = (|| {
            if path.exists() {
                self.worktree_identity(&record, &path)?;
                if !record.isolated
                    && (git::submodules(&self.root, &path, None)?
                        || record
                            .git_dir
                            .as_ref()
                            .is_some_and(|p| p.join("modules").exists()))
                {
                    return Err(error("worktree_submodules_unsupported"));
                }
                if !git::status(&self.root, &path)?.is_empty() {
                    return Err(error("worktree_dirty"));
                }
                if retained.exists() {
                    return Err(error("worktree_ownership"));
                }
                fs::rename(&path, &retained)?;
            } else if record.git_dir.is_none() && !retained.exists() {
                fs::create_dir(&retained)?;
            }
            if !record.isolated
                && let Some(admin) = &record.git_dir
            {
                // The original path is absent. Git removes only its exact registry entry;
                // all tracked and ignored files remain in retained. No force or global prune.
                if admin.exists() {
                    self.worktree_identity(&record, &retained)?;
                    if git::attached_branch(&self.root, &retained)? != plan.branch
                        || git::oid(&self.root, &retained, "HEAD")? != plan.head
                    {
                        return Err(error("worktree_review_changed"));
                    }
                    if fs::symlink_metadata(&path).is_ok() {
                        return Err(error("worktree_ownership"));
                    }
                    self.worktree_git(
                        &record.common_dir,
                        vec![
                            "worktree".into(),
                            "remove".into(),
                            "--".into(),
                            dunce::simplified(&path).as_os_str().into(),
                        ],
                        cancel,
                    )?;
                }
            }
            if let Some(project) = &record.project_id {
                if let Ok(p) = self.projects.get(project) {
                    if p.worktree.as_deref() == Some(record.id.as_str()) {
                        self.projects
                            .retire_worktree(project, p.revision, &record.id)?;
                    } else if p.worktree.is_none() {
                        self.projects.forget(project, p.revision)?;
                    } else {
                        return Err(error("worktree_ownership"));
                    }
                }
                self.sites.remove(project)?;
                self.preferences.development.remove(project);
                self.preferences.navigation.remove(project);
                self.diagnostics.remove(project);
                self.save()?;
            }
            if !plan.head.is_empty() {
                record.commit = plan.head.clone();
                record.input.branch = plan.branch.clone();
            }
            record.stage = "removed".into();
            record.last_error = None;
            self.save_worktree(&record)?;
            Ok(record.clone())
        })();
        if let Err(e) = &result {
            record.stage = "failed".into();
            record.last_error = Some(e.code.clone());
            self.save_worktree(&record)?;
        }
        if let Ok(mut p) = self.progress.lock() {
            *p = None;
        }
        result
    }
}
