//! Metadata/inert-header contracts only. No fixture executable is launched.
use devforge_core::{ErrorCode, SecretStore};
use devforge_runtime::{
    Cancellation,
    package::{Archive, Package, Target, hash_file},
};
use devforge_workspace::{
    Command, Error, Workspace, database_control::database_snapshot_fingerprint,
};
use std::{collections::BTreeMap, fs, path::Path, sync::Arc};
struct NoSecrets;
impl SecretStore for NoSecrets {
    fn get(&self, _: &str) -> Result<Option<String>, ErrorCode> {
        panic!("preflight must not read credentials")
    }
    fn set(&self, _: &str, _: &str) -> Result<(), ErrorCode> {
        panic!("preflight must not write credentials")
    }
    fn delete(&self, _: &str) -> Result<(), ErrorCode> {
        panic!("preflight must not delete credentials")
    }
}
fn project(w: &Workspace, path: &Path, name: &str) -> devforge_projects::project::Project {
    fs::create_dir_all(path.join("public")).unwrap();
    fs::write(path.join("public/index.php"), b"<?php").unwrap();
    fs::write(
        path.join("composer.json"),
        br#"{"require":{"php":"^8.3","laravel/framework":"^13"}}"#,
    )
    .unwrap();
    let select = |kind: &str, tool: &str| {
        w.catalog
            .packages
            .iter()
            .find(|(_, p)| {
                p.kind == kind
                    && p.target == Target::current().unwrap()
                    && p.executables.contains_key(tool)
            })
            .unwrap()
            .0
            .clone()
    };
    w.projects
        .import(
            path,
            name,
            &select("php", "php"),
            &select("node", "node"),
            &w.catalog,
        )
        .unwrap()
}
fn fixture() -> (tempfile::TempDir, Workspace, String, String, Package) {
    let root = tempfile::tempdir().unwrap();
    let mut w = Workspace::open(root.path().join("data"), Arc::new(NoSecrets)).unwrap();
    let project = project(&w, &root.path().join("project Ўзбек"), "Owned");
    let mut header = vec![0; 96];
    match Target::current().unwrap() {
        Target::MacosArm64 => header[..8].copy_from_slice(&[0xcf, 0xfa, 0xed, 0xfe, 12, 0, 0, 1]),
        Target::MacosX64 => header[..8].copy_from_slice(&[0xcf, 0xfa, 0xed, 0xfe, 7, 0, 0, 1]),
        Target::WindowsX64 => {
            header[..2].copy_from_slice(b"MZ");
            header[60..64].copy_from_slice(&64u32.to_le_bytes());
            header[64..70].copy_from_slice(&[b'P', b'E', 0, 0, 0x64, 0x86]);
        }
    }
    let archive = root.path().join("inert-server");
    fs::write(&archive, header).unwrap();
    let package = Package {
        kind: "mysql".into(),
        version: "8.4.11".into(),
        variant: "server".into(),
        target: Target::current().unwrap(),
        url: "https://example.invalid/inert".into(),
        sha256: hash_file(&archive).unwrap(),
        archive: Archive::File {
            path: "server".into(),
        },
        executables: BTreeMap::from([
            ("mysqld".into(), "server".into()),
            ("mysql".into(), "server".into()),
            ("mysqldump".into(), "server".into()),
        ]),
        resources: BTreeMap::new(),
    };
    w.packages
        .install_cached(&package, &archive, &Cancellation::default(), |_| {})
        .unwrap();
    w.catalog
        .packages
        .insert("db-fixture".into(), package.clone());
    let instance = "a".repeat(32);
    fs::create_dir_all(
        root.path()
            .join("data/projects/databases")
            .join(&instance)
            .join("data"),
    )
    .unwrap();
    let db =
        rusqlite::Connection::open(root.path().join("data/projects/projects.sqlite3")).unwrap();
    db.execute("INSERT INTO instances VALUES(?1,?2)",rusqlite::params![instance,serde_json::json!({"engine":"mysql","id":instance,"package":"db-fixture","port":32123,"admin":"fixture","secret_ref":"ADMIN_REFERENCE_NOT_FOR_EXTERNAL_OUTPUT","ready":true}).to_string()]).unwrap();
    (root, w, project.id, instance, package)
}
#[test]
fn live_state_and_ids_only_choices_do_not_start_or_probe_secrets_or_create_backup_namespace() {
    let (root, w, id, instance, _) = fixture();
    let command = Command::SwitchDatabase {
        id: id.clone(),
        instance: instance.clone(),
        consent: true,
    };
    let state = w.database_action_state(&command, &|| Ok(())).unwrap();
    assert_eq!(state.fingerprint().unwrap().len(), 64);
    let choices = w.database_choices(&id).unwrap();
    let output = serde_json::to_string(&choices).unwrap();
    assert!(!output.contains("ADMIN_REFERENCE"));
    assert!(!output.contains("fixture"));
    assert!(!root.path().join("data/projects/backups").exists());
    assert!(w.projects.binding(&id).unwrap().is_none());
    assert!(w.projects.binding_history(&id).unwrap().is_empty());
    assert!(
        w.database_action_preflight(&Command::StartDatabase { instance })
            .is_err()
    );
}
#[test]
fn reviewed_state_change_without_project_revision_change_refuses_before_startup() {
    let (root, mut w, id, instance, _) = fixture();
    let command = Command::SwitchDatabase {
        id: id.clone(),
        instance: instance.clone(),
        consent: true,
    };
    let state = w.database_action_state(&command, &|| Ok(())).unwrap();
    let revision = w.projects.get(&id).unwrap().revision;
    let before = database_snapshot_fingerprint(&w.snapshot().unwrap(), &command).unwrap();
    let db =
        rusqlite::Connection::open(root.path().join("data/projects/projects.sqlite3")).unwrap();
    let mut record: serde_json::Value = serde_json::from_str(
        &db.query_row::<String, _, _>(
            "SELECT record FROM instances WHERE id=?1",
            [&instance],
            |r| r.get(0),
        )
        .unwrap(),
    )
    .unwrap();
    record["port"] = 32124.into();
    db.execute(
        "UPDATE instances SET record=?1 WHERE id=?2",
        rusqlite::params![record.to_string(), instance],
    )
    .unwrap();
    assert_eq!(w.projects.get(&id).unwrap().revision, revision);
    assert_ne!(
        before,
        database_snapshot_fingerprint(&w.snapshot().unwrap(), &command).unwrap()
    );
    let error = w
        .execute_database_action(command, &state, &Cancellation::default(), &|| Ok(()))
        .unwrap_err();
    assert_eq!(error.code, "database_review_changed");
    assert!(w.projects.binding(&id).unwrap().is_none());
}
#[test]
fn rejected_or_cancelled_authority_preserves_error_and_binding_before_effects() {
    let (_root, mut w, id, instance, _) = fixture();
    let command = Command::SwitchDatabase {
        id: id.clone(),
        instance,
        consent: true,
    };
    let state = w.database_action_state(&command, &|| Ok(())).unwrap();
    let denied = || {
        Err(Error {
            scope: "workspace",
            code: "timed_out".into(),
            project_id: Some(id.clone()),
        })
    };
    let failure = w
        .execute_database_action(command.clone(), &state, &Cancellation::default(), &denied)
        .unwrap_err();
    assert_eq!(failure.code, "timed_out");
    assert_eq!(failure.project_id.as_deref(), Some(id.as_str()));
    let cancel = Cancellation::default();
    cancel.cancel();
    assert_eq!(
        w.execute_database_action(command, &state, &cancel, &|| Ok(()))
            .unwrap_err()
            .code,
        "cancelled"
    );
    assert!(w.projects.binding(&id).unwrap().is_none());
}
#[test]
fn missing_cache_and_missing_owned_backup_fail_without_download_or_namespace_creation() {
    let (root, w, id, instance, package) = fixture();
    let command = Command::SwitchDatabase {
        id: id.clone(),
        instance: instance.clone(),
        consent: true,
    };
    w.packages.remove(&package).unwrap();
    assert_eq!(
        w.database_action_preflight(&command).unwrap_err().code,
        "database_precondition_missing"
    );
    let missing = Command::RestoreBackup {
        id: id.clone(),
        backup: "b".repeat(32),
        consent: true,
    };
    assert!(w.database_action_preflight(&missing).is_err());
    assert!(!root.path().join("data/projects/backups").exists());
    assert_eq!(
        fs::read_dir(root.path().join("data/runtimes/staging"))
            .unwrap()
            .count(),
        0
    );
}

