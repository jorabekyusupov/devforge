use super::*;
pub use devforge_debugger::server::DispatchGuard;
pub type DebugStartGuard = std::sync::Arc<dyn Fn(&str) -> bool + Send + Sync>;
use devforge_debugger::{self as debug, Settings, server};
pub use devforge_debugger::{Assignment, Breakpoint, Mode};
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "command", rename_all = "snake_case", deny_unknown_fields)]
pub enum Control {
    Run,
    StepInto,
    StepOver,
    StepOut,
    Stop,
    Detach,
    Frame {
        depth: u32,
    },
    Set {
        value: debug::Assignment,
        revision: u64,
    },
}
/// External projections are explicitly bounded and omit raw debugger transport,
/// provider paths, request details and unrequested variable contents.
#[derive(Serialize)]
pub struct ExternalDebugView {
    pub project_revision: u64,
    pub mode: &'static str,
    pub session: Option<ExternalDebugSession>,
    pub profiles: Vec<ExternalProfile>,
    pub profile_count: u64,
}
#[derive(Serialize)]
pub struct ExternalDebugSession {
    pub session_id: String,
    pub revision: u64,
    pub state: &'static str,
    pub depth: u32,
    pub frame_count: u32,
    pub variable_count: u32,
    pub frames: Vec<ExternalFrame>,
    pub locals: Vec<ExternalLocal>,
}
#[derive(Serialize)]
pub struct ExternalFrame {
    pub depth: u32,
    pub function: String,
    pub file: Option<String>,
    pub line: u32,
}
#[derive(Serialize)]
pub struct ExternalLocal {
    pub name: String,
    pub kind: String,
    pub value: String,
}
#[derive(Serialize)]
pub struct ExternalProfile {
    pub id: String,
    pub created_ms: u64,
    pub bytes: u64,
    pub oversized: bool,
}
const SAFE_INT_MAX: u64 = 9_007_199_254_740_991;
fn safe_text(value: &str, max: usize) -> String {
    let mut out = String::new();
    for c in value.chars().filter(|c| !c.is_control()) {
        if out.len() + c.len_utf8() > max {
            break;
        }
        out.push(c);
    }
    out
}
fn safe_relative(value: &str) -> Option<String> {
    let path = std::path::Path::new(value);
    if value.is_empty()
        || value.len() > 512
        || value.contains(['\\', ':'])
        || value.chars().any(char::is_control)
        || !path
            .components()
            .all(|c| matches!(c, std::path::Component::Normal(_)))
    {
        return None;
    }
    Some(value.replace('\\', "/"))
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
        self.ensure_debugging_with_retention(id, true)
    }
    fn ensure_debugging_with_retention(&mut self, id: &str, prune: bool) -> Result<()> {
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
            let start = if prune {
                debug::session::Activation::start
            } else {
                debug::session::Activation::start_unpruned
            };
            let activation = start(
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
        settings.validate(&p.path).map_err(convert)?;
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
    /// Caller must authorize the project and explicit debugger-management scope.
    /// Preserve the current breakpoints; configure retains its stopped-site and
    /// managed-runtime checks, and the actor serializes this revision comparison.
    pub fn debug_mode_reviewed(
        &mut self,
        id: &str,
        mode: Mode,
        runtime: Option<String>,
        revision: u64,
    ) -> Result<serde_json::Value> {
        let project = self.projects.get(id)?;
        if project.revision != revision {
            return Err(devforge_projects::ProjectError::StaleRevision.into());
        }
        let mut settings = project.debugger;
        settings.mode = mode;
        settings.runtime = runtime;
        self.debug_configure(id, revision, settings, true)
    }

    /// Manager calls this only after reviewing the exact finite primitive action.
    /// Queue admission and the debugger worker each reject stale pause revisions.
    pub fn debug_assign_reviewed(
        &mut self,
        id: &str,
        value: debug::Assignment,
        expected_session_id: &str,
        session_revision: u64,
        guard: DispatchGuard,
    ) -> Result<serde_json::Value> {
        self.projects.get(id)?;
        value.validate().map_err(convert)?;
        let activation = self
            .debugging
            .get(id)
            .ok_or_else(|| convert(debug::Error::Unavailable))?;
        if activation.launch().instance != expected_session_id {
            return Err(convert(debug::Error::InvalidVariable));
        }
        activation
            .set_at_revision(value, session_revision, guard)
            .map_err(convert)?;
        Ok(serde_json::json!({"queued": true}))
    }

    /// A managed-site GET only. The caller may not choose an authority, query,
    /// credentials or method; sending the request can change application state.
    pub fn debug_start_scoped(
        &mut self,
        id: &str,
        path: &str,
        guard: DebugStartGuard,
    ) -> Result<serde_json::Value> {
        if path.is_empty()
            || path.len() > 1024
            || !path.starts_with('/')
            || path.starts_with("//")
            || path
                .bytes()
                .any(|b| b <= 0x20 || b >= 0x7f || matches!(b, b'?' | b'#' | b'\\'))
        {
            return Err(convert(debug::Error::InvalidRequest));
        }
        self.projects.get(id)?;
        let origin = self
            .sessions
            .get(id)
            .and_then(|s| {
                s.web
                    .status()
                    .sites
                    .into_iter()
                    .find(|site| site.project_id == id)
            })
            .ok_or_else(|| error("not_running"))?
            .fallback_url;
        self.ensure_debugging_with_retention(id, false)?;
        if self
            .debugging
            .get(id)
            .is_some_and(|a| a.request().state == "running")
        {
            return Err(convert(debug::Error::Busy));
        }
        let activation = self
            .debugging
            .get_mut(id)
            .ok_or_else(|| convert(debug::Error::Unavailable))?;
        let instance = activation.launch().instance.clone();
        let dispatch: DispatchGuard = std::sync::Arc::new(move || guard(&instance));
        activation
            .send_guarded(
                &origin,
                debug::session::HttpRequest {
                    method: "GET".into(),
                    path: path.into(),
                    headers: Default::default(),
                    body: String::new(),
                },
                dispatch,
            )
            .map_err(convert)?;
        Ok(serde_json::json!({"queued":true}))
    }

    /// Non-secret in-memory identity; remains available in profiling mode.
    pub fn external_debug_activation_identity(&self, id: &str) -> Result<Option<String>> {
        self.projects.get(id)?;
        Ok(self.debugging.get(id).map(|a| a.launch().instance.clone()))
    }

    /// Synthetic paused owner fixture; never part of production IPC.
    #[cfg(feature = "test-support")]
    pub fn test_synthetic_paused_debugger(&mut self, id: &str) -> Result<(String, u64)> {
        let project = self.projects.get(id)?;
        self.debugging.remove(id);
        let activation = debug::session::Activation::start(
            &self.debug_directory(id)?,
            id,
            "fixture".into(),
            Mode::Debug,
            &project.path,
            vec![],
            vec![],
        )
        .map_err(convert)?;
        let revision = activation.test_set_paused().map_err(convert)?;
        let instance = activation.launch().instance.clone();
        self.debugging.insert(id.to_owned(), activation);
        Ok((instance, revision))
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
            Control::Set { value, revision } => server::Control::Set(value, revision),
        };
        self.debugging
            .get(id)
            .ok_or_else(|| convert(debug::Error::Unavailable))?
            .control(command)
            .map_err(convert)?;
        Ok(serde_json::json!({"queued":true}))
    }
    /// In-memory session identity and snapshot revision, without filesystem or secret IO.
    pub fn external_debug_session_identity(&self, id: &str) -> Result<Option<(String, u64)>> {
        self.projects.get(id)?;
        Ok(self.debugging.get(id).and_then(|a| {
            a.snapshot()
                .map(|s| (a.launch().instance.clone(), s.revision))
        }))
    }
    /// Scope authorization and the worker guard are provided by the manager.
    pub fn debug_control_guarded(
        &mut self,
        id: &str,
        control: Control,
        expected_session_id: &str,
        session_revision: u64,
        guard: DispatchGuard,
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
            Control::Set { .. } => return Err(convert(debug::Error::Unauthorized)),
        };
        let activation = self
            .debugging
            .get(id)
            .ok_or_else(|| convert(debug::Error::Unavailable))?;
        if activation.launch().instance != expected_session_id {
            return Err(convert(debug::Error::InvalidVariable));
        }
        activation
            .control_at_revision(command, session_revision, guard)
            .map_err(convert)?;
        Ok(serde_json::json!({"queued": true}))
    }
    /// Agent breakpoint edits cannot race an existing activation's DBGp queue.
    /// The native UI's existing breakpoint flow is unchanged.
    pub fn debug_breakpoints_guarded(
        &mut self,
        id: &str,
        revision: u64,
        breakpoints: Vec<Breakpoint>,
    ) -> Result<serde_json::Value> {
        self.projects.get(id)?;
        if self.debugging.contains_key(id) {
            return Err(convert(debug::Error::Busy));
        }
        self.debug_breakpoints(id, revision, breakpoints)
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
        for b in breakpoints
            .iter()
            .filter(|b| !p.debugger.breakpoints.contains(b))
        {
            debug::source(&p.path, &b.file).map_err(convert)?;
        }
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
        self.debug_http(
            id,
            debug::session::HttpRequest {
                method: "GET".into(),
                path: path.into(),
                headers: Default::default(),
                body: String::new(),
            },
        )
    }
    pub(crate) fn debug_http(
        &mut self,
        id: &str,
        request: debug::session::HttpRequest,
    ) -> Result<serde_json::Value> {
        request.validate().map_err(convert)?;
        let origin = self
            .sessions
            .get(id)
            .and_then(|s| {
                s.web
                    .status()
                    .sites
                    .into_iter()
                    .find(|site| site.project_id == id)
            })
            .ok_or_else(|| error("not_running"))?
            .fallback_url;
        self.ensure_debugging(id)?;
        if self
            .debugging
            .get(id)
            .is_some_and(|a| a.request().state == "running")
        {
            return Err(convert(debug::Error::Busy));
        }
        let directory = self.debug_directory(id)?.join("profiles");
        debug::store::trim(&directory).map_err(convert)?;
        self.debugging
            .get_mut(id)
            .ok_or_else(|| convert(debug::Error::Unavailable))?
            .send(&origin, request)
            .map_err(convert)?;
        Ok(serde_json::json!({"queued":true}))
    }
    /// Read the private profiler directory without creating or pruning anything.
    fn external_debug_profiles_dir(&self, id: &str) -> Result<Option<PathBuf>> {
        self.projects.get(id)?;
        let mut path = self.projects.root().to_path_buf();
        for component in ["debugging", id, "profiles"] {
            path.push(component);
            match fs::symlink_metadata(&path) {
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
                Err(e) => return Err(e.into()),
                Ok(metadata) => {
                    if !metadata.is_dir()
                        || metadata.file_type().is_symlink()
                        || fs::canonicalize(&path)? != path
                    {
                        return Err(convert(debug::Error::Unauthorized));
                    }
                }
            }
        }
        Ok(Some(path))
    }
    /// Caller must check the correct project and explicit information scopes
    /// before and after this read. Known-secret masking is best effort.
    pub fn external_debug_view(
        &self,
        id: &str,
        detail: bool,
        locals: bool,
    ) -> Result<ExternalDebugView> {
        let project = self.projects.get(id)?;
        if project.revision > SAFE_INT_MAX {
            return Err(convert(debug::Error::Unavailable));
        }
        let mode = match project.debugger.mode {
            Mode::Off => "off",
            Mode::Debug => "debug",
            Mode::Profile => "profile",
        };
        let all_profiles = match self.external_debug_profiles_dir(id)? {
            Some(path) => debug::store::list(&path).map_err(convert)?,
            None => vec![],
        };
        let profile_count = all_profiles.len() as u64;
        let mut profiles = Vec::new();
        if detail {
            for item in all_profiles.into_iter().take(100) {
                if item.created_ms > SAFE_INT_MAX || item.bytes > SAFE_INT_MAX {
                    return Err(convert(debug::Error::Unavailable));
                }
                profiles.push(ExternalProfile {
                    id: item.id,
                    created_ms: item.created_ms,
                    bytes: item.bytes,
                    oversized: item.oversized,
                });
            }
        }
        let session = self
            .debugging
            .get(id)
            .and_then(|a| a.snapshot().map(|s| (a.launch().instance.clone(), s)))
            .map(|(session_id, s)| -> Result<_> {
                if s.revision > SAFE_INT_MAX {
                    return Err(convert(debug::Error::Unavailable));
                }
                let state = match s.state.as_str() {
                    "connecting" => "connecting",
                    "listening" => "listening",
                    "running" => "running",
                    "paused" => "paused",
                    "stopped" => "stopped",
                    "detached" => "detached",
                    "error" => "error",
                    _ => "unknown",
                };
                let frames = if detail {
                    s.frames
                        .iter()
                        .take(100)
                        .map(|f| ExternalFrame {
                            depth: f.depth,
                            function: safe_text(&f.function, 512),
                            file: f.file.as_deref().and_then(safe_relative),
                            line: f.line,
                        })
                        .collect()
                } else {
                    vec![]
                };
                let locals = if locals {
                    s.variables
                        .iter()
                        .filter_map(|v| {
                            let name = v.assign.as_ref()?;
                            if !matches!(
                                v.kind.as_str(),
                                "string" | "int" | "float" | "bool" | "null"
                            ) || !v.children.is_empty()
                            {
                                return None;
                            }
                            Some(ExternalLocal {
                                name: safe_text(name, 80),
                                kind: v.kind.clone(),
                                value: safe_text(&v.value, 512),
                            })
                        })
                        .take(32)
                        .collect()
                } else {
                    vec![]
                };
                Ok(ExternalDebugSession {
                    session_id,
                    revision: s.revision,
                    state,
                    depth: s.depth,
                    frame_count: s.frames.len().min(u32::MAX as usize) as u32,
                    variable_count: s.variables.len().min(u32::MAX as usize) as u32,
                    frames,
                    locals,
                })
            })
            .transpose()?;
        Ok(ExternalDebugView {
            project_revision: project.revision,
            mode,
            session,
            profiles,
            profile_count,
        })
    }
    /// Existing guarded parser, noncreating directory, bounded output. Never
    /// export a raw profile file or arbitrary caller-supplied path.
    pub fn external_debug_profile(&self, id: &str, profile: &str) -> Result<serde_json::Value> {
        let project = self.projects.get(id)?;
        if self
            .debugging
            .get(id)
            .is_some_and(|s| s.request().state == "running")
        {
            return Err(convert(debug::Error::Busy));
        }
        let directory = self
            .external_debug_profiles_dir(id)?
            .ok_or_else(|| convert(debug::Error::Unavailable))?;
        let mut result = debug::store::load(
            &directory,
            profile,
            &project.path,
            &self.debug_known_secrets(id)?,
        )
        .map_err(convert)?;
        let valid_number = |n: f64| n.is_finite() && n >= 0.0 && n <= SAFE_INT_MAX as f64;
        let valid_signed = |n: i64| n.unsigned_abs() <= SAFE_INT_MAX;
        if !valid_number(result.total_ms) || !valid_signed(result.memory_bytes) {
            return Err(convert(debug::Error::ProfileInvalid));
        }
        if result.functions.len() > 100 {
            result.functions.truncate(100);
            result.truncated = true;
        }
        for function in &mut result.functions {
            if !valid_number(function.self_ms)
                || !valid_number(function.inclusive_ms)
                || !valid_signed(function.memory_bytes)
                || function.calls > SAFE_INT_MAX
            {
                return Err(convert(debug::Error::ProfileInvalid));
            }
            function.file =
                safe_relative(&function.file).unwrap_or_else(|| "[outside-project]".into());
            function.name = safe_text(&function.name, 512);
        }
        serde_json::to_value(result).map_err(|_| convert(debug::Error::ProfileInvalid))
    }
    /// Returns bounded, redacted profile JSON for a manager-owned reviewed
    /// export. Never reads or returns raw cachegrind bytes or a destination path.
    pub fn debug_profile_export_preview(&self, id: &str, profile: &str) -> Result<Vec<u8>> {
        let bytes = serde_json::to_vec(&self.external_debug_profile(id, profile)?)
            .map_err(|_| convert(debug::Error::ProfileInvalid))?;
        if bytes.len() > 128 * 1024 {
            return Err(convert(debug::Error::ProfileLimit));
        }
        Ok(bytes)
    }
    pub fn debug_view(&self, id: &str) -> Result<serde_json::Value> {
        let p = self.projects.get(id)?;
        let directory = self.debug_directory(id)?;
        let dir = directory.join("profiles");
        let a = self.debugging.get(id);
        // A CLI reader has no in-memory activation for the desktop's session.
        // Never prune files belonging to that other live owner.
        let external_owner = a.is_none()
            && debug::session::read(&directory, id)
                .map_err(convert)?
                .is_some();
        if !external_owner && a.is_none_or(|s| s.request().state != "running") {
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
