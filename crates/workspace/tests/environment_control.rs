//! Actual isolated YAML/history/journal and package-integrity contracts. Cached
//! native-header fixtures are never executed and do not prove PHP/Node health.
use devforge_core::{ErrorCode, SecretStore};
use devforge_projects::project::Catalog;
use devforge_runtime::{
    Cancellation,
    package::{Archive, Package, Target, hash_file},
};
use devforge_workspace::{Command, Error, Workspace};
use serde_json::{Value, json};
use std::{
    cell::Cell,
    collections::BTreeMap,
    fs,
    io::Read,
    path::{Path, PathBuf},
    sync::Arc,
};

struct DenySecrets;
impl SecretStore for DenySecrets {
    fn get(&self, _: &str) -> Result<Option<String>, ErrorCode> {
        panic!("empty manifest must not probe credentials")
    }
    fn set(&self, _: &str, _: &str) -> Result<(), ErrorCode> {
        panic!("metadata-only fixture must not set credentials")
    }
    fn delete(&self, _: &str) -> Result<(), ErrorCode> {
        panic!("fixture must not delete credentials")
    }
}
fn cached_catalog(workspace: &mut Workspace, root: &Path) {
    let mut bytes = Vec::new();
    fs::File::open(std::env::current_exe().unwrap())
        .unwrap()
        .take(4096)
        .read_to_end(&mut bytes)
        .unwrap();
    let archive = root.join("native-header.fixture");
    fs::write(&archive, bytes).unwrap();
    let mut catalog = Catalog {
        packages: BTreeMap::new(),
        superseded: BTreeMap::new(),
        defaults: BTreeMap::from([
            ("composer".into(), "2.11.2".into()),
            ("caddy".into(), "2.99.0".into()),
        ]),
    };
    let mut descriptors = vec![
        (
            "php",
            "php",
            "8.5.12",
            if cfg!(windows) { "default" } else { "cli" },
            if cfg!(windows) {
                vec!["php", "php-cgi"]
            } else {
                vec!["php"]
            },
        ),
        ("node", "node", "24.22.2", "native", vec!["node"]),
        ("composer", "composer", "2.11.2", "phar", vec![]),
        ("caddy", "caddy", "2.99.0", "native", vec!["caddy"]),
    ];
    if !cfg!(windows) {
        descriptors.push(("php-web", "php", "8.5.12", "fpm", vec!["php-fpm"]));
    }
    for (id, kind, version, variant, names) in descriptors {
        let package = Package {
            kind: kind.into(),
            version: version.into(),
            variant: variant.into(),
            target: Target::current().unwrap(),
            url: "https://example.invalid/cache-only.fixture".into(),
            sha256: hash_file(&archive).unwrap(),
            archive: Archive::File {
                path: "payload.exe".into(),
            },
            executables: names
                .iter()
                .map(|name| ((*name).into(), "payload.exe".into()))
                .collect(),
            resources: if names.is_empty() {
                BTreeMap::from([("composer".into(), "payload.exe".into())])
            } else {
                BTreeMap::new()
            },
        };
        workspace
            .packages
            .install_cached(&package, &archive, &Cancellation::default(), |_| {})
            .unwrap();
        catalog.packages.insert(id.into(), package);
    }
    workspace.catalog = catalog;
    workspace.refresh_runtimes().unwrap();
}
fn fixture(cached: bool) -> (tempfile::TempDir, Workspace, String, PathBuf, String) {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("Repo");
    fs::create_dir_all(path.join("public")).unwrap();
    fs::write(path.join("public/index.php"), "<?php").unwrap();
    fs::write(
        path.join("composer.json"),
        r#"{"require":{"php":"^8.3","laravel/framework":"^13"}}"#,
    )
    .unwrap();
    fs::write(path.join(".env"), "UNRELATED=preserved\n").unwrap();
    let mut workspace = Workspace::open(temp.path().join("data"), Arc::new(DenySecrets)).unwrap();
    if cached {
        cached_catalog(&mut workspace, temp.path());
    }
    workspace
        .manifest_init(&path, "8.5.12", "24.22.2", "2.11.2")
        .unwrap();
    let plan = workspace.manifest_plan(&path, false).unwrap();
    let imported = workspace
        .execute(
            Command::Import {
                path: path.clone(),
                name: "Env fixture".into(),
                php: plan.packages["php"].clone(),
                node: plan.packages["node"].clone(),
                domain: "repo.devforge.test".into(),
                https: false,
                vite: false,
                queue: false,
            },
            &Cancellation::default(),
        )
        .unwrap();
    let text = fs::read_to_string(path.join("devforge.yaml")).unwrap();
    (
        temp,
        workspace,
        imported["project_id"].as_str().unwrap().into(),
        path,
        text,
    )
}
fn journal(root: &Path) -> Option<Value> {
    fs::read_dir(root.join("data/manifests"))
        .ok()?
        .filter_map(Result::ok)
        .find_map(|entry| {
            fs::read(entry.path().join("journal.json"))
                .ok()
                .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        })
}
fn save(id: &str, expected: Option<String>, text: String) -> Command {
    Command::ConfigurationSave {
        id: id.into(),
        expected,
        text,
        consent: true,
    }
}