#[test]
fn foreign_project_manifest_in_owned_backup_path_is_rejected_before_effects() {
    let (root, mut w, id, instance, _) = fixture();
    let binding = devforge_projects::database::Binding {
        project_id: id.clone(),
        instance_id: instance.clone(),
        database: format!("df_{id}"),
        role: "fixture-role".into(),
        secret_ref: "MUST_NOT_PROBE_FOREIGN_BACKUP".into(),
        ready: true,
    };
    w.projects.activate_binding(&id, Some(&binding)).unwrap();
    let backup = "c".repeat(32);
    let directory = root.path().join("data/projects/backups").join(&id);
    fs::create_dir_all(&directory).unwrap();
    fs::write(
        directory.join(format!("{backup}.dfbackup")),
        include_bytes!("fixtures/foreign-project.dfbackup"),
    )
    .unwrap();
    assert!(
        !w.database_choices(&id)
            .unwrap()
            .backup_ids
            .contains(&backup)
    );
    let command = Command::RestoreBackup {
        id: id.clone(),
        backup,
        consent: true,
    };
    assert_eq!(
        w.database_action_state(&command, &|| Ok(()))
            .err()
            .unwrap()
            .code,
        "backup_unsupported"
    );
    assert_eq!(
        database_snapshot_fingerprint(&w.snapshot().unwrap(), &command)
            .unwrap_err()
            .code,
        "database_precondition_missing"
    );
    let expected = w
        .database_action_state(
            &Command::SwitchDatabase {
                id: id.clone(),
                instance,
                consent: true,
            },
            &|| Ok(()),
        )
        .unwrap();
    let error = w
        .execute_database_action(command, &expected, &Cancellation::default(), &|| Ok(()))
        .unwrap_err();
    assert_eq!(error.code, "backup_unsupported");
    let after = w.projects.binding(&id).unwrap().unwrap();
    assert_eq!(after.database, binding.database);
    assert_eq!(after.secret_ref, binding.secret_ref);
    assert!(w.projects.binding_history(&id).unwrap().is_empty());
}
