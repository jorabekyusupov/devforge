//! Real isolated registry/cache facts. No PHP process, native credentials or
//! downloads; cached fact fixtures do not establish runtime startability.
use devforge_core::{ErrorCode, SecretStore};
use devforge_runtime::package::Target;
use devforge_workspace::{Command, Error, Workspace, service_control::cached_preflight};
use std::{cell::Cell, fs, sync::Arc};

struct NoSecrets;
impl SecretStore for NoSecrets {
    fn get(&self, _: &str) -> Result<Option<String>, ErrorCode> {
        panic!("service preflight must not read credentials")
    }
    fn set(&self, _: &str, _: &str) -> Result<(), ErrorCode> {
        panic!("refused service action must not write credentials")
    }
    fn delete(&self, _: &str) -> Result<(), ErrorCode> {
        panic!("service action must not delete credentials")
    }
}
fn fixture() -> (tempfile::TempDir, Workspace, String) {
    let temp = tempfile::tempdir().unwrap();
    let project = temp.path().join("project");
    fs::create_dir_all(project.join("public")).unwrap();
    fs::write(project.join("public/index.php"), "<?php").unwrap();
    fs::write(
        project.join("composer.json"),
        r#"{"require":{"php":"^8.3","laravel/framework":"^13"}}"#,
    )
    .unwrap();
    let mut workspace = Workspace::open(temp.path().join("data"), Arc::new(NoSecrets)).unwrap();
    let target = Target::current().unwrap();
    let php = workspace
        .catalog
        .packages
        .iter()
        .find(|(_, p)| p.kind == "php" && p.target == target && p.executables.contains_key("php"))
        .unwrap()
        .0
        .clone();
    let node = workspace
        .catalog
        .packages
        .iter()
        .find(|(_, p)| p.kind == "node" && p.target == target && p.executables.contains_key("node"))
        .unwrap()
        .0
        .clone();
    let result = workspace
        .execute(
            Command::Import {
                path: project,
                name: "Service fixture".into(),
                php,
                node,
                domain: "service-fixture.devforge.test".into(),
                https: false,
                vite: false,
                queue: false,
            },
            &Default::default(),
        )
        .unwrap();
    (
        temp,
        workspace,
        result["project_id"].as_str().unwrap().into(),
    )
}

#[test]
fn denied_actions_preserve_metadata_without_startup_side_effects() {
    let (root, mut workspace, id) = fixture();
    let before = workspace.projects.get(&id).unwrap();
    for action in ["start", "stop", "restart"] {
        let check = || {
            Err(Error {
                scope: "workspace",
                code: "deadline_exceeded".into(),
                project_id: Some(id.clone()),
            })
        };
        let error = workspace
            .execute_service_action(&id, action, &check)
            .unwrap_err();
        assert_eq!(error.code, "deadline_exceeded");
        assert_eq!(error.project_id.as_deref(), Some(id.as_str()));
    }
    assert_eq!(
        workspace.projects.get(&id).unwrap().revision,
        before.revision
    );
    assert!(!root.path().join("data/serving/sessions").exists());
    assert!(!root.path().join("data/queue").exists());
}

#[test]
fn live_missing_cache_is_refused_without_download_and_advisory_facts_are_not_full_preflight() {
    let (root, mut workspace, id) = fixture();
    let mut snapshot = workspace.snapshot().unwrap();
    assert_eq!(
        cached_preflight(&snapshot, &id).unwrap_err().code,
        "service_precondition_missing"
    );
    // Synthetic observed cache facts exercise the pure policy, not fake PHP.
    for runtime in &mut snapshot.runtimes {
        runtime.installed = true;
        runtime.problem = None;
    }
    assert!(cached_preflight(&snapshot, &id).is_ok());
    for action in ["start", "restart"] {
        assert_eq!(
            workspace
                .execute_service_action(&id, action, &|| Ok(()))
                .unwrap_err()
                .code,
            "service_precondition_missing"
        );
    }
    assert_eq!(
        fs::read_dir(root.path().join("data/runtimes/packages"))
            .unwrap()
            .count(),
        0
    );
    assert!(!root.path().join("data/serving/sessions").exists());
}

#[test]
fn stop_does_not_require_runtime_cache_or_existing_project_directory_and_action_family_is_closed() {
    let (_root, mut workspace, id) = fixture();
    let path = workspace.projects.get(&id).unwrap().path;
    fs::remove_dir_all(path).unwrap();
    assert_eq!(
        workspace
            .execute_service_action(&id, "stop", &|| Ok(()))
            .unwrap()["stopped"],
        true
    );
    let checks = Cell::new(0);
    let result = workspace.execute_service_action(&id, "runtime_install", &|| {
        checks.set(checks.get() + 1);
        Ok(())
    });
    assert_eq!(result.unwrap_err().code, "service_action_unsupported");
    assert_eq!(checks.get(), 0);
}
