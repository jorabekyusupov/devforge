//! Guarded access to the existing native YAML/manifest transaction. Raw plans,
//! document SHA/history and native journals stay inside the manager composition.
use super::*;
use devforge_projects::{execution::ExecutionResult, manifest};
use std::cell::RefCell;

/// Desired semantics of the candidate only, not a baseline diff or approval.
/// Explicitly copied fields prevent raw values, aliases, comments and hashes
/// from entering the trusted native owner's review DTO.
#[derive(Clone, Serialize, Deserialize)]
pub struct YamlNativeReviewSummary {
    pub runtimes: YamlRuntimeSelectors,
    pub domain: String,
    pub https: bool,
    pub vite: bool,
    pub queue: bool,
    pub database: Option<YamlDatabaseSelector>,
    pub redis: Option<YamlRedisSelector>,
    pub cache_driver: devforge_projects::cache::CacheDriver,
    pub queue_driver: devforge_projects::cache::QueueDriver,
    pub mail: Option<bool>,
    pub preparation: YamlPreparationSummary,
    pub environment_keys: Vec<String>,
    pub environment_key_count: usize,
    pub pin_count: usize,
    pub php_setting_names: Vec<String>,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct YamlRuntimeSelectors {
    pub php: String,
    pub node: String,
    pub composer: String,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct YamlDatabaseSelector {
    pub engine: devforge_projects::database::Engine,
    pub version: String,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct YamlRedisSelector {
    pub version: String,
    pub dotnet: String,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct YamlPreparationSummary {
    pub composer: bool,
    pub npm: bool,
    pub create_env: bool,
    pub generate_key: bool,
    pub predis: bool,
}

pub fn yaml_native_review_summary(candidate: &str) -> Result<YamlNativeReviewSummary> {
    let manifest = manifest::Manifest::parse(candidate.as_bytes())?;
    let mut php_setting_names = Vec::new();
    for (name, present) in [
        ("memory_limit", manifest.php_settings.memory_limit.is_some()),
        ("upload_mb", manifest.php_settings.upload_mb.is_some()),
        ("post_mb", manifest.php_settings.post_mb.is_some()),
        (
            "execution_seconds",
            manifest.php_settings.execution_seconds.is_some(),
        ),
        ("timezone", manifest.php_settings.timezone.is_some()),
    ] {
        if present {
            php_setting_names.push(name.into());
        }
    }
    Ok(YamlNativeReviewSummary {
        runtimes: YamlRuntimeSelectors {
            php: manifest.runtimes.php,
            node: manifest.runtimes.node,
            composer: manifest.runtimes.composer,
        },
        domain: manifest.project.domain,
        https: manifest.project.https,
        vite: manifest.project.vite,
        queue: manifest.project.queue,
        database: manifest.database.map(|database| YamlDatabaseSelector {
            engine: database.engine,
            version: database.version,
        }),
        redis: manifest.redis.map(|redis| YamlRedisSelector {
            version: redis.version,
            dotnet: redis.dotnet,
        }),
        cache_driver: manifest.services.cache,
        queue_driver: manifest.services.queue,
        mail: manifest.mail,
        preparation: YamlPreparationSummary {
            composer: manifest.setup.composer,
            npm: manifest.setup.npm,
            create_env: manifest.setup.create_env,
            generate_key: manifest.setup.generate_key,
            predis: manifest.setup.predis,
        },
        environment_key_count: manifest.environment.len(),
        environment_keys: manifest.environment.into_keys().collect(),
        pin_count: manifest.pins.len(),
        php_setting_names,
    })
}

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

#[cfg(test)]
mod native_yaml_summary_tests {
    use super::*;
    #[test]
    fn candidate_semantics_use_native_parser_without_echoing_private_values_names_or_hashes() {
        let manifest:manifest::Manifest=serde_json::from_value(serde_json::json!({"schema":1,
            "project":{"name":"PRIVATE_PROJECT_NAME_CANARY","domain":"review.devforge.test","https":true,"vite":true,"queue":true},
            "runtimes":{"php":"8.5.12","node":"24.22.2","composer":"2.11.2"},
            "database":{"engine":"mysql","version":"8.4.11"},"redis":{"version":"1.1.0","dotnet":"10.0.1"},
            "services":{"cache":"redis","queue":"redis"},"mail":true,
            "php_settings":{"memory_limit":"256M","timezone":"PRIVATE_TIMEZONE_CANARY"},
            "setup":{"composer":true,"npm":true,"create_env":true,"generate_key":true,"predis":true},
            "environment":{"PRIVATE_API_TOKEN":"private_alias_canary"},"pins":{"private-package-canary":"a".repeat(64)}})).unwrap();
        let candidate = format!("{}\n# PRIVATE_COMMENT_CANARY\n", manifest.yaml().unwrap());
        let summary = yaml_native_review_summary(&candidate).unwrap();
        assert_eq!(summary.domain, "review.devforge.test");
        assert_eq!(summary.runtimes.php, "8.5.12");
        assert!(summary.https && summary.vite && summary.queue && summary.preparation.generate_key);
        assert_eq!(summary.environment_keys, vec!["PRIVATE_API_TOKEN"]);
        assert_eq!(summary.environment_key_count, 1);
        assert_eq!(summary.pin_count, 1);
        assert_eq!(summary.php_setting_names, vec!["memory_limit", "timezone"]);
        let value = serde_json::to_value(&summary).unwrap();
        let encoded = value.to_string();
        for private in [
            "PRIVATE_PROJECT_NAME_CANARY",
            "PRIVATE_TIMEZONE_CANARY",
            "private_alias_canary",
            "private-package-canary",
            "PRIVATE_COMMENT_CANARY",
            "256M",
            &"a".repeat(64),
        ] {
            assert!(!encoded.contains(private));
        }
        let decoded: YamlNativeReviewSummary = serde_json::from_value(value).unwrap();
        assert_eq!(decoded.runtimes.composer, "2.11.2");
        assert!(yaml_native_review_summary("schema: invalid\n").is_err());
    }
}
