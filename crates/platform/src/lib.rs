pub mod distribution;
use devforge_core::{ErrorCode, SecretStore, Settings, SettingsRepository};
use rusqlite::{Connection, OptionalExtension};
use serde::Serialize;
use std::path::{Path, PathBuf};

pub const APP_IDENTIFIER: &str = "com.devforge.desktop";
pub fn app_data_dir() -> Result<PathBuf, ErrorCode> {
    if let Some(path) = std::env::var_os("DEVFORGE_DATA_DIR") {
        return Ok(path.into());
    }
    dirs::data_dir()
        .map(|p| p.join(APP_IDENTIFIER))
        .ok_or(ErrorCode::StorageUnavailable)
}

pub struct SqliteSettings {
    connection: Connection,
    path: PathBuf,
}
impl SqliteSettings {
    pub fn open(directory: &Path) -> Result<Self, ErrorCode> {
        std::fs::create_dir_all(directory).map_err(|_| ErrorCode::StorageUnavailable)?;
        let path = directory.join("preferences.sqlite3");
        let mut connection = Connection::open(&path).map_err(|_| ErrorCode::StorageUnavailable)?;
        connection
            .busy_timeout(std::time::Duration::from_secs(3))
            .map_err(|_| ErrorCode::StorageUnavailable)?;
        let version: u32 = connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .map_err(|_| ErrorCode::StorageUnavailable)?;
        if version > 1 {
            return Err(ErrorCode::StorageUnavailable);
        }
        if version == 0 {
            let transaction = connection
                .transaction()
                .map_err(|_| ErrorCode::StorageUnavailable)?;
            transaction.execute_batch("CREATE TABLE IF NOT EXISTS preferences (id INTEGER PRIMARY KEY CHECK(id=1), value TEXT NOT NULL); PRAGMA user_version=1;")
                .map_err(|_| ErrorCode::StorageUnavailable)?;
            transaction
                .commit()
                .map_err(|_| ErrorCode::StorageUnavailable)?;
        }
        Ok(Self { connection, path })
    }
}
impl SettingsRepository for SqliteSettings {
    fn load(&self) -> Result<Settings, ErrorCode> {
        let stored: Option<String> = self
            .connection
            .query_row("SELECT value FROM preferences WHERE id=1", [], |r| r.get(0))
            .optional()
            .map_err(|_| ErrorCode::StorageUnavailable)?;
        stored
            .map(|value| serde_json::from_str(&value).map_err(|_| ErrorCode::StorageUnavailable))
            .unwrap_or_else(|| Ok(Settings::default()))
    }
    fn save(&mut self, settings: &Settings) -> Result<(), ErrorCode> {
        let value = serde_json::to_string(settings).map_err(|_| ErrorCode::InvalidSettings)?;
        self.connection.execute("INSERT INTO preferences(id,value) VALUES(1,?1) ON CONFLICT(id) DO UPDATE SET value=excluded.value", [value])
            .map(|_| ()).map_err(|_| ErrorCode::StorageUnavailable)
    }
}

/// Secrets are never stored in the settings database. Access is only on explicit
/// credential operations; startup and diagnostics do not probe the OS keychain.
pub struct NativeSecretStore {
    service: String,
}
impl NativeSecretStore {
    pub fn new(service: impl Into<String>) -> Self {
        Self {
            service: service.into(),
        }
    }
    fn entry(&self, key: &str) -> Result<keyring::Entry, ErrorCode> {
        keyring::Entry::new(&self.service, key).map_err(|_| ErrorCode::SecretUnavailable)
    }
}
impl SecretStore for NativeSecretStore {
    fn get(&self, key: &str) -> Result<Option<String>, ErrorCode> {
        match self.entry(key)?.get_password() {
            Ok(value) => Ok(Some(value)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(_) => Err(ErrorCode::SecretUnavailable),
        }
    }
    fn set(&self, key: &str, value: &str) -> Result<(), ErrorCode> {
        self.entry(key)?
            .set_password(value)
            .map_err(|_| ErrorCode::SecretUnavailable)
    }
    fn delete(&self, key: &str) -> Result<(), ErrorCode> {
        match self.entry(key)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(_) => Err(ErrorCode::SecretUnavailable),
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Diagnostics {
    pub os: String,
    pub architecture: String,
    pub app_version: String,
    pub database_path: String,
    pub schema_version: u32,
}
pub fn diagnostics(database: &SqliteSettings) -> Diagnostics {
    Diagnostics {
        os: std::env::consts::OS.into(),
        architecture: std::env::consts::ARCH.into(),
        app_version: env!("CARGO_PKG_VERSION").into(),
        database_path: database.path.to_string_lossy().into(),
        schema_version: 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use devforge_core::Theme;
    #[test]
    fn settings_survive_reopen_and_migration_is_idempotent() {
        let root = tempfile::tempdir().unwrap();
        let chosen = Settings {
            language: "ru".into(),
            theme: Theme::Dark,
        };
        {
            let mut db = SqliteSettings::open(root.path()).unwrap();
            assert_eq!(db.load().unwrap(), Settings::default());
            db.save(&chosen).unwrap();
        }
        let db = SqliteSettings::open(root.path()).unwrap();
        assert_eq!(db.load().unwrap(), chosen);
        assert_eq!(diagnostics(&db).schema_version, 1);
    }
    #[test]
    fn unknown_language_is_preserved_for_fallback_not_silently_overwritten() {
        let root = tempfile::tempdir().unwrap();
        let mut db = SqliteSettings::open(root.path()).unwrap();
        let setting = Settings {
            language: "removed".into(),
            ..Settings::default()
        };
        db.save(&setting).unwrap();
        assert_eq!(db.load().unwrap(), setting);
    }
    #[test]
    fn corrupt_and_future_schemas_are_not_reset() {
        let root = tempfile::tempdir().unwrap();
        let db = SqliteSettings::open(root.path()).unwrap();
        db.connection
            .execute(
                "INSERT INTO preferences(id,value) VALUES(1,'broken json')",
                [],
            )
            .unwrap();
        assert_eq!(db.load(), Err(ErrorCode::StorageUnavailable));
        db.connection
            .pragma_update(None, "user_version", 2)
            .unwrap();
        drop(db);
        assert!(SqliteSettings::open(root.path()).is_err());
        let raw = Connection::open(root.path().join("preferences.sqlite3")).unwrap();
        assert_eq!(
            raw.query_row("SELECT value FROM preferences", [], |r| r
                .get::<_, String>(0))
                .unwrap(),
            "broken json"
        );
    }
    #[test]
    #[ignore = "Explicit native credential fixture; not run on unverified/headless runners"]
    fn native_secret_roundtrip_removes_its_ephemeral_item() {
        let store = NativeSecretStore::new(format!("{APP_IDENTIFIER}.m1-fixture"));
        let key = format!(
            "{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let outcome = (|| -> Result<(), ErrorCode> {
            store.set(&key, "ephemeral-devforge-m1-fixture")?;
            if store.get(&key)?.as_deref() != Some("ephemeral-devforge-m1-fixture") {
                return Err(ErrorCode::SecretUnavailable);
            }
            Ok(())
        })();
        let cleanup = store.delete(&key);
        assert!(cleanup.is_ok(), "task credential cleanup failed");
        assert!(outcome.is_ok(), "native credential roundtrip failed");
        assert_eq!(store.get(&key).unwrap(), None);
    }
}
