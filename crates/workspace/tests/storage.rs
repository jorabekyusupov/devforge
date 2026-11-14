use devforge_core::{ErrorCode, SecretStore};
use devforge_runtime::{
    Cancellation,
    package::{Archive, Package, Target, hash_file},
};
use devforge_workspace::{Workspace, storage::StorageLimits};
use std::{collections::BTreeMap, fs, path::Path, sync::Arc};
struct DenySecrets;
impl SecretStore for DenySecrets {
    fn get(&self, _: &str) -> Result<Option<String>, ErrorCode> {
        panic!("inventory must not read credentials")
    }
    fn set(&self, _: &str, _: &str) -> Result<(), ErrorCode> {
        panic!("inventory must not write credentials")
    }
    fn delete(&self, _: &str) -> Result<(), ErrorCode> {
        panic!("inventory must not delete credentials")
    }
}
fn project(w: &Workspace, root: &Path) -> devforge_projects::project::Project {
    fs::create_dir_all(root.join("public")).unwrap();
    fs::write(root.join("public/index.php"), "<?php").unwrap();
    fs::write(
        root.join("composer.json"),
        r#"{"require":{"php":"^8.3","laravel/framework":"^13"}}"#,
    )
    .unwrap();
    fs::write(root.join(".env"), "TOKEN=INVENTORY_SECRET_CANARY").unwrap();
    let choose = |kind: &str| {
        w.catalog
            .packages
            .iter()
            .find(|(_, p)| {
                p.kind == kind
                    && Some(p.target) == Target::current().ok()
                    && (kind != "php" || p.executables.contains_key("php"))
            })
            .unwrap()
            .0
            .clone()
    };
    w.projects
        .import(root, "Storage", &choose("php"), &choose("node"), &w.catalog)
        .unwrap()
}
fn fixture(root: &Path) -> Workspace {
    Workspace::open(root.join("data"), Arc::new(DenySecrets)).unwrap()
}
#[test]
fn inventory_is_read_only_and_does_not_visit_user_project_or_create_missing_stores() {
    let temp = tempfile::tempdir().unwrap();
    let w = fixture(temp.path());
    let p = project(&w, &temp.path().join("USER_PROJECT"));
    let inventory = w.storage_inventory(StorageLimits::default());
    let json = serde_json::to_string(&inventory).unwrap();
    assert!(!json.contains("INVENTORY_SECRET_CANARY"));
    assert!(!json.contains("USER_PROJECT"));
    assert!(!temp.path().join("data/worktrees").exists());
    assert!(!temp.path().join("data/projects/backups").exists());
    assert!(!temp.path().join("data/mail").exists());
    assert_eq!(w.projects.get(&p.id).unwrap().revision, p.revision);
    assert!(
        inventory
            .size_semantics
            .contains("not_allocated_or_freeable")
    );
}
#[test]
fn known_package_has_active_pin_reference_and_unknown_package_is_not_traversed() {
    let temp = tempfile::tempdir().unwrap();
    let mut w = fixture(temp.path());
    let p = project(&w, &temp.path().join("repo"));
    let archive = temp.path().join("composer.fixture");
    fs::write(&archive, "INVENTORY_PAYLOAD_CANARY").unwrap();
    let package = Package {
        kind: "composer".into(),
        version: "2.88.0".into(),
        variant: "fixture".into(),
        target: Target::current().unwrap(),
        url: "https://example.invalid/fixture".into(),
        sha256: hash_file(&archive).unwrap(),
        archive: Archive::File {
            path: "composer.phar".into(),
        },
        executables: BTreeMap::new(),
        resources: BTreeMap::from([("composer".into(), "composer.phar".into())]),
    };
    w.packages
        .install_cached(&package, &archive, &Cancellation::default(), |_| {})
        .unwrap();
    w.catalog
        .packages
        .insert("test-pinned-composer".into(), package);
    w.projects
        .configure_composer(&p.id, p.revision, "test-pinned-composer", &w.catalog)
        .unwrap();
    fs::create_dir_all(temp.path().join("data/runtimes/packages/UNKNOWN")).unwrap();
    fs::write(
        temp.path()
            .join("data/runtimes/packages/UNKNOWN/never-read"),
        "unknown payload",
    )
    .unwrap();
    let inventory = w.storage_inventory(StorageLimits::default());
    let known = inventory
        .resources
        .iter()
        .find(|r| r.resource_id == "test-pinned-composer")
        .unwrap();
    assert_eq!(known.ownership, "package_receipt");
    assert!(known.logical_bytes.unwrap() >= 24);
    assert!(
        known
            .active_refs
            .iter()
            .any(|r| r.id == p.id && r.kind == "project_runtime_selection")
    );
    let unknown = inventory
        .resources
        .iter()
        .find(|r| r.category == "runtime_package" && r.ownership == "unknown")
        .unwrap();
    assert_eq!(unknown.logical_bytes, None);
    assert!(
        unknown
            .problems
            .contains(&"ownership_unverified_not_traversed")
    );
    assert!(
        !serde_json::to_string(&inventory)
            .unwrap()
            .contains("INVENTORY_PAYLOAD_CANARY")
    );
}
#[test]
fn budget_and_depth_truncation_are_explicit_and_oversized_limits_are_refused() {
    let temp = tempfile::tempdir().unwrap();
    let w = fixture(temp.path());
    let p = project(&w, &temp.path().join("repo"));
    let nested = temp
        .path()
        .join("data/projects/config")
        .join(&p.id)
        .join("a/b/c");
    fs::create_dir_all(&nested).unwrap();
    fs::write(nested.join("data"), "bounded").unwrap();
    let budget = w.storage_inventory(StorageLimits {
        max_entries: 2,
        ..Default::default()
    });
    assert!(!budget.measurement_complete);
    assert!(budget.problems.contains(&"entry_limit"));
    assert!(budget.entries_examined <= 2);
    let depth = w.storage_inventory(StorageLimits {
        max_depth: 1,
        ..Default::default()
    });
    let config = depth
        .resources
        .iter()
        .find(|r| r.category == "project_configuration")
        .unwrap();
    assert!(!config.measurement_complete);
    assert!(config.problems.contains(&"depth_limit"));
    let invalid = w.storage_inventory(StorageLimits {
        max_entries: usize::MAX,
        ..Default::default()
    });
    assert_eq!(invalid.problems, ["invalid_limits"]);
    assert!(invalid.resources.is_empty());
}
#[cfg(unix)]
#[test]
fn linked_category_and_nested_symlink_are_never_followed() {
    use std::os::unix::fs::symlink;
    let temp = tempfile::tempdir().unwrap();
    let w = fixture(temp.path());
    let p = project(&w, &temp.path().join("repo"));
    let external = temp.path().join("outside");
    fs::create_dir_all(&external).unwrap();
    fs::write(external.join("outside-data"), vec![0; 65536]).unwrap();
    symlink(&external, temp.path().join("data/mail")).unwrap();
    let config = temp.path().join("data/projects/config").join(&p.id);
    fs::create_dir_all(&config).unwrap();
    symlink(&external, config.join("external-link")).unwrap();
    let inventory = w.storage_inventory(StorageLimits::default());
    assert!(inventory.problems.contains(&"link_not_followed"));
    let config = inventory
        .resources
        .iter()
        .find(|r| r.category == "project_configuration")
        .unwrap();
    assert!(config.problems.contains(&"link_not_followed"));
    assert_eq!(config.logical_bytes, Some(0));
}
#[test]
fn registry_namespace_artifacts_and_unknown_owners_are_distinguished() {
    let temp = tempfile::tempdir().unwrap();
    let w = fixture(temp.path());
    let p = project(&w, &temp.path().join("repo"));
    let backup = temp.path().join("data/projects/backups").join(&p.id);
    fs::create_dir_all(&backup).unwrap();
    fs::write(
        backup.join("not-validated.dfbackup"),
        "opaque-backup-payload",
    )
    .unwrap();
    let mail = temp.path().join("data/mail").join(&p.id);
    fs::create_dir_all(&mail).unwrap();
    fs::write(mail.join(".devforge-inbox"), "devforge-inbox-v1\n").unwrap();
    fs::write(mail.join("inbox.sqlite3"), "DO_NOT_PARSE_MAIL").unwrap();
    let unknown = temp.path().join("data/mail").join("f".repeat(32));
    fs::create_dir_all(&unknown).unwrap();
    fs::write(unknown.join("inbox.sqlite3"), vec![0; 65536]).unwrap();
    let inventory = w.storage_inventory(StorageLimits::default());
    let backup = inventory
        .resources
        .iter()
        .find(|r| r.category == "backups")
        .unwrap();
    assert_eq!(
        backup.retained_reason,
        "retained_backup_payloads_not_validated"
    );
    assert_eq!(backup.project_id.as_deref(), Some(p.id.as_str()));
    let unknown = inventory
        .resources
        .iter()
        .find(|r| r.category == "mail" && r.ownership == "unknown")
        .unwrap();
    assert_eq!(unknown.logical_bytes, None);
    assert!(
        !serde_json::to_string(&inventory)
            .unwrap()
            .contains("DO_NOT_PARSE_MAIL")
    );
}

