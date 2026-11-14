use devforge_core::{ErrorCode, SecretStore};
use devforge_runtime::{
    Cancellation,
    package::{Archive, Package, RemovalLeaseState, Target, hash_file},
};
use devforge_workspace::{
    Workspace,
    cleanup_preview::{PreviewLimits, PreviewState},
};
use std::{collections::BTreeMap, fs, path::Path, sync::Arc};
struct DenySecrets;
impl SecretStore for DenySecrets {
    fn get(&self, _: &str) -> Result<Option<String>, ErrorCode> {
        panic!("preview must not read secrets")
    }
    fn set(&self, _: &str, _: &str) -> Result<(), ErrorCode> {
        panic!("preview must not write secrets")
    }
    fn delete(&self, _: &str) -> Result<(), ErrorCode> {
        panic!("preview must not remove secrets")
    }
}
fn fixture(root: &Path) -> (Workspace, Package) {
    let mut w = Workspace::open(root.join("data"), Arc::new(DenySecrets)).unwrap();
    let archive = root.join("resource.fixture");
    fs::write(&archive, b"PAYLOAD_MUST_NOT_EXPORT").unwrap();
    let p = Package {
        kind: "composer".into(),
        version: "2.99.0".into(),
        variant: "fixture".into(),
        target: Target::current().unwrap(),
        url: "https://example.invalid/inert".into(),
        sha256: hash_file(&archive).unwrap(),
        archive: Archive::File {
            path: "composer.phar".into(),
        },
        executables: BTreeMap::new(),
        resources: BTreeMap::from([("composer".into(), "composer.phar".into())]),
    };
    w.packages
        .install_cached(&p, &archive, &Cancellation::default(), |_| {})
        .unwrap();
    w.catalog
        .packages
        .insert("reviewed-composer".into(), p.clone());
    (w, p)
}
fn project(w: &Workspace, root: &Path) -> devforge_projects::project::Project {
    fs::create_dir_all(root.join("public")).unwrap();
    fs::write(root.join("public/index.php"), b"<?php").unwrap();
    fs::write(
        root.join("composer.json"),
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
            root,
            "Preview",
            &select("php", "php"),
            &select("node", "node"),
            &w.catalog,
        )
        .unwrap()
}
#[test]
fn unreferenced_owned_package_is_only_a_review_candidate_and_shared_lease_blocks() {
    let temp = tempfile::tempdir().unwrap();
    let (w, p) = fixture(temp.path());
    let path = temp
        .path()
        .join("data/runtimes/leases")
        .join(p.key().unwrap());
    assert!(!path.exists());
    let unknown = w.runtime_cleanup_preview("reviewed-composer", PreviewLimits::default());
    assert_eq!(unknown.state, PreviewState::Unknown);
    assert_eq!(unknown.lease.state, RemovalLeaseState::Missing);
    assert!(!path.exists());
    let active = w.packages.resource(&p, "composer").unwrap();
    assert_eq!(
        w.runtime_cleanup_preview("reviewed-composer", PreviewLimits::default())
            .state,
        PreviewState::Blocked
    );
    drop(active);
    let candidate = w.runtime_cleanup_preview("reviewed-composer", PreviewLimits::default());
    assert_eq!(
        candidate.state,
        PreviewState::ReviewCandidate,
        "{:?}",
        candidate.problems
    );
    assert!(candidate.references.is_empty());
    assert!(candidate.logical_bytes.unwrap() > 0);
    assert!(candidate.apply_semantics.contains("not_authorization"));
    assert!(
        candidate
            .size_semantics
            .contains("not_allocated_or_freeable")
    );
    assert!(
        !serde_json::to_string(&candidate)
            .unwrap()
            .contains("PAYLOAD_MUST_NOT_EXPORT")
    );
    assert!(
        temp.path()
            .join("data/runtimes/packages")
            .join(p.key().unwrap())
            .exists()
    );
}
#[test]
fn inactive_project_pins_and_registered_pending_database_instances_block_review() {
    let temp = tempfile::tempdir().unwrap();
    let (mut w, p) = fixture(temp.path());
    drop(w.packages.resource(&p, "composer").unwrap());
    let project = project(&w, &temp.path().join("repo"));
    w.catalog
        .packages
        .insert("alias-composer".into(), p.clone());
    let updated = w
        .projects
        .configure_composer(&project.id, project.revision, "alias-composer", &w.catalog)
        .unwrap();
    w.projects.forget(&updated.id, updated.revision).unwrap();
    let preview = w.runtime_cleanup_preview("reviewed-composer", PreviewLimits::default());
    assert_eq!(preview.state, PreviewState::Blocked);
    assert!(
        preview
            .references
            .iter()
            .any(|r| r.kind == "inactive_project_runtime_selection" && r.id == project.id)
    );
    let instance = "a".repeat(32);
    let db =
        rusqlite::Connection::open(temp.path().join("data/projects/projects.sqlite3")).unwrap();
    db.execute("INSERT INTO instances VALUES(?1,?2)",rusqlite::params![instance,serde_json::json!({"id":instance,"package":"reviewed-composer","port":1,"admin":"fixture","secret_ref":"PRIVATE_SECRET","ready":false}).to_string()]).unwrap();
    db.execute("INSERT INTO database_binding_history VALUES(?1,'drill',?2)",rusqlite::params![project.id,serde_json::json!({"project_id":project.id,"instance_id":instance,"database":"df_pending_drill","role":"PRIVATE_ROLE","secret_ref":"PRIVATE_SECRET","ready":false}).to_string()]).unwrap();
    let preview = w.runtime_cleanup_preview("reviewed-composer", PreviewLimits::default());
    assert!(
        preview
            .references
            .iter()
            .any(|r| r.kind == "registered_database_instance" && r.id == instance)
    );
    assert!(
        preview
            .references
            .iter()
            .any(|r| r.kind == "pending_database_generation_reference")
    );
    assert!(
        !serde_json::to_string(&preview)
            .unwrap()
            .contains("PRIVATE_SECRET")
    );
}
#[test]
fn incomplete_registry_unknown_ownership_and_limits_prevent_candidates() {
    let temp = tempfile::tempdir().unwrap();
    let (w, p) = fixture(temp.path());
    drop(w.packages.resource(&p, "composer").unwrap());
    let db =
        rusqlite::Connection::open(temp.path().join("data/projects/projects.sqlite3")).unwrap();
    db.execute(
        "INSERT INTO projects VALUES(?1,?1,0,1,'malformed')",
        ["a".repeat(32)],
    )
    .unwrap();
    let preview = w.runtime_cleanup_preview("reviewed-composer", PreviewLimits::default());
    assert_eq!(preview.state, PreviewState::Unknown);
    assert!(
        preview
            .problems
            .contains(&"all_registered_reference_coverage_incomplete")
    );
    db.execute("DELETE FROM projects", []).unwrap();
    fs::create_dir_all(temp.path().join("data/worktrees").join("b".repeat(32))).unwrap();
    let preview = w.runtime_cleanup_preview("reviewed-composer", PreviewLimits::default());
    assert_eq!(preview.state, PreviewState::Unknown);
    assert!(preview.problems.contains(&"inventory_ownership_unknown"));
    let mut limits = PreviewLimits::default();
    limits.storage.max_entries = 1;
    assert_eq!(
        w.runtime_cleanup_preview("reviewed-composer", limits).state,
        PreviewState::Unknown
    );
    assert_eq!(
        w.runtime_cleanup_preview(
            "reviewed-composer",
            PreviewLimits {
                max_references: 0,
                ..Default::default()
            }
        )
        .state,
        PreviewState::Unknown
    );
}

