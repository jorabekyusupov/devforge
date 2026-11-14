use devforge_control::Id;
use devforge_core::{ErrorCode, SecretStore};
use devforge_manager::{actor::ROOT_SUBJECT, report_reads::ReportReadRequest};
use devforge_projects::{manifest::Manifest, project::Project};
use devforge_runtime::package::Target;
use devforge_workspace::{
    Workspace, catalog::OfflineLimits, cleanup_preview::PreviewLimits,
    environment_report::ReportLimits, storage::StorageLimits,
};
use serde_json::json;
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};
struct DenySecrets;
impl SecretStore for DenySecrets {
    fn get(&self, _: &str) -> Result<Option<String>, ErrorCode> {
        panic!("native report must not query workspace credentials")
    }
    fn set(&self, _: &str, _: &str) -> Result<(), ErrorCode> {
        panic!("native report must not change workspace credentials")
    }
    fn delete(&self, _: &str) -> Result<(), ErrorCode> {
        panic!("native report must not delete workspace credentials")
    }
}
fn id(value: &str) -> Id {
    Id::new(value).unwrap()
}
fn fixture(root: &Path) -> (Workspace, Project) {
    let path = root.join("project");
    fs::create_dir_all(path.join("public")).unwrap();
    fs::write(path.join("public/index.php"), "<?php").unwrap();
    fs::write(
        path.join("composer.json"),
        r#"{"require":{"php":"^8.3","laravel/framework":"^13"}}"#,
    )
    .unwrap();
    fs::write(path.join(".env"), "APP_KEY=NATIVE_REPORT_ENV_CANARY\n").unwrap();
    let w = Workspace::open(root.join("data"), Arc::new(DenySecrets)).unwrap();
    w.manifest_init(&path, "8.5.12", "24.22.2", "2.11.2")
        .unwrap();
    let m = Manifest::parse(&fs::read(path.join("devforge.yaml")).unwrap()).unwrap();
    let packages = m.resolve(&w.catalog, Target::current().unwrap()).unwrap();
    let p = w
        .projects
        .import_with_composer(
            &path,
            "Project",
            &packages["php"],
            &packages["node"],
            &packages["composer"],
            &w.catalog,
        )
        .unwrap();
    w.sites
        .configure(
            &w.projects,
            serde_json::from_value(
                json!({"project_id":p.id,"domain":m.project.domain,"https":false,"vite":false}),
            )
            .unwrap(),
        )
        .unwrap();
    (w, p)
}
fn tree(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut files = BTreeMap::new();
    fn visit(root: &Path, path: &Path, files: &mut BTreeMap<PathBuf, Vec<u8>>) {
        for entry in fs::read_dir(path).unwrap() {
            let path = entry.unwrap().path();
            let meta = fs::symlink_metadata(&path).unwrap();
            if meta.is_dir() {
                visit(root, &path, files)
            } else if meta.is_file() {
                files.insert(
                    path.strip_prefix(root).unwrap().into(),
                    fs::read(path).unwrap(),
                );
            }
        }
    }
    visit(root, root, &mut files);
    files
}
#[test]
fn report_wire_cannot_override_default_budgets_subjects_or_filesystem_paths() {
    for value in [
        json!({"kind":"storage_inventory","project_id":"forged"}),
        json!({"kind":"storage_inventory","limits":{"max_entries":999999}}),
        json!({"kind":"runtime_cleanup_preview","package_id":"php","path":"/private"}),
        json!({"kind":"environment_report","project_id":"p","secret":true}),
    ] {
        assert!(serde_json::from_value::<ReportReadRequest>(value).is_err());
    }
    assert_eq!(
        ReportReadRequest::StorageInventory {}.subject().as_str(),
        ROOT_SUBJECT
    );
    assert_eq!(
        ReportReadRequest::RuntimeCleanupPreview {
            package_id: "php-runtime".into()
        }
        .subject()
        .as_str(),
        ROOT_SUBJECT
    );
    assert!(
        !ReportReadRequest::OfflineReadiness {
            project_id: id(ROOT_SUBJECT)
        }
        .valid()
    );
    assert!(
        !ReportReadRequest::EnvironmentReport {
            project_id: id(ROOT_SUBJECT)
        }
        .valid()
    );
    for package_id in ["", ".", "..", "../php", "php;echo secret"] {
        assert!(
            !ReportReadRequest::RuntimeCleanupPreview {
                package_id: package_id.into()
            }
            .valid()
        );
    }
    assert!(
        !ReportReadRequest::RuntimeCleanupPreview {
            package_id: "a".repeat(201)
        }
        .valid()
    );
}
#[test]
fn all_four_reads_return_exact_default_library_reports_without_side_effects() {
    let root = tempfile::tempdir().unwrap();
    let (w, p) = fixture(root.path());
    let before = tree(root.path());
    let requests = [
        (
            ReportReadRequest::StorageInventory {},
            serde_json::to_value(w.storage_inventory(StorageLimits::default())).unwrap(),
        ),
        (
            ReportReadRequest::RuntimeCleanupPreview {
                package_id: p.php.clone(),
            },
            serde_json::to_value(w.runtime_cleanup_preview(&p.php, PreviewLimits::default()))
                .unwrap(),
        ),
        (
            ReportReadRequest::OfflineReadiness {
                project_id: id(&p.id),
            },
            serde_json::to_value(
                w.offline_readiness(&p.id, OfflineLimits::default())
                    .unwrap(),
            )
            .unwrap(),
        ),
        (
            ReportReadRequest::EnvironmentReport {
                project_id: id(&p.id),
            },
            serde_json::to_value(
                w.environment_report(&p.id, ReportLimits::default())
                    .unwrap(),
            )
            .unwrap(),
        ),
    ];
    for (request, expected) in requests {
        assert!(request.valid());
        let value = request.execute(&w).unwrap();
        assert_eq!(value, expected);
        let encoded = value.to_string();
        assert!(!encoded.contains("NATIVE_REPORT_ENV_CANARY"));
        assert!(!encoded.contains(&p.path.display().to_string()));
    }
    assert_eq!(tree(root.path()), before);
}
#[test]
fn invalid_project_and_invalid_package_do_not_become_a_global_fallback() {
    let root = tempfile::tempdir().unwrap();
    let (w, _) = fixture(root.path());
    let request = ReportReadRequest::EnvironmentReport {
        project_id: id("00000000000000000000000000000000"),
    };
    assert_ne!(request.subject().as_str(), ROOT_SUBJECT);
    assert!(request.execute(&w).is_err());
    assert!(
        ReportReadRequest::RuntimeCleanupPreview {
            package_id: "../package".into()
        }
        .execute(&w)
        .is_err()
    );
    let unknown = ReportReadRequest::RuntimeCleanupPreview {
        package_id: "unknown-package".into(),
    }
    .execute(&w)
    .unwrap();
    assert_eq!(unknown["state"], json!("unknown"));
    assert!(
        unknown["apply_semantics"]
            .as_str()
            .unwrap()
            .contains("not_authorization")
    );
}

