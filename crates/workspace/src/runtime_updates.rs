//! Explicit manual updates of reviewed runtime families. Downloads never mutate pins.
use crate::{Result, Workspace, error};
use devforge_projects::runtime_branch::{release, same_branch};
use devforge_runtime::{
    Cancellation,
    package::{Package, Target},
};
use std::collections::BTreeMap;

impl Workspace {
    pub(crate) fn select_runtime_default(&mut self, id: &str) -> Result<()> {
        let package = self.catalog.get(id)?;
        if !matches!(package.kind.as_str(), "caddy" | "composer")
            || !self.default_uses_runtime_branch(package)
            || self
                .catalog
                .branch_latest(package)
                .is_none_or(|(latest, _)| latest != id)
        {
            return Ok(());
        }
        let kind = package.kind.clone();
        let version = package.version.clone();
        let previous = self
            .preferences
            .runtime_defaults
            .insert(kind.clone(), id.to_owned());
        if let Err(error) = self.save() {
            if let Some(previous) = previous {
                self.preferences
                    .runtime_defaults
                    .insert(kind.clone(), previous);
            } else {
                self.preferences.runtime_defaults.remove(&kind);
            }
            return Err(error);
        }
        self.catalog.defaults.insert(kind, version);
        Ok(())
    }

    fn default_uses_runtime_branch(&self, package: &Package) -> bool {
        self.catalog
            .defaults
            .get(&package.kind)
            .is_some_and(|version| {
                self.catalog
                    .packages
                    .values()
                    .any(|old| old.version == *version && same_branch(old, package))
            })
    }

    fn update_replacements(&self, package: &Package, target_id: &str) -> BTreeMap<String, String> {
        self.catalog
            .packages
            .iter()
            .filter(|(id, p)| {
                id.as_str() != target_id
                    && same_branch(p, package)
                    && (release(&p.version) < release(&package.version)
                        || (release(&p.version) == release(&package.version)
                            && self
                                .catalog
                                .superseded
                                .get(*id)
                                .is_some_and(|new| new == target_id)))
            })
            .map(|(id, _)| (id.clone(), target_id.to_owned()))
            .collect()
    }
    pub(crate) fn runtime_branch_update_available(&self, package: &Package) -> Result<bool> {
        let Some((id, latest)) = self.catalog.branch_latest(package) else {
            return Ok(false);
        };
        let replacements = self.update_replacements(latest, id);
        let referenced = self.projects.list()?.iter().any(|p| {
            replacements.contains_key(&p.php)
                || replacements.contains_key(&p.node)
                || replacements.contains_key(&p.composer)
        }) || self
            .projects
            .instances()?
            .iter()
            .any(|i| replacements.contains_key(&i.package))
            || self.projects.cache_instances()?.iter().any(|i| {
                replacements.contains_key(&i.package) || replacements.contains_key(&i.runtime)
            });
        let default_pending = self.default_uses_runtime_branch(package)
            && self.catalog.defaults.get(&package.kind) != Some(&latest.version)
            && self.packages.is_present(package);
        Ok(referenced
            || default_pending
            || (!self.packages.is_present(latest)
                && replacements.keys().any(|old| {
                    self.catalog
                        .get(old)
                        .is_ok_and(|p| self.packages.is_present(p))
                })))
    }
    pub(crate) fn install_runtime_family(&mut self, id: &str, cancel: &Cancellation) -> Result<()> {
        let package = self.catalog.get(id)?.clone();
        if package.target != Target::current()? {
            return Err(devforge_runtime::RuntimeError::UnsupportedTarget.into());
        }
        if self.catalog.superseded.contains_key(id) && !self.packages.is_present(&package) {
            return Err(devforge_runtime::RuntimeError::PackageSuperseded.into());
        }
        self.packages.download(&package, cancel, |_| {})?;
        if package.kind == "php" && package.executables.contains_key("php") {
            let server = self.catalog.php_server(&package)?.clone();
            self.packages.download(&server, cancel, |_| {})?;
        }
        Ok(())
    }
    pub(crate) fn update_runtime_branch(
        &mut self,
        id: &str,
        cancel: &Cancellation,
    ) -> Result<serde_json::Value> {
        if cancel.is_cancelled() {
            return Err(error("cancelled"));
        }
        let package = self.catalog.get(id)?.clone();
        let latest = self
            .catalog
            .branch_latest(&package)
            .ok_or_else(|| error("runtime_update_unavailable"))?;
        if latest.0 != id || package.target != Target::current()? {
            return Err(error("runtime_update_unavailable"));
        }
        let mut replacements = self.update_replacements(&package, id);
        if package.kind == "php" && package.executables.contains_key("php") {
            let server = self.catalog.php_server(&package)?;
            let server_id = self
                .catalog
                .packages
                .iter()
                .find(|(_, p)| std::ptr::eq(*p, server))
                .map(|(id, _)| id)
                .ok_or_else(|| error("runtime_update_unavailable"))?;
            replacements.extend(self.update_replacements(server, server_id));
        }
        if package.kind == "caddy" && !self.sessions.is_empty() {
            return Err(error("runtime_update_in_use"));
        }
        let projects = self.projects.list()?;
        for p in &projects {
            let php = replacements.get(&p.php).unwrap_or(&p.php);
            let node = replacements.get(&p.node).unwrap_or(&p.node);
            if php != &p.php || node != &p.node || replacements.contains_key(&p.composer) {
                if self.sessions.contains_key(&p.id) || self.debugging.contains_key(&p.id) {
                    return Err(error("runtime_update_in_use"));
                }
                if php != &p.php || node != &p.node {
                    p.validate_configuration(&p.name, php, node, &self.catalog)?;
                }
            }
        }
        for i in self.projects.instances()? {
            if replacements.contains_key(&i.package) && self.databases.contains_key(&i.id) {
                return Err(error("runtime_update_in_use"));
            }
        }
        for i in self.projects.cache_instances()? {
            if (replacements.contains_key(&i.package) || replacements.contains_key(&i.runtime))
                && self.caches.contains_key(&i.id)
            {
                return Err(error("runtime_update_in_use"));
            }
        }
        if cancel.is_cancelled() {
            return Err(error("cancelled"));
        }
        self.install_runtime_family(id, cancel)?;
        if cancel.is_cancelled() {
            return Err(error("cancelled"));
        }
        let changed = self
            .projects
            .apply_runtime_patch(&replacements, &self.catalog)?;
        self.select_runtime_default(id)?;
        self.refresh_runtimes()?;
        Ok(
            serde_json::json!({"updated":true,"package_id":id,"changed_references":changed,"old_packages_retained":true,"services_restarted":false}),
        )
    }
}
