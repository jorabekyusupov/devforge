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
