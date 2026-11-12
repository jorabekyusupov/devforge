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
    /// Prepare only a missing .env and an empty APP_KEY. Never rotates a key or
    /// returns its value; the existing optimistic write/backup contract applies.
    pub fn application(
        store: &ProjectStore,
        id: &str,
        create_env: bool,
        generate_key: bool,
    ) -> Result<Option<Self>> {
        use base64::Engine;
        let project = store.get(id)?;
        let path = project.path.join(".env");
        let old = read(&path)?.map(Zeroizing::new);
        let copied = old.is_none() && create_env;
        let base = if let Some(old) = &old {
            Zeroizing::new(old.to_vec())
        } else if create_env {
            Zeroizing::new(read(&project.path.join(".env.example"))?.ok_or(E::InvalidEnvFile)?)
        } else {
            return Ok(None);
        };
        let text = std::str::from_utf8(&base).map_err(|_| E::InvalidEnvFile)?;
        let newline = if text.contains("\r\n") { "\r\n" } else { "\n" };
        let mut key_line = None;
        for (n, line) in text.split_inclusive('\n').enumerate() {
            let content = assignment(line);
            if let Some((name, value)) = content.split_once('=')
                && name.trim() == "APP_KEY"
            {
                if key_line.is_some() {
                    return Err(E::InvalidEnvFile);
                }
                let value = value.trim();
                let empty = empty_value(value);
                key_line = Some((n, empty));
            }
        }
        let generate = generate_key && key_line.is_none_or(|(_, empty)| empty);
        if !copied && !generate {
            return Ok(None);
        }
        let mut next = Zeroizing::new(String::new());
        if generate {
            let mut random = Zeroizing::new([0u8; 32]);
            getrandom::fill(random.as_mut()).map_err(|_| E::SecretUnavailable)?;
            let key = Zeroizing::new(format!(
                "base64:{}",
                base64::engine::general_purpose::STANDARD.encode(random.as_ref())
            ));
            for (n, line) in text.split_inclusive('\n').enumerate() {
                if key_line.is_some_and(|(at, _)| at == n) {
                    if n == 0 && line.starts_with('\u{feff}') {
                        next.push('\u{feff}');
                    }
                    next.push_str(&format!(
                        "APP_KEY={}{}",
                        key.as_str(),
                        if line.ends_with('\n') { newline } else { "" }
                    ));
                } else {
                    next.push_str(line);
                }
            }
            if key_line.is_none() {
                if !next.is_empty() && !next.ends_with('\n') {
                    next.push_str(newline);
                }
                next.push_str(&format!("APP_KEY={}{newline}", key.as_str()));
            }
        } else {
            next.push_str(text);
        }
        Ok(Some(Self {
            project_id: id.into(),
            path,
            old,
            next: Zeroizing::new(next.as_bytes().to_vec()),
            keys: if generate {
                vec!["APP_KEY".into()]
            } else {
                vec![]
            },
        }))
    }
    pub fn application_status(path: &std::path::Path) -> Result<(bool, bool)> {
        let Some(bytes) = read(&path.join(".env"))?.map(Zeroizing::new) else {
            return Ok((false, false));
        };
        let text = std::str::from_utf8(&bytes).map_err(|_| E::InvalidEnvFile)?;
        let key = text
            .lines()
            .filter_map(|line| assignment(line).split_once('='))
            .find(|(name, _)| name.trim() == "APP_KEY")
            .map(|(_, value)| value.trim());
        Ok((true, key.is_some_and(|v| !empty_value(v))))
    }
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
fn assignment(line: &str) -> &str {
    let line = line.trim_start_matches('\u{feff}').trim_start();
    if let Some(rest) = line.strip_prefix("export")
        && rest.starts_with(char::is_whitespace)
    {
        rest.trim_start()
    } else {
        line
    }
}
fn empty_value(value: &str) -> bool {
    let value = value.trim();
    if let Some(quote) = value.chars().next().filter(|c| *c == '\'' || *c == '"') {
        let tail = &value[1..];
        return tail.starts_with(quote)
            && (tail[1..].trim().is_empty() || tail[1..].trim().starts_with('#'));
    }
    let value = value.split_once('#').map_or(value, |(v, _)| v).trim();
    value.is_empty() || value.eq_ignore_ascii_case("null")
}
pub(crate) fn app_key_secret(path: &std::path::Path) -> Result<Option<Zeroizing<String>>> {
    let Some(bytes) = read(&path.join(".env"))?.map(Zeroizing::new) else {
        return Ok(None);
    };
    let text = std::str::from_utf8(&bytes).map_err(|_| E::InvalidEnvFile)?;
    for line in text.lines() {
        let line = assignment(line);
        if let Some((name, value)) = line.split_once('=')
            && name.trim() == "APP_KEY"
            && !empty_value(value)
        {
            let value = value.trim();
            let value = if let Some(q) = value.chars().next().filter(|c| *c == '\'' || *c == '"') {
                value[1..].split(q).next().unwrap_or("")
            } else {
                value.split('#').next().unwrap_or("").trim()
            };
            return Ok(Some(Zeroizing::new(value.to_owned())));
        }
    }
    Ok(None)
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
    #[test]
    fn application_key_is_random_and_existing_keys_are_never_rotated() {
        use base64::Engine;
        let (root, store, id, _) = fixture();
        let path = root.path().join("project");
        fs::write(
            path.join(".env.example"),
            "APP_KEY=\"\" # empty\r\nCUSTOM=keep\r\nDB_CONNECTION=sqlite\r\n",
        )
        .unwrap();
        let applied = EnvPlan::application(&store, &id, true, true)
            .unwrap()
            .unwrap()
            .apply(&store)
            .unwrap();
        assert!(applied.backup.is_none());
        let before = fs::read(path.join(".env")).unwrap();
        let text = std::str::from_utf8(&before).unwrap();
        let key = text
            .lines()
            .find_map(|line| line.strip_prefix("APP_KEY=base64:"))
            .unwrap();
        assert_eq!(
            base64::engine::general_purpose::STANDARD
                .decode(key)
                .unwrap()
                .len(),
            32
        );
        assert!(text.contains("CUSTOM=keep\r\n"));
        assert!(
            EnvPlan::application(&store, &id, true, true)
                .unwrap()
                .is_none()
        );
        assert_eq!(before, fs::read(path.join(".env")).unwrap());
        assert_eq!(EnvPlan::application_status(&path).unwrap(), (true, true));
        fs::write(path.join(".env"), "APP_KEY=\nOTHER=old\n").unwrap();
        let plan = EnvPlan::application(&store, &id, true, true)
            .unwrap()
            .unwrap();
        fs::write(path.join(".env"), "APP_KEY=user-key\nOTHER=new\n").unwrap();
        assert!(matches!(plan.apply(&store), Err(E::EnvFileChanged)));
        assert!(
            fs::read_to_string(path.join(".env"))
                .unwrap()
                .contains("user-key")
        );
        fs::write(path.join(".env"), "APP_KEY=\nAPP_KEY=\n").unwrap();
        assert!(EnvPlan::application(&store, &id, true, true).is_err());
        let bom = "\u{feff}export\tAPP_KEY=base64:existing-user-key\r\nCUSTOM=preserve\r\n";
        fs::write(path.join(".env"), bom).unwrap();
        assert!(
            EnvPlan::application(&store, &id, true, true)
                .unwrap()
                .is_none()
        );
        assert_eq!(fs::read_to_string(path.join(".env")).unwrap(), bom);
        assert_eq!(
            app_key_secret(&path)
                .unwrap()
                .as_deref()
                .map(|v| v.as_str()),
            Some("base64:existing-user-key")
        );
    }
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
