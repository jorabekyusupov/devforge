use super::*;
use devforge_debugger::{self as debug, Breakpoint, Mode, Settings, server};
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "command", rename_all = "snake_case", deny_unknown_fields)]
pub enum Control {
    Run,
    StepInto,
    StepOver,
    StepOut,
    Stop,
    Detach,
    Frame { depth: u32 },
}
fn convert(e: debug::Error) -> Error {
    Error {
        scope: "debugger",
        code: e.to_string(),
        project_id: None,
    }
}
impl Workspace {
    fn debug_directory(&self, id: &str) -> Result<PathBuf> {
        self.projects.get(id)?;
        let base = self.projects.root().join("debugging");
        if !base.exists() {
            fs::create_dir(&base)?;
        }
        if fs::symlink_metadata(&base)?.file_type().is_symlink() || fs::canonicalize(&base)? != base
        {
            return Err(convert(debug::Error::Unauthorized));
        }
        let path = base.join(id);
        if path.exists()
            && (fs::symlink_metadata(&path)?.file_type().is_symlink()
                || fs::canonicalize(&path)? != path)
        {
            return Err(convert(debug::Error::Unauthorized));
        }
        Ok(path)
    }
    fn debug_known_secrets(&self, id: &str) -> Result<Vec<String>> {
        let p = self.projects.get(id)?;
        let mut refs = p
            .environment_names
            .iter()
            .map(|n| {
                p.environment_references
                    .get(n)
                    .cloned()
                    .unwrap_or_else(|| devforge_projects::environment::key(&p, n))
            })
            .collect::<Vec<_>>();
        if let Some(b) = self.projects.binding(id)? {
            refs.push(b.secret_ref);
        }
        if let Some(cache) = p.services.redis_instance {
            refs.push(self.projects.cache_instance(&cache)?.secret_ref);
        }
        refs.push(devforge_projects::mail::reference(id));
        let mut values = vec![];
        for key in refs {
            if let Some(v) = self
                .secrets
                .get(&key)
                .map_err(|_| convert(debug::Error::Unavailable))?
                && !v.is_empty()
            {
                values.push(v);
            }
        }
        if let Some(key) = devforge_projects::env_file::app_key_secret(&p.path)? {
            values.push(key.to_string());
        }
        Ok(values)
    }
    pub(crate) fn ensure_debugging(&mut self, id: &str) -> Result<()> {
        let p = self.projects.get(id)?;
        if p.debugger.mode == Mode::Off {
            return Ok(());
        }
        if !self.debugging.contains_key(id) {
            let runtime = p
                .debugger
                .runtime
                .clone()
                .ok_or_else(|| convert(debug::Error::RuntimeMissing))?;
            let base = self.catalog.get(&p.php)?.version.split('-').next().unwrap();
            debug::provider::resolve(
                self.projects.root(),
                &runtime,
                base,
                &self.packages,
                self.secrets.as_ref(),
            )
            .map_err(convert)?;
            let activation = debug::session::Activation::start(
                &self.debug_directory(id)?,
                id,
                runtime,
                p.debugger.mode,
                &p.path,
                p.debugger.breakpoints,
                self.debug_known_secrets(id)?,
            )
            .map_err(convert)?;
            self.debugging.insert(id.into(), activation);
        }
        Ok(())
    }
    pub(crate) fn debug_install(
        &mut self,
        id: &str,
        trusted: bool,
        cancel: &Cancellation,
    ) -> Result<serde_json::Value> {
        self.require_stopped(id)?;
        let _guard = self.sites.project_mutation_guard(id)?;
        let p = self.projects.get(id)?;
        let version = self
            .catalog
            .get(&p.php)?
            .version
            .split('-')
            .next()
            .unwrap()
            .to_string();
        if let Ok(mut progress) = self.progress.lock() {
            *progress = Some(setup::Progress {
                stage: "debug_build".into(),
                project_id: Some(id.into()),
            });
        }
        let result = debug::provider::install(
            self.projects.root(),
            &version,
            &self.packages,
            self.secrets.as_ref(),
            trusted,
            cancel,
        )
        .map_err(convert);
        if let Ok(mut progress) = self.progress.lock() {
            *progress = None;
        }
        Ok(serde_json::to_value(result?.info(&self.packages)).unwrap())
    }
    pub(crate) fn debug_configure(
        &mut self,
        id: &str,
        revision: u64,
        settings: Settings,
        consent: bool,
    ) -> Result<serde_json::Value> {
        self.require_stopped(id)?;
        if !consent {
            return Err(convert(debug::Error::Unauthorized));
        }
        let _guard = self.sites.project_mutation_guard(id)?;
        let p = self.projects.get(id)?;
        if settings.mode != Mode::Off {
            let version = self.catalog.get(&p.php)?.version.split('-').next().unwrap();
            debug::provider::resolve(
                self.projects.root(),
                settings
                    .runtime
                    .as_deref()
                    .ok_or_else(|| convert(debug::Error::RuntimeMissing))?,
                version,
                &self.packages,
                self.secrets.as_ref(),
            )
            .map_err(convert)?;
        }
        self.debugging.remove(id);
        self.projects.save_debugger(id, revision, settings)?;
        self.php_capabilities.remove(id);
        Ok(serde_json::json!({"configured":true}))
    }
    pub(crate) fn debug_control(
        &mut self,
        id: &str,
        control: Control,
    ) -> Result<serde_json::Value> {
        self.projects.get(id)?;
        let command = match control {
            Control::Run => server::Control::Run,
            Control::StepInto => server::Control::StepInto,
            Control::StepOver => server::Control::StepOver,
            Control::StepOut => server::Control::StepOut,
            Control::Stop => server::Control::Stop,
            Control::Detach => server::Control::Detach,
            Control::Frame { depth } => server::Control::Frame(depth),
        };
        self.debugging
            .get(id)
            .ok_or_else(|| convert(debug::Error::Unavailable))?
            .control(command)
            .map_err(convert)?;
        Ok(serde_json::json!({"queued":true}))
    }
    pub(crate) fn debug_breakpoints(
        &mut self,
        id: &str,
        revision: u64,
        breakpoints: Vec<Breakpoint>,
    ) -> Result<serde_json::Value> {
        let p = self.projects.get(id)?;
        if p.revision != revision {
            return Err(devforge_projects::ProjectError::StaleRevision.into());
        }
        let mut next = p.debugger.clone();
        next.breakpoints = breakpoints.clone();
        next.validate(&p.path).map_err(convert)?;
        if let Some(a) = self.debugging.get(id)
            && let Some(s) = a.snapshot()
        {
            if s.state == "running" || s.state == "connecting" {
                return Err(convert(debug::Error::Busy));
            }
            if s.state == "paused" {
                a.control(server::Control::Breakpoints(breakpoints))
                    .map_err(convert)?;
            }
        }
        self.projects.save_debugger(id, revision, next)?;
        Ok(serde_json::json!({"saved":true}))
    }
    pub(crate) fn debug_request(&mut self, id: &str, path: &str) -> Result<serde_json::Value> {
        let origin = self
            .sessions
            .get(id)
            .and_then(|s| s.web.status().sites.into_iter().next())
            .ok_or_else(|| error("not_running"))?
            .fallback_url;
        self.ensure_debugging(id)?;
        let directory = self.debug_directory(id)?.join("profiles");
        debug::store::trim(&directory).map_err(convert)?;
        self.debugging
            .get_mut(id)
            .ok_or_else(|| convert(debug::Error::Unavailable))?
            .get(&origin, path)
            .map_err(convert)?;
        Ok(serde_json::json!({"queued":true}))
    }
    pub fn debug_view(&self, id: &str) -> Result<serde_json::Value> {
        let p = self.projects.get(id)?;
        let dir = self.debug_directory(id)?.join("profiles");
        let a = self.debugging.get(id);
        if a.is_none_or(|s| s.request().state != "running") {
            debug::store::trim(&dir).map_err(convert)?;
        }
        let version = self.catalog.get(&p.php)?.version.split('-').next().unwrap();
        let sdks = debug::provider::entries(self.projects.root(), self.secrets.as_ref())
            .map_err(convert)?
            .into_iter()
            .filter(|e| e.php == version && Some(e.target) == Target::current().ok())
            .map(|e| e.info(&self.packages))
            .collect::<Vec<_>>();
        Ok(
            serde_json::json!({"settings":p.debugger,"runtimes":sdks,"session":a.and_then(|s|s.snapshot()),"request":a.map(|s|s.request()),"profiles":debug::store::list(&dir).map_err(convert)?}),
        )
    }
    pub fn debug_profile(&self, id: &str, profile: &str) -> Result<serde_json::Value> {
        let p = self.projects.get(id)?;
        if self
            .debugging
            .get(id)
            .is_some_and(|s| s.request().state == "running")
        {
            return Err(convert(debug::Error::Busy));
        }
        let data = debug::store::load(
            &self.debug_directory(id)?.join("profiles"),
            profile,
            &p.path,
            &self.debug_known_secrets(id)?,
        )
        .map_err(convert)?;
        Ok(serde_json::to_value(data).unwrap())
    }
    pub fn debug_source(&self, id: &str, file: &str) -> Result<serde_json::Value> {
        let p = self.projects.get(id)?;
        let source = debug::source(&p.path, file).map_err(convert)?;
        let bytes = devforge_projects::manifest::read_file(&source, 256 * 1024)?;
        let mut code =
            String::from_utf8(bytes).map_err(|_| convert(debug::Error::SourceOutside))?;
        for secret in self.debug_known_secrets(id)? {
            code = code.replace(&secret, "[REDACTED]");
        }
        Ok(serde_json::json!({"file":file,"code":code}))
    }
    pub(crate) fn debug_profile_delete(
        &self,
        id: &str,
        profile: &str,
        consent: bool,
    ) -> Result<serde_json::Value> {
        if !consent {
            return Err(convert(debug::Error::Unauthorized));
        }
        if self
            .debugging
            .get(id)
            .is_some_and(|s| s.request().state == "running")
        {
            return Err(convert(debug::Error::Busy));
        }
        debug::store::delete(&self.debug_directory(id)?.join("profiles"), profile)
            .map_err(convert)?;
        Ok(serde_json::json!({"deleted":true}))
    }
    pub(crate) fn debug_profile_export(
        &self,
        id: &str,
        profile: &str,
        path: &Path,
    ) -> Result<serde_json::Value> {
        if self
            .debugging
            .get(id)
            .is_some_and(|s| s.request().state == "running")
        {
            return Err(convert(debug::Error::Busy));
        }
        debug::store::export(&self.debug_directory(id)?.join("profiles"), profile, path)
            .map_err(convert)?;
        Ok(serde_json::json!({"exported":true}))
    }
}
