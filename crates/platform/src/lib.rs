pub mod distribution;
use devforge_core::{
    ErrorCode, LocaleRegistry, SecretStore, Settings, SettingsPatch, SettingsRepository,
};
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

pub fn default_project_folder(home: &Path) -> PathBuf {
    home.join("Devforge")
}
pub fn home_dir() -> Result<PathBuf, ErrorCode> {
    dirs::home_dir().ok_or(ErrorCode::ProjectFolderUnavailable)
}

/// Resolve only a leading home marker, never shell expressions or environment
/// substitutions. Canonicalization is required before activating a root.
pub fn resolve_project_folder(input: &str, home: &Path) -> Result<PathBuf, ErrorCode> {
    if input.trim().is_empty() || input.chars().any(char::is_control) {
        return Err(ErrorCode::InvalidProjectFolder);
    }
    let path = if input == "~" {
        home.to_path_buf()
    } else if let Some(rest) = input
        .strip_prefix("~/")
        .or_else(|| input.strip_prefix("~\\"))
    {
        home.join(rest)
    } else {
        PathBuf::from(input)
    };
    if !path.is_absolute() {
        return Err(ErrorCode::InvalidProjectFolder);
    }
    let metadata = std::fs::metadata(&path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            ErrorCode::ProjectFolderMissing
        } else {
            ErrorCode::ProjectFolderUnavailable
        }
    })?;
    if !metadata.is_dir() {
        return Err(ErrorCode::InvalidProjectFolder);
    }
    std::fs::read_dir(&path).map_err(|_| ErrorCode::ProjectFolderUnavailable)?;
    std::fs::canonicalize(path)
        .map(|p| dunce::simplified(&p).to_path_buf())
        .map_err(|_| ErrorCode::ProjectFolderUnavailable)
}

