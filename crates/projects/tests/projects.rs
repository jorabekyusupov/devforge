use devforge_core::{ErrorCode, SecretStore};
use devforge_projects::{
    ProjectError, environment,
    project::{Catalog, ProjectStore, inspect},
};
use std::{collections::BTreeMap, fs, path::Path, sync::Mutex};
#[derive(Default)]
struct Secrets(Mutex<BTreeMap<String, String>>);
impl SecretStore for Secrets {
    fn get(&self, k: &str) -> Result<Option<String>, ErrorCode> {
        Ok(self.0.lock().unwrap().get(k).cloned())
    }
    fn set(&self, k: &str, v: &str) -> Result<(), ErrorCode> {
        self.0.lock().unwrap().insert(k.into(), v.into());
        Ok(())
    }
    fn delete(&self, k: &str) -> Result<(), ErrorCode> {
        self.0.lock().unwrap().remove(k);
        Ok(())
    }
}
fn project(path: &Path, php: &str, node: &str) {
    fs::create_dir_all(path.join("public")).unwrap();
    fs::write(path.join("public/index.php"), b"<?php").unwrap();
    fs::write(path.join("artisan"), b"<?php").unwrap();
    fs::write(path.join("composer.json"),serde_json::to_vec(&serde_json::json!({"require":{"php":php,"laravel/framework":"^13"},"scripts":{"post-install-cmd":"touch should-not-run"}})).unwrap()).unwrap();
    fs::write(path.join("package.json"),serde_json::to_vec(&serde_json::json!({"engines":{"node":node},"scripts":{"postinstall":"touch should-not-run"}})).unwrap()).unwrap();
}
fn selection(c: &Catalog, kind: &str, version: &str) -> String {
    c.packages
        .iter()
        .find(|(_, p)| {
            p.kind == kind
                && p.version.starts_with(version)
                && p.target == devforge_runtime::package::Target::current().unwrap()
                && (kind != "php" || p.executables.contains_key("php"))
        })
        .unwrap()
        .0
        .clone()
}
#[test]
fn import_is_read_only_and_registry_survives_reopen_and_forget() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("Ўзбек & Русский ' A");
    project(&path, "^8.3", "^22 || ^24");
    fs::write(path.join(".env"), b"APP_KEY=keep\n").unwrap();
    let c = Catalog::bundled();
    let php = selection(&c, "php", "8.4");
    let node = selection(&c, "node", "v22");
    let store = ProjectStore::open(root.path().join("metadata")).unwrap();
    let p = store.import(&path, "A", &php, &node, &c).unwrap();
    assert!(!path.join("should-not-run").exists());
    assert_eq!(fs::read(path.join(".env")).unwrap(), b"APP_KEY=keep\n");
    assert!(!path.join("node_modules").exists());
    assert!(!path.join("vendor").exists());
    assert_eq!(
        store.import(&path, "Duplicate", &php, &node, &c).err(),
        Some(ProjectError::DuplicateProject)
    );
    drop(store);
    let store = ProjectStore::open(root.path().join("metadata")).unwrap();
    assert_eq!(store.get(&p.id).unwrap().php, php);
    store.forget(&p.id, 1).unwrap();
    assert!(store.list().unwrap().is_empty());
    assert!(path.join("composer.json").exists());
    assert_eq!(fs::read(path.join(".env")).unwrap(), b"APP_KEY=keep\n");
}
#[test]
fn php_composer_and_npm_range_semantics_are_not_interchanged() {
    let root = tempfile::tempdir().unwrap();
    let c = Catalog::bundled();
    let store = ProjectStore::open(root.path().join("meta")).unwrap();
    let php = selection(&c, "php", "8.4");
    let node = selection(&c, "node", "v22");
    let path = root.path().join("A");
    project(&path, "~8.4", ">=22 <25");
    store.import(&path, "A", &php, &node, &c).unwrap();
    let path = root.path().join("B");
    project(&path, "~8.5.0", "*");
    assert_eq!(
        store.import(&path, "B", &php, &node, &c).err(),
        Some(ProjectError::IncompatiblePhp)
    );
    project(&path, "^8.3", ">=24");
    assert_eq!(
        store.import(&path, "B", &php, &node, &c).err(),
        Some(ProjectError::IncompatibleNode)
    );
    fs::write(path.join("composer.json"), b"not json").unwrap();
    assert_eq!(inspect(&path).err(), Some(ProjectError::InvalidManifest));
    assert_eq!(store.list().unwrap().len(), 1);
}
#[test]
fn project_changes_are_isolated_and_stale_revisions_rejected() {
    let root = tempfile::tempdir().unwrap();
    let c = Catalog::bundled();
    let store = ProjectStore::open(root.path().join("meta")).unwrap();
    let php = selection(&c, "php", "8.4");
    let node = selection(&c, "node", "v22");
    let a = root.path().join("A");
    let b = root.path().join("B");
    project(&a, "^8.3", "^22 || ^24");
    project(&b, "^8.3", "^22 || ^24");
    let a = store.import(&a, "A", &php, &node, &c).unwrap();
    let b = store.import(&b, "B", &php, &node, &c).unwrap();
    let changed = store
        .configure(
            &a.id,
            1,
            "A2",
            &selection(&c, "php", "8.5"),
            &selection(&c, "node", "v24"),
            &c,
        )
        .unwrap();
    assert_eq!(changed.revision, 2);
    assert_eq!(store.get(&b.id).unwrap().node, node);
    assert_eq!(store.get(&b.id).unwrap().revision, 1);
    assert_eq!(
        store.configure(&a.id, 1, "stale", &php, &node, &c).err(),
        Some(ProjectError::StaleRevision)
    );
}
#[test]
fn secret_environment_values_never_enter_sqlite_or_metadata() {
    let root = tempfile::tempdir().unwrap();
    let c = Catalog::bundled();
    let store = ProjectStore::open(root.path().join("meta")).unwrap();
    let path = root.path().join("A");
    project(&path, "^8.3", "*");
    let p = store
        .import(
            &path,
            "A",
            &selection(&c, "php", "8.4"),
            &selection(&c, "node", "v22"),
            &c,
        )
        .unwrap();
    let secrets = Secrets::default();
    let p = environment::set(
        &store,
        &secrets,
        &p.id,
        p.revision,
        "APP_KEY",
        "private-fixture-value-123",
    )
    .unwrap();
    assert!(p.environment_names.contains(&"APP_KEY".into()));
    assert!(
        !String::from_utf8_lossy(&fs::read(store.root().join("projects.sqlite3")).unwrap())
            .contains("private-fixture-value-123")
    );
    assert!(
        !serde_json::to_string(&p)
            .unwrap()
            .contains("private-fixture-value-123")
    );
    for key in [
        "PATH",
        "DB_PASSWORD",
        "PHPRC",
        "NODE_OPTIONS",
        "LD_PRELOAD",
        "ZDOTDIR",
    ] {
        assert_eq!(
            environment::set(&store, &secrets, &p.id, p.revision, key, "bad").err(),
            Some(ProjectError::InvalidEnvironment)
        );
    }
}

