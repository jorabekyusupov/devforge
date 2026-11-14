//! Expected/repository versus registered/local metadata. No setup plan, secret
//! fetch, runtime probe, download, environment write or service start is performed.
use crate::{
    Result, Workspace,
    catalog::{OfflineLimits, OfflineReport},
    error,
};
use devforge_projects::{manifest::Manifest, project::Project, storage_metadata};
use devforge_runtime::package::Target;
use serde::Serialize;
use std::{collections::BTreeSet, fs, io::Read, path::Path};

#[derive(Clone, Copy)]
pub struct ReportLimits {
    pub max_registry_rows: usize,
    pub max_facts: usize,
    pub max_file_bytes: usize,
    pub max_total_file_bytes: usize,
    pub max_extensions: usize,
}
impl Default for ReportLimits {
    fn default() -> Self {
        Self {
            max_registry_rows: 2048,
            max_facts: 128,
            max_file_bytes: 65536,
            max_total_file_bytes: 262144,
            max_extensions: 128,
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Difference {
    Match,
    Mismatch,
    Missing,
    NotApplicable,
    Unverified,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Support {
    Supported,
    Unsupported,
    Unverified,
}
#[derive(Debug, Serialize)]
pub struct Comparison {
    pub key: String,
    pub expected: Option<String>,
    pub actual: Option<String>,
    pub state: Difference,
    pub reason: &'static str,
    pub next_action: &'static str,
}
#[derive(Debug, Serialize)]
pub struct Portability {
    pub target: Target,
    pub catalog_support: Support,
    pub native_support: Support,
    pub scope: &'static str,
    pub reason: &'static str,
}
#[derive(Debug, Serialize)]
pub struct SecretRequirement {
    pub environment: String,
    pub reference_alias: String,
    pub registered_name: bool,
    pub availability: Support,
    pub reason: &'static str,
}
#[derive(Debug, Serialize)]
pub struct ExtensionRequirement {
    pub extension: String,
    pub cached_present: Option<bool>,
    pub current_support: Support,
    pub evidence: &'static str,
}
#[derive(Debug, Serialize)]
pub struct EnvironmentReport {
    pub schema: u32,
    pub project_id: String,
    pub project_revision: Option<u64>,
    pub scope: &'static str,
    pub snapshot_semantics: &'static str,
    pub matches_recorded_configuration: Option<bool>,
    pub environment_ready: Option<bool>,
    pub manifest_sha256: Option<String>,
    pub comparisons: Vec<Comparison>,
    pub portability: Vec<Portability>,
    pub secrets: Vec<SecretRequirement>,
    pub extensions: Vec<ExtensionRequirement>,
    pub artifact_evidence: OfflineReport,
    pub problems: Vec<&'static str>,
    pub file_bytes_read: usize,
}
impl EnvironmentReport {
    fn problem(&mut self, code: &'static str) {
        if !self.problems.contains(&code) {
            self.problems.push(code);
        }
    }
    fn compare(
        &mut self,
        limits: ReportLimits,
        key: impl Into<String>,
        expected: Option<String>,
        actual: Option<String>,
    ) {
        if self.comparisons.len() >= limits.max_facts {
            self.problem("comparison_limit");
            return;
        }
        if expected.as_ref().is_some_and(|v| v.len() > 256)
            || actual.as_ref().is_some_and(|v| v.len() > 256)
        {
            self.problem("comparison_value_limit");
            self.comparisons.push(Comparison {
                key: key.into(),
                expected: None,
                actual: None,
                state: Difference::Unverified,
                reason: "bounded_metadata_value_unavailable",
                next_action: "review_project_configuration",
            });
            return;
        }
        let (state, reason, next_action) = match (&expected, &actual) {
            (None, None) => (
                Difference::NotApplicable,
                "no_explicit_setting_on_either_side",
                "none",
            ),
            (Some(a), Some(b)) if a == b => (
                Difference::Match,
                "matches_registered_metadata_not_runtime_verification",
                "none",
            ),
            (Some(_), None) => (
                Difference::Missing,
                "declared_setting_not_registered",
                "review_environment_plan",
            ),
            _ => (
                Difference::Mismatch,
                "repository_and_registered_metadata_differ",
                "review_environment_plan",
            ),
        };
        self.comparisons.push(Comparison {
            key: key.into(),
            expected,
            actual,
            state,
            reason,
            next_action,
        });
    }
}
fn linked(meta: &fs::Metadata) -> bool {
    if meta.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if meta.file_attributes() & 0x400 != 0 {
            return true;
        }
    }
    false
}
fn safe_root(project: &Project) -> bool {
    project.path.as_os_str().len() <= 4096
        && fs::symlink_metadata(&project.path).is_ok_and(|m| m.is_dir() && !linked(&m))
        && fs::canonicalize(&project.path).is_ok_and(|p| p == project.path)
        && project
            .filesystem_identity
            .as_ref()
            .is_some_and(|expected| {
                devforge_projects::discovery::identity(&project.path)
                    .is_ok_and(|actual| &actual == expected)
            })
}
fn read(
    path: &Path,
    limits: ReportLimits,
    total: &mut usize,
) -> std::result::Result<Vec<u8>, &'static str> {
    let cap = limits
        .max_file_bytes
        .min(limits.max_total_file_bytes.saturating_sub(*total));
    let meta = fs::symlink_metadata(path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            "metadata_file_missing"
        } else {
            "metadata_file_unavailable"
        }
    })?;
    if !meta.is_file() || linked(&meta) {
        return Err("metadata_file_not_regular");
    }
    if cap == 0 || meta.len() > cap as u64 {
        return Err("metadata_file_byte_limit");
    }
    let mut bytes = vec![];
    crate::storage::open_metadata_file(path, cap as u64)
        .ok_or("metadata_file_open_unverified")?
        .take(cap as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "metadata_file_read_unavailable")?;
    *total = total.saturating_add(bytes.len());
    if bytes.len() > cap {
        return Err("metadata_file_changed_or_limit");
    }
    Ok(bytes)
}
fn version(workspace: &Workspace, package: &str) -> Option<String> {
    // Same logical-family semantics as Manifest::resolve; the exact package ID
    // comparison and OfflineReport retain publisher/rebuild identity separately.
    workspace
        .catalog
        .get(package)
        .ok()
        .map(|p| {
            if p.kind == "php" {
                p.version.split('-').next().unwrap_or(&p.version).into()
            } else {
                p.version.trim_start_matches('v').to_string()
            }
        })
        .filter(|v: &String| v.len() <= 256)
}
fn extension_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"_-".contains(&c))
}
fn settings(
    settings: &devforge_projects::php_settings::PhpSettings,
) -> Vec<(&'static str, Option<String>)> {
    vec![
        ("php_settings.memory_limit", settings.memory_limit.clone()),
        (
            "php_settings.upload_mb",
            settings.upload_mb.map(|v| v.to_string()),
        ),
        (
            "php_settings.post_mb",
            settings.post_mb.map(|v| v.to_string()),
        ),
        (
            "php_settings.execution_seconds",
            settings.execution_seconds.map(|v| v.to_string()),
        ),
        ("php_settings.timezone", settings.timezone.clone()),
    ]
}
fn extension_evidence(
    required: BTreeSet<String>,
    cache: Option<&serde_json::Value>,
    max: usize,
    capacity: usize,
) -> Vec<ExtensionRequirement> {
    let cached = cache
        .and_then(|v| v.get("extensions"))
        .and_then(|v| v.as_array())
        .filter(|v| {
            v.len() <= max
                && v.iter().all(|v| {
                    v.as_str().is_some_and(|name| {
                        !name.is_empty() && name.len() <= 64 && !name.chars().any(char::is_control)
                    })
                })
        });
    required
        .into_iter()
        .take(capacity)
        .map(|extension| {
            let observed = cached.map(|values| {
                values.iter().any(|v| {
                    v.as_str()
                        .is_some_and(|v| v.eq_ignore_ascii_case(&extension))
                })
            });
            ExtensionRequirement {
                extension,
                cached_present: observed,
                current_support: Support::Unverified,
                evidence: if cached.is_some() {
                    "cached_php_diagnostics_without_timestamp_or_revision_not_current_verification"
                } else {
                    "php_diagnostics_not_run"
                },
            }
        })
        .collect()
}
impl Workspace {
    pub fn environment_report(&self, id: &str, limits: ReportLimits) -> Result<EnvironmentReport> {
        if !devforge_inspector::valid_id(id) {
            return Err(error("invalid_project"));
        }
        if limits.max_registry_rows == 0
            || limits.max_registry_rows > 4096
            || limits.max_facts == 0
            || limits.max_facts > 256
            || limits.max_file_bytes == 0
            || limits.max_file_bytes > 65536
            || limits.max_total_file_bytes == 0
            || limits.max_total_file_bytes > 262144
            || limits.max_extensions == 0
            || limits.max_extensions > 128
        {
            return Err(error("environment_report_limits_invalid"));
        }
        // Reuse E07 metadata evidence. It does not fetch credentials or probe tools.
        let artifacts = self.offline_readiness(
            id,
            OfflineLimits {
                max_registry_rows: limits.max_registry_rows,
                ..Default::default()
            },
        )?;
        let mut report = EnvironmentReport {
            schema: 1,
            project_id: id.into(),
            project_revision: artifacts.project_revision,
            scope: "repository_expected_vs_registered_configuration_and_bounded_metadata_only",
            snapshot_semantics: "non_atomic_no_project_writes_no_secret_queries_no_native_probes",
            matches_recorded_configuration: None,
            environment_ready: artifacts.offline_ready,
            manifest_sha256: None,
            comparisons: vec![],
            portability: vec![],
            secrets: vec![],
            extensions: vec![],
            artifact_evidence: artifacts,
            problems: vec![],
            file_bytes_read: 0,
        };
        let snapshot = self.projects.storage_metadata(storage_metadata::Limits {
            max_rows: limits.max_registry_rows,
            ..Default::default()
        });
        if !snapshot.complete {
            for code in snapshot.problems {
                report.problem(code);
            }
        }
        let Some(project) = snapshot.projects.iter().find(|p| p.id == id) else {
            report.problem("project_metadata_incomplete");
            return Ok(report);
        };
        if report.project_revision != Some(project.revision) {
            report.problem("registry_changed_between_metadata_reads");
            return Ok(report);
        }
        if !safe_root(project) {
            report.problem("project_identity_unverified_no_repository_files_visited");
            return Ok(report);
        }
        let mut bytes = 0;
        let document = match read(&project.path.join("devforge.yaml"), limits, &mut bytes) {
            Ok(v) => v,
            Err(code) => {
                report.problem(code);
                report.file_bytes_read = bytes;
                return Ok(report);
            }
        };
        let manifest = match Manifest::parse(&document) {
            Ok(m) => m,
            Err(_) => {
                report.problem("manifest_invalid");
                report.file_bytes_read = bytes;
                return Ok(report);
            }
        };
        {
            use sha2::{Digest, Sha256};
            report.manifest_sha256 = Some(
                Sha256::digest(&document)
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect(),
            );
        }
        for target in [Target::MacosArm64, Target::MacosX64, Target::WindowsX64] {
            let (catalog_support, reason) = if manifest.resolve(&self.catalog, target).is_err() {
                (
                    Support::Unsupported,
                    "exact_runtime_family_unavailable_for_target",
                )
            } else if manifest.verify_pins(&self.catalog, target).is_err() {
                (
                    Support::Unverified,
                    "manifest_pin_digest_does_not_match_catalog",
                )
            } else {
                (Support::Supported, "exact_catalog_target_resolution_only")
            };
            report.portability.push(Portability{target,catalog_support,native_support:Support::Unverified,
                scope:"catalog_descriptors_only_os_edition_prerequisites_native_execution_unverified",reason});
        }
        let resolved = Target::current()
            .ok()
            .and_then(|target| manifest.resolve(&self.catalog, target).ok());
        let pin_valid = Target::current()
            .ok()
            .is_some_and(|target| manifest.verify_pins(&self.catalog, target).is_ok());
        if resolved.is_none() {
            report.problem("manifest_exact_runtime_unavailable_for_current_target");
            report.environment_ready = Some(false);
        } else if !pin_valid {
            report.problem("manifest_pin_digest_mismatch");
            report.environment_ready = Some(false);
        }
        report.compare(
            limits,
            "project.name",
            Some(manifest.project.name.clone()),
            Some(project.name.clone()),
        );
        for environment in manifest.environment.keys() {
            report.compare(
                limits,
                format!("environment.{environment}.registered_name"),
                Some("true".into()),
                Some(project.environment_names.contains(environment).to_string()),
            );
        }
        for (role, expected_version, selected) in [
            ("php", &manifest.runtimes.php, &project.php),
            ("node", &manifest.runtimes.node, &project.node),
            ("composer", &manifest.runtimes.composer, &project.composer),
        ] {
            report.compare(
                limits,
                format!("runtimes.{role}.version"),
                Some(expected_version.clone()),
                version(self, selected),
            );
            if let Some(packages) = &resolved {
                report.compare(
                    limits,
                    format!("runtimes.{role}.package"),
                    packages.get(role).cloned(),
                    Some(selected.clone()),
                );
            }
        }
        let binding = snapshot.active_bindings.iter().find(|b| b.project_id == id);
        let instance =
            binding.and_then(|b| snapshot.instances.iter().find(|i| i.id == b.instance_id));
        report.compare(
            limits,
            "database.engine",
            manifest.database.as_ref().map(|d| d.engine.driver().into()),
            instance.map(|i| i.engine.driver().into()),
        );
        report.compare(
            limits,
            "database.version",
            manifest.database.as_ref().map(|d| d.version.clone()),
            instance.and_then(|i| version(self, &i.package)),
        );
        if binding.is_some() && instance.is_none() {
            report.problem("database_binding_instance_unverified");
        }
        let cache_name = |driver| match driver {
            devforge_projects::cache::CacheDriver::Project => "project",
            devforge_projects::cache::CacheDriver::File => "file",
            devforge_projects::cache::CacheDriver::Database => "database",
            devforge_projects::cache::CacheDriver::Redis => "redis",
        };
        let queue_name = |driver| match driver {
            devforge_projects::cache::QueueDriver::Project => "project",
            devforge_projects::cache::QueueDriver::Sync => "sync",
            devforge_projects::cache::QueueDriver::Database => "database",
            devforge_projects::cache::QueueDriver::Redis => "redis",
        };
        report.compare(
            limits,
            "cache.driver",
            Some(cache_name(manifest.services.cache).into()),
            Some(cache_name(project.services.cache).into()),
        );
        report.compare(
            limits,
            "queue.driver",
            Some(queue_name(manifest.services.queue).into()),
            Some(queue_name(project.services.queue).into()),
        );
        let cache = project
            .services
            .redis_instance
            .as_ref()
            .and_then(|id| snapshot.caches.iter().find(|c| &c.id == id));
        report.compare(
            limits,
            "cache.version",
            manifest.redis.as_ref().map(|r| r.version.clone()),
            cache.and_then(|c| version(self, &c.package)),
        );
        report.compare(
            limits,
            "cache.runtime_version",
            manifest.redis.as_ref().map(|r| r.dotnet.clone()),
            cache.and_then(|c| version(self, &c.runtime)),
        );
        let serving_root = self.sites.root();
        let serving_safe = fs::symlink_metadata(serving_root)
            .is_ok_and(|m| m.is_dir() && !linked(&m))
            && fs::canonicalize(serving_root).is_ok_and(|p| p == serving_root);
        if serving_safe {
            match read(&serving_root.join("sites.json"), limits, &mut bytes) {
                Ok(doc) => {
                    #[derive(serde::Deserialize)]
                    struct Sites {
                        schema: u32,
                        sites: Vec<devforge_serving::site::Site>,
                    }
                    match serde_json::from_slice::<Sites>(&doc) {
                        Ok(sites)
                            if sites.schema == 1
                                && sites
                                    .sites
                                    .iter()
                                    .all(|s| devforge_serving::site::domain(&s.domain).is_ok()) =>
                        {
                            let matching = sites
                                .sites
                                .iter()
                                .filter(|s| s.project_id == id)
                                .collect::<Vec<_>>();
                            if matching.len() > 1 {
                                report.problem("site_registry_identity_conflict");
                            } else {
                                let site = matching.first();
                                report.compare(
                                    limits,
                                    "site.domain",
                                    Some(manifest.project.domain.clone()),
                                    site.map(|s| s.domain.clone()),
                                );
                                report.compare(
                                    limits,
                                    "site.https",
                                    Some(manifest.project.https.to_string()),
                                    site.map(|s| s.https.to_string()),
                                );
                                report.compare(
                                    limits,
                                    "site.vite",
                                    Some(manifest.project.vite.to_string()),
                                    site.map(|s| s.vite.to_string()),
                                );
                            }
                        }
                        _ => report.problem("site_registry_invalid"),
                    }
                }
                Err(code) => report.problem(code),
            }
        } else {
            report.problem("site_registry_root_unverified");
        }
        report.compare(
            limits,
            "development.queue",
            Some(manifest.project.queue.to_string()),
            Some(
                self.preferences
                    .development
                    .get(id)
                    .is_some_and(|p| p.queue)
                    .to_string(),
            ),
        );
        report.compare(
            limits,
            "mail.enabled",
            Some(manifest.mail.unwrap_or(project.mail).to_string()),
            Some(project.mail.to_string()),
        );
        if project.php_settings.validate().is_ok() {
            for ((key, expected), (_, actual)) in settings(&manifest.php_settings)
                .into_iter()
                .zip(settings(&project.php_settings))
            {
                report.compare(limits, key, expected, actual);
            }
        } else {
            report.problem("registered_php_settings_invalid");
        }
        // Alias registration is not evidence that a credential exists; no SecretStore call.
        let secret_capacity = limits.max_facts.saturating_sub(report.comparisons.len());
        for (environment, alias) in manifest.environment.iter().take(secret_capacity) {
            report.secrets.push(SecretRequirement {
                environment: environment.clone(),
                reference_alias: alias.clone(),
                registered_name: project.environment_names.contains(environment),
                availability: Support::Unverified,
                reason: "credential_availability_not_queried_registered_name_is_metadata_only",
            });
        }
        if manifest.environment.len() > secret_capacity {
            report.problem("secret_requirement_limit");
        }
        let mut required = BTreeSet::new();
        match read(&project.path.join("composer.json"), limits, &mut bytes) {
            Ok(doc) => match serde_json::from_slice::<serde_json::Value>(&doc) {
                Ok(doc) if doc.is_object() => {
                    for section in ["require", "require-dev"] {
                        if doc.get(section).is_some_and(|v| !v.is_object()) {
                            report.problem("composer_requirement_section_invalid");
                        }
                        if let Some(requirements) = doc.get(section).and_then(|v| v.as_object()) {
                            for name in requirements.keys() {
                                if let Some(extension) = name.strip_prefix("ext-") {
                                    if !extension_name(extension) {
                                        report.problem("extension_requirement_name_invalid");
                                        continue;
                                    }
                                    let extension = extension.to_ascii_lowercase();
                                    if required.len() >= limits.max_extensions
                                        && !required.contains(&extension)
                                    {
                                        report.problem("extension_requirement_limit");
                                        continue;
                                    }
                                    required.insert(extension);
                                }
                            }
                        }
                    }
                }
                _ => report.problem("composer_manifest_invalid"),
            },
            Err(code) => report.problem(code),
        }
        let extension_capacity = limits.max_extensions.min(
            limits
                .max_facts
                .saturating_sub(report.comparisons.len() + report.secrets.len()),
        );
        if required.len() > extension_capacity {
            report.problem("extension_fact_limit");
        }
        report.extensions = extension_evidence(
            required,
            self.diagnostics.get(id),
            limits.max_extensions,
            extension_capacity,
        );
        if !snapshot.complete {
            for fact in &mut report.comparisons {
                if (fact.key.starts_with("database.")
                    || matches!(fact.key.as_str(), "cache.version" | "cache.runtime_version"))
                    && fact.actual.is_none()
                {
                    fact.state = Difference::Unverified;
                    fact.reason = "registry_snapshot_incomplete_absence_not_established";
                    fact.next_action = "refresh_complete_registry_metadata";
                }
            }
        }
        report.file_bytes_read = bytes;
        report.problem("secret_availability_not_queried");
        report.problem("transitive_lock_extension_requirements_and_platform_check_unverified");
        report.problem("database_service_connection_dns_https_and_os_edition_not_probed");
        if report
            .comparisons
            .iter()
            .any(|c| matches!(c.state, Difference::Mismatch | Difference::Missing))
        {
            report.matches_recorded_configuration = Some(false);
            report.environment_ready = Some(false);
        } else if snapshot.complete
            && report.problems.iter().all(|code| {
                matches!(
                    *code,
                    "secret_availability_not_queried"
                        | "transitive_lock_extension_requirements_and_platform_check_unverified"
                        | "database_service_connection_dns_https_and_os_edition_not_probed"
                )
            })
        {
            report.matches_recorded_configuration = Some(true);
        }
        Ok(report)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cached_php_extensions_never_become_current_native_support() {
        let required = BTreeSet::from(["pdo_mysql".into(), "mbstring".into()]);
        let cache = serde_json::json!({"extensions":["pdo_mysql"],"settings":{"password":"PRIVATE_CACHE_CANARY"}});
        let report = extension_evidence(required.clone(), Some(&cache), 128, 128);
        assert_eq!(
            report
                .iter()
                .find(|e| e.extension == "pdo_mysql")
                .unwrap()
                .cached_present,
            Some(true)
        );
        assert_eq!(
            report
                .iter()
                .find(|e| e.extension == "mbstring")
                .unwrap()
                .cached_present,
            Some(false)
        );
        assert!(
            report
                .iter()
                .all(|e| e.current_support == Support::Unverified
                    && e.evidence.contains("without_timestamp_or_revision"))
        );
        assert!(
            !serde_json::to_string(&report)
                .unwrap()
                .contains("PRIVATE_CACHE_CANARY")
        );
        let invalid = serde_json::json!({"extensions":["pdo_mysql",null]});
        assert!(
            extension_evidence(required, Some(&invalid), 128, 128)
                .iter()
                .all(|e| e.cached_present.is_none())
        );
    }
}
