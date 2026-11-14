//! Narrow external runtime installation and PHP/Node selection. Native commands
//! remain unchanged; this boundary never removes packages or changes other fields.
use crate::{Command, Error, Result, Snapshot, Workspace, error};
use devforge_projects::project::Project;
use devforge_runtime::{
    Cancellation,
    package::{InstallStage, Target},
};
use devforge_serving::site::Site;
use std::cell::{Cell, RefCell};

struct Selection<'a> {
    id: &'a str,
    revision: u64,
    name: &'a str,
    php: &'a str,
    node: &'a str,
    domain: &'a str,
    https: bool,
    vite: bool,
    queue: bool,
}
fn selection(command: &Command) -> Result<Selection<'_>> {
    match command {
        Command::Configure {
            id,
            revision,
            name,
            php,
            node,
            domain,
            https,
            vite,
            queue,
        } => Ok(Selection {
            id,
            revision: *revision,
            name,
            php,
            node,
            domain,
            https: *https,
            vite: *vite,
            queue: *queue,
        }),
        _ => Err(error("runtime_action_unsupported")),
    }
}
fn refused(id: &str, code: &str) -> Error {
    Error {
        scope: "workspace",
        code: code.into(),
        project_id: Some(id.into()),
    }
}
fn preserved(
    input: &Selection<'_>,
    project: &Project,
    site: Option<&Site>,
    queue: bool,
) -> Result<()> {
    if project.revision != input.revision {
        return Err(devforge_projects::ProjectError::StaleRevision.into());
    }
    let site = site.ok_or_else(|| refused(input.id, "runtime_selection_precondition_missing"))?;
    if project.id != input.id
        || project.name != input.name
        || site.project_id != input.id
        || site.domain != input.domain
        || site.https != input.https
        || site.vite != input.vite
        || queue != input.queue
    {
        return Err(refused(input.id, "runtime_selection_fields_changed"));
    }
    if project.debugger.mode != devforge_debugger::Mode::Off && project.php != input.php {
        return Err(devforge_projects::ProjectError::DebugRuntimeChange.into());
    }
    Ok(())
}

/// Advisory only: current nonselection fields/revision and projected cached
/// candidate bytes. Actual constraints, catalog identity/integrity and debugger
/// policy are revalidated live by the worker before stopping any session.
pub fn validate_selection_snapshot(snapshot: &Snapshot, command: &Command) -> Result<()> {
    let input = selection(command)?;
    if snapshot.projects.len() > 2048 || snapshot.runtimes.len() > 2048 {
        return Err(refused(input.id, "runtime_selection_precondition_missing"));
    }
    let mut rows = snapshot
        .projects
        .iter()
        .filter(|row| row.project.id == input.id && row.project.active);
    let row = rows
        .next()
        .ok_or_else(|| refused(input.id, "runtime_selection_precondition_missing"))?;
    if rows.next().is_some() {
        return Err(refused(input.id, "runtime_selection_precondition_missing"));
    }
    preserved(
        &input,
        &row.project,
        row.site.as_ref(),
        row.development.queue,
    )?;
    let cached = |id: &str, kind: &str| -> Result<&crate::Runtime> {
        let mut candidates = snapshot.runtimes.iter().filter(|runtime| runtime.id == id);
        let runtime = candidates
            .next()
            .ok_or_else(|| refused(input.id, "runtime_selection_precondition_missing"))?;
        if candidates.next().is_some()
            || runtime.kind != kind
            || !runtime.installed
            || runtime.problem.is_some()
        {
            return Err(refused(input.id, "runtime_selection_precondition_missing"));
        }
        Ok(runtime)
    };
    let php = cached(input.php, "php")?;
    cached(input.node, "node")?;
    if !cfg!(windows)
        && !snapshot.runtimes.iter().any(|runtime| {
            runtime.kind == "php"
                && runtime.version == php.version
                && runtime.variant == "fpm"
                && runtime.installed
                && runtime.problem.is_none()
        })
    {
        return Err(refused(input.id, "runtime_selection_precondition_missing"));
    }
    Ok(())
}

fn policy(cancel: &Cancellation, check: &dyn Fn() -> Result<()>) -> Result<()> {
    if cancel.is_cancelled() {
        return Err(error("cancelled"));
    }
    check()
}