#[test]
fn schema_one_and_pre_service_project_records_upgrade_without_losing_bindings() {
    let root = tempfile::tempdir().unwrap();
    let catalog = Catalog::bundled();
    let php = selection(&catalog, "php", "8.4");
    let node = selection(&catalog, "node", "v22");
    let path = root.path().join("legacy");
    project(&path, "^8.3", "*");
    let metadata = root.path().join("metadata");
    let store = ProjectStore::open(&metadata).unwrap();
    let p = store
        .import(&path, "legacy", &php, &node, &catalog)
        .unwrap();
    drop(store);
    let connection = rusqlite::Connection::open(metadata.join("projects.sqlite3")).unwrap();
    let raw: String = connection
        .query_row("SELECT record FROM projects WHERE id=?1", [&p.id], |r| {
            r.get(0)
        })
        .unwrap();
    let mut record: serde_json::Value = serde_json::from_str(&raw).unwrap();
    record.as_object_mut().unwrap().remove("services");
    connection
        .execute(
            "UPDATE projects SET record=?1 WHERE id=?2",
            rusqlite::params![record.to_string(), p.id],
        )
        .unwrap();
    connection
        .execute_batch(
            "DROP TABLE cache_instances;DROP TABLE database_binding_history;PRAGMA user_version=1",
        )
        .unwrap();
    drop(connection);
    let store = ProjectStore::open(&metadata).unwrap();
    let actual = store.get(&p.id).unwrap();
    assert_eq!(actual.php, php);
    assert!(!actual.services.uses_redis());
    assert!(store.cache_instances().unwrap().is_empty());
    assert!(store.binding_history(&p.id).unwrap().is_empty());
    let legacy:devforge_projects::database::Instance=serde_json::from_value(serde_json::json!({"id":"old","package":"pg","port":5432,"admin":"owned","secret_ref":"old-ref","ready":true})).unwrap();
    assert_eq!(
        legacy.engine,
        devforge_projects::database::Engine::Postgresql
    );
}