#[test]
fn invalid_old_bytes_have_private_version_and_native_history_is_preserved_but_external_result_masked()
 {
    let (temp, mut workspace, id, path, valid) = fixture(false);
    let original = b"invalid YAML \xff\nPRIVATE_OLD_FIXTURE_CANARY";
    fs::write(path.join("devforge.yaml"), original).unwrap();
    let (exists, expected) = workspace.environment_document_version(&id).unwrap();
    assert!(exists);
    let expected = expected.unwrap();
    let updated = format!("{valid}\n# PRIVATE_NEW_FIXTURE_CANARY\n");
    assert_eq!(
        workspace
            .execute_environment_action(
                save(&id, Some(expected.clone()), updated.clone()),
                &Cancellation::default(),
                &|| Ok(())
            )
            .unwrap(),
        json!({"saved":true})
    );
    assert_eq!(
        fs::read_to_string(path.join("devforge.yaml")).unwrap(),
        updated
    );
    let scope = fs::read_dir(temp.path().join("data/manifests"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    assert_eq!(
        fs::read(
            scope
                .join("configuration-history")
                .join(format!("{expected}.yaml"))
        )
        .unwrap(),
        original
    );
    let (_, current) = workspace.environment_document_version(&id).unwrap();
    let native = workspace
        .configuration_save(&id, current.as_deref(), &valid, true)
        .unwrap();
    assert_eq!(native["text"], valid);
    assert!(native["expected"].is_string());
}

#[test]
fn absent_version_is_create_only_and_denied_save_does_not_overwrite() {
    let (_temp, mut workspace, id, path, valid) = fixture(false);
    fs::remove_file(path.join("devforge.yaml")).unwrap();
    assert_eq!(
        workspace.environment_document_version(&id).unwrap(),
        (false, None)
    );
    workspace
        .execute_environment_action(
            save(&id, None, valid.clone()),
            &Cancellation::default(),
            &|| Ok(()),
        )
        .unwrap();
    assert_eq!(
        workspace
            .execute_environment_action(
                save(&id, None, valid.clone()),
                &Cancellation::default(),
                &|| Ok(())
            )
            .unwrap_err()
            .code,
        "manifest_review_changed"
    );
    let expected = workspace.environment_document_version(&id).unwrap().1;
    let denied = || {
        Err(Error {
            scope: "workspace",
            code: "timed_out".into(),
            project_id: Some(id.clone()),
        })
    };
    assert_eq!(
        workspace
            .execute_environment_action(
                save(&id, expected, format!("{valid}\n# must-not-commit")),
                &Cancellation::default(),
                &denied
            )
            .unwrap_err()
            .code,
        "timed_out"
    );
    assert_eq!(
        fs::read_to_string(path.join("devforge.yaml")).unwrap(),
        valid
    );
}

#[test]
fn precommit_guard_preserves_a_concurrent_editor_change() {
    let (_temp, mut workspace, id, path, valid) = fixture(false);
    let expected = workspace.environment_document_version(&id).unwrap().1;
    let changed = Cell::new(false);
    let callback = || {
        if !changed.get()
            && fs::read_dir(&path)
                .unwrap()
                .filter_map(Result::ok)
                .any(|entry| entry.file_name().to_string_lossy().starts_with(".tmp"))
        {
            fs::write(path.join("devforge.yaml"), "# concurrent editor fixture\n").unwrap();
            changed.set(true);
        }
        Ok(())
    };
    assert_eq!(
        workspace
            .execute_environment_action(
                save(&id, expected, format!("{valid}\n# proposed")),
                &Cancellation::default(),
                &callback
            )
            .unwrap_err()
            .code,
        "manifest_review_changed"
    );
    assert!(changed.get());
    assert_eq!(
        fs::read_to_string(path.join("devforge.yaml")).unwrap(),
        "# concurrent editor fixture\n"
    );
}

#[test]
fn guarded_plan_is_the_exact_native_plan_and_stale_apply_refuses_before_download() {
    let (temp, mut workspace, id, path, _valid) = fixture(false);
    let guarded = workspace
        .environment_plan_guarded(&id, false, &|| Ok(()))
        .unwrap();
    assert_eq!(
        guarded.digest,
        workspace.manifest_plan(&path, false).unwrap().digest
    );
    fs::write(path.join(".env"), "UNRELATED=external-change\n").unwrap();
    let error = workspace
        .execute_environment_action(
            Command::ConfigurationApply {
                id,
                review: guarded.digest,
                trusted: true,
                write_env: false,
            },
            &Cancellation::default(),
            &|| Ok(()),
        )
        .unwrap_err();
    assert_eq!(error.code, "manifest_review_changed");
    assert_eq!(
        fs::read_dir(temp.path().join("data/runtimes/packages"))
            .unwrap()
            .count(),
        0
    );
    assert!(journal(temp.path()).is_none());
}

#[test]
fn interrupted_apply_records_existing_stage_journal_and_success_returns_no_raw_journal_digest() {
    let (temp, mut workspace, id, path, _valid) = fixture(true);
    let plan = workspace
        .environment_plan_guarded(&id, false, &|| Ok(()))
        .unwrap();
    let callback = || {
        if journal(temp.path()).is_some_and(|value| value["stage"] == "download") {
            Err(Error {
                scope: "workspace",
                code: "timed_out".into(),
                project_id: Some(id.clone()),
            })
        } else {
            Ok(())
        }
    };
    assert_eq!(
        workspace
            .execute_environment_action(
                Command::ConfigurationApply {
                    id: id.clone(),
                    review: plan.digest,
                    trusted: true,
                    write_env: false
                },
                &Cancellation::default(),
                &callback
            )
            .unwrap_err()
            .code,
        "timed_out"
    );
    let failed = journal(temp.path()).unwrap();
    assert_eq!(failed["stage"], "download");
    assert_eq!(failed["last_error"], "timed_out");
    assert_eq!(failed["completed"], false);
    let refreshed = workspace
        .environment_plan_guarded(&id, false, &|| Ok(()))
        .unwrap();
    let digest = refreshed.digest.clone();
    let result = workspace
        .execute_environment_action(
            Command::ConfigurationApply {
                id,
                review: refreshed.digest,
                trusted: true,
                write_env: false,
            },
            &Cancellation::default(),
            &|| Ok(()),
        )
        .unwrap();
    assert_eq!(result["applied"], true);
    assert_eq!(result["stage"], "complete");
    assert!(!result.to_string().contains(&digest));
    assert!(result.get("manifest").is_none());
    assert!(result.get("database").is_none());
    assert!(result.get("redis").is_none());
    assert_eq!(journal(temp.path()).unwrap()["completed"], true);
    assert_eq!(
        fs::read_to_string(path.join(".env")).unwrap(),
        "UNRELATED=preserved\n"
    );
}
