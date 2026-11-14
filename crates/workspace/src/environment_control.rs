//! Guarded access to the existing native YAML/manifest transaction. Raw plans,
//! document SHA/history and native journals stay inside the manager composition.
use super::*;
use devforge_projects::{execution::ExecutionResult, manifest};
use std::cell::RefCell;

pub(crate) fn policy(cancel: &Cancellation, check: &dyn Fn() -> Result<()>) -> Result<()> {
    if cancel.is_cancelled() {
        return Err(error("setup_cancelled"));
    }
    check()
}
fn project_guard(project: &Project, check: &dyn Fn() -> Result<()>) -> Result<()> {
    check()?;
    if fs::canonicalize(&project.path).ok().as_ref() != Some(&project.path)
        || project
            .filesystem_identity
            .as_ref()
            .is_some_and(|identity| {
                devforge_projects::discovery::identity(&project.path)
                    .ok()
                    .as_ref()
                    != Some(identity)
            })
    {
        return Err(error("discovery_unavailable"));
    }
    Ok(())
}

pub(crate) fn run_checked(
    invocation: &Invocation,
    logs: impl AsRef<Path>,
    cancel: &Cancellation,
    check: &dyn Fn() -> Result<()>,
) -> Result<ExecutionResult> {
    let original = RefCell::new(None);
    let callback = || {
        check().map_err(|error| {
            *original.borrow_mut() = Some(error);
            devforge_projects::ProjectError::ExecutionFailed
        })
    };
    invocation
        .run_cancellable_checked(logs, cancel, &callback)
        .map_err(|error| original.borrow_mut().take().unwrap_or_else(|| error.into()))
}

pub(crate) fn download_checked(
    packages: &devforge_runtime::package::PackageStore,
    descriptor: &devforge_runtime::package::Package,
    cancel: &Cancellation,
    check: &dyn Fn() -> Result<()>,
) -> Result<()> {
    use devforge_runtime::package::InstallStage;
    let original = RefCell::new(None);
    packages
        .download(descriptor, cancel, |progress| {
            if matches!(
                progress.stage,
                InstallStage::Installed | InstallStage::Failed | InstallStage::Cancelled
            ) || cancel.is_cancelled()
                || original.borrow().is_some()
            {
                return;
            }
            if let Err(error) = check() {
                *original.borrow_mut() = Some(error);
                cancel.cancel();
            }
        })
        .map_err(|error| original.borrow_mut().take().unwrap_or_else(|| error.into()))?;
    Ok(())
}

impl Workspace {
    /// Private actual SHA for opaque manager version mapping. Does not parse the
    /// old document, create manifest state, read secrets or synthesize fallback.
    pub fn environment_document_version(&self, id: &str) -> Result<(bool, Option<String>)> {
        let project = self.available_project(id)?;
        let path = project.path.join("devforge.yaml");
        match fs::symlink_metadata(&path) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok((false, None)),
            Err(_) => Err(error("storage_unavailable")),
            Ok(_) => {
                let bytes = manifest::read_file(&path, 65536)?;
                Ok((true, Some(crate::configuration::hash(&bytes))))
            }
        }
    }

    /// Full existing private Plan, including its actual native digest. The
    /// manager must project/mask it before any external response or cache.
    pub fn environment_plan_guarded(
        &self,
        id: &str,
        write_env: bool,
        check: &dyn Fn() -> Result<()>,
    ) -> Result<repro::Plan> {
        check()?;
        let project = self.available_project(id)?;
        project_guard(&project, check)?;
        let plan = self.manifest_plan(&project.path, write_env)?;
        project_guard(&project, check)?;
        if plan.root != project.path || plan.project_id.as_deref() != Some(id) {
            return Err(error("manifest_review_changed"));
        }
        Ok(plan)
    }

    /// Native consent/trust is supplied only by the already-authorized adapter.
    /// The existing compound apply journal/history is deliberately retained.
    pub fn execute_environment_action(
        &mut self,
        command: Command,
        cancel: &Cancellation,
        check: &dyn Fn() -> Result<()>,
    ) -> Result<serde_json::Value> {
        match command {
            Command::ConfigurationSave {
                id,
                expected,
                text,
                consent,
            } => {
                policy(cancel, check)?;
                let project = self.available_project(&id)?;
                let guarded = || {
                    policy(cancel, check)?;
                    project_guard(&project, &|| Ok(()))
                };
                // Do not let native raw text/document hashes escape into the
                // caller's result cache. Native configuration_save stays intact.
                let _private_document = self.configuration_save_checked(
                    &id,
                    expected.as_deref(),
                    &text,
                    consent,
                    &guarded,
                )?;
                Ok(serde_json::json!({"saved":true}))
            }
            Command::ConfigurationApply {
                id,
                review,
                trusted,
                write_env,
            } => {
                policy(cancel, check)?;
                self.require_stopped(&id)?;
                let project = self.available_project(&id)?;
                let guarded = || {
                    policy(cancel, check)?;
                    project_guard(&project, &|| Ok(()))
                };
                let journal = self.manifest_setup_checked(
                    &project.path,
                    &review,
                    write_env,
                    trusted,
                    cancel,
                    &guarded,
                )?;
                Ok(
                    serde_json::json!({"applied":true,"completed":journal.completed,"stage":journal.stage,
                    "warnings":["environment_partial_effects_possible"]}),
                )
            }
            _ => Err(error("environment_action_unsupported")),
        }
    }
}
