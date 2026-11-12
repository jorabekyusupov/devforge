//! Explicit project setup workflows. Never overwrite an existing destination.
use super::*;
use std::{ffi::OsString, io::Write};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Navigation {
    pub favorite: bool,
    pub last_opened_ms: u64,
}
#[derive(Clone, Debug, Serialize)]
pub struct Progress {
    pub stage: String,
    pub project_id: Option<String>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PrepareOptions {
    pub composer: bool,
    pub npm: bool,
    pub create_env: bool,
    pub generate_key: bool,
    pub run_scripts: bool,
    pub initialize_sqlite: bool,
    pub trusted: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Source {
    Laravel {
        version: String,
    },
    Git {
        repository: String,
        branch: Option<String>,
    },
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateInput {
    pub source: Source,
    pub parent: PathBuf,
    pub folder: String,
    pub name: String,
    pub php: String,
    pub node: String,
    pub domain: String,
    pub https: bool,
    pub options: PrepareOptions,
}
fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
struct ProgressGuard(Arc<Mutex<Option<Progress>>>);
impl Drop for ProgressGuard {
    fn drop(&mut self) {
        if let Ok(mut p) = self.0.lock() {
            *p = None;
        }
    }
}
struct Destination {
    path: PathBuf,
    marker: Vec<u8>,
}
fn link(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|m| {
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if m.file_attributes() & 0x400 != 0 {
                return true;
            }
        }
        m.file_type().is_symlink()
    })
}
impl Destination {
    fn reserve(parent: &Path, folder: &str, marker: Vec<u8>) -> Result<Self> {
        valid_folder(folder)?;
        let parent = dunce::canonicalize(parent).map_err(|_| error("setup_parent"))?;
        if !parent.is_dir() {
            return Err(error("setup_parent"));
        }
        let path = parent.join(folder);
        fs::create_dir(&path).map_err(|_| error("setup_destination_exists"))?;
        let mut guard = Self {
            path,
            marker: vec![],
        };
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(guard.path.join(".devforge-setup-reservation"))?;
        file.write_all(&marker)?;
        file.sync_all()?;
        guard.marker = marker;
        Ok(guard)
    }
    fn owned(&self) -> bool {
        !link(&self.path)
            && !link(&self.path.join(".devforge-setup-reservation"))
            && fs::read(self.path.join(".devforge-setup-reservation"))
                .ok()
                .as_deref()
                == Some(self.marker.as_slice())
    }
    fn publish(&self, source: &Path, cancel: &Cancellation) -> Result<()> {
        if !self.owned()
            || fs::read_dir(&self.path)?.count() != 1
            || source.join(".devforge-setup-reservation").exists()
        {
            return Err(error("setup_destination_changed"));
        }
        let mut count = 0;
        publish_tree(source, &self.path, &mut count, 0, cancel)?;
        if !self.owned() {
            return Err(error("setup_destination_changed"));
        }
        fs::remove_file(self.path.join(".devforge-setup-reservation"))?;
        Ok(())
    }
}
impl Drop for Destination {
    fn drop(&mut self) {
        if self.owned() && fs::read_dir(&self.path).is_ok_and(|d| d.count() == 1) {
            let _ = fs::remove_file(self.path.join(".devforge-setup-reservation"));
            let _ = fs::remove_dir(&self.path);
        }
    }
}
fn publish_tree(
    source: &Path,
    target: &Path,
    count: &mut usize,
    depth: usize,
    cancel: &Cancellation,
) -> Result<()> {
    if depth > 128 {
        return Err(error("setup_source_limit"));
    }
    for entry in fs::read_dir(source)? {
        if cancel.is_cancelled() {
            return Err(error("setup_cancelled"));
        }
        let entry = entry?;
        *count += 1;
        if *count > 100_000 {
            return Err(error("setup_source_limit"));
        }
        let from = entry.path();
        let to = target.join(entry.file_name());
        let metadata = fs::symlink_metadata(&from)?;
        if metadata.file_type().is_symlink() {
            #[cfg(unix)]
            std::os::unix::fs::symlink(fs::read_link(&from)?, &to)?;
            #[cfg(windows)]
            {
                return Err(error("setup_source_link"));
            }
        } else if metadata.is_dir() && !link(&from) {
            fs::create_dir(&to).map_err(|_| error("setup_destination_changed"))?;
            publish_tree(&from, &to, count, depth + 1, cancel)?;
        } else if metadata.is_file() && !link(&from) {
            // Atomic create-new works on ordinary and removable filesystems.
            // Never replace a concurrently created target or follow its links.
            let mut output = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&to)
                .map_err(|_| error("setup_destination_changed"))?;
            let mut input = fs::File::open(&from)?;
            std::io::copy(&mut input, &mut output)?;
            output.sync_all()?;
            fs::set_permissions(&to, metadata.permissions())?;
        } else {
            return Err(error("setup_source_link"));
        }
    }
    Ok(())
}
pub fn valid_folder(value: &str) -> Result<()> {
    let upper = value.split('.').next().unwrap_or("").to_ascii_uppercase();
    if value.is_empty()
        || value.len() > 80
        || !value.as_bytes()[0].is_ascii_alphanumeric()
        || !value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c))
        || value.ends_with('.')
        || [
            "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7",
            "COM8", "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
        ]
        .contains(&upper.as_str())
    {
        return Err(error("setup_folder"));
    }
    Ok(())
}
pub fn validate_repository(value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 2048
        || value.chars().any(|c| c.is_control())
        || value.starts_with('-')
    {
        return Err(error("setup_repository"));
    }
    if Path::new(value).is_absolute() {
        return if Path::new(value).is_dir() {
            Ok(())
        } else {
            Err(error("setup_repository"))
        };
    }
    if let Some(path) = value.strip_prefix("git@")
        && path.split_once(':').is_some_and(|(host, path)| {
            !host.is_empty()
                && !path.is_empty()
                && host
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b".-".contains(&c))
                && path
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"/_-.~".contains(&c))
        })
    {
        return Ok(());
    }
    let u = url::Url::parse(value).map_err(|_| error("setup_repository"))?;
    if !matches!(u.scheme(), "https" | "ssh")
        || u.host_str().is_none()
        || u.password().is_some()
        || u.query().is_some()
        || u.fragment().is_some()
        || (u.scheme() == "https" && !u.username().is_empty())
        || u.path().is_empty()
        || (u.scheme() == "ssh"
            && !u
                .username()
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c)))
    {
        return Err(error("setup_repository"));
    }
    Ok(())
}
impl Workspace {
    fn stage(&self, stage: &str, id: Option<&str>) {
        if let Ok(mut p) = self.progress.lock() {
            *p = Some(Progress {
                stage: stage.into(),
                project_id: id.map(String::from),
            });
        }
    }
    fn tooling(&self, store: &ProjectStore, path: &Path, php: &str, node: &str) -> Result<Context> {
        Ok(Context::bootstrap(
            store,
            path,
            php,
            node,
            &self.catalog,
            &self.packages,
            self.secrets.as_ref(),
        )?)
    }
    fn setup_run(
        &mut self,
        context: Context,
        tool: Tool,
        args: Vec<OsString>,
        stage: &str,
        id: Option<&str>,
        cancel: &Cancellation,
    ) -> Result<serde_json::Value> {
        self.stage(stage, id);
        if cancel.is_cancelled() {
            return Err(error("setup_cancelled"));
        }
        let result = context
            .invocation(tool, args, &self.catalog, &self.packages)?
            .run_cancellable(self.projects.root().join("setup-logs"), cancel);
        if cancel.is_cancelled() {
            return Err(error("setup_cancelled"));
        }
        let result = result?;
        for line in &result.logs {
            self.history.push(Log {
                project_id: id.map(String::from),
                process: stage.into(),
                stream: line.stream.into(),
                text: line.text.clone(),
            });
        }
        if self.history.len() > 800 {
            self.history.drain(..self.history.len() - 800);
        }
        if !result.success {
            return Err(error(&format!("setup_{stage}_failed")));
        }
        Ok(serde_json::json!({"completed":true}))
    }
    pub fn create_project(
        &mut self,
        input: CreateInput,
        cancel: &Cancellation,
    ) -> Result<serde_json::Value> {
        let _progress = ProgressGuard(self.progress.clone());
        if cancel.is_cancelled() {
            return Err(error("setup_cancelled"));
        }
        if !input.options.trusted {
            return Err(error("setup_trust_required"));
        }
        if devforge_projects::project::validate_name(&input.name).is_err()
            || input.name.chars().count() > 120
        {
            return Err(error("setup_name"));
        }
        self.validate_site("", &input.domain)?;
        match &input.source {
            Source::Laravel { version } => {
                if input.options.initialize_sqlite && !input.options.composer {
                    return Err(error("setup_composer_required"));
                }
                if version.is_empty()
                    || version.len() > 80
                    || !version
                        .bytes()
                        .all(|c| c.is_ascii_digit() || b".^~*<>=| -".contains(&c))
                    || version.starts_with('-')
                {
                    return Err(error("setup_version"));
                }
            }
            Source::Git { repository, branch } => {
                validate_repository(repository)?;
                if devforge_projects::execution::find_git().is_err() {
                    return Err(error("setup_git_unavailable"));
                }
                if branch.as_ref().is_some_and(|b| {
                    b.is_empty()
                        || b.len() > 200
                        || b.starts_with('-')
                        || b.contains(['\0', '\n', '\r', ' ', ':', '~', '^', '?', '*', '[', '\\'])
                        || b.contains("..")
                        || b.contains("@{")
                        || b.ends_with('.')
                }) {
                    return Err(error("setup_branch"));
                }
                if input.options.initialize_sqlite {
                    return Err(error("setup_sqlite_scope"));
                }
            }
        }
        let stage = tempfile::Builder::new()
            .prefix(".devforge-stage-")
            .tempdir_in(&input.parent)
            .map_err(|_| error("setup_parent"))?;
        let destination = Destination::reserve(
            &input.parent,
            &input.folder,
            stage
                .path()
                .file_name()
                .unwrap()
                .to_string_lossy()
                .as_bytes()
                .to_vec(),
        )?;
        // Configuration belongs to app data, not the user's source parent.
        // In particular Windows PHP cannot reliably open a non-ASCII -c file.
        let bootstrap_root = self.root.join("setup-context");
        fs::create_dir_all(&bootstrap_root)?;
        let bootstrap = tempfile::Builder::new()
            .prefix("bootstrap-")
            .tempdir_in(bootstrap_root)?;
        let tooling = ProjectStore::open(bootstrap.path())?;
        let context = self.tooling(&tooling, stage.path(), &input.php, &input.node)?;
        let payload = stage.path().join("payload");
        match &input.source {
            Source::Laravel { version } => {
                self.setup_run(
                    context,
                    Tool::Composer,
                    vec![
                        "create-project".into(),
                        "laravel/laravel".into(),
                        dunce::simplified(&payload).as_os_str().to_owned(),
                        version.into(),
                        "--no-install".into(),
                        "--no-scripts".into(),
                        "--no-plugins".into(),
                        "--no-interaction".into(),
                        "--prefer-dist".into(),
                    ],
                    "source",
                    None,
                    cancel,
                )?;
            }
            Source::Git { repository, branch } => {
                let hooks = stage.path().join("empty-hooks");
                fs::create_dir(&hooks)?;
                let mut args = vec![
                    "-c".into(),
                    format!("core.hooksPath={}", dunce::simplified(&hooks).display()).into(),
                    "-c".into(),
                    "protocol.ext.allow=never".into(),
                    "clone".into(),
                    "--no-local".into(),
                    "--no-recurse-submodules".into(),
                ];
                if let Some(branch) = branch {
                    args.extend(["--branch".into(), branch.into()]);
                }
                args.extend([
                    "--".into(),
                    if Path::new(repository).is_absolute() {
                        dunce::simplified(Path::new(repository))
                            .as_os_str()
                            .to_owned()
                    } else {
                        repository.into()
                    },
                    dunce::simplified(&payload).as_os_str().to_owned(),
                ]);
                self.setup_run(context.frontend(), Tool::Git, args, "clone", None, cancel)?;
            }
        }
        if cancel.is_cancelled() {
            return Err(error("setup_cancelled"));
        }
        compatible(
            &inspect(&payload)?,
            self.catalog.get(&input.php)?,
            self.catalog.get(&input.node)?,
        )?;
        self.stage("register", None);
        destination.publish(&payload, cancel)?;
        let result = self.execute(
            Command::Import {
                path: destination.path.clone(),
                name: input.name,
                php: input.php,
                node: input.node,
                domain: input.domain,
                https: input.https,
                vite: false,
                queue: false,
            },
            cancel,
        )?;
        let id = result["project_id"]
            .as_str()
            .ok_or_else(|| error("setup_register_failed"))?
            .to_owned();
        if matches!(input.source, Source::Laravel { .. }) {
            let dir = self.projects.root().join("setup-owned");
            fs::create_dir_all(&dir)?;
            fs::write(
                dir.join(format!("{id}.json")),
                b"{\"sqlite_started\":false,\"sqlite_complete\":false}",
            )?;
        }
        self.prepare_project(&id, &input.options, cancel)
            .map_err(|mut e| {
                e.project_id = Some(id.clone());
                e
            })?;
        Ok(serde_json::json!({"project_id":id,"created":true,"prepared":true}))
    }
    pub fn prepare_project(
        &mut self,
        id: &str,
        options: &PrepareOptions,
        cancel: &Cancellation,
    ) -> Result<serde_json::Value> {
        let _progress = ProgressGuard(self.progress.clone());
        let result = (|| {
            if cancel.is_cancelled() {
                return Err(error("setup_cancelled"));
            }
            if !options.trusted {
                return Err(error("setup_trust_required"));
            }
            if self.sessions.contains_key(id) {
                return Err(error("setup_stop_first"));
            }
            let project = self.projects.get(id)?;
            let mut changed = vec![];
            self.stage("environment", Some(id));
            if let Some(plan) = devforge_projects::env_file::EnvPlan::application(
                &self.projects,
                id,
                options.create_env,
                options.generate_key,
            )? {
                changed = plan.apply(&self.projects)?.changed_keys;
            }
            let context = || {
                Context::resolve(
                    &self.projects,
                    id,
                    &self.catalog,
                    &self.packages,
                    self.secrets.as_ref(),
                )
            };
            if options.composer {
                let context = context()?;
                let mut args = vec![
                    "install".into(),
                    "--no-interaction".into(),
                    "--prefer-dist".into(),
                    "--no-plugins".into(),
                ];
                if !options.run_scripts {
                    args.push("--no-scripts".into());
                }
                self.setup_run(context, Tool::Composer, args, "composer", Some(id), cancel)?;
            }
            if options.npm && project.path.join("package.json").is_file() {
                let context = Context::resolve(
                    &self.projects,
                    id,
                    &self.catalog,
                    &self.packages,
                    self.secrets.as_ref(),
                )?
                .frontend();
                let mut args = vec![
                    if project.path.join("package-lock.json").is_file() {
                        "ci"
                    } else {
                        "install"
                    }
                    .into(),
                    "--no-audit".into(),
                    "--no-fund".into(),
                ];
                if !options.run_scripts {
                    args.push("--ignore-scripts".into());
                }
                self.setup_run(context, Tool::Npm, args, "npm", Some(id), cancel)?;
            }
            if options.initialize_sqlite {
                self.initialize_sqlite(id, cancel)?;
            }
            self.diagnostics.remove(id);
            Ok(serde_json::json!({"project_id":id,"prepared":true,"changed_keys":changed}))
        })();
        result.map_err(|mut e: Error| {
            e.project_id = Some(id.into());
            e
        })
    }
    fn initialize_sqlite(&mut self, id: &str, cancel: &Cancellation) -> Result<()> {
        let receipt = self
            .projects
            .root()
            .join("setup-owned")
            .join(format!("{id}.json"));
        let mut state: serde_json::Value =
            serde_json::from_slice(&fs::read(&receipt).map_err(|_| error("setup_sqlite_scope"))?)
                .map_err(|_| error("setup_sqlite_scope"))?;
        if state["sqlite_complete"] == true {
            return Ok(());
        }
        let p = self.projects.get(id)?;
        let dir = dunce::canonicalize(p.path.join("database"))?;
        if !dir.starts_with(&p.path) || link(&dir.join("database.sqlite")) {
            return Err(error("setup_sqlite_scope"));
        }
        let path = dir.join("database.sqlite");
        if state["sqlite_started"] != true && path.exists() && fs::metadata(&path)?.len() != 0 {
            return Err(error("setup_sqlite_scope"));
        }
        fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)?;
        state["sqlite_started"] = true.into();
        fs::write(&receipt, serde_json::to_vec(&state).unwrap())?;
        let context = Context::resolve(
            &self.projects,
            id,
            &self.catalog,
            &self.packages,
            self.secrets.as_ref(),
        )?
        .setup_sqlite(&path)?;
        self.setup_run(
            context,
            Tool::Artisan,
            vec![
                "migrate".into(),
                "--force".into(),
                "--no-interaction".into(),
            ],
            "sqlite",
            Some(id),
            cancel,
        )?;
        state["sqlite_complete"] = true.into();
        fs::write(receipt, serde_json::to_vec(&state).unwrap())?;
        Ok(())
    }
    pub fn navigation(&mut self, id: &str, favorite: Option<bool>, visit: bool) -> Result<()> {
        self.projects.get(id)?;
        let old = self.preferences.navigation.clone();
        let n = self.preferences.navigation.entry(id.into()).or_default();
        if let Some(f) = favorite {
            n.favorite = f;
        }
        if visit {
            n.last_opened_ms = now();
        }
        if let Err(e) = self.save() {
            self.preferences.navigation = old;
            return Err(e);
        }
        Ok(())
    }
    pub fn php_diagnostics(
        &mut self,
        id: &str,
        cancel: &Cancellation,
    ) -> Result<serde_json::Value> {
        let project = self.projects.get(id)?;
        let context = Context::resolve(
            &self.projects,
            id,
            &self.catalog,
            &self.packages,
            self.secrets.as_ref(),
        )?;
        let script = r#"$e=get_loaded_extensions();sort($e);echo json_encode(['version'=>PHP_VERSION,'sapi'=>PHP_SAPI,'ini'=>php_ini_loaded_file(),'extensions'=>$e,'pdo_drivers'=>PDO::getAvailableDrivers(),'settings'=>array_combine(['memory_limit','upload_max_filesize','post_max_size','max_execution_time','date.timezone'],array_map('ini_get',['memory_limit','upload_max_filesize','post_max_size','max_execution_time','date.timezone']))]);"#;
        let result = context
            .invocation(
                Tool::Php,
                vec!["-r".into(), script.into()],
                &self.catalog,
                &self.packages,
            )?
            .run_cancellable(self.projects.root().join("php-diagnostics"), cancel)?;
        if !result.success {
            return Err(error("setup_php_failed"));
        }
        let text = result
            .logs
            .iter()
            .filter(|l| l.stream == "stdout")
            .map(|l| l.text.as_str())
            .collect::<String>();
        let mut value: serde_json::Value =
            serde_json::from_str(text.trim()).map_err(|_| error("setup_php_failed"))?;
        let (env, key) = devforge_projects::env_file::EnvPlan::application_status(&project.path)?;
        value["environment_exists"] = env.into();
        value["app_key_present"] = key.into();
        value["vendor_present"] = project.path.join("vendor/autoload.php").is_file().into();
        value["platform_check_available"] = false.into();
        value["platform_requirements"] = serde_json::json!([]);
        if project.path.join("composer.lock").is_file() {
            let context = Context::resolve(
                &self.projects,
                id,
                &self.catalog,
                &self.packages,
                self.secrets.as_ref(),
            )?;
            if let Ok(invocation) = context.invocation(
                Tool::Composer,
                vec![
                    "--no-plugins".into(),
                    "--no-scripts".into(),
                    "check-platform-reqs".into(),
                    "--lock".into(),
                    "--format=json".into(),
                ],
                &self.catalog,
                &self.packages,
            ) && let Ok(result) =
                invocation.run_cancellable(self.projects.root().join("php-diagnostics"), cancel)
            {
                let text = result
                    .logs
                    .iter()
                    .filter(|l| l.stream == "stdout")
                    .map(|l| l.text.as_str())
                    .collect::<String>();
                if let Ok(requirements) = serde_json::from_str::<serde_json::Value>(text.trim())
                    && requirements.is_array()
                {
                    value["platform_check_available"] = true.into();
                    value["platform_requirements"] = requirements;
                }
            }
        }
        if cancel.is_cancelled() {
            return Err(error("setup_cancelled"));
        }
        self.diagnostics.insert(id.into(), value.clone());
        Ok(value)
    }
    pub fn save_php_settings(
        &mut self,
        id: &str,
        revision: u64,
        settings: devforge_projects::php_settings::PhpSettings,
        cancel: &Cancellation,
    ) -> Result<serde_json::Value> {
        settings.validate()?;
        if let Some(tz) = &settings.timezone {
            let context = Context::resolve(
                &self.projects,
                id,
                &self.catalog,
                &self.packages,
                self.secrets.as_ref(),
            )?;
            let result=context.invocation(Tool::Php,vec!["-r".into(),"exit(in_array($argv[1],timezone_identifiers_list(DateTimeZone::ALL_WITH_BC),true)?0:1);".into(),tz.into()],&self.catalog,&self.packages)?.run_cancellable(self.projects.root().join("php-diagnostics"),cancel)?;
            if !result.success {
                return Err(error("setup_timezone"));
            }
        }
        if self.projects.get(id)?.revision != revision {
            return Err(devforge_projects::ProjectError::StaleRevision.into());
        }
        let running = self.sessions.contains_key(id);
        self.stop(id)?;
        devforge_projects::php_settings::save(&self.projects, id, revision, settings)?;
        self.diagnostics.remove(id);
        if running {
            self.start(id)?;
        }
        Ok(serde_json::json!({"saved":true}))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validates_portable_destinations_and_safe_git_sources() {
        for bad in ["../mine", "CON", "aux.txt", "foo.", "a/b", "-flag", ""] {
            assert!(valid_folder(bad).is_err(), "{bad}");
        }
        assert!(valid_folder("laravel-app_2").is_ok());
        for bad in [
            "ext::evil",
            "https://token:password@example.test/repo",
            "--upload-pack=evil",
            "git@host:repo;evil",
            "https://example.test/repo?token=x",
        ] {
            assert!(validate_repository(bad).is_err(), "{bad}");
        }
        for good in [
            "https://github.com/example/atlas.git",
            "ssh://git@example.test/team/atlas.git",
            "git@example.test:team/atlas.git",
        ] {
            assert!(validate_repository(good).is_ok(), "{good}");
        }
    }
    #[test]
    fn reservation_preserves_existing_and_concurrent_user_data() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("existing")).unwrap();
        fs::write(root.path().join("existing/user"), "keep").unwrap();
        assert!(Destination::reserve(root.path(), "existing", b"x".to_vec()).is_err());
        let target = Destination::reserve(root.path(), "new", b"x".to_vec()).unwrap();
        fs::write(target.path.join("user"), "keep").unwrap();
        drop(target);
        assert_eq!(fs::read(root.path().join("new/user")).unwrap(), b"keep");
        let payload = root.path().join("payload");
        fs::create_dir(&payload).unwrap();
        fs::write(payload.join("file"), "contents").unwrap();
        let target = Destination::reserve(root.path(), "published", b"x".to_vec()).unwrap();
        target.publish(&payload, &Cancellation::default()).unwrap();
        drop(target);
        assert_eq!(
            fs::read(root.path().join("published/file")).unwrap(),
            b"contents"
        );
    }
}