mod common;
#[derive(Default)]
struct AuthSecrets(std::sync::Mutex<BTreeMap<String, String>>);
impl SecretStore for AuthSecrets {
    fn get(&self, key: &str) -> Result<Option<String>, ErrorCode> {
        Ok(self.0.lock().unwrap().get(key).cloned())
    }
    fn set(&self, key: &str, value: &str) -> Result<(), ErrorCode> {
        self.0.lock().unwrap().insert(key.into(), value.into());
        Ok(())
    }
    fn delete(&self, key: &str) -> Result<(), ErrorCode> {
        self.0.lock().unwrap().remove(key);
        Ok(())
    }
}
fn wait_terminal(
    handle: &devforge_manager::actor::ActorHandle,
    proof: &devforge_manager::auth::DesktopAuthority<'_>,
    receipt: &devforge_manager::actor::Receipt,
) -> devforge_manager::actor::Receipt {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    loop {
        let current = handle
            .status(proof, &receipt.project_id, &receipt.operation_id)
            .unwrap();
        if current.state.terminal() {
            return current;
        }
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
}
#[test]
fn native_facade_uses_fixed_scopes_and_ephemeral_images_for_all_four_reports() {
    use devforge_control::OperationState;
    use devforge_manager::{
        PeerIdentity,
        actor::{ActorError, OperationIdentity, WorkspaceActor},
        auth::{DesktopAuth, Policy, TrustedDesktopAction},
        desktop::ReadRequest,
    };
    let root = common::root("df-report-");
    let owner = common::acquire(root.path()).unwrap();
    let actor =
        WorkspaceActor::start(owner.claim_workspace().unwrap(), Arc::new(DenySecrets)).unwrap();
    let handle = actor.handle();
    let peer = PeerIdentity {
        user: "native-report-fixture".into(),
        process_id: Some(1),
    };
    let auth_store = AuthSecrets::default();
    let mut auth = DesktopAuth::new(
        owner.endpoint().clone(),
        peer.user.clone(),
        Policy::default(),
    )
    .unwrap();
    let key = auth
        .rotate_desktop_key(
            &TrustedDesktopAction::from_explicit_desktop_action(),
            &auth_store,
            std::time::Instant::now(),
        )
        .unwrap();
    let session = auth
        .authenticate(&peer, &key, &auth_store, std::time::Instant::now())
        .unwrap();
    let proof = auth
        .authorize(&peer, &session, std::time::Instant::now())
        .unwrap();
    let project = root.path().join("project");
    fs::create_dir_all(project.join("public")).unwrap();
    fs::write(project.join("public/index.php"), "<?php").unwrap();
    fs::write(
        project.join("composer.json"),
        r#"{"require":{"php":"^8.3","laravel/framework":"^13"}}"#,
    )
    .unwrap();
    fs::write(project.join(".env"), "APP_KEY=NATIVE_ACTOR_REPORT_CANARY\n").unwrap();
    let catalog = devforge_projects::project::Catalog::bundled();
    let mut manifest:Manifest=serde_json::from_value(json!({"schema":1,"project":{"name":"Native report","domain":"native-report.devforge.test"},"runtimes":{"php":"8.5.12","node":"24.22.2","composer":"2.11.2"},"pins":{}})).unwrap();
    manifest.pin_all(&catalog).unwrap();
    fs::write(project.join("devforge.yaml"), manifest.yaml().unwrap()).unwrap();
    let packages = manifest
        .resolve(&catalog, Target::current().unwrap())
        .unwrap();
    let registration = handle
        .submit_desktop(
            &proof,
            OperationIdentity {
                project_id: id(ROOT_SUBJECT),
                idempotency_key: id("register"),
            },
            devforge_workspace::Command::Import {
                path: project.clone(),
                name: "Native report".into(),
                php: packages["php"].clone(),
                node: packages["node"].clone(),
                domain: manifest.project.domain,
                https: false,
                vite: false,
                queue: false,
            },
        )
        .unwrap();
    assert_eq!(
        wait_terminal(&handle, &proof, &registration).state,
        OperationState::Succeeded
    );
    let result = handle
        .result(&proof, &registration.project_id, &registration.operation_id)
        .unwrap();
    let project_id = id(result["project_id"].as_str().unwrap());
    let requests = [
        ReportReadRequest::StorageInventory {},
        ReportReadRequest::RuntimeCleanupPreview {
            package_id: packages["php"].clone(),
        },
        ReportReadRequest::OfflineReadiness {
            project_id: project_id.clone(),
        },
        ReportReadRequest::EnvironmentReport {
            project_id: project_id.clone(),
        },
    ];
    let db = rusqlite::Connection::open(root.path().join("control/operations.sqlite3")).unwrap();
    for (index, request) in requests.into_iter().enumerate() {
        let expected = request.subject().clone();
        assert_eq!(
            expected,
            if index < 2 {
                id(ROOT_SUBJECT)
            } else {
                project_id.clone()
            }
        );
        let receipt = handle
            .desktop_read(
                &proof,
                id(&format!("report-{index}")),
                ReadRequest::Report { request },
            )
            .unwrap();
        let receipt = wait_terminal(&handle, &proof, &receipt);
        assert_eq!(receipt.state, OperationState::Succeeded);
        assert_eq!(receipt.project_id, expected);
        let image = handle
            .read_image(&proof, &receipt.project_id, &receipt.operation_id)
            .unwrap();
        let wire: serde_json::Value = serde_json::from_slice(image.bytes()).unwrap();
        assert_eq!(wire["ok"], true);
        assert_eq!(wire["data"]["schema"], 1);
        if index >= 2 {
            assert_eq!(wire["data"]["project_id"], project_id.as_str());
        }
        assert!(!wire.to_string().contains("NATIVE_ACTOR_REPORT_CANARY"));
        assert!(!wire.to_string().contains(&project.display().to_string()));
        assert_eq!(
            handle.result(&proof, &receipt.project_id, &receipt.operation_id),
            Err(ActorError::ResultUnavailable)
        );
        let persisted: i64 = db
            .query_row(
                "SELECT count(*) FROM operations WHERE operation_id=?1",
                [receipt.operation_id.as_str()],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(persisted, 0);
    }
    for request in [
        ReportReadRequest::EnvironmentReport {
            project_id: id(ROOT_SUBJECT),
        },
        ReportReadRequest::OfflineReadiness {
            project_id: id(ROOT_SUBJECT),
        },
    ] {
        assert!(matches!(
            handle.desktop_read(
                &proof,
                id("bad-root-report"),
                ReadRequest::Report { request }
            ),
            Err(ActorError::InvalidInput)
        ));
    }
    db.close().unwrap();
    actor.shutdown().unwrap();
}