impl Workspace {
    fn current_selection<'a>(&self, command: &'a Command) -> Result<(Selection<'a>, Project)> {
        let input = selection(command)?;
        let project = self.available_project(input.id)?;
        let sites = self.sites.sites()?;
        let site = sites.iter().find(|site| site.project_id == input.id);
        let queue = self
            .preferences
            .development
            .get(input.id)
            .is_some_and(|development| development.queue);
        preserved(&input, &project, site, queue)?;
        Ok((input, project))
    }

    /// Exact cached candidate checks and the existing debugger compatibility
    /// guard, performed before any stop. No probes, downloads or secret reads.
    pub fn runtime_selection_preflight(&self, command: &Command) -> Result<()> {
        let (input, project) = self.current_selection(command)?;
        project.validate_configuration(input.name, input.php, input.node, &self.catalog)?;
        let cached = (|| -> Result<()> {
            let php = self.catalog.get(input.php)?;
            let node = self.catalog.get(input.node)?;
            self.packages.executable(php, "php")?;
            self.packages.executable(node, "node")?;
            let web = self.catalog.php_server(php)?;
            self.packages
                .executable(web, if cfg!(windows) { "php-cgi" } else { "php-fpm" })?;
            Ok(())
        })();
        cached.map_err(|_| refused(input.id, "runtime_selection_precondition_missing"))
    }

    /// Installation affects the shared immutable cache, never project pins.
    /// Selection may stop this project and persist new pins before restart fails
    /// or policy is revoked. Such failures do not claim a rollback. Existing
    /// network request waits have cancellation-token polling; policy callbacks
    /// run at download progress/stages, not inside a silent pending HTTP request.
    pub fn execute_runtime_action(
        &mut self,
        command: Command,
        cancel: &Cancellation,
        check: &dyn Fn() -> Result<()>,
    ) -> Result<serde_json::Value> {
        match &command {
            Command::Install { package } => {
                policy(cancel, check)?;
                let descriptor = self.catalog.get(package)?.clone();
                descriptor.key()?;
                if descriptor.target != Target::current()? {
                    return Err(devforge_runtime::RuntimeError::UnsupportedTarget.into());
                }
                if self.catalog.superseded.contains_key(package)
                    && !self.packages.is_present(&descriptor)
                {
                    return Err(devforge_runtime::RuntimeError::PackageSuperseded.into());
                }
                policy(cancel, check)?;
                let check_error = RefCell::new(None);
                let installed = self
                    .packages
                    .download(&descriptor, cancel, |progress| {
                        // Installed is after the immutable activation commit. Do not
                        // turn a completed cache install into a late cancellation.
                        if matches!(
                            progress.stage,
                            InstallStage::Installed
                                | InstallStage::Failed
                                | InstallStage::Cancelled
                        ) || cancel.is_cancelled()
                            || check_error.borrow().is_some()
                        {
                            return;
                        }
                        if let Err(error) = check() {
                            *check_error.borrow_mut() = Some(error);
                            cancel.cancel();
                        }
                    })
                    .map_err(|error| {
                        check_error
                            .borrow_mut()
                            .take()
                            .unwrap_or_else(|| error.into())
                    })?;
                self.refresh_runtimes()?;
                Ok(
                    serde_json::json!({"installed":true,"package_id":package,"archive_sha256":descriptor.sha256,
                    "reused":installed.reused,"warnings":["runtime_shared_cache_effect"]}),
                )
            }
            Command::Configure { .. } => {
                policy(cancel, check)?;
                self.runtime_selection_preflight(&command)?;
                policy(cancel, check)?;
                let (input, project) = self.current_selection(&command)?;
                if project.php == input.php && project.node == input.node {
                    return Ok(serde_json::json!({"saved":true,"changed":false,"restarted":false}));
                }
                let running = self.sessions.contains_key(input.id);
                policy(cancel, check)?;
                self.stop(input.id)?;
                policy(cancel, check)?;
                self.projects.configure(
                    input.id,
                    input.revision,
                    &project.name,
                    input.php,
                    input.node,
                    &self.catalog,
                )?;
                if running {
                    policy(cancel, check)?;
                    let policy_failed = Cell::new(false);
                    let restart_check =
                        || policy(cancel, check).inspect_err(|_| policy_failed.set(true));
                    if let Err(error) = self.start_checked(input.id, &restart_check) {
                        return Err(if policy_failed.get() {
                            error
                        } else {
                            refused(input.id, "runtime_selection_applied_restart_failed")
                        });
                    }
                }
                // No check after the successful selection/restart commit.
                Ok(
                    serde_json::json!({"saved":true,"changed":true,"restarted":running,
                    "warnings":["runtime_selection_partial_effects_possible"]}),
                )
            }
            _ => Err(error("runtime_action_unsupported")),
        }
    }
}