#[test]
fn metadata_only_db_fixture_distinguishes_selected_binding_retained_generation_and_unknown_owner() {
    let temp = tempfile::tempdir().unwrap();
    let w = fixture(temp.path());
    let p = project(&w, &temp.path().join("repo"));
    // Registry accounting only: these are synthetic metadata rows, not a ready
    // native database acceptance or an actual connection to an engine.
    let instance = "a".repeat(32);
    let db =
        rusqlite::Connection::open(temp.path().join("data/projects/projects.sqlite3")).unwrap();
    let record = serde_json::json!({"id":instance,"package":p.php,"port":1,"admin":"fixture","secret_ref":"DB_SECRET_REFERENCE_MUST_NOT_EXPORT","ready":false});
    db.execute(
        "INSERT INTO instances VALUES(?1,?2)",
        rusqlite::params![instance, record.to_string()],
    )
    .unwrap();
    for (database, active) in [("selected", true), ("retained", false)] {
        let binding = serde_json::json!({"project_id":p.id,"instance_id":instance,"database":database,"role":"fixture","secret_ref":"DB_SECRET_REFERENCE_MUST_NOT_EXPORT","ready":false});
        if active {
            db.execute(
                "INSERT INTO database_bindings VALUES(?1,?2,?3)",
                rusqlite::params![p.id, instance, binding.to_string()],
            )
            .unwrap();
        } else {
            db.execute(
                "INSERT INTO database_binding_history VALUES(?1,?2,?3)",
                rusqlite::params![p.id, "old", binding.to_string()],
            )
            .unwrap();
        }
    }
    let data = temp.path().join("data/projects/databases").join(&instance);
    fs::create_dir_all(data.join("data")).unwrap();
    fs::write(
        data.join("data/opaque-db-file"),
        "DB_PAYLOAD_MUST_NOT_EXPORT",
    )
    .unwrap();
    fs::create_dir_all(
        temp.path()
            .join("data/projects/databases")
            .join("f".repeat(32)),
    )
    .unwrap();
    let inventory = w.storage_inventory(StorageLimits::default());
    let owned = inventory
        .resources
        .iter()
        .find(|r| r.category == "database_instance" && r.resource_id == instance)
        .unwrap();
    assert_eq!(owned.ownership, "instance_registry");
    assert!(
        owned
            .active_refs
            .iter()
            .any(|r| r.kind == "active_database_binding" && r.id == p.id)
    );
    let generations: Vec<_> = inventory
        .resources
        .iter()
        .filter(|r| r.category == "database_generation")
        .collect();
    assert_eq!(generations.len(), 2);
    assert!(
        generations
            .iter()
            .any(|r| r.retained_reason == "active_binding")
    );
    assert!(
        generations
            .iter()
            .any(|r| r.retained_reason == "retained_generation")
    );
    assert!(generations.iter().all(|r| r.logical_bytes.is_none()));
    assert!(
        inventory
            .resources
            .iter()
            .any(|r| r.category == "database_instance" && r.ownership == "unknown")
    );
    let json = serde_json::to_string(&inventory).unwrap();
    assert!(!json.contains("DB_PAYLOAD_MUST_NOT_EXPORT"));
    assert!(!json.contains("DB_SECRET_REFERENCE_MUST_NOT_EXPORT"));
}
#[test]
fn corrupt_registry_explicitly_marks_reference_coverage_unknown() {
    let temp = tempfile::tempdir().unwrap();
    let w = fixture(temp.path());
    let db =
        rusqlite::Connection::open(temp.path().join("data/projects/projects.sqlite3")).unwrap();
    db.execute(
        "INSERT INTO projects VALUES(?1,?1,1,1,'malformed')",
        ["a".repeat(32)],
    )
    .unwrap();
    let inventory = w.storage_inventory(StorageLimits::default());
    assert!(!inventory.registry_complete);
    assert!(inventory.problems.contains(&"metadata_record_invalid"));
    assert!(inventory.resources.iter().all(|r| r.reference_coverage == "partial_registry_metadata_missing_references_unknown"));
}