#[test]
fn staged_generations_preserve_active_binding_and_use_engine_specific_role_limits() {
    use devforge_projects::database::{Engine, Instance};
    let root = tempfile::tempdir().unwrap();
    let catalog = Catalog::bundled();
    let php = selection(&catalog, "php", "8.4");
    let node = selection(&catalog, "node", "v22");
    let path = root.path().join("project");
    project(&path, "^8.3", "*");
    let metadata = root.path().join("metadata");
    let store = ProjectStore::open(&metadata).unwrap();
    let p = store.import(&path, "A", &php, &node, &catalog).unwrap();
    let connection = rusqlite::Connection::open(metadata.join("projects.sqlite3")).unwrap();
    for (id, engine) in [("pg", Engine::Postgresql), ("mysql", Engine::Mysql)] {
        let i = Instance {
            engine,
            id: id.into(),
            package: id.into(),
            port: 1,
            admin: "admin".into(),
            secret_ref: "admin-ref".into(),
            ready: true,
        };
        connection
            .execute(
                "INSERT INTO instances VALUES(?1,?2)",
                rusqlite::params![id, serde_json::to_string(&i).unwrap()],
            )
            .unwrap();
    }
    let secrets = Secrets::default();
    let mut pg = store.staged_binding(&p.id, "pg", &secrets).unwrap();
    assert_eq!(pg.role, pg.database);
    let mut mysql = store.staged_binding(&p.id, "mysql", &secrets).unwrap();
    assert!(mysql.role.len() <= 32);
    assert_ne!(pg.secret_ref, mysql.secret_ref);
    assert!(store.binding(&p.id).unwrap().is_none());
    assert!(store.retained_binding(&p.id, "pg").unwrap().is_none());
    assert_eq!(
        store.activate_binding(&p.id, Some(&pg)).err(),
        Some(ProjectError::RecoveryRequired)
    );
    pg.ready = true;
    mysql.ready = true;
    store.activate_binding(&p.id, Some(&pg)).unwrap();
    store.activate_binding(&p.id, Some(&mysql)).unwrap();
    assert_eq!(
        store.binding(&p.id).unwrap().unwrap().database,
        mysql.database
    );
    assert_eq!(
        store
            .retained_binding(&p.id, "pg")
            .unwrap()
            .unwrap()
            .database,
        pg.database
    );
    let mut bad = pg.clone();
    bad.project_id = "another-project".into();
    assert_eq!(
        store.activate_binding(&p.id, Some(&bad)).err(),
        Some(ProjectError::DatabaseConflict)
    );
    assert_eq!(
        store.binding(&p.id).unwrap().unwrap().database,
        mysql.database
    );
    store.activate_binding(&p.id, None).unwrap();
    assert!(store.binding(&p.id).unwrap().is_none());
    assert_eq!(
        store
            .retained_binding(&p.id, "mysql")
            .unwrap()
            .unwrap()
            .database,
        mysql.database
    );
}

#[test]
fn schema_four_records_keep_environment_and_runtime_selections_in_schema_eight() {
    let root = tempfile::tempdir().unwrap();
    let catalog = Catalog::bundled();
    let php = selection(&catalog, "php", "8.4");
    let node = selection(&catalog, "node", "v22");
    let path = root.path().join("existing");
    project(&path, "^8.3", "*");
    let metadata = root.path().join("metadata");
    let store = ProjectStore::open(&metadata).unwrap();
    let p = store
        .import(&path, "Existing M11 project", &php, &node, &catalog)
        .unwrap();
    drop(store);
    let connection = rusqlite::Connection::open(metadata.join("projects.sqlite3")).unwrap();
    let mut json = serde_json::to_value(&p).unwrap();
    json.as_object_mut().unwrap().remove("worktree");
    json.as_object_mut().unwrap().remove("filesystem_identity");
    connection
        .execute(
            "UPDATE projects SET record=?1 WHERE id=?2",
            rusqlite::params![json.to_string(), p.id],
        )
        .unwrap();
    connection.pragma_update(None, "user_version", 4).unwrap();
    drop(connection);
    let store = ProjectStore::open(&metadata).unwrap();
    let loaded = store.get(&p.id).unwrap();
    assert_eq!(loaded.php, p.php);
    assert_eq!(loaded.node, p.node);
    assert_eq!(loaded.revision, p.revision);
    assert_eq!(loaded.path, p.path);
    assert!(loaded.worktree.is_none());
    let connection = rusqlite::Connection::open(metadata.join("projects.sqlite3")).unwrap();
    assert_eq!(
        connection
            .pragma_query_value(None, "user_version", |r| r.get::<_, u32>(0))
            .unwrap(),
        8
    );
}

