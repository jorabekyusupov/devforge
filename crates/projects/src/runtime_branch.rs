//! Reviewed catalog release families; never resolves an untrusted network version.
use crate::{
    ProjectError as E, Result,
    project::{Catalog, ProjectStore},
};
use devforge_runtime::package::Package;
use rusqlite::params;
use std::{collections::BTreeMap, fs::OpenOptions, net::TcpListener};

pub fn release(version: &str) -> Option<Vec<u64>> {
    let value = version.strip_prefix('v').unwrap_or(version);
    let value = if let Some((base, suffix)) = value.split_once('-') {
        let digits = suffix.strip_prefix("staticphp").unwrap_or(suffix);
        if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        base
    } else {
        value
    };
    let parts = value
        .split('.')
        .map(str::parse::<u64>)
        .collect::<std::result::Result<Vec<_>, _>>()
        .ok()?;
    (parts.len() >= 2 && parts.len() <= 4).then_some(parts)
}
pub fn branch(package: &Package) -> Option<String> {
    let version = release(&package.version)?;
    let count = if matches!(
        package.kind.as_str(),
        "node" | "postgresql" | "postgresql-tools"
    ) {
        1
    } else {
        2
    };
    Some(
        version[..count]
            .iter()
            .map(u64::to_string)
            .collect::<Vec<_>>()
            .join("."),
    )
}
pub fn same_branch(a: &Package, b: &Package) -> bool {
    a.kind == b.kind
        && a.target == b.target
        && a.variant == b.variant
        && branch(a).is_some()
        && branch(a) == branch(b)
}
impl Catalog {
    pub fn branch_latest(&self, package: &Package) -> Option<(&str, &Package)> {
        self.packages
            .iter()
            .filter(|(id, p)| same_branch(p, package) && !self.superseded.contains_key(*id))
            .max_by(|(aid, a), (bid, b)| {
                release(&a.version)
                    .cmp(&release(&b.version))
                    .then_with(|| aid.cmp(bid))
            })
            .map(|(id, p)| (id.as_str(), p))
    }
}
impl ProjectStore {
    /// Apply reviewed same-branch package substitutions to stopped registry users.
    /// Data directories, secrets, retained packages and project YAML remain untouched.
    /// Instance locks remain held until the single SQLite transaction commits.
    pub fn apply_runtime_patch(
        &self,
        replacements: &BTreeMap<String, String>,
        catalog: &Catalog,
    ) -> Result<usize> {
        let _operation = self.operation_lock()?;
        for (old, new) in replacements {
            if !same_branch(catalog.get(old)?, catalog.get(new)?) {
                return Err(E::RuntimeUnavailable);
            }
        }
        let mut projects = self.list()?;
        let mut databases = self.instances()?;
        let mut caches = self.cache_instances()?;
        for i in databases
            .iter()
            .filter(|i| replacements.contains_key(&i.package))
        {
            let marker = if i.engine == crate::database::Engine::Mysql {
                b"devforge-mysql-v1\n".as_slice()
            } else {
                b"devforge-postgres-v1\n".as_slice()
            };
            if !i.ready
                || std::fs::read(
                    self.root()
                        .join("databases")
                        .join(&i.id)
                        .join("initialized"),
                )
                .ok()
                .as_deref()
                    != Some(marker)
            {
                return Err(E::RecoveryRequired);
            }
        }
        if caches.iter().any(|i| {
            (!i.ready)
                && (replacements.contains_key(&i.package) || replacements.contains_key(&i.runtime))
        }) {
            return Err(E::RecoveryRequired);
        }
        let mut locks = Vec::new();
        let mut listeners = Vec::new();
        for (id, port, directory) in databases
            .iter()
            .filter(|i| replacements.contains_key(&i.package))
            .map(|i| (&i.id, i.port, "databases"))
            .chain(
                caches
                    .iter()
                    .filter(|i| {
                        replacements.contains_key(&i.package)
                            || replacements.contains_key(&i.runtime)
                    })
                    .map(|i| (&i.id, i.port, "cache-services")),
            )
        {
            let file = OpenOptions::new()
                .read(true)
                .write(true)
                .open(self.root().join(directory).join(id).join("instance.lock"))?;
            file.try_lock().map_err(|_| E::InstanceBusy)?;
            locks.push(file);
            listeners.push(TcpListener::bind(("127.0.0.1", port)).map_err(|_| E::InstanceBusy)?);
        }
        for p in &projects {
            let php = replacements.get(&p.php).unwrap_or(&p.php);
            let node = replacements.get(&p.node).unwrap_or(&p.node);
            if php != &p.php || node != &p.node {
                p.validate_configuration(&p.name, php, node, catalog)?;
            }
        }
        let transaction = self.connection.unchecked_transaction()?;
        let mut count = 0;
        for p in &mut projects {
            let revision = p.revision;
            let mut changed = false;
            for field in [&mut p.php, &mut p.node, &mut p.composer] {
                if let Some(new) = replacements.get(field) {
                    *field = new.clone();
                    changed = true;
                }
            }
            if changed {
                self.save(p, revision)?;
                count += 1;
            }
        }
        for i in &mut databases {
            if let Some(new) = replacements.get(&i.package) {
                i.package = new.clone();
                self.connection.execute(
                    "UPDATE instances SET record=?1 WHERE id=?2",
                    params![
                        serde_json::to_string(i).map_err(|_| E::StorageUnavailable)?,
                        i.id
                    ],
                )?;
                count += 1;
            }
        }
        for i in &mut caches {
            let mut changed = false;
            for field in [&mut i.package, &mut i.runtime] {
                if let Some(new) = replacements.get(field) {
                    *field = new.clone();
                    changed = true;
                }
            }
            if changed {
                self.connection.execute(
                    "UPDATE cache_instances SET record=?1 WHERE id=?2",
                    params![
                        serde_json::to_string(i).map_err(|_| E::StorageUnavailable)?,
                        i.id
                    ],
                )?;
                count += 1;
            }
        }
        transaction.commit()?;
        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reviewed_suffixes_are_stable_but_prereleases_are_not() {
        assert_eq!(release("8.5.12-staticphp20261113"), Some(vec![8, 5, 12]));
        assert_eq!(release("8.5.12-1"), Some(vec![8, 5, 12]));
        assert_eq!(release("v22.23.4"), Some(vec![22, 23, 4]));
        for value in [
            "8.5.99-RC1",
            "8.5.99-staticphpRC",
            "8.5.99-staticphp",
            "8.5.99-1rc",
        ] {
            assert_eq!(release(value), None);
        }
    }
}

#[cfg(test)]
#[path = "runtime_branch_tests.rs"]
mod store_tests;
