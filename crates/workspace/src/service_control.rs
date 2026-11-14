//! Narrow cooperative project start/stop/restart, sharing native orchestration.
//! Preflight inspects metadata and cached package integrity, never native keys.
use super::*;
use std::cell::RefCell;

fn missing(id: &str) -> Error {
    Error {
        scope: "workspace",
        code: "service_precondition_missing".into(),
        project_id: Some(id.into()),
    }
}

/// Advisory cache gate over facts actually present in Snapshot. It does not
/// establish full startability: exact Caddy/adopted catalog, debugger artifacts,
/// live filesystem/integrity and credentials remain worker-time checks.
pub fn cached_preflight(snapshot: &Snapshot, id: &str) -> Result<()> {
    if snapshot.projects.len() > 2048
        || snapshot.runtimes.len() > 2048
        || snapshot.databases.len() > 2048
        || snapshot.caches.len() > 2048
    {
        return Err(missing(id));
    }
    let mut rows = snapshot.projects.iter().filter(|row| row.project.id == id);
    let row = rows.next().ok_or_else(|| missing(id))?;
    if rows.next().is_some() || !row.project.active {
        return Err(missing(id));
    }
    let cached = |package: &str, kind: &str| -> Result<&Runtime> {
        let mut selected = snapshot
            .runtimes
            .iter()
            .filter(|runtime| runtime.id == package);
        let runtime = selected.next().ok_or_else(|| missing(id))?;
        if selected.next().is_some()
            || runtime.kind != kind
            || !runtime.installed
            || runtime.problem.is_some()
        {
            return Err(missing(id));
        }
        Ok(runtime)
    };
    let php = cached(&row.project.php, "php")?;
    cached(&row.project.node, "node")?;
    if !cfg!(windows)
        && !snapshot.runtimes.iter().any(|runtime| {
            runtime.kind == "php"
                && runtime.variant == "fpm"
                && runtime.version == php.version
                && runtime.installed
                && runtime.problem.is_none()
        })
    {
        return Err(missing(id));
    }
    if let Some(binding) = &row.database {
        let mut instances = snapshot
            .databases
            .iter()
            .filter(|database| database.instance.id == binding.instance_id);
        let instance = &instances.next().ok_or_else(|| missing(id))?.instance;
        if instances.next().is_some()
            || binding.project_id != id
            || !binding.ready
            || !instance.ready
        {
            return Err(missing(id));
        }
        cached(
            &instance.package,
            match instance.engine {
                devforge_projects::database::Engine::Postgresql => "postgresql",
                devforge_projects::database::Engine::Mysql => "mysql",
            },
        )?;
    }
    if row.project.services.uses_redis() {
        let selected = row
            .project
            .services
            .redis_instance
            .as_deref()
            .ok_or_else(|| missing(id))?;
        let mut caches = snapshot
            .caches
            .iter()
            .filter(|cache| cache.instance.id == selected);
        let cache = &caches.next().ok_or_else(|| missing(id))?.instance;
        if caches.next().is_some() || cache.project_id != id || !cache.ready {
            return Err(missing(id));
        }
        cached(&cache.package, "garnet")?;
        cached(&cache.runtime, "dotnet")?;
    }
    Ok(())
}