#[test]
fn provider_rebuild_keeps_legacy_pins_and_selects_a_separate_reviewed_family() {
    let catalog = Catalog::bundled();
    let old = "php-8.5.12-cli-macos-arm64";
    assert_eq!(
        catalog.get(old).unwrap().sha256,
        "cfbb53b904bb29f553c23cb51dfb9ec82bb69adaf9aeb6df8b6da29ea9f03553"
    );
    let next = catalog.superseded.get(old).unwrap();
    let cli = catalog.get(next).unwrap();
    assert_eq!(cli.version, "8.5.12-staticphp20261113");
    assert_ne!(cli.key().unwrap(), catalog.get(old).unwrap().key().unwrap());
    assert_eq!(catalog.php_server(cli).unwrap().version, cli.version);
    assert_eq!(
        catalog
            .php_server(catalog.get(old).unwrap())
            .unwrap()
            .version,
        "8.5.12"
    );
    let portable=devforge_projects::manifest::Manifest::parse(b"schema: 1\nproject:\n  name: Fixture\n  domain: fixture.test\nruntimes:\n  php: 8.5.12\n  node: 24.22.2\n  composer: 2.11.2\npins: {}\n").unwrap();
    assert_eq!(
        portable
            .resolve(&catalog, devforge_runtime::package::Target::MacosArm64)
            .unwrap()["php"],
        *next
    );
    let mut locked = portable;
    locked
        .pins
        .insert(old.into(), catalog.get(old).unwrap().sha256.clone());
    assert_eq!(
        locked
            .resolve(&catalog, devforge_runtime::package::Target::MacosArm64)
            .unwrap()["php"],
        old
    );
}

#[test]
fn schema_six_from_either_history_preserves_debugger_profiles_and_discovery() {
    for with_ledger in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let catalog = Catalog::bundled();
        let php = selection(&catalog, "php", "8.4");
        let node = selection(&catalog, "node", "v22");
        let path = root.path().join("project");
        project(&path, "^8.3", "*");
        let data = root.path().join("metadata");
        let store = ProjectStore::open(&data).unwrap();
        let imported = store
            .import(&path, "Preserved", &php, &node, &catalog)
            .unwrap();
        store
            .save_debugger(
                &imported.id,
                imported.revision,
                devforge_debugger::Settings {
                    mode: devforge_debugger::Mode::Profile,
                    runtime: Some("a".repeat(32)),
                    breakpoints: vec![],
                },
            )
            .unwrap();
        let expected = serde_json::to_value(store.get(&imported.id).unwrap()).unwrap();
        if with_ledger {
            store
                .save_discovery(&devforge_projects::discovery::Entry {
                    identity: devforge_projects::discovery::identity(&path).unwrap(),
                    root: root.path().to_owned(),
                    path: path.clone(),
                    name: "Preserved".into(),
                    project_id: Some(imported.id.clone()),
                    state: "ignored".into(),
                    problem: None,
                    ignored: true,
                })
                .unwrap();
        }
        drop(store);
        let db = rusqlite::Connection::open(data.join("projects.sqlite3")).unwrap();
        if !with_ledger {
            db.execute_batch("DROP TABLE discovery_entries").unwrap();
        }
        db.pragma_update(None, "user_version", 6).unwrap();
        drop(db);
        let store = ProjectStore::open(&data).unwrap();
        assert_eq!(
            serde_json::to_value(store.get(&imported.id).unwrap()).unwrap(),
            expected
        );
        assert_eq!(
            store.discovery_entries().unwrap().len(),
            usize::from(with_ledger)
        );
        drop(store);
        // Replay must be harmless after either migration history.
        let reopened = ProjectStore::open(&data).unwrap();
        assert_eq!(
            serde_json::to_value(reopened.get(&imported.id).unwrap()).unwrap(),
            expected
        );
    }
}