pub struct SqliteSettings {
    connection: Connection,
    path: PathBuf,
}
impl SqliteSettings {
    /// Initialization is recorded by saving the resolved path. Later loss of the
    /// directory must remain visible, rather than re-creating it on every launch.
    pub fn initialize_project_folder(&mut self, home: &Path) -> Result<(), ErrorCode> {
        let transaction = self
            .connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|_| ErrorCode::StorageUnavailable)?;
        let (settings, mut record) = read_settings_record(&transaction)?;
        if settings.project_folder.is_none() {
            let path = default_project_folder(home);
            // Only the initializer holding the SQLite write reservation attempts
            // first creation. Never overwrite a concurrently saved manual folder.
            let _ = std::fs::create_dir(&path);
            record.insert(
                "project_folder".into(),
                path.to_string_lossy().into_owned().into(),
            );
            write_settings_record(&transaction, record)?;
        }
        transaction
            .commit()
            .map_err(|_| ErrorCode::StorageUnavailable)
    }
    /// Merge only supplied fields against the latest persisted record while
    /// holding a cross-connection SQLite write reservation. Unknown saved fields
    /// and unsupported saved locale choices survive unrelated field updates.
    pub fn patch(
        &mut self,
        patch: SettingsPatch,
        registry: &LocaleRegistry,
    ) -> Result<Settings, ErrorCode> {
        if let Some(language) = &patch.language
            && !registry.supports_choice(language)
        {
            return Err(ErrorCode::InvalidSettings);
        }
        if let Some(folder) = &patch.project_folder
            && (folder.trim().is_empty()
                || folder.len() > 32768
                || folder.chars().any(char::is_control)
                || !Path::new(folder).is_absolute())
        {
            return Err(ErrorCode::InvalidProjectFolder);
        }
        let transaction = self
            .connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|_| ErrorCode::StorageUnavailable)?;
        let (mut settings, mut record) = read_settings_record(&transaction)?;
        let changed =
            patch.language.is_some() || patch.theme.is_some() || patch.project_folder.is_some();
        if let Some(language) = patch.language {
            settings.language = language.clone();
            record.insert("language".into(), language.into());
        }
        if let Some(theme) = patch.theme {
            settings.theme = theme;
            record.insert(
                "theme".into(),
                serde_json::to_value(theme).map_err(|_| ErrorCode::InvalidSettings)?,
            );
        }
        if let Some(folder) = patch.project_folder {
            settings.project_folder = Some(folder.clone());
            record.insert("project_folder".into(), folder.into());
        }
        if changed {
            write_settings_record(&transaction, record)?;
        }
        transaction
            .commit()
            .map_err(|_| ErrorCode::StorageUnavailable)?;
        Ok(settings)
    }
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
fn read_settings_record(
    connection: &Connection,
) -> Result<(Settings, serde_json::Map<String, serde_json::Value>), ErrorCode> {
    let stored: Option<String> = connection
        .query_row("SELECT value FROM preferences WHERE id=1", [], |r| r.get(0))
        .optional()
        .map_err(|_| ErrorCode::StorageUnavailable)?;
    let value = match stored {
        Some(value) => serde_json::from_str::<serde_json::Value>(&value)
            .map_err(|_| ErrorCode::StorageUnavailable)?,
        None => {
            serde_json::to_value(Settings::default()).map_err(|_| ErrorCode::StorageUnavailable)?
        }
    };
    let settings = serde_json::from_value::<Settings>(value.clone())
        .map_err(|_| ErrorCode::StorageUnavailable)?;
    let serde_json::Value::Object(record) = value else {
        return Err(ErrorCode::StorageUnavailable);
    };
    Ok((settings, record))
}
fn write_settings_record(
    connection: &Connection,
    record: serde_json::Map<String, serde_json::Value>,
) -> Result<(), ErrorCode> {
    let value = serde_json::to_string(&record).map_err(|_| ErrorCode::InvalidSettings)?;
    connection.execute("INSERT INTO preferences(id,value) VALUES(1,?1) ON CONFLICT(id) DO UPDATE SET value=excluded.value", [value])
        .map(|_| ()).map_err(|_| ErrorCode::StorageUnavailable)
}
impl SettingsRepository for SqliteSettings {
    fn load(&self) -> Result<Settings, ErrorCode> {
        read_settings_record(&self.connection).map(|(settings, _)| settings)
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
            project_folder: None,
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

#[cfg(test)]
mod discovery_tests {
    use super::*;
    #[test]
    fn legacy_preferences_and_one_time_default_initialization() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("home");
        std::fs::create_dir(&home).unwrap();
        let mut db = SqliteSettings::open(&temp.path().join("data")).unwrap();
        db.connection
            .execute(
                "INSERT INTO preferences VALUES(1,?1)",
                [r#"{"language":"ru","theme":"dark"}"#],
            )
            .unwrap();
        db.initialize_project_folder(&home).unwrap();
        let settings = db.load().unwrap();
        assert_eq!(settings.language, "ru");
        assert_eq!(settings.theme, devforge_core::Theme::Dark);
        assert_eq!(
            settings.project_folder.as_deref(),
            Some(home.join("Devforge").to_str().unwrap())
        );
        std::fs::remove_dir(home.join("Devforge")).unwrap();
        db.initialize_project_folder(&home).unwrap();
        assert!(!home.join("Devforge").exists());
        let stale: devforge_core::SettingsPatch =
            serde_json::from_str(r#"{"language":"en"}"#).unwrap();
        let updated = stale.apply(settings.clone());
        assert_eq!(updated.project_folder, settings.project_folder);
        assert_eq!(updated.theme, settings.theme);
        db.save(&updated).unwrap();
        drop(db);
        assert_eq!(
            SqliteSettings::open(&temp.path().join("data"))
                .unwrap()
                .load()
                .unwrap(),
            updated
        );
    }
    #[test]
    fn resolve_rejects_invalid_missing_and_file_without_creating() {
        let home = tempfile::tempdir().unwrap();
        std::fs::create_dir(home.path().join("Devforge")).unwrap();
        assert_eq!(
            resolve_project_folder("~/Devforge", home.path()).unwrap(),
            dunce::simplified(&std::fs::canonicalize(home.path().join("Devforge")).unwrap())
                .to_path_buf()
        );
        for input in ["", "  ", "relative", "bad\npath"] {
            assert_eq!(
                resolve_project_folder(input, home.path()),
                Err(ErrorCode::InvalidProjectFolder)
            );
        }
        let missing = home.path().join("missing");
        assert_eq!(
            resolve_project_folder(missing.to_str().unwrap(), home.path()),
            Err(ErrorCode::ProjectFolderMissing)
        );
        assert!(!missing.exists());
        let file = home.path().join("file");
        std::fs::write(&file, "").unwrap();
        assert_eq!(
            resolve_project_folder(file.to_str().unwrap(), home.path()),
            Err(ErrorCode::InvalidProjectFolder)
        );
    }
}

#[cfg(test)]
mod atomic_settings_tests {
    use super::*;
    use devforge_core::Theme;
    use std::sync::{Arc, Barrier};
    fn registry() -> LocaleRegistry {
        LocaleRegistry::from_json(include_str!("../../../shared/locales/registry.json")).unwrap()
    }
    #[test]
    fn stale_gui_snapshot_does_not_overwrite_other_connection_folder_or_language() {
        let temp = tempfile::tempdir().unwrap();
        let mut gui = SqliteSettings::open(temp.path()).unwrap();
        let mut manager = SqliteSettings::open(temp.path()).unwrap();
        let stale_gui = gui.load().unwrap();
        let folder = temp.path().join("chosen").to_string_lossy().into_owned();
        manager
            .patch(
                SettingsPatch {
                    language: Some("ru".into()),
                    project_folder: Some(folder.clone()),
                    ..Default::default()
                },
                &registry(),
            )
            .unwrap();
        assert_eq!(stale_gui.language, "system");
        let merged = gui
            .patch(
                SettingsPatch {
                    theme: Some(Theme::Dark),
                    ..Default::default()
                },
                &registry(),
            )
            .unwrap();
        assert_eq!(merged.language, "ru");
        assert_eq!(merged.project_folder.as_deref(), Some(folder.as_str()));
        assert_eq!(merged.theme, Theme::Dark);
        assert_eq!(manager.load().unwrap(), merged);
    }
    #[test]
    fn simultaneous_independent_connections_merge_disjoint_fields() {
        let temp = tempfile::tempdir().unwrap();
        let mut gui = SqliteSettings::open(temp.path()).unwrap();
        let mut manager = SqliteSettings::open(temp.path()).unwrap();
        let gate = Arc::new(Barrier::new(3));
        let gui_gate = gate.clone();
        let a = std::thread::spawn(move || {
            gui_gate.wait();
            gui.patch(
                SettingsPatch {
                    language: Some("uz-Latn".into()),
                    ..Default::default()
                },
                &registry(),
            )
            .unwrap();
        });
        let manager_gate = gate.clone();
        let folder = temp.path().join("manual").to_string_lossy().into_owned();
        let chosen = folder.clone();
        let b = std::thread::spawn(move || {
            manager_gate.wait();
            manager
                .patch(
                    SettingsPatch {
                        theme: Some(Theme::Light),
                        project_folder: Some(chosen),
                        ..Default::default()
                    },
                    &registry(),
                )
                .unwrap();
        });
        gate.wait();
        a.join().unwrap();
        b.join().unwrap();
        let stored = SqliteSettings::open(temp.path()).unwrap().load().unwrap();
        assert_eq!(stored.language, "uz-Latn");
        assert_eq!(stored.theme, Theme::Light);
        assert_eq!(stored.project_folder, Some(folder));
    }
    #[test]
    fn first_folder_initialization_racing_manual_patch_preserves_manual_choice() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("home");
        std::fs::create_dir(&home).unwrap();
        let mut initializer = SqliteSettings::open(&temp.path().join("data")).unwrap();
        let mut manager = SqliteSettings::open(&temp.path().join("data")).unwrap();
        let gate = Arc::new(Barrier::new(3));
        let init_gate = gate.clone();
        let a = std::thread::spawn(move || {
            init_gate.wait();
            initializer.initialize_project_folder(&home).unwrap();
        });
        let folder = temp
            .path()
            .join("manually-selected")
            .to_string_lossy()
            .into_owned();
        let chosen = folder.clone();
        let manager_gate = gate.clone();
        let b = std::thread::spawn(move || {
            manager_gate.wait();
            manager
                .patch(
                    SettingsPatch {
                        project_folder: Some(chosen),
                        language: Some("ru".into()),
                        theme: Some(Theme::Dark),
                    },
                    &registry(),
                )
                .unwrap();
        });
        gate.wait();
        a.join().unwrap();
        b.join().unwrap();
        let settings = SqliteSettings::open(&temp.path().join("data"))
            .unwrap()
            .load()
            .unwrap();
        assert_eq!(settings.project_folder, Some(folder));
        assert_eq!(settings.language, "ru");
        assert_eq!(settings.theme, Theme::Dark);
    }
    #[test]
    fn unknown_saved_locale_and_future_fields_survive_unrelated_patch_and_initialization() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("home");
        std::fs::create_dir(&home).unwrap();
        let mut db = SqliteSettings::open(&temp.path().join("data")).unwrap();
        let raw = r#"{"language":"removed-locale","theme":"light","future":{"density":"compact"}}"#;
        db.connection
            .execute("INSERT INTO preferences VALUES(1,?1)", [raw])
            .unwrap();
        db.initialize_project_folder(&home).unwrap();
        let settings = db
            .patch(
                SettingsPatch {
                    theme: Some(Theme::Dark),
                    ..Default::default()
                },
                &registry(),
            )
            .unwrap();
        assert_eq!(settings.language, "removed-locale");
        let stored: String = db
            .connection
            .query_row("SELECT value FROM preferences", [], |r| r.get(0))
            .unwrap();
        let value: serde_json::Value = serde_json::from_str(&stored).unwrap();
        assert_eq!(value["future"]["density"], "compact");
        assert_eq!(
            value["project_folder"],
            home.join("Devforge").to_string_lossy().as_ref()
        );
        assert_eq!(
            db.patch(
                SettingsPatch {
                    language: Some("removed-locale".into()),
                    ..Default::default()
                },
                &registry()
            ),
            Err(ErrorCode::InvalidSettings)
        );
        assert_eq!(
            db.patch(
                SettingsPatch {
                    project_folder: Some("relative".into()),
                    ..Default::default()
                },
                &registry()
            ),
            Err(ErrorCode::InvalidProjectFolder)
        );
        db.patch(SettingsPatch::default(), &registry()).unwrap();
        assert_eq!(
            db.connection
                .query_row("SELECT value FROM preferences", [], |r| r
                    .get::<_, String>(0))
                .unwrap(),
            stored
        );
    }
}
