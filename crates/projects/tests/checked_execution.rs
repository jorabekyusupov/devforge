//! Actual owned native-test child for the checked Invocation poll. Catalog
//! declarations supply a synthetic context, not PHP/Node compatibility evidence.
use devforge_core::{ErrorCode, SecretStore};
use devforge_projects::{
    ProjectError,
    execution::{Context, Tool},
    project::{Catalog, ProjectStore},
};
use devforge_runtime::{
    Cancellation,
    package::{Archive, Limits, Package, PackageStore, Target, hash_file},
};
use std::{
    cell::Cell,
    collections::BTreeMap,
    fs,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

struct DenySecrets;
impl SecretStore for DenySecrets {
    fn get(&self, _: &str) -> Result<Option<String>, ErrorCode> {
        panic!("uncredentialed context must not read native secrets")
    }
    fn set(&self, _: &str, _: &str) -> Result<(), ErrorCode> {
        panic!("fixture must not write native secrets")
    }
    fn delete(&self, _: &str) -> Result<(), ErrorCode> {
        panic!("fixture must not delete native secrets")
    }
}
fn cache(store: &PackageStore, root: &Path, kind: &str, version: &str) -> Package {
    let source = if kind == "composer" {
        let path = root.join("inert-composer.fixture");
        fs::write(&path, "not executed").unwrap();
        path
    } else {
        std::env::current_exe().unwrap()
    };
    let resource = kind == "composer";
    let path = if resource {
        "composer.phar"
    } else {
        "native-fixture.exe"
    };
    let package = Package {
        kind: kind.into(),
        version: version.into(),
        variant: "checked-fixture".into(),
        target: Target::current().unwrap(),
        url: "https://example.invalid/cache-only".into(),
        sha256: hash_file(&source).unwrap(),
        archive: Archive::File { path: path.into() },
        executables: if resource {
            BTreeMap::new()
        } else {
            BTreeMap::from([(kind.into(), path.into())])
        },
        resources: if resource {
            BTreeMap::from([("composer".into(), path.into())])
        } else {
            BTreeMap::new()
        },
    };
    store
        .install_cached(&package, &source, &Cancellation::default(), |_| {})
        .unwrap();
    package
}

#[test]
fn checked_dependency_child_fixture() {
    if Path::new(".checked-dependency-child").is_file() {
        fs::write("child-started", b"ready").unwrap();
        std::thread::sleep(Duration::from_secs(10));
        fs::write("child-completed", b"unexpected").unwrap();
    }
}

#[test]
fn policy_denial_inside_dependency_poll_stops_owned_child_and_preserves_policy_error() {
    let temp = tempfile::tempdir().unwrap();
    let packages = PackageStore::open(temp.path().join("packages"), Limits::default()).unwrap();
    let php = cache(&packages, temp.path(), "php", "8.5.12");
    let node = cache(&packages, temp.path(), "node", "24.22.2");
    let composer = cache(&packages, temp.path(), "composer", "2.11.2");
    let catalog = Catalog {
        packages: BTreeMap::from([
            ("php".into(), php),
            ("node".into(), node),
            ("composer".into(), composer),
        ]),
        superseded: BTreeMap::new(),
        defaults: BTreeMap::from([("composer".into(), "2.11.2".into())]),
    };
    let project = temp.path().join("project");
    fs::create_dir_all(project.join("public")).unwrap();
    fs::write(project.join("public/index.php"), "<?php").unwrap();
    fs::write(
        project.join("composer.json"),
        r#"{"require":{"php":"^8.3","laravel/framework":"^13"}}"#,
    )
    .unwrap();
    fs::write(project.join(".checked-dependency-child"), b"isolated").unwrap();
    let store = ProjectStore::open(temp.path().join("meta")).unwrap();
    let record = store
        .import(&project, "Checked child", "php", "node", &catalog)
        .unwrap();
    let invocation = Context::resolve(&store, &record.id, &catalog, &packages, &DenySecrets)
        .unwrap()
        .invocation(
            Tool::Node,
            vec![
                "--exact".into(),
                "checked_dependency_child_fixture".into(),
                "--nocapture".into(),
            ],
            &catalog,
            &packages,
        )
        .unwrap();
    // Observe readiness independently of Invocation's launch/hash cost. Its
    // Windows cleanup intentionally permits a 5s grace before terminating a
    // fixture that does not read stdin; that is not policy polling latency.
    let stop_observer = Arc::new(AtomicBool::new(false));
    let observer_stop = stop_observer.clone();
    let ready_file = project.join("child-started");
    let observer = std::thread::spawn(move || {
        loop {
            if ready_file.is_file() {
                return Some(Instant::now());
            }
            if observer_stop.load(Ordering::Acquire) {
                return None;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    });
    let denied_at = Cell::new(None);
    let check = || {
        if project.join("child-started").is_file() {
            denied_at.set(Some(Instant::now()));
            Err(ProjectError::SecretUnavailable)
        } else {
            Ok(())
        }
    };
    let result = invocation.run_cancellable_checked(
        temp.path().join("logs"),
        &Cancellation::default(),
        &check,
    );
    stop_observer.store(true, Ordering::Release);
    let observed_ready = observer.join().unwrap();
    assert!(matches!(result, Err(ProjectError::SecretUnavailable)));
    assert!(project.join("child-started").is_file());
    assert!(!project.join("child-completed").exists());
    let denied_at = denied_at
        .get()
        .expect("native child never reached policy denial");
    let observed_ready = observed_ready.expect("native child never reached fixture readiness");
    // If denial wins before the observer is scheduled, the policy response was
    // already complete at the first independent readiness observation.
    assert!(
        denied_at.saturating_duration_since(observed_ready) < Duration::from_secs(5),
        "policy polling waited after the dependency child was ready"
    );
}