#[test]
fn every_reviewed_php_family_pairs_web_and_cli_without_replacing_locked_ids() {
    let catalog = Catalog::bundled();
    for (legacy, replacement) in &catalog.superseded {
        let old = catalog.get(legacy).unwrap();
        let next = catalog.get(replacement).unwrap();
        assert_ne!(old.key().unwrap(), next.key().unwrap());
        assert_ne!(old.sha256, next.sha256);
        assert_eq!(old.target, next.target);
        assert_eq!(
            old.version.split('-').next(),
            next.version.split('-').next()
        );
        if next.executables.contains_key("php") {
            assert_eq!(catalog.php_server(next).unwrap().version, next.version);
            assert_eq!(catalog.php_server(old).unwrap().version, old.version);
        }
    }
    assert!(catalog.superseded.contains_key("php-8.5.12-cli-macos-x64"));
    assert!(catalog.superseded.contains_key("php-8.5.12-fpm-macos-x64"));
}

#[test]
fn composer_binding_survives_catalog_default_changes_and_requires_explicit_revision() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("Composer project");
    project(&path, "^8.3", "*");
    let mut catalog = Catalog::bundled();
    let store = ProjectStore::open(root.path().join("meta")).unwrap();
    let p = store
        .import(
            &path,
            "Composer",
            &selection(&catalog, "php", "8.4"),
            &selection(&catalog, "node", "v22"),
            &catalog,
        )
        .unwrap();
    let original = p.composer.clone();
    let mut second = catalog.composer(&original).unwrap().clone();
    second.version = "2.99.0".into();
    let second_id = "composer-test-second";
    catalog.packages.insert(second_id.into(), second);
    catalog.defaults.insert("composer".into(), "2.99.0".into());
    let p = store
        .configure(&p.id, p.revision, "Renamed", &p.php, &p.node, &catalog)
        .unwrap();
    assert_eq!(p.composer, original);
    assert_eq!(store.get(&p.id).unwrap().composer, original);
    assert_eq!(
        store
            .configure_composer(&p.id, p.revision - 1, second_id, &catalog)
            .err(),
        Some(ProjectError::StaleRevision)
    );
    assert_eq!(
        store
            .configure_composer(&p.id, p.revision, &p.php, &catalog)
            .err(),
        Some(ProjectError::RuntimeUnavailable)
    );
    let p = store
        .configure_composer(&p.id, p.revision, second_id, &catalog)
        .unwrap();
    assert_eq!(p.composer, second_id);
    drop(store);
    assert_eq!(
        ProjectStore::open(root.path().join("meta"))
            .unwrap()
            .get(&p.id)
            .unwrap()
            .composer,
        second_id
    );
}

#[test]
fn schema_seven_backfills_historical_composer_without_resetting_record_or_revision() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("Legacy");
    project(&path, "^8.3", "*");
    let catalog = Catalog::bundled();
    let metadata = root.path().join("meta");
    let store = ProjectStore::open(&metadata).unwrap();
    let p = store
        .import(
            &path,
            "Legacy",
            &selection(&catalog, "php", "8.4"),
            &selection(&catalog, "node", "v22"),
            &catalog,
        )
        .unwrap();
    drop(store);
    let db = rusqlite::Connection::open(metadata.join("projects.sqlite3")).unwrap();
    let mut value = serde_json::to_value(&p).unwrap();
    value.as_object_mut().unwrap().remove("composer");
    value["future_preserved"] = "keep".into();
    db.execute(
        "UPDATE projects SET record=?1 WHERE id=?2",
        rusqlite::params![value.to_string(), p.id],
    )
    .unwrap();
    db.pragma_update(None, "user_version", 7).unwrap();
    drop(db);
    let store = ProjectStore::open(&metadata).unwrap();
    let migrated = store.get(&p.id).unwrap();
    assert_eq!(migrated.composer, p.composer);
    assert_eq!(migrated.revision, p.revision);
    assert_eq!(migrated.php, p.php);
    assert_eq!(migrated.node, p.node);
    let db = rusqlite::Connection::open(metadata.join("projects.sqlite3")).unwrap();
    let raw: String = db
        .query_row("SELECT record FROM projects WHERE id=?1", [&p.id], |r| {
            r.get(0)
        })
        .unwrap();
    let preserved: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(preserved["composer"], p.composer);
    assert_eq!(preserved["future_preserved"], "keep");
    assert_eq!(
        db.pragma_query_value(None, "user_version", |r| r.get::<_, u32>(0))
            .unwrap(),
        8
    );
}
