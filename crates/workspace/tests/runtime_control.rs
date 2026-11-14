//! Isolated registry + package-integrity contracts, not PHP/Node execution.
//! Tiny native-header fixtures are never launched and make no runtime-health claim.
use devforge_core::{ErrorCode, SecretStore};
use devforge_runtime::{
    Cancellation,
    package::{Archive, Package, Target, hash_file},
};
use devforge_workspace::{Command, Error, Workspace, runtime_control::validate_selection_snapshot};
use std::{cell::Cell, collections::BTreeMap, fs, io::Read, path::Path, sync::Arc};

struct NoSecrets;
impl SecretStore for NoSecrets {
    fn get(&self, _: &str) -> Result<Option<String>, ErrorCode> {
        panic!("runtime preflight must not read credentials")
    }
    fn set(&self, _: &str, _: &str) -> Result<(), ErrorCode> {
        panic!("runtime selection must not write credentials")
    }
    fn delete(&self, _: &str) -> Result<(), ErrorCode> {
        panic!("runtime action must not delete credentials")
    }
}
fn fixture() -> (tempfile::TempDir, Workspace, String) {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("project");
    fs::create_dir_all(path.join("public")).unwrap();
    fs::write(path.join("public/index.php"), "<?php").unwrap();
    fs::write(
        path.join("composer.json"),
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
    let value = workspace
        .execute(
            Command::Import {
                path,
                name: "Retained name".into(),
                php,
                node,
                domain: "runtime-fixture.devforge.test".into(),
                https: false,
                vite: false,
                queue: true,
            },
            &Cancellation::default(),
        )
        .unwrap();
    (
        temp,
        workspace,
        value["project_id"].as_str().unwrap().into(),
    )
}
fn configure(workspace: &Workspace, id: &str, php: &str, node: &str) -> Command {
    let snapshot = workspace.snapshot().unwrap();
    let row = snapshot
        .projects
        .iter()
        .find(|row| row.project.id == id)
        .unwrap();
    let site = row.site.as_ref().unwrap();
    Command::Configure {
        id: id.into(),
        revision: row.project.revision,
        name: row.project.name.clone(),
        php: php.into(),
        node: node.into(),
        domain: site.domain.clone(),
        https: site.https,
        vite: site.vite,
        queue: row.development.queue,
    }
}
fn cached_fixture(
    workspace: &mut Workspace,
    root: &Path,
    id: &str,
    kind: &str,
    version: &str,
    variant: &str,
    names: &[&str],
) {
    let archive = root.join(format!("{id}.fixture"));
    let mut bytes = Vec::new();
    fs::File::open(std::env::current_exe().unwrap())
        .unwrap()
        .take(4096)
        .read_to_end(&mut bytes)
        .unwrap();
    fs::write(&archive, bytes).unwrap();
    let package = Package {
        kind: kind.into(),
        version: version.into(),
        variant: variant.into(),
        target: Target::current().unwrap(),
        url: "https://example.invalid/never-downloaded".into(),
        sha256: hash_file(&archive).unwrap(),
        archive: Archive::File {
            path: "fixture.exe".into(),
        },
        executables: names
            .iter()
            .map(|name| ((*name).into(), "fixture.exe".into()))
            .collect(),
        resources: if names.is_empty() {
            BTreeMap::from([("fixture".into(), "fixture.exe".into())])
        } else {
            BTreeMap::new()
        },
    };
    workspace
        .packages
        .install_cached(&package, &archive, &Cancellation::default(), |_| {})
        .unwrap();
    workspace.catalog.packages.insert(id.into(), package);
    workspace.refresh_runtimes().unwrap();
}
fn candidates(workspace: &mut Workspace, root: &Path) {
    cached_fixture(
        workspace,
        root,
        "candidate-php",
        "php",
        "8.4.99",
        if cfg!(windows) { "default" } else { "cli" },
        if cfg!(windows) {
            &["php", "php-cgi"]
        } else {
            &["php"]
        },
    );
    if !cfg!(windows) {
        cached_fixture(
            workspace,
            root,
            "candidate-web",
            "php",
            "8.4.99",
            "fpm",
            &["php-fpm"],
        );
    }
    cached_fixture(
        workspace,
        root,
        "candidate-node",
        "node",
        "24.99.0",
        "native",
        &["node"],
    );
}

#[test]
fn nonselection_fields_and_stale_revision_are_refused_without_registry_or_site_changes() {
    let (_root, mut workspace, id) = fixture();
    let project = workspace.projects.get(&id).unwrap();
    let before_sites = fs::read(workspace.sites.root().join("sites.json")).unwrap();
    for field in ["name", "domain", "https", "vite", "queue", "revision"] {
        let mut command = configure(&workspace, &id, &project.php, &project.node);
        if let Command::Configure {
            name,
            domain,
            https,
            vite,
            queue,
            revision,
            ..
        } = &mut command
        {
            match field {
                "name" => *name = "Other name".into(),
                "domain" => *domain = "other.devforge.test".into(),
                "https" => *https = true,
                "vite" => *vite = true,
                "queue" => *queue = false,
                "revision" => *revision += 1,
                _ => unreachable!(),
            }
        }
        let expected = if field == "revision" {
            "stale_revision"
        } else {
            "runtime_selection_fields_changed"
        };
        assert_eq!(
            validate_selection_snapshot(&workspace.snapshot().unwrap(), &command)
                .unwrap_err()
                .code,
            expected
        );
        assert_eq!(
            workspace
                .execute_runtime_action(command, &Cancellation::default(), &|| Ok(()))
                .unwrap_err()
                .code,
            expected
        );
    }
    assert_eq!(
        workspace.projects.get(&id).unwrap().revision,
        project.revision
    );
    assert_eq!(
        fs::read(workspace.sites.root().join("sites.json")).unwrap(),
        before_sites
    );
}

#[test]
fn debug_php_change_and_missing_cache_are_rejected_before_selection() {
    let (_root, mut workspace, id) = fixture();
    let original = workspace.projects.get(&id).unwrap();
    let command = configure(&workspace, &id, &original.php, &original.node);
    assert_eq!(
        workspace
            .execute_runtime_action(command, &Cancellation::default(), &|| Ok(()))
            .unwrap_err()
            .code,
        "runtime_selection_precondition_missing"
    );
    workspace
        .projects
        .save_debugger(
            &id,
            original.revision,
            devforge_debugger::Settings {
                runtime: Some("a".repeat(32)),
                mode: devforge_debugger::Mode::Debug,
                breakpoints: Vec::new(),
            },
        )
        .unwrap();
    let current = workspace.projects.get(&id).unwrap();
    let command = configure(&workspace, &id, "different-php", &current.node);
    assert_eq!(
        workspace
            .execute_runtime_action(command, &Cancellation::default(), &|| Ok(()))
            .unwrap_err()
            .code,
        "debug_runtime_change"
    );
    assert_eq!(workspace.projects.get(&id).unwrap().php, original.php);
    assert_eq!(
        workspace.projects.get(&id).unwrap().revision,
        current.revision
    );
}

#[test]
fn cached_selection_changes_php_node_only_preserving_composer_site_queue_and_noop_revision() {
    let (root, mut workspace, id) = fixture();
    candidates(&mut workspace, root.path());
    let before = workspace.snapshot().unwrap();
    let project = workspace.projects.get(&id).unwrap();
    let sites = fs::read(workspace.sites.root().join("sites.json")).unwrap();
    let command = configure(&workspace, &id, "candidate-php", "candidate-node");
    validate_selection_snapshot(&workspace.snapshot().unwrap(), &command).unwrap();
    let result = workspace
        .execute_runtime_action(command, &Cancellation::default(), &|| Ok(()))
        .unwrap();
    assert_eq!(result["changed"], true);
    assert_eq!(result["restarted"], false);
    let after = workspace.projects.get(&id).unwrap();
    assert_eq!(after.php, "candidate-php");
    assert_eq!(after.node, "candidate-node");
    assert_eq!(after.composer, project.composer);
    assert_eq!(after.name, project.name);
    assert_eq!(
        fs::read(workspace.sites.root().join("sites.json")).unwrap(),
        sites
    );
    assert_eq!(
        workspace.snapshot().unwrap().projects[0].development.queue,
        before.projects[0].development.queue
    );
    let noop = configure(&workspace, &id, "candidate-php", "candidate-node");
    assert_eq!(
        workspace
            .execute_runtime_action(noop, &Cancellation::default(), &|| Ok(()))
            .unwrap()["changed"],
        false
    );
    assert_eq!(
        workspace.projects.get(&id).unwrap().revision,
        after.revision
    );
}

#[test]
fn cached_install_reuse_has_shared_effect_without_pin_changes_or_postcommit_policy_check() {
    let (root, mut workspace, id) = fixture();
    cached_fixture(
        &mut workspace,
        root.path(),
        "cached-resource",
        "fixture-resource",
        "1.0.0",
        "resource",
        &[],
    );
    let before = workspace.projects.get(&id).unwrap();
    let calls = Cell::new(0);
    let check = || {
        calls.set(calls.get() + 1);
        if calls.get() > 2 {
            Err(Error {
                scope: "workspace",
                code: "timed_out".into(),
                project_id: None,
            })
        } else {
            Ok(())
        }
    };
    let result = workspace
        .execute_runtime_action(
            Command::Install {
                package: "cached-resource".into(),
            },
            &Cancellation::default(),
            &check,
        )
        .unwrap();
    assert_eq!(result["installed"], true);
    assert_eq!(result["reused"], true);
    assert_eq!(calls.get(), 2);
    let after = workspace.projects.get(&id).unwrap();
    assert_eq!(
        (after.php, after.node, after.composer, after.revision),
        (before.php, before.node, before.composer, before.revision)
    );
}

#[test]
fn cancelled_denied_foreign_target_and_remove_requests_have_no_runtime_effects() {
    let (_root, mut workspace, id) = fixture();
    let before = workspace.projects.get(&id).unwrap();
    let cancelled = Cancellation::default();
    cancelled.cancel();
    assert_eq!(
        workspace
            .execute_runtime_action(
                Command::Install {
                    package: "not-resolved".into()
                },
                &cancelled,
                &|| Ok(())
            )
            .unwrap_err()
            .code,
        "cancelled"
    );
    assert_eq!(
        workspace
            .execute_runtime_action(
                Command::RemoveRuntime {
                    package: before.php.clone()
                },
                &Cancellation::default(),
                &|| Ok(())
            )
            .unwrap_err()
            .code,
        "runtime_action_unsupported"
    );
    let command = configure(&workspace, &id, &before.php, &before.node);
    assert_eq!(
        workspace
            .execute_runtime_action(command, &Cancellation::default(), &|| Err(Error {
                scope: "workspace",
                code: "timed_out".into(),
                project_id: Some(id.clone())
            }))
            .unwrap_err()
            .code,
        "timed_out"
    );
    let foreign = workspace
        .catalog
        .packages
        .iter()
        .find(|(_, p)| p.target != Target::current().unwrap())
        .unwrap()
        .0
        .clone();
    assert_eq!(
        workspace
            .execute_runtime_action(
                Command::Install { package: foreign },
                &Cancellation::default(),
                &|| Ok(())
            )
            .unwrap_err()
            .code,
        "unsupported_target"
    );
    assert_eq!(
        workspace.projects.get(&id).unwrap().revision,
        before.revision
    );
}
