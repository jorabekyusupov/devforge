use crate::{ProjectError as E, Result, private_dir, project::ProjectStore};
use devforge_core::SecretStore;
use serde::Serialize;
use std::{
    collections::{BTreeMap, HashSet},
    fs,
    io::Write,
    path::PathBuf,
};
use zeroize::Zeroizing;
pub struct EnvPlan {
    project_id: String,
    path: PathBuf,
    old: Option<Zeroizing<Vec<u8>>>,
    next: Zeroizing<Vec<u8>>,
    keys: Vec<String>,
}
#[derive(Serialize)]
pub struct Applied {
    pub backup: Option<PathBuf>,
    pub changed_keys: Vec<String>,
}
impl EnvPlan {
    pub fn changed_keys(&self) -> &[String] {
        &self.keys
    }
    pub fn build(store: &ProjectStore, id: &str, secrets: &dyn SecretStore) -> Result<Self> {
        let project = store.get(id)?;
        let c = store.credentials(id, secrets)?;
        let path = project.path.join(".env");
        let old = read(&path)?.map(Zeroizing::new);
        let text = std::str::from_utf8(old.as_ref().map_or(&[], |v| v.as_slice()))
            .map_err(|_| E::InvalidEnvFile)?;
        let values = BTreeMap::from([
            ("DB_CONNECTION", "pgsql".to_owned()),
            ("DB_HOST", "127.0.0.1".to_owned()),
            ("DB_PORT", c.port.to_string()),
            ("DB_DATABASE", c.binding.database.clone()),
            ("DB_USERNAME", c.binding.role.clone()),
            ("DB_PASSWORD", c.password.to_string()),
            ("DB_URL", String::new()),
            ("DATABASE_URL", String::new()),
        ]);
        let newline = if text.contains("\r\n") { "\r\n" } else { "\n" };
        let mut seen = HashSet::new();
        let mut next = Zeroizing::new(String::new());
        for line in text.split_inclusive('\n') {
            let stripped = line.trim_end_matches(['\r', '\n']);
            let trimmed = stripped.trim_start();
            let trimmed = trimmed.strip_prefix("export ").unwrap_or(trimmed);
            if let Some((name, _)) = trimmed.split_once('=')
                && let Some(value) = values.get(name.trim())
            {
                if !seen.insert(name.trim().to_owned()) {
                    return Err(E::InvalidEnvFile);
                }
                next.push_str(&format!(
                    "{}=\"{}\"{}",
                    name.trim(),
                    escape(value),
                    if line.ends_with('\n') { newline } else { "" }
                ));
            } else {
                next.push_str(line);
            }
        }
        for (name, value) in &values {
            if !seen.contains(*name) {
                if !next.is_empty() && !next.ends_with('\n') {
                    next.push_str(newline);
                }
                next.push_str(&format!("{name}=\"{}\"{newline}", escape(value)));
            }
        }
        Ok(Self {
            project_id: id.into(),
            path,
            old,
            next: Zeroizing::new(next.as_bytes().to_vec()),
            keys: values.keys().map(|s| s.to_string()).collect(),
        })
    }
    pub fn apply(self, store: &ProjectStore) -> Result<Applied> {
        let _lock = store.operation_lock()?;
        if store.get(&self.project_id)?.path.join(".env") != self.path {
            return Err(E::InvalidEnvFile);
        }
        if read(&self.path)?.as_deref() != self.old.as_ref().map(|v| v.as_slice()) {
            return Err(E::EnvFileChanged);
        }
        let backup = if let Some(old) = &self.old {
            let dir = store.root.join("env-backups").join(&self.project_id);
            private_dir(&dir)?;
            let target = dir.join(format!("{}.env", crate::id()));
            let mut f = fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&target)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                f.set_permissions(fs::Permissions::from_mode(0o600))?;
            }
            f.write_all(old)?;
            f.sync_all()?;
            Some(target)
        } else {
            None
        };
        let mut staged = tempfile::NamedTempFile::new_in(self.path.parent().unwrap())?;
        staged.write_all(&self.next)?;
        staged.as_file().sync_all()?;
        if read(&self.path)?.as_deref() != self.old.as_ref().map(|v| v.as_slice()) {
            return Err(E::EnvFileChanged);
        }
        staged
            .persist(&self.path)
            .map_err(|_| E::StorageUnavailable)?;
        Ok(Applied {
            backup,
            changed_keys: self.keys,
        })
    }
}
fn read(path: &std::path::Path) -> Result<Option<Vec<u8>>> {
    match fs::symlink_metadata(path) {
        Ok(m) => {
            if !m.is_file() || m.file_type().is_symlink() || m.len() > 1024 * 1024 {
                return Err(E::InvalidEnvFile);
            }
            Ok(Some(fs::read(path)?))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err(E::InvalidEnvFile),
    }
}
fn escape(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('$', "\\$")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        database::{Binding, Instance},
        project::Catalog,
    };
    use devforge_core::ErrorCode;
    use std::{collections::BTreeMap, sync::Mutex};
    #[derive(Default)]
    struct Secrets(Mutex<BTreeMap<String, String>>);
    impl SecretStore for Secrets {
        fn get(&self, k: &str) -> std::result::Result<Option<String>, ErrorCode> {
            Ok(self.0.lock().unwrap().get(k).cloned())
        }
        fn set(&self, k: &str, v: &str) -> std::result::Result<(), ErrorCode> {
            self.0.lock().unwrap().insert(k.into(), v.into());
            Ok(())
        }
        fn delete(&self, k: &str) -> std::result::Result<(), ErrorCode> {
            self.0.lock().unwrap().remove(k);
            Ok(())
        }
    }
    fn fixture() -> (tempfile::TempDir, ProjectStore, String, Secrets) {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("project");
        fs::create_dir_all(path.join("public")).unwrap();
        fs::write(path.join("public/index.php"), b"<?php").unwrap();
        fs::write(
            path.join("composer.json"),
            br#"{"require":{"php":"^8.3","laravel/framework":"^13"}}"#,
        )
        .unwrap();
        let c = Catalog::bundled();
        let target = devforge_runtime::package::Target::current().unwrap();
        let php = c
            .packages
            .iter()
            .find(|(_, p)| {
                p.target == target && p.kind == "php" && p.executables.contains_key("php")
            })
            .unwrap()
            .0;
        let node = c
            .packages
            .iter()
            .find(|(_, p)| p.target == target && p.kind == "node")
            .unwrap()
            .0;
        let store = ProjectStore::open(root.path().join("metadata")).unwrap();
        let p = store.import(&path, "Fixture", php, node, &c).unwrap();
        let instance = Instance {
            id: "fixture".into(),
            package: "fixture".into(),
            port: 12345,
            admin: "admin".into(),
            secret_ref: "admin-ref".into(),
            ready: true,
        };
        let binding = Binding {
            project_id: p.id.clone(),
            instance_id: instance.id.clone(),
            database: "df_fixture".into(),
            role: "df_fixture".into(),
            secret_ref: "db-ref".into(),
            ready: true,
        };
        store
            .connection
            .execute(
                "INSERT INTO instances VALUES(?1,?2)",
                rusqlite::params![instance.id, serde_json::to_string(&instance).unwrap()],
            )
            .unwrap();
        store
            .connection
            .execute(
                "INSERT INTO database_bindings VALUES(?1,?2,?3)",
                rusqlite::params![p.id, instance.id, serde_json::to_string(&binding).unwrap()],
            )
            .unwrap();
        let secrets = Secrets::default();
        secrets.set("db-ref", "test-fixture-db-password").unwrap();
        (root, store, p.id, secrets)
    }
    #[test]
    fn duplicate_database_keys_are_rejected_without_writes() {
        let (_r, s, id, secrets) = fixture();
        let path = s.get(&id).unwrap().path.join(".env");
        fs::write(&path, b"DB_HOST=first\nexport DB_HOST=second\nKEEP=yes\n").unwrap();
        assert_eq!(
            EnvPlan::build(&s, &id, &secrets).err(),
            Some(E::InvalidEnvFile)
        );
        assert!(fs::read_to_string(path).unwrap().contains("KEEP=yes"));
    }
    #[test]
    fn first_explicit_apply_creates_file_and_subsequent_apply_has_exact_backup() {
        let (_r, s, id, secrets) = fixture();
        let path = s.get(&id).unwrap().path.join(".env");
        let applied = EnvPlan::build(&s, &id, &secrets)
            .unwrap()
            .apply(&s)
            .unwrap();
        assert!(applied.backup.is_none());
        let original = fs::read(&path).unwrap();
        let applied = EnvPlan::build(&s, &id, &secrets)
            .unwrap()
            .apply(&s)
            .unwrap();
        assert_eq!(fs::read(applied.backup.unwrap()).unwrap(), original);
    }
    #[cfg(unix)]
    #[test]
    fn symlink_env_is_rejected() {
        let (r, s, id, secrets) = fixture();
        let outside = r.path().join("outside");
        fs::write(&outside, b"keep").unwrap();
        std::os::unix::fs::symlink(&outside, s.get(&id).unwrap().path.join(".env")).unwrap();
        assert_eq!(
            EnvPlan::build(&s, &id, &secrets).err(),
            Some(E::InvalidEnvFile)
        );
        assert_eq!(fs::read(outside).unwrap(), b"keep");
    }
}