impl Workspace {
    /// Live prerequisite check for start/restart only. Stop intentionally does
    /// not require installed packages or a still-present project directory.
    /// File-backed package verification may acquire temporary executable leases;
    /// it never downloads, changes runtime pins or fetches credential values.
    pub fn service_preflight(&self, id: &str) -> Result<()> {
        let result = (|| -> Result<()> {
            let project = self.available_project(id)?;
            let inspection = devforge_projects::project::inspect(&project.path)?;
            let php = self.catalog.get(&project.php)?;
            let node = self.catalog.get(&project.node)?;
            devforge_projects::project::compatible(&inspection, php, node)?;
            self.packages.executable(php, "php")?;
            // Context::resolve requires Node even when Vite is disabled.
            self.packages.executable(node, "node")?;
            let web = self.catalog.php_server(php)?;
            self.packages
                .executable(web, if cfg!(windows) { "php-cgi" } else { "php-fpm" })?;
            let target = devforge_runtime::package::Target::current()?;
            let version = self
                .catalog
                .defaults
                .get("caddy")
                .ok_or_else(|| missing(id))?;
            let caddy = self
                .catalog
                .packages
                .values()
                .find(|package| {
                    package.kind == "caddy"
                        && package.version == *version
                        && package.target == target
                })
                .ok_or_else(|| missing(id))?;
            self.packages.executable(caddy, "caddy")?;
            let site = self
                .sites
                .sites()?
                .into_iter()
                .find(|site| site.project_id == id)
                .ok_or_else(|| missing(id))?;
            let public = fs::canonicalize(project.path.join("public"))?;
            if !public.starts_with(&project.path)
                || !public.join("index.php").is_file()
                || (site.vite
                    && !project
                        .path
                        .join("node_modules/vite/dist/node/index.js")
                        .is_file())
            {
                return Err(missing(id));
            }
            if let Some(binding) = self.projects.binding(id)? {
                let instance = self.projects.instance(&binding.instance_id)?;
                if binding.project_id != id || !binding.ready || !instance.ready {
                    return Err(missing(id));
                }
                let package = self.catalog.get(&instance.package)?;
                match instance.engine {
                    devforge_projects::database::Engine::Postgresql => {
                        if package.kind != "postgresql" {
                            return Err(missing(id));
                        }
                        self.packages.executable(package, "postgres")?;
                        self.packages.executable(package, "pg_ctl")?;
                    }
                    devforge_projects::database::Engine::Mysql => {
                        if package.kind != "mysql" {
                            return Err(missing(id));
                        }
                        self.packages.executable(package, "mysqld")?;
                    }
                }
            }
            if project.services.uses_redis() {
                let cache = self.projects.cache_instance(
                    project
                        .services
                        .redis_instance
                        .as_deref()
                        .ok_or_else(|| missing(id))?,
                )?;
                if cache.project_id != id || !cache.ready {
                    return Err(missing(id));
                }
                let package = self.catalog.get(&cache.package)?;
                let runtime = self.catalog.get(&cache.runtime)?;
                if package.kind != "garnet" || runtime.kind != "dotnet" {
                    return Err(missing(id));
                }
                self.packages.executable(package, "garnet")?;
                self.packages.executable(runtime, "dotnet")?;
            }
            if project.debugger.mode != devforge_debugger::Mode::Off {
                self.cached_debug_runtime(id, &project, php.version.split('-').next().unwrap())?;
            }
            Ok(())
        })();
        result.map_err(|_| missing(id))
    }

    fn cached_debug_runtime(&self, id: &str, project: &Project, php: &str) -> Result<()> {
        let selected = project
            .debugger
            .runtime
            .as_deref()
            .ok_or_else(|| missing(id))?;
        if selected.len() != 32 || !selected.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(missing(id));
        }
        let directory = self.projects.root().join("debug-runtimes");
        if fs::canonicalize(&directory)? != directory
            || fs::symlink_metadata(&directory)?.file_type().is_symlink()
        {
            return Err(missing(id));
        }
        let bytes = devforge_projects::manifest::read_file(
            &directory.join(format!("{selected}.json")),
            16384,
        )?;
        let entry: devforge_debugger::provider::Entry =
            serde_json::from_slice(&bytes).map_err(|_| missing(id))?;
        // Availability only: do NOT read the signing key here. The authorized
        // ensure_debugging/Context path still authenticates before using it.
        if entry.id != selected
            || entry.php != php
            || entry.target != devforge_runtime::package::Target::current()?
            || entry.signature.len() != 64
            || !entry.signature.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(missing(id));
        }
        self.packages.resource(&entry.package, "xdebug")?;
        if entry.source_build {
            self.packages.executable(&entry.package, "php")?;
            self.packages.executable(&entry.package, "php-fpm")?;
        }
        Ok(())
    }

    /// Checks are cooperative. Existing DB/cache startup, OS credential calls,
    /// DNS/probes and graceful stop retain their bounded/noninterruptible spans.
    /// On failure/cancel, already-started auxiliary DB/cache/SMTP are retained;
    /// restart may already have stopped this selected project's session. Never
    /// stop another project's consumers or claim a rollback of these effects.
    pub fn execute_service_action(
        &mut self,
        id: &str,
        action: &str,
        check: &dyn Fn() -> Result<()>,
    ) -> Result<serde_json::Value> {
        match action {
            "start" => {
                check()?;
                self.service_preflight(id)?;
                self.start_checked(id, check)?;
                Ok(
                    serde_json::json!({"started":true,"warnings":["service_partial_dependencies_retained"]}),
                )
            }
            "stop" => {
                check()?;
                self.projects.get(id)?;
                check()?;
                self.stop(id)?;
                Ok(
                    serde_json::json!({"stopped":true,"warnings":["service_partial_dependencies_retained"]}),
                )
            }
            "restart" => {
                check()?;
                self.service_preflight(id)?;
                check()?;
                self.stop(id)?;
                check()?;
                self.start_checked(id, check)?;
                Ok(
                    serde_json::json!({"restarted":true,"warnings":["service_partial_dependencies_retained"]}),
                )
            }
            _ => Err(error("service_action_unsupported")),
        }
    }
}

