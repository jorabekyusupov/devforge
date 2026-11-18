//! Runtime advice for explicit imports and watched folders.
use super::*;
use devforge_projects::{
    discovery::{DetectedSettings, RuntimeSuggestion},
    project::{Inspection, compatible_node, compatible_php},
    project_metadata::{Metadata, read},
};
use std::{collections::BTreeSet, process::Command as ProcessCommand};

fn version_key(version: &str) -> Option<[u64; 3]> {
    let mut parts = version
        .trim_start_matches('v')
        .split('-')
        .next()?
        .split('.');
    let key = [
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
    ];
    parts.next().is_none().then_some(key)
}

impl Workspace {
    fn detection_runtime(&self, checked: &Inspection, kind: &str) -> Option<RuntimeSuggestion> {
        self.runtime_cache
            .iter()
            .filter(|runtime| runtime.kind == kind && runtime.problem.is_none())
            .filter_map(|runtime| {
                let version = version_key(&runtime.version)?;
                let package = self.catalog.get(&runtime.id).ok()?;
                let installed = if kind == "php" {
                    compatible_php(checked, package).ok()?;
                    let server = self.catalog.php_server(package).ok()?;
                    runtime.installed
                        && self.runtime_cache.iter().any(|entry| {
                            entry.installed
                                && self.catalog.get(&entry.id).is_ok_and(|p| p == server)
                        })
                } else {
                    compatible_node(checked, package).ok()?;
                    runtime.installed
                };
                Some((
                    (
                        installed,
                        version,
                        runtime.latest_package.as_ref() == Some(&runtime.id),
                    ),
                    runtime,
                ))
            })
            .max_by_key(|(key, _)| *key)
            .map(|((installed, _, _), runtime)| RuntimeSuggestion {
                id: runtime.id.clone(),
                version: runtime.version.clone(),
                installed,
            })
    }

    fn detection_extensions(&self, php: &str) -> Option<BTreeSet<String>> {
        let package = self.catalog.get(php).ok()?;
        let executable = self.packages.executable(package, "php").ok()?;
        let mut command = ProcessCommand::new(executable.path());
        command
            .env_clear()
            .current_dir(&self.root)
            .args(["-n", "-m"]);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
            let ext = executable.path().parent()?.join("ext");
            command.args(["-d", &format!("extension_dir={}", ext.display())]);
            for name in devforge_projects::execution::WINDOWS_PHP_EXTENSIONS {
                if ext.join(format!("php_{name}.dll")).is_file() {
                    command.args(["-d", &format!("extension={name}")]);
                }
            }
        }
        let output = command.output().ok()?;
        if !output.status.success() {
            return None;
        }
        let text = std::str::from_utf8(&output.stdout).ok()?;
        Some(
            text.lines()
                .map(|line| line.trim().to_ascii_lowercase())
                .collect(),
        )
    }

    pub(crate) fn settings_from_metadata(&self, metadata: Metadata) -> DetectedSettings {
        let Metadata {
            inspection,
            extensions,
            database,
        } = metadata;
        DetectedSettings {
            php: self.detection_runtime(&inspection, "php"),
            node: self.detection_runtime(&inspection, "node"),
            php_constraint: inspection.php_constraint,
            node_constraint: inspection.node_constraint,
            extensions,
            missing_extensions: None,
            database,
        }
    }

    pub(crate) fn detect_project(&self, path: &Path) -> Result<serde_json::Value> {
        let mut settings = self.settings_from_metadata(read(path)?);
        if !settings.extensions.is_empty() {
            settings.missing_extensions = settings
                .php
                .as_ref()
                .and_then(|php| self.detection_extensions(&php.id))
                .map(|loaded| settings.extensions.difference(&loaded).cloned().collect());
        }
        Ok(serde_json::json!(settings))
    }
}
