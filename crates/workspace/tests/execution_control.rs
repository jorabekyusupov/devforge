//! Actual owned native-test child; synthetic context declarations do not prove
//! PHP/Node compatibility. No native credentials, downloads or application DB.
use devforge_core::{ErrorCode, SecretStore};
use devforge_projects::project::Catalog;
use devforge_runtime::{
    Cancellation,
    package::{Archive, Package, Target, hash_file},
};
use devforge_workspace::{
    Workspace,
    execution_control::{ExecutionRequest, ExecutionTool},
};
use std::{collections::BTreeMap, fs, path::Path, sync::Arc};
struct NoSecrets;
impl SecretStore for NoSecrets {
    fn get(&self, _: &str) -> Result<Option<String>, ErrorCode> {
        panic!("data-free fixture must not probe native credentials")
    }
    fn set(&self, _: &str, _: &str) -> Result<(), ErrorCode> {
        panic!("execution must not set native credentials")
    }
    fn delete(&self, _: &str) -> Result<(), ErrorCode> {
        panic!("execution must not delete native credentials")
    }
}
fn fixture() -> (tempfile::TempDir, Workspace, String) {
    let temp = tempfile::tempdir().unwrap();
    let mut workspace = Workspace::open(temp.path().join("data"), Arc::new(NoSecrets)).unwrap();
    let source = std::env::current_exe().unwrap();
    let mut catalog = Catalog {
        packages: BTreeMap::new(),
        superseded: BTreeMap::new(),
        defaults: BTreeMap::from([("composer".into(), "2.11.2".into())]),
    };
    for (id, kind, version) in [
        ("php", "php", "8.5.12"),
        ("node", "node", "24.22.2"),
        ("composer", "composer", "2.11.2"),
    ] {
        let resource = kind == "composer";
        let package = Package {
            kind: kind.into(),
            version: version.into(),
            variant: "native-test-fixture".into(),
            target: Target::current().unwrap(),
            url: "https://example.invalid/never-downloaded".into(),
            sha256: hash_file(&source).unwrap(),
            archive: Archive::File {
                path: "fixture.exe".into(),
            },
            executables: if resource {
                BTreeMap::new()
            } else {
                BTreeMap::from([(kind.into(), "fixture.exe".into())])
            },
            resources: if resource {
                BTreeMap::from([("composer".into(), "fixture.exe".into())])
            } else {
                BTreeMap::new()
            },
        };
        workspace
            .packages
            .install_cached(&package, &source, &Cancellation::default(), |_| {})
            .unwrap();
        catalog.packages.insert(id.into(), package);
    }
    workspace.catalog = catalog;
    let project = temp.path().join("project");
    fs::create_dir_all(project.join("public")).unwrap();
    fs::write(project.join("public/index.php"), "<?php").unwrap();
    fs::write(
        project.join("composer.json"),
        r#"{"require":{"php":"^8.3","laravel/framework":"^13"}}"#,
    )
    .unwrap();
    fs::write(project.join(".execution-child-fixture"), "owned").unwrap();
    let record = workspace
        .projects
        .import(
            &project,
            "Execution fixture",
            "php",
            "node",
            &workspace.catalog,
        )
        .unwrap();
    (temp, workspace, record.id)
}
fn request() -> ExecutionRequest {
    ExecutionRequest {
        tool: ExecutionTool::Node,
        args: vec![
            "--exact".into(),
            "execution_child_fixture".into(),
            "--nocapture".into(),
            "--skip".into(),
            "PRIVATE_ARG_CANARY".into(),
        ],
        timeout_ms: 30000,
        managed_test: true,
    }
}
#[test]
fn execution_child_fixture() {
    if Path::new(".execution-child-fixture").is_file() {
        fs::write("child-started", "yes").unwrap();
        for arg in std::env::args().skip(1) {
            println!("{arg}");
        }
        match fs::read_to_string("mode").unwrap_or_default().as_str() {
            "fail" => std::process::exit(7),
            "sleep" => std::thread::sleep(std::time::Duration::from_secs(10)),
            "flood" => {
                for _ in 0..100 {
                    println!("{}", "x".repeat(4096));
                }
            }
            _ => println!("owned-success"),
        }
    }
}
#[test]
fn completed_nonzero_and_success_are_typed_outcomes_and_supplied_argv_never_reach_logs() {
    let (root, mut workspace, id) = fixture();
    let project = workspace.projects.get(&id).unwrap().path;
    let request = request();
    fs::write(project.join("mode"), "fail").unwrap();
    let facts = workspace.execution_preflight(&id, &request).unwrap();
    assert!(facts.native_summary().test_isolated);
    assert!(!facts.sensitive_context());
    let failed = workspace
        .execute_managed_execution(&id, &request, &facts, &Cancellation::default(), &|| Ok(()))
        .unwrap();
    assert!(!failed.success);
    assert_eq!(failed.exit_code, Some(7));
    assert!(
        !serde_json::to_string(&failed)
            .unwrap()
            .contains("PRIVATE_ARG_CANARY")
    );
    fs::write(project.join("mode"), "success").unwrap();
    let passed = workspace
        .execute_managed_execution(&id, &request, &facts, &Cancellation::default(), &|| Ok(()))
        .unwrap();
    assert!(passed.success);
    assert_eq!(passed.exit_code, Some(0));
    let logs = root.path().join("data/projects/managed-execution-logs");
    for entry in fs::read_dir(logs).unwrap() {
        let path = entry.unwrap().path();
        if path.is_file() {
            assert!(
                !String::from_utf8_lossy(&fs::read(path).unwrap()).contains("PRIVATE_ARG_CANARY")
            );
        }
    }
}
#[test]
fn captured_context_cannot_be_rebased_and_managed_test_refuses_env_and_testing_overrides() {
    let (_root, mut workspace, id) = fixture();
    let project = workspace.projects.get(&id).unwrap().path;
    let mut request = request();
    let facts = workspace.execution_preflight(&id, &request).unwrap();
    fs::write(project.join(".env"), "DB_DATABASE=development\n").unwrap();
    assert_eq!(
        workspace
            .execution_preflight(&id, &request)
            .err()
            .unwrap()
            .code,
        "execution_test_context_unverified"
    );
    request.managed_test = false;
    assert_eq!(
        workspace
            .execute_managed_execution(&id, &request, &facts, &Cancellation::default(), &|| Ok(()))
            .err()
            .unwrap()
            .code,
        "execution_context_changed"
    );
    assert!(!project.join("child-started").exists());
    fs::remove_file(project.join(".env")).unwrap();
    fs::write(project.join("phpunit.xml"), "<phpunit/>").unwrap();
    request.managed_test = true;
    assert_eq!(
        workspace
            .execution_preflight(&id, &request)
            .err()
            .unwrap()
            .code,
        "execution_test_context_unverified"
    );
}
#[test]
fn native_argument_preview_masks_inline_and_next_secret_values_even_verified_scripts() {
    let (_root, workspace, id) = fixture();
    let project = workspace.projects.get(&id).unwrap().path;
    fs::write(project.join("safe.php"), "<?php").unwrap();
    let mut request = request();
    request.args = vec![
        "--password".into(),
        "safe.php".into(),
        "-r".into(),
        "echo 'PRIVATE_INLINE';".into(),
        "--help".into(),
        "safe.php".into(),
    ];
    let facts = workspace.execution_preflight(&id, &request).unwrap();
    let preview = facts.argument_preview(&request.args);
    assert_eq!(preview[0], "[REDACTED_ARGUMENT]");
    assert_eq!(preview[1], "[REDACTED_ARGUMENT]");
    assert_eq!(preview[3], "[REDACTED_INLINE_CODE]");
    assert_eq!(preview[4], "--help");
    assert_eq!(preview[5], "safe.php");
    assert!(
        !serde_json::to_string(&preview)
            .unwrap()
            .contains("PRIVATE_INLINE")
    );
}
#[test]
fn native_git_context_is_sensitive_without_executing_host_git_or_exposing_agent_path() {
    let (_root, workspace, id) = fixture();
    let request = ExecutionRequest {
        tool: ExecutionTool::Git,
        args: vec!["status".into()],
        timeout_ms: 30000,
        managed_test: false,
    };
    let facts = workspace.execution_preflight(&id, &request).unwrap();
    assert!(facts.sensitive_context());
    assert!(facts.native_summary().environment);
    if let Some(path) = std::env::var_os("SSH_AUTH_SOCK") {
        let encoded = serde_json::to_string(&facts).unwrap();
        if !path.is_empty() {
            assert!(!encoded.contains(path.to_string_lossy().as_ref()));
        }
    }
}
#[test]
fn output_is_bounded_and_prelaunch_cancel_and_timeout_are_truthful() {
    let (_root, mut workspace, id) = fixture();
    let project = workspace.projects.get(&id).unwrap().path;
    let mut request = request();
    fs::write(project.join("mode"), "flood").unwrap();
    let facts = workspace.execution_preflight(&id, &request).unwrap();
    let output = workspace
        .execute_managed_execution(&id, &request, &facts, &Cancellation::default(), &|| Ok(()))
        .unwrap();
    assert!(output.success && output.truncated && output.dropped_bytes > 0);
    assert!(serde_json::to_vec(&output).unwrap().len() <= 32768);
    assert!(output.logs.iter().map(|log| log.text.len()).sum::<usize>() <= 16384);
    let cancel = Cancellation::default();
    cancel.cancel();
    assert_eq!(
        workspace
            .execute_managed_execution(&id, &request, &facts, &cancel, &|| Ok(()))
            .err()
            .unwrap()
            .code,
        "cancelled"
    );
    request.timeout_ms = 1;
    let facts = workspace.execution_preflight(&id, &request).unwrap();
    assert_eq!(
        workspace
            .execute_managed_execution(&id, &request, &facts, &Cancellation::default(), &|| Ok(()))
            .err()
            .unwrap()
            .code,
        "timed_out"
    );
}