#[cfg(test)]
mod callback_tests {
    use super::*;
    use devforge_core::{ErrorCode, SecretStore};
    struct NoSecrets;
    impl SecretStore for NoSecrets {
        fn get(&self, _: &str) -> std::result::Result<Option<String>, ErrorCode> {
            panic!("denied serving must not read credentials")
        }
        fn set(&self, _: &str, _: &str) -> std::result::Result<(), ErrorCode> {
            panic!("denied serving must not write credentials")
        }
        fn delete(&self, _: &str) -> std::result::Result<(), ErrorCode> {
            panic!("denied serving must not delete credentials")
        }
    }
    #[test]
    fn serving_adapter_preserves_cancel_and_deadline_codes_before_effects() {
        let root = tempfile::tempdir().unwrap();
        let workspace = Workspace::open(root.path().join("data"), Arc::new(NoSecrets)).unwrap();
        for code in ["cancelled", "timed_out"] {
            let check = || {
                Err(Error {
                    scope: "workspace",
                    code: code.into(),
                    project_id: Some("project-a".into()),
                })
            };
            let error = match workspace.start_web_checked("project-a", &check) {
                Ok(_) => panic!("denied startup succeeded"),
                Err(error) => error,
            };
            assert_eq!(error.code, code);
            assert_eq!(error.scope, "workspace");
            assert_eq!(error.project_id.as_deref(), Some("project-a"));
        }
        assert!(!root.path().join("data/serving/sessions").exists());
    }
}

impl Workspace {
    fn start_web_checked(&self, id: &str, check: &dyn Fn() -> Result<()>) -> Result<Serving> {
        let check_error = RefCell::new(None);
        let serving_check = || {
            check().map_err(|error| {
                *check_error.borrow_mut() = Some(error);
                devforge_serving::ServingError::StartFailed
            })
        };
        Serving::start_selected_checked(
            &self.sites,
            &self.projects,
            &self.catalog,
            &self.packages,
            self.secrets.as_ref(),
            Options::default(),
            Some(&[id.into()]),
            &serving_check,
        )
        .map_err(|error| {
            check_error
                .borrow_mut()
                .take()
                .unwrap_or_else(|| error.into())
        })
    }
    pub(crate) fn start_checked(&mut self, id: &str, check: &dyn Fn() -> Result<()>) -> Result<()> {
        check()?;
        let project = self.available_project(id)?;
        if self.sessions.contains_key(id) {
            return Err(error("already_running"));
        }
        if project.mail {
            check()?;
            self.ensure_mail(id)?;
            check()?;
        }
        check()?;
        self.ensure_debugging(id)?;
        check()?;
        if project.services.uses_redis() {
            check()?;
            self.ensure_cache(
                project
                    .services
                    .redis_instance
                    .as_deref()
                    .ok_or_else(|| error("database_unbound"))?,
            )?;
        }
        check()?;
        if let Some(binding) = self.projects.binding(id)? {
            check()?;
            self.ensure_database(&binding.instance_id)?;
            check()?;
        }
        let web = self.start_web_checked(id, check)?;
        check()?;
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
            check()?;
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
            check()?;
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
                check()?;
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
        check()?;
        self.sessions.insert(id.into(), session);
        self.navigation(id, None, true)?;
        Ok(())
    }
}