#[test]
fn pinned_retained_worktree_configuration_blocks_and_unverified_pins_are_unknown() {
    let temp = tempfile::tempdir().unwrap();
    let (w, p) = fixture(temp.path());
    drop(w.packages.resource(&p, "composer").unwrap());
    let source = project(&w, &temp.path().join("repo"));
    let php = w
        .catalog
        .get(&source.php)
        .unwrap()
        .version
        .split('-')
        .next()
        .unwrap();
    let node = w
        .catalog
        .get(&source.node)
        .unwrap()
        .version
        .trim_start_matches('v');
    let yaml = format!(
        "schema: 1\nproject:\n  name: Archived\n  domain: archived.devforge.test\nruntimes:\n  php: {php}\n  node: {node}\n  composer: 2.99.0\npins: {{}}\n"
    );
    let mut configuration = devforge_projects::manifest::Manifest::parse(yaml.as_bytes()).unwrap();
    configuration.pin_all(&w.catalog).unwrap();
    let id = "b".repeat(32);
    let dir = temp.path().join("data/worktrees").join(&id);
    fs::create_dir_all(dir.join("retained")).unwrap();
    let mut record = devforge_workspace::worktrees::Record {
        isolated: true,
        schema: 2,
        id: id.clone(),
        input: devforge_workspace::worktrees::Input {
            source_id: source.id.clone(),
            name: "Archived".into(),
            branch: "feature/archive".into(),
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
        domain: "archived.devforge.test".into(),
        review_digest: "fixture".into(),
        project_id: None,
        stage: "archived".into(),
        last_error: None,
        configuration,
    };
    let path = dir.join("receipt.json");
    fs::write(&path, serde_json::to_vec(&record).unwrap()).unwrap();
    let preview = w.runtime_cleanup_preview("reviewed-composer", PreviewLimits::default());
    assert_eq!(
        preview.state,
        PreviewState::Blocked,
        "{:?}",
        preview.problems
    );
    assert!(
        preview
            .references
            .iter()
            .any(|r| r.kind == "retained_worktree_configuration" && r.id == id)
    );
    record.configuration.pins.clear();
    fs::write(path, serde_json::to_vec(&record).unwrap()).unwrap();
    let unknown = w.runtime_cleanup_preview("reviewed-composer", PreviewLimits::default());
    assert_eq!(unknown.state, PreviewState::Unknown);
    assert!(
        unknown
            .problems
            .contains(&"worktree_configuration_unresolved")
    );
}

#[test]
fn malformed_receipt_hash_metadata_is_unknown_even_when_descriptor_matches() {
    let temp = tempfile::tempdir().unwrap();
    let (w, p) = fixture(temp.path());
    drop(w.packages.resource(&p, "composer").unwrap());
    let path = temp
        .path()
        .join("data/runtimes/packages")
        .join(p.key().unwrap())
        .join(".devforge-receipt.json");
    let mut receipt: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    receipt["resource_hashes"]["composer"] = "invalid".into();
    fs::write(&path, receipt.to_string()).unwrap();
    let preview = w.runtime_cleanup_preview("reviewed-composer", PreviewLimits::default());
    assert_eq!(preview.state, PreviewState::Unknown);
    assert!(
        preview
            .problems
            .contains(&"runtime_receipt_metadata_unverified")
    );
    assert!(path.exists());
}

#[test]
fn physical_key_collision_blocks_inactive_project_and_retained_worktree_for_descriptor_changes() {
    for change in ["url", "sha", "maps"] {
        for retained_worktree in [false, true] {
            let temp = tempfile::tempdir().unwrap();
            let (mut w, target) = fixture(temp.path());
            drop(w.packages.resource(&target, "composer").unwrap());
            let mut collision = target.clone();
            match change {
                "url" => collision.url.push_str("?different-source"),
                "sha" => collision.sha256 = "f".repeat(64),
                "maps" => {
                    collision
                        .resources
                        .insert("composer".into(), "different.phar".into());
                    collision
                        .executables
                        .insert("tool".into(), "different-tool".into());
                }
                _ => unreachable!(),
            }
            assert_ne!(collision, target);
            assert_eq!(collision.key().unwrap(), target.key().unwrap());
            w.catalog
                .packages
                .insert("collision-composer".into(), collision);
            let source = project(&w, &temp.path().join("repo"));
            let expected_reference = if retained_worktree {
                let php = w
                    .catalog
                    .get(&source.php)
                    .unwrap()
                    .version
                    .split('-')
                    .next()
                    .unwrap();
                let node = w
                    .catalog
                    .get(&source.node)
                    .unwrap()
                    .version
                    .trim_start_matches('v');
                let yaml = format!(
                    "schema: 1\nproject:\n  name: Archived\n  domain: archived.devforge.test\nruntimes:\n  php: {php}\n  node: {node}\n  composer: 2.99.0\npins: {{}}\n"
                );
                let mut configuration =
                    devforge_projects::manifest::Manifest::parse(yaml.as_bytes()).unwrap();
                configuration.pin_all(&w.catalog).unwrap();
                assert_eq!(
                    configuration
                        .verify_pins(&w.catalog, Target::current().unwrap())
                        .unwrap()["composer"],
                    "collision-composer"
                );
                let id = "b".repeat(32);
                let dir = temp.path().join("data/worktrees").join(&id);
                fs::create_dir_all(dir.join("retained")).unwrap();
                let record = serde_json::json!({
                    "isolated":true,"schema":2,"id":id,"input":{"source_id":source.id,"name":"Archived","branch":"feature/archive","base":"HEAD","new_branch":true,"database":"empty","dependencies":false,"copy_environment":false},
                    "source_path":source.path,"common_dir":source.path.join(".git"),"git_dir":null,"commit":"fixture","domain":"archived.devforge.test","review_digest":"fixture","project_id":null,"stage":"archived","last_error":null,"configuration":configuration
                });
                fs::write(dir.join("receipt.json"), record.to_string()).unwrap();
                "retained_worktree_configuration"
            } else {
                let updated = w
                    .projects
                    .configure_composer(
                        &source.id,
                        source.revision,
                        "collision-composer",
                        &w.catalog,
                    )
                    .unwrap();
                w.projects.forget(&updated.id, updated.revision).unwrap();
                "inactive_project_runtime_selection"
            };
            let preview = w.runtime_cleanup_preview("reviewed-composer", PreviewLimits::default());
            assert_eq!(
                preview.state,
                PreviewState::Blocked,
                "change={change}, retained_worktree={retained_worktree}, problems={:?}",
                preview.problems
            );
            assert!(
                preview
                    .references
                    .iter()
                    .any(|r| r.kind == expected_reference)
            );
            assert!(
                preview
                    .problems
                    .contains(&"physical_package_descriptor_collision_unverified")
            );
        }
    }
}

#[test]
fn derived_web_runtime_same_physical_key_blocks_descriptor_drift() {
    let temp = tempfile::tempdir().unwrap();
    let (mut w, _) = fixture(temp.path());
    let source = project(&w, &temp.path().join("repo"));
    let mut header = vec![0u8; 96];
    match Target::current().unwrap() {
        Target::MacosArm64 => header[..8].copy_from_slice(&[0xcf, 0xfa, 0xed, 0xfe, 12, 0, 0, 1]),
        Target::MacosX64 => header[..8].copy_from_slice(&[0xcf, 0xfa, 0xed, 0xfe, 7, 0, 0, 1]),
        Target::WindowsX64 => {
            header[..2].copy_from_slice(b"MZ");
            header[60..64].copy_from_slice(&64u32.to_le_bytes());
            header[64..70].copy_from_slice(&[b'P', b'E', 0, 0, 0x64, 0x86]);
        }
    }
    let archive = temp.path().join("inert-web-header");
    fs::write(&archive, header).unwrap();
    let tool = if cfg!(windows) { "php-cgi" } else { "php-fpm" };
    // Format metadata only; this fixture never executes PHP/FPM/CGI.
    let target = Package {
        kind: "php".into(),
        version: w.catalog.get(&source.php).unwrap().version.clone(),
        variant: "fpm".into(),
        target: Target::current().unwrap(),
        url: "https://example.invalid/web".into(),
        sha256: hash_file(&archive).unwrap(),
        archive: Archive::File {
            path: "php-web".into(),
        },
        executables: BTreeMap::from([(tool.into(), "php-web".into())]),
        resources: BTreeMap::new(),
    };
    w.packages
        .install_cached(&target, &archive, &Cancellation::default(), |_| {})
        .unwrap();
    drop(w.packages.executable(&target, tool).unwrap());
    w.catalog
        .packages
        .insert("reviewed-web".into(), target.clone());
    let mut collision = target.clone();
    collision.url.push_str("?different-source");
    assert_eq!(collision.key().unwrap(), target.key().unwrap());
    w.catalog
        .packages
        .insert("collision-web".into(), collision.clone());
    assert_eq!(
        w.catalog
            .php_server(w.catalog.get(&source.php).unwrap())
            .unwrap(),
        &collision
    );
    let preview = w.runtime_cleanup_preview("reviewed-web", PreviewLimits::default());
    assert_eq!(
        preview.state,
        PreviewState::Blocked,
        "{:?}",
        preview.problems
    );
    assert!(
        preview
            .references
            .iter()
            .any(|r| r.kind == "project_web_runtime_selection")
    );
    assert!(
        preview
            .problems
            .contains(&"physical_package_descriptor_collision_unverified")
    );
}