#[test]
fn worktree_archive_requires_matching_receipt_and_source_registry_and_stays_in_owned_root() {
    let temp = tempfile::tempdir().unwrap();
    let w = fixture(temp.path());
    let source = project(&w, &temp.path().join("repo"));
    let id = "b".repeat(32);
    let dir = temp.path().join("data/worktrees").join(&id);
    fs::create_dir_all(dir.join("retained")).unwrap();
    fs::write(dir.join("retained/archive-file"), "archive fixture").unwrap();
    let configuration = devforge_projects::manifest::Manifest::parse(b"schema: 1\nproject:\n  name: Fixture\n  domain: fixture.devforge.test\nruntimes:\n  php: 8.5.12\n  node: 24.22.2\n  composer: 2.11.2\npins: {}\n").unwrap();
    let mut record = devforge_workspace::worktrees::Record {
        isolated: false,
        schema: 2,
        id: id.clone(),
        input: devforge_workspace::worktrees::Input {
            source_id: source.id.clone(),
            name: "Fixture".into(),
            branch: "feature/fixture".into(),
            base: "HEAD".into(),
            new_branch: true,
            database: devforge_workspace::worktrees::DatabaseMode::Empty,
            dependencies: false,
            copy_environment: false,
        },
        source_path: source.path.clone(),
        common_dir: source.path.join(".git"),
        git_dir: None,
        commit: "fixture".into(),
        domain: "fixture.devforge.test".into(),
        review_digest: "fixture".into(),
        project_id: None,
        stage: "archived".into(),
        last_error: None,
        configuration,
    };
    fs::write(
        dir.join("receipt.json"),
        serde_json::to_vec(&record).unwrap(),
    )
    .unwrap();
    let inventory = w.storage_inventory(StorageLimits::default());
    let owned = inventory
        .resources
        .iter()
        .find(|r| r.category == "worktree_payload")
        .unwrap();
    assert_eq!(owned.workspace_id.as_deref(), Some(id.as_str()));
    assert_eq!(owned.ownership, "worktree_receipt_and_source_registry");
    assert!(owned.logical_bytes.unwrap() >= 15);
    record.source_path = temp.path().join("wrong-source");
    fs::write(
        dir.join("receipt.json"),
        serde_json::to_vec(&record).unwrap(),
    )
    .unwrap();
    let inventory = w.storage_inventory(StorageLimits::default());
    let unknown = inventory
        .resources
        .iter()
        .find(|r| r.category == "worktree_payload")
        .unwrap();
    assert_eq!(unknown.ownership, "unknown");
    assert!(unknown.logical_bytes.is_none());
}
