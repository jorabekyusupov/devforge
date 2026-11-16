//! Finite trusted-project execution, not a hostile-code or filesystem sandbox.
//! Preflight never resolves credentials; only the authorized execution phase does.
use crate::{Result, Workspace, error};
use devforge_projects::{
    execution::{Context, Tool},
    manifest,
};
use devforge_runtime::Cancellation;
use serde::{Deserialize, Serialize};
use std::{
    cell::RefCell,
    collections::BTreeMap,
    fs,
    path::{Component, Path},
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionTool {
    Php,
    Artisan,
    Composer,
    Node,
    Npm,
    Git,
}
impl ExecutionTool {
    fn native(self) -> Tool {
        match self {
            Self::Php => Tool::Php,
            Self::Artisan => Tool::Artisan,
            Self::Composer => Tool::Composer,
            Self::Node => Tool::Node,
            Self::Npm => Tool::Npm,
            Self::Git => Tool::Git,
        }
    }
}
#[derive(Clone)]
pub struct ExecutionRequest {
    pub tool: ExecutionTool,
    pub args: Vec<String>,
    pub timeout_ms: u64,
    pub managed_test: bool,
}
impl Drop for ExecutionRequest {
    fn drop(&mut self) {
        use zeroize::Zeroize;
        self.args.zeroize();
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionContextSummary {
    pub environment: bool,
    pub database: bool,
    pub cache: bool,
    pub mail: bool,
    pub test_isolated: bool,
}
/// Private RAM-only context fingerprint; never send this object to a renderer.
#[derive(Clone, Serialize)]
pub struct ExecutionContextFacts {
    fingerprint: String,
    summary: ExecutionContextSummary,
    debugger: bool,
    tool: ExecutionTool,
    scripts: Vec<String>,
}
impl ExecutionContextFacts {
    pub fn fingerprint(&self) -> Result<String> {
        Ok(self.fingerprint.clone())
    }
    pub fn sensitive_context(&self) -> bool {
        self.summary.environment
            || self.summary.database
            || self.summary.cache
            || self.summary.mail
            || self.debugger
    }
    pub fn native_summary(&self) -> ExecutionContextSummary {
        self.summary.clone()
    }
    /// Display only, never an argv validator or authority. Inline code, URLs,
    /// arbitrary values and secret-looking options are deliberately hidden.
    pub fn argument_preview(&self, args: &[String]) -> Vec<String> {
        let mut inline = false;
        let mut secret_value = false;
        args.iter()
            .enumerate()
            .map(|(index, arg)| {
                if secret_value {
                    secret_value = false;
                    return "[REDACTED_ARGUMENT]".into();
                }
                if inline {
                    inline = false;
                    return "[REDACTED_INLINE_CODE]".into();
                }
                if matches!(arg.as_str(), "-r" | "-e" | "--eval" | "--print") {
                    inline = true;
                    return arg.clone();
                }
                let lower = arg.to_ascii_lowercase();
                if [
                    "password",
                    "passwd",
                    "secret",
                    "token",
                    "credential",
                    "authorization",
                    "api-key",
                    "api_key",
                    "private-key",
                ]
                .iter()
                .any(|part| lower.contains(part))
                {
                    if arg.starts_with('-') && !arg.contains('=') {
                        secret_value = true;
                    }
                    return "[REDACTED_ARGUMENT]".into();
                }
                if [
                    "--version",
                    "--help",
                    "-v",
                    "-vv",
                    "-vvv",
                    "--no-interaction",
                    "--no-plugins",
                    "--no-scripts",
                    "--ignore-scripts",
                    "--ansi",
                    "--no-ansi",
                    "--force",
                    "--hard",
                    "--cached",
                    "--stat",
                    "--quiet",
                ]
                .contains(&arg.as_str())
                    || (index == 0
                        && [
                            "list",
                            "about",
                            "test",
                            "migrate",
                            "install",
                            "update",
                            "require",
                            "dump-autoload",
                            "status",
                            "diff",
                            "show",
                            "log",
                            "branch",
                            "checkout",
                            "reset",
                            "worktree",
                            "commit",
                            "add",
                            "run",
                            "ci",
                            "audit",
                            "cache:clear",
                            "config:cache",
                            "route:list",
                            "queue:work",
                        ]
                        .contains(&arg.as_str()))
                    || self.scripts.contains(arg)
                {
                    arg.clone()
                } else {
                    "[REDACTED_ARGUMENT]".into()
                }
            })
            .collect()
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionLog {
    pub stream: String,
    pub text: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionOutput {
    pub success: bool,
    pub exit_code: Option<i32>,
    pub logs: Vec<ExecutionLog>,
    pub dropped_bytes: u64,
    pub truncated: bool,
}
fn validate(request: &ExecutionRequest) -> Result<()> {
    if request.args.len() > 64
        || request.args.iter().map(|arg| arg.len()).sum::<usize>() > 16384
        || request.args.iter().any(|arg| arg.contains('\0'))
        || request.timeout_ms == 0
        || request.timeout_ms > 300000
    {
        return Err(error("execution_invalid_input"));
    }
    Ok(())
}
fn file_hash(path: &Path, limit: u64) -> Result<Option<String>> {
    match fs::symlink_metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err(error("execution_context_unavailable")),
        Ok(_) => Ok(Some(devforge_serving::hash(&manifest::read_file(
            path, limit,
        )?))),
    }
}
impl Workspace {
    pub fn execution_preflight(
        &self,
        id: &str,
        request: &ExecutionRequest,
    ) -> Result<ExecutionContextFacts> {
        validate(request)?;
        let project = self.available_project(id)?;
        let php = self.catalog.get(&project.php)?;
        let node = self.catalog.get(&project.node)?;
        devforge_projects::project::compatible(
            &devforge_projects::project::inspect(&project.path)?,
            php,
            node,
        )?;
        self.packages
            .executable(php, "php")
            .map_err(|_| error("execution_precondition_missing"))?;
        self.packages
            .executable(node, "node")
            .map_err(|_| error("execution_precondition_missing"))?;
        if matches!(request.tool, ExecutionTool::Composer) {
            self.packages
                .resource(self.catalog.composer(&project.composer)?, "composer")
                .map_err(|_| error("execution_precondition_missing"))?;
        }
        if matches!(request.tool, ExecutionTool::Artisan) && !project.path.join("artisan").is_file()
        {
            return Err(error("execution_precondition_missing"));
        }
        if matches!(request.tool, ExecutionTool::Npm) {
            let executable = self.packages.executable(node, "node")?;
            let npm = if cfg!(windows) {
                executable
                    .path()
                    .parent()
                    .unwrap()
                    .join("node_modules/npm/bin/npm-cli.js")
            } else {
                executable
                    .path()
                    .parent()
                    .unwrap()
                    .parent()
                    .ok_or_else(|| error("execution_precondition_missing"))?
                    .join("lib/node_modules/npm/bin/npm-cli.js")
            };
            if !npm.is_file() {
                return Err(error("execution_precondition_missing"));
            }
        }
        if matches!(request.tool, ExecutionTool::Git) {
            devforge_projects::execution::find_git()?;
        }
        let debugger = project.debugger.mode != devforge_debugger::Mode::Off;
        let mut debug_hash = None;
        if debugger {
            let runtime = project
                .debugger
                .runtime
                .as_deref()
                .ok_or_else(|| error("execution_precondition_missing"))?;
            if runtime.len() != 32 || !runtime.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Err(error("execution_precondition_missing"));
            }
            let bytes = manifest::read_file(
                &self
                    .projects
                    .root()
                    .join("debug-runtimes")
                    .join(format!("{runtime}.json")),
                16384,
            )?;
            let entry: devforge_debugger::provider::Entry = serde_json::from_slice(&bytes)
                .map_err(|_| error("execution_precondition_missing"))?;
            if entry.id != runtime
                || entry.php != php.version.split('-').next().unwrap()
                || entry.target != php.target
            {
                return Err(error("execution_precondition_missing"));
            }
            self.packages.resource(&entry.package, "xdebug")?;
            if entry.source_build {
                self.packages.executable(&entry.package, "php")?;
                self.packages.executable(&entry.package, "php-fpm")?;
            }
            debug_hash = Some(devforge_serving::hash(&bytes));
        }
        let binding = self.projects.binding(id)?;
        let instance = binding
            .as_ref()
            .map(|binding| self.projects.instance(&binding.instance_id))
            .transpose()?;
        let cache = project
            .services
            .redis_instance
            .as_deref()
            .map(|id| self.projects.cache_instance(id))
            .transpose()?;
        if binding
            .as_ref()
            .is_some_and(|binding| binding.project_id != id)
            || cache.as_ref().is_some_and(|cache| cache.project_id != id)
        {
            return Err(error("execution_context_unavailable"));
        }
        let mut files = BTreeMap::new();
        for (name, limit) in [
            ("composer.json", 2 * 1024 * 1024),
            ("composer.lock", 8 * 1024 * 1024),
            ("package.json", 2 * 1024 * 1024),
            ("package-lock.json", 8 * 1024 * 1024),
            (".env", 1024 * 1024),
            (".env.testing", 1024 * 1024),
            (".env.test", 1024 * 1024),
            ("phpunit.xml", 1024 * 1024),
            ("phpunit.xml.dist", 1024 * 1024),
            ("config/database.php", 2 * 1024 * 1024),
        ] {
            files.insert(
                name.to_string(),
                file_hash(&project.path.join(name), limit)?,
            );
        }
        let mut environment =
            !project.environment_names.is_empty() || files.get(".env").is_some_and(Option::is_some);
        let database = binding.is_some();
        let cache_context = cache.is_some()
            || project.services.uses_redis()
            || project.services.cache == devforge_projects::cache::CacheDriver::Database
            || project.services.queue == devforge_projects::cache::QueueDriver::Database;
        let mut override_file = false;
        let mut count = 0;
        for entry in fs::read_dir(&project.path)? {
            count += 1;
            if count > 4096 {
                return Err(error("execution_context_unavailable"));
            }
            let name = entry?.file_name();
            let name = name.to_string_lossy();
            if name == ".env" || name.starts_with(".env.") || name.starts_with("phpunit.xml") {
                override_file = true;
                environment = true;
                files.insert(
                    name.to_string(),
                    file_hash(&project.path.join(name.as_ref()), 1024 * 1024)?,
                );
            }
        }
        let inspection = file_hash(
            &self
                .projects
                .root()
                .join("inspection")
                .join(format!("{id}.json")),
            4096,
        )?;
        environment |= inspection.is_some();
        // Git intentionally retains native HOME/credential-helper behavior.
        // Classify that context before resolving any environment/credentials.
        let git = matches!(request.tool, ExecutionTool::Git);
        environment |= git;
        let ssh_agent_hash = if git {
            std::env::var_os("SSH_AUTH_SOCK")
                .map(|value| devforge_serving::hash(value.as_encoded_bytes()))
        } else {
            None
        };
        // Strict data-free managed-test context. This is not proof about what
        // arbitrary trusted code can do outside the managed environment.
        let test_isolated = !database
            && !cache_context
            && !project.mail
            && project.environment_names.is_empty()
            && !override_file
            && files
                .get("config/database.php")
                .is_some_and(Option::is_none)
            && !project.path.join("database").exists();
        if request.managed_test && !test_isolated {
            return Err(error("execution_test_context_unverified"));
        }
        let mut scripts = Vec::new();
        let mut script_hashes = BTreeMap::new();
        for arg in &request.args {
            let relative = Path::new(arg);
            if !relative.is_absolute()
                && relative
                    .components()
                    .all(|component| matches!(component, Component::Normal(_)))
                && matches!(
                    relative.extension().and_then(|ext| ext.to_str()),
                    Some("php" | "js" | "mjs")
                )
                && let Ok(path) = fs::canonicalize(project.path.join(relative))
                && path.starts_with(&project.path)
                && path.is_file()
            {
                scripts.push(arg.clone());
                script_hashes.insert(arg.clone(), file_hash(&path, 2 * 1024 * 1024)?);
            }
        }
        let summary = ExecutionContextSummary {
            environment,
            database,
            cache: cache_context,
            mail: project.mail,
            test_isolated,
        };
        // Serialize borrowed fields directly: no intermediate Value cloning raw
        // argv. The owned private hash material is zeroized when it leaves RAM.
        let material = zeroize::Zeroizing::new(
            serde_json::to_vec(&(
                &project,
                php,
                node,
                self.catalog.get(&project.composer)?,
                &binding,
                &instance,
                &cache,
                (&files, &inspection),
                &debug_hash,
                request.tool,
                &request.args,
                request.timeout_ms,
                request.managed_test,
                &script_hashes,
            ))
            .map_err(|_| error("execution_context_unavailable"))?,
        );
        let mut fingerprint = devforge_serving::hash(&material);
        if git {
            fingerprint = devforge_serving::hash(
                &serde_json::to_vec(&(&fingerprint, &ssh_agent_hash))
                    .map_err(|_| error("execution_context_unavailable"))?,
            );
        }
        Ok(ExecutionContextFacts {
            fingerprint,
            summary,
            debugger,
            tool: request.tool,
            scripts,
        })
    }
    pub fn execute_managed_execution(
        &mut self,
        id: &str,
        request: &ExecutionRequest,
        expected: &ExecutionContextFacts,
        cancel: &Cancellation,
        check: &dyn Fn() -> Result<()>,
    ) -> Result<ExecutionOutput> {
        validate(request)?;
        let deadline = Instant::now() + Duration::from_millis(request.timeout_ms);
        let policy = || {
            if cancel.is_cancelled() {
                return Err(error("cancelled"));
            }
            if Instant::now() >= deadline {
                return Err(error("timed_out"));
            }
            check()
        };
        policy()?;
        let current = self.execution_preflight(id, request)?;
        if current.fingerprint != expected.fingerprint {
            return Err(error("execution_context_changed"));
        }
        policy()?;
        let context = Context::resolve(
            &self.projects,
            id,
            &self.catalog,
            &self.packages,
            self.secrets.as_ref(),
        )?;
        policy()?;
        if self.execution_preflight(id, request)?.fingerprint != expected.fingerprint {
            return Err(error("execution_context_changed"));
        }
        let mut invocation = context.invocation(
            request.tool.native(),
            request.args.iter().map(Into::into).collect(),
            &self.catalog,
            &self.packages,
        )?;
        invocation.redact_supplied_arguments(&request.args)?;
        invocation.limit_captured_output(16384)?;
        let original = RefCell::new(None);
        let guard = || {
            policy().map_err(|error| {
                *original.borrow_mut() = Some(error);
                devforge_projects::ProjectError::ExecutionFailed
            })
        };
        let result = invocation
            .run_cancellable_checked_timeout(
                self.projects.root().join("managed-execution-logs"),
                cancel,
                Duration::from_millis(request.timeout_ms),
                &guard,
            )
            .map_err(|error| original.borrow_mut().take().unwrap_or_else(|| error.into()))?;
        let total = result
            .logs
            .iter()
            .map(|chunk| chunk.text.len() as u64)
            .sum::<u64>();
        let mut budget = 16384usize;
        loop {
            let mut left = budget;
            let mut included = 0u64;
            let mut logs = Vec::new();
            for chunk in result.logs.iter().take(32) {
                let mut text = String::new();
                for character in chunk.text.chars() {
                    if character.is_control() && !['\n', '\r', '\t'].contains(&character) {
                        continue;
                    }
                    if character.len_utf8() > left {
                        break;
                    }
                    left -= character.len_utf8();
                    included += character.len_utf8() as u64;
                    text.push(character);
                }
                if !text.is_empty() {
                    logs.push(ExecutionLog {
                        stream: chunk.stream.into(),
                        text,
                    });
                }
                if left == 0 {
                    break;
                }
            }
            let output = ExecutionOutput {
                success: result.success,
                exit_code: result.exit_code,
                logs,
                dropped_bytes: result
                    .dropped_bytes
                    .saturating_add(total.saturating_sub(included)),
                truncated: result.dropped_bytes > 0 || included < total,
            };
            if serde_json::to_vec(&output)
                .map_err(|_| error("execution_output_unavailable"))?
                .len()
                <= 32768
            {
                return Ok(output);
            }
            budget /= 2;
            if budget == 0 {
                return Err(error("execution_output_unavailable"));
            }
        }
    }
}
