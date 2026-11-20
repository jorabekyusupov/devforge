use super::*;
use crate::{
    cache::CacheInstance,
    database::{Engine, Instance},
};
use devforge_runtime::package::{Archive, Target};
use std::{fs, net::TcpListener, path::Path};

struct Fixture {
    _root: tempfile::TempDir,
    store: ProjectStore,
    catalog: Catalog,
    replacements: BTreeMap<String, String>,
}

fn package(kind: &str, version: &str) -> Package {
    Package {
        kind: kind.into(),
        version: version.into(),
        variant: "server".into(),
        target: Target::current().unwrap(),
        url: "https://example.invalid/fixture.tar.gz".into(),
        sha256: "0".repeat(64),
        archive: Archive::TarGz,
        executables: BTreeMap::from([("fixture".into(), "bin/fixture".into())]),
        resources: BTreeMap::new(),
    }
}

fn unused_port() -> u16 {
    TcpListener::bind(("127.0.0.1", 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn instance_files(root: &Path, directory: &str, id: &str, marker: &[u8]) {
    let path = root.join(directory).join(id);
    fs::create_dir_all(&path).unwrap();
    fs::write(path.join("instance.lock"), []).unwrap();
    fs::write(path.join("initialized"), marker).unwrap();
    fs::write(path.join("retained-data"), b"fixture data must survive").unwrap();
}

fn fixture() -> Fixture {
    let root = tempfile::tempdir().unwrap();
    let store = ProjectStore::open(root.path()).unwrap();
    let mysql = Instance {
        engine: Engine::Mysql,
        id: "database".into(),
        package: "mysql-old".into(),
        port: unused_port(),
        admin: "fixture-admin".into(),
        secret_ref: "fixture/database-secret".into(),
        ready: true,
        settings: Default::default(),
    };
    let cache = CacheInstance {
        id: "cache".into(),
        project_id: "fixture-project".into(),
        package: "garnet-old".into(),
        runtime: "dotnet-old".into(),
        port: unused_port(),
        username: "fixture-cache-user".into(),
        secret_ref: "fixture/cache-secret".into(),
        admin_ref: "fixture/cache-admin".into(),
        ready: true,
        settings: Default::default(),
    };
    store
        .connection
        .execute(
            "INSERT INTO instances VALUES(?1,?2)",
            params![mysql.id, serde_json::to_string(&mysql).unwrap()],
        )
        .unwrap();
    store
        .connection
        .execute(
            "INSERT INTO cache_instances VALUES(?1,?2)",
            params![cache.id, serde_json::to_string(&cache).unwrap()],
        )
        .unwrap();
    instance_files(store.root(), "databases", &mysql.id, b"devforge-mysql-v1\n");
    instance_files(
        store.root(),
        "cache-services",
        &cache.id,
        b"cache fixture\n",
    );
    let mut catalog = Catalog {
        packages: BTreeMap::new(),
        superseded: BTreeMap::new(),
        defaults: BTreeMap::new(),
    };
    for (id, kind, version) in [
        ("mysql-old", "mysql", "8.4.11"),
        ("mysql-new", "mysql", "8.4.13"),
        ("garnet-old", "garnet", "2.2.10"),
        ("garnet-new", "garnet", "2.2.11"),
        ("dotnet-old", "dotnet", "10.0.14"),
        ("dotnet-new", "dotnet", "10.0.15"),
    ] {
        catalog.packages.insert(id.into(), package(kind, version));
    }
    Fixture {
        _root: root,
        store,
        catalog,
        replacements: BTreeMap::from([
            ("mysql-old".into(), "mysql-new".into()),
            ("garnet-old".into(), "garnet-new".into()),
            ("dotnet-old".into(), "dotnet-new".into()),
        ]),
    }
}

fn records(store: &ProjectStore) -> (serde_json::Value, serde_json::Value) {
    (
        serde_json::to_value(store.instance("database").unwrap()).unwrap(),
        serde_json::to_value(store.cache_instance("cache").unwrap()).unwrap(),
    )
}

#[test]
fn stopped_database_and_both_cache_package_fields_change_without_touching_data_or_identity() {
    let f = fixture();
    let (mut expected_db, mut expected_cache) = records(&f.store);
    expected_db["package"] = "mysql-new".into();
    expected_cache["package"] = "garnet-new".into();
    expected_cache["runtime"] = "dotnet-new".into();
    assert_eq!(
        f.store
            .apply_runtime_patch(&f.replacements, &f.catalog)
            .unwrap(),
        2
    );
    assert_eq!(records(&f.store), (expected_db, expected_cache));
    for (directory, id) in [("databases", "database"), ("cache-services", "cache")] {
        assert_eq!(
            fs::read(
                f.store
                    .root()
                    .join(directory)
                    .join(id)
                    .join("retained-data")
            )
            .unwrap(),
            b"fixture data must survive"
        );
    }
}

#[test]
fn held_cache_lifetime_lock_refuses_all_registry_changes_and_releases_prior_locks() {
    let f = fixture();
    let before = records(&f.store);
    let cache_lock = OpenOptions::new()
        .read(true)
        .write(true)
        .open(f.store.root().join("cache-services/cache/instance.lock"))
        .unwrap();
    cache_lock.try_lock().unwrap();
    assert!(matches!(
        f.store.apply_runtime_patch(&f.replacements, &f.catalog),
        Err(E::InstanceBusy)
    ));
    assert_eq!(records(&f.store), before);
    let database_lock = OpenOptions::new()
        .read(true)
        .write(true)
        .open(f.store.root().join("databases/database/instance.lock"))
        .unwrap();
    database_lock
        .try_lock()
        .expect("an earlier acquired database lock must be released on refusal");
}

#[test]
fn cache_write_failure_rolls_back_the_prior_database_write() {
    let f = fixture();
    let before = records(&f.store);
    f.store.connection.execute_batch("CREATE TRIGGER reject_cache_patch BEFORE UPDATE ON cache_instances BEGIN SELECT RAISE(FAIL, 'fixture rejected cache update'); END;").unwrap();
    assert!(matches!(
        f.store.apply_runtime_patch(&f.replacements, &f.catalog),
        Err(E::StorageUnavailable)
    ));
    assert_eq!(records(&f.store), before);
}

#[test]
fn unready_database_refuses_updates_without_mutating_other_users() {
    let f = fixture();
    fs::remove_file(f.store.root().join("databases/database/initialized")).unwrap();
    let before = records(&f.store);
    assert!(matches!(
        f.store.apply_runtime_patch(&f.replacements, &f.catalog),
        Err(E::RecoveryRequired)
    ));
    assert_eq!(records(&f.store), before);
}

#[test]
fn another_store_operation_refuses_patch_without_registry_changes() {
    let f = fixture();
    let before = records(&f.store);
    let _operation = f.store.operation_lock().unwrap();
    assert!(matches!(
        f.store.apply_runtime_patch(&f.replacements, &f.catalog),
        Err(E::InstanceBusy)
    ));
    assert_eq!(records(&f.store), before);
}

#[test]
fn occupied_database_port_refuses_patch_without_registry_changes() {
    let f = fixture();
    let before = records(&f.store);
    let port = f.store.instance("database").unwrap().port;
    let _foreign_listener = TcpListener::bind(("127.0.0.1", port)).unwrap();
    assert!(matches!(
        f.store.apply_runtime_patch(&f.replacements, &f.catalog),
        Err(E::InstanceBusy)
    ));
    assert_eq!(records(&f.store), before);
}
