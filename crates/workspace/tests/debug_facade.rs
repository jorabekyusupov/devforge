use devforge_core::{ErrorCode, SecretStore};
use devforge_runtime::package::Target;
use devforge_workspace::{Command, Workspace};
use std::{fs, sync::Arc};

struct NoSecrets;
impl SecretStore for NoSecrets {
    fn get(&self, _: &str) -> Result<Option<String>, ErrorCode> {
        Ok(None)
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
fn scoped_debug_projection_does_not_create_prune_or_leak_profiles() {
    let (_temp, workspace, project) = fixture();
    let base = workspace.projects.root().join("debugging");
    let empty = serde_json::to_value(
        workspace
            .external_debug_view(&project, false, false)
            .unwrap(),
    )
    .unwrap();
    assert_eq!(empty["mode"], "off");
    assert!(empty["session"].is_null());
    assert_eq!(empty["profile_count"], 0);
    assert!(
        !base.exists(),
        "observer must not create debugging directories"
    );
    let directory = base.join(&project).join("profiles");
    fs::create_dir_all(&directory).unwrap();
    let profile = directory.join("cachegrind.a.out");
    fs::write(&profile, b"events: Time_(10ns) Memory_(bytes)\nfl=(1) /outside/private.php\nfn=(1) main\n1 100000 8\nsummary: 100000 8\n").unwrap();
    let metadata = serde_json::to_value(
        workspace
            .external_debug_view(&project, false, false)
            .unwrap(),
    )
    .unwrap();
    assert_eq!(metadata["profile_count"], 1);
    assert_eq!(metadata["profiles"].as_array().unwrap().len(), 0);
    assert!(profile.exists());
    let detailed = serde_json::to_value(
        workspace
            .external_debug_view(&project, true, false)
            .unwrap(),
    )
    .unwrap();
    let id = detailed["profiles"][0]["id"].as_str().unwrap();
    assert_eq!(id.len(), 32);
    let data = workspace.external_debug_profile(&project, id).unwrap();
    assert!(!data.to_string().contains("/outside/"));
    let export = workspace
        .debug_profile_export_preview(&project, id)
        .unwrap();
    assert!(export.len() <= 128 * 1024);
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&export).unwrap(),
        data
    );
    assert!(!String::from_utf8(export).unwrap().contains("/outside/"));
    assert!(profile.exists());
    assert!(
        workspace
            .external_debug_profile(&project, "../../private")
            .is_err()
    );
}

#[test]
fn reviewed_debug_helpers_preserve_breakpoints_and_reject_invalid_inputs() {
    use devforge_debugger::{Assignment, Breakpoint, Mode};
    let (_temp, mut workspace, project) = fixture();
    let original = workspace.projects.get(&project).unwrap();
    let mut settings = original.debugger.clone();
    settings.breakpoints = vec![Breakpoint {
        file: "public/index.php".into(),
        line: 1,
    }];
    workspace
        .projects
        .save_debugger(&project, original.revision, settings.clone())
        .unwrap();
    let before = workspace.projects.get(&project).unwrap();
    assert!(
        workspace
            .debug_mode_reviewed(&project, Mode::Off, None, original.revision)
            .is_err()
    );
    assert_eq!(workspace.projects.get(&project).unwrap().debugger, settings);
    workspace
        .debug_mode_reviewed(&project, Mode::Off, None, before.revision)
        .unwrap();
    let after = workspace.projects.get(&project).unwrap();
    assert_eq!(after.debugger.breakpoints, settings.breakpoints);
    assert!(
        workspace
            .external_debug_session_identity(&project)
            .unwrap()
            .is_none()
    );
    assert!(
        workspace
            .debug_breakpoints_guarded(&project, before.revision, vec![])
            .is_err()
    );
    workspace
        .debug_breakpoints_guarded(&project, after.revision, vec![])
        .unwrap();
    assert!(
        workspace
            .projects
            .get(&project)
            .unwrap()
            .debugger
            .breakpoints
            .is_empty()
    );
    for path in [
        "//other.test/x",
        "https://external.test/",
        "/x?token=secret",
        "/x#fragment",
        "/x\\y",
        "/has space",
    ] {
        assert!(
            workspace
                .debug_start_scoped(&project, path, Arc::new(|_| true))
                .is_err(),
            "invalid path accepted"
        );
    }
    let value = Assignment {
        name: "$nested->x".into(),
        kind: "string".into(),
        value: "secret".into(),
    };
    assert!(
        workspace
            .debug_assign_reviewed(&project, value, "not-a-session", 1, Arc::new(|| true))
            .is_err()
    );
    assert!(!workspace.projects.root().join("debugging").exists());
}

#[test]
fn external_debug_activation_preserves_old_profile_history() {
    use devforge_debugger::{Mode, session::Activation};
    use std::time::{Duration, SystemTime};
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("debug");
    let profiles = root.join("profiles");
    std::fs::create_dir_all(&profiles).unwrap();
    std::fs::write(root.join(".devforge-debug"), "a".repeat(32)).unwrap();
    for n in 0..24 {
        let file = std::fs::File::create(profiles.join(format!("cachegrind.{n}.out"))).unwrap();
        file.set_modified(SystemTime::now() - Duration::from_secs(60))
            .unwrap();
    }
    assert_eq!(devforge_debugger::store::list(&profiles).unwrap().len(), 24);
    let _activation = Activation::start_unpruned(
        &root,
        &"a".repeat(32),
        "fixture".into(),
        Mode::Profile,
        temp.path(),
        vec![],
        vec![],
    )
    .unwrap();
    assert_eq!(devforge_debugger::store::list(&profiles).unwrap().len(), 24);
}

#[cfg(feature = "test-support")]
#[test]
fn agent_breakpoint_edit_returns_busy_for_active_session_without_mutation() {
    use devforge_debugger::Breakpoint;
    let (_temp, mut workspace, project) = fixture();
    let before = workspace.projects.get(&project).unwrap();
    let _ = workspace.test_synthetic_paused_debugger(&project).unwrap();
    let result = workspace.debug_breakpoints_guarded(
        &project,
        before.revision,
        vec![Breakpoint {
            file: "public/index.php".into(),
            line: 1,
        }],
    );
    assert_eq!(result.err().unwrap().code, "busy");
    let after = workspace.projects.get(&project).unwrap();
    assert_eq!(after.revision, before.revision);
    assert_eq!(after.debugger.breakpoints, before.debugger.breakpoints);
}
