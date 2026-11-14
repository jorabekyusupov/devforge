//! Explicit native regression; copies reviewed binaries into disposable owned data.
use super::*;
use std::path::Path;

struct Secrets(Mutex<BTreeMap<String, String>>);
impl SecretStore for Secrets {
    fn get(&self, key: &str) -> std::result::Result<Option<String>, devforge_core::ErrorCode> {
        Ok(self.0.lock().unwrap().get(key).cloned())
    }
    fn set(&self, key: &str, value: &str) -> std::result::Result<(), devforge_core::ErrorCode> {
        self.0.lock().unwrap().insert(key.into(), value.into());
        Ok(())
    }
    fn delete(&self, key: &str) -> std::result::Result<(), devforge_core::ErrorCode> {
        self.0.lock().unwrap().remove(key);
        Ok(())
    }
}
fn copy_owned_tree(source: &Path, destination: &Path, boundary: &Path) {
    assert!(fs::canonicalize(source).unwrap().starts_with(boundary));
    if fs::metadata(source).unwrap().is_dir() {
        fs::create_dir_all(destination).unwrap();
        for entry in fs::read_dir(source).unwrap() {
            let entry = entry.unwrap();
            copy_owned_tree(
                &entry.path(),
                &destination.join(entry.file_name()),
                boundary,
            );
        }
    } else {
        fs::copy(source, destination).unwrap();
    }
}
#[test]
#[ignore = "explicit native PHP/FPM/Caddy cached-store fixture; no downloads"]
fn invalid_configure_name_preserves_live_session_and_http() {
    assert_rejected_configuration_preserves_live_session(false);
}
#[test]
#[ignore = "explicit native PHP/FPM/Caddy cached-store fixture; no downloads"]
fn debug_php_rejection_preserves_live_session_and_http() {
    assert_rejected_configuration_preserves_live_session(true);
}
fn assert_rejected_configuration_preserves_live_session(debug_php_change: bool) {
    let source = std::env::var_os("DEVFORGE_CONFIGURE_RUNTIME_STORE")
        .map(PathBuf::from)
        .expect("set disposable reviewed runtime-store cache path");
    let source = fs::canonicalize(source).unwrap();
    let temp = tempfile::tempdir().unwrap();
    let secrets = Arc::new(Secrets(Mutex::new(BTreeMap::new())));
    let mut w = Workspace::open(temp.path().join("data"), secrets.clone()).unwrap();
    let present = |package: &devforge_runtime::package::Package| {
        source
            .join("packages")
            .join(package.key().unwrap())
            .is_dir()
    };
    let select = |kind: &str| {
        w.catalog
            .packages
            .iter()
            .find(|(_, p)| {
                p.kind == kind
                    && Some(p.target) == Target::current().ok()
                    && present(p)
                    && (kind != "php"
                        || (p.executables.contains_key("php")
                            && w.catalog.php_server(p).is_ok_and(present)))
            })
            .map(|(id, _)| id.clone())
            .expect("reviewed native package missing in cache")
    };
    let php = select("php");
    let node = select("node");
    let caddy = select("caddy");
    let server = w.catalog.php_server(w.catalog.get(&php).unwrap()).unwrap();
    for package in [
        w.catalog.get(&php).unwrap(),
        server,
        w.catalog.get(&node).unwrap(),
        w.catalog.get(&caddy).unwrap(),
    ] {
        let key = package.key().unwrap();
        copy_owned_tree(
            &source.join("packages").join(&key),
            &temp.path().join("data/runtimes/packages").join(&key),
            &source,
        );
    }
    let project_path = temp.path().join("project");
    fs::create_dir_all(project_path.join("public")).unwrap();
    let token = "DEVFORGE_CONFIGURE_LIVE_CANARY";
    fs::write(
        project_path.join("public/index.php"),
        format!("<?php echo '{token}';"),
    )
    .unwrap();
    fs::write(
        project_path.join("composer.json"),
        r#"{"require":{"php":"^8.3","laravel/framework":"^13"}}"#,
    )
    .unwrap();
    let p = w
        .projects
        .import(&project_path, "Running fixture", &php, &node, &w.catalog)
        .unwrap();
    let domain = "configure-fixture.devforge.test";
    w.sites
        .configure(
            &w.projects,
            Site {
                project_id: p.id.clone(),
                domain: domain.into(),
                https: false,
                vite: false,
            },
        )
        .unwrap();
    let web = Serving::start_selected(
        &w.sites,
        &w.projects,
        &w.catalog,
        &w.packages,
        secrets.as_ref(),
        Options {
            http_port: 0,
            https_port: 0,
        },
        Some(std::slice::from_ref(&p.id)),
    )
    .unwrap();
    let status = web.status();
    assert!(status.running && status.sites[0].php_ready);
    let pids = web
        .process_snapshots()
        .into_iter()
        .map(|(name, snapshot)| (name, snapshot.pid.expect("ready process must have PID")))
        .collect::<BTreeMap<_, _>>();
    assert!(pids.contains_key("caddy"));
    let url = format!("http://127.0.0.1:{}/", status.http.selected);
    let client = reqwest::blocking::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(3))
        .build()
        .unwrap();
    let response = || {
        client
            .get(&url)
            .header("Host", domain)
            .send()
            .unwrap()
            .error_for_status()
            .unwrap()
            .text()
            .unwrap()
    };
    assert_eq!(response(), token);
    w.sessions.insert(
        p.id.clone(),
        Session {
            web,
            queue: None,
            supervisor: Supervisor::open(temp.path().join("queue-logs")).unwrap(),
        },
    );
    // Save valid debugger policy through the real store API after starting the
    // ordinary HTTP session. This tests policy refusal, not Xdebug activation.
    let expected = if debug_php_change {
        w.projects
            .save_debugger(
                &p.id,
                p.revision,
                devforge_debugger::Settings {
                    mode: devforge_debugger::Mode::Debug,
                    runtime: Some("a".repeat(32)),
                    breakpoints: Vec::new(),
                },
            )
            .unwrap();
        w.projects.get(&p.id).unwrap()
    } else {
        p.clone()
    };
    let selected_php = if debug_php_change {
        w.catalog
            .packages
            .iter()
            .find(|(id, package)| {
                *id != &php
                    && package.kind == "php"
                    && package.executables.contains_key("php")
                    && Some(package.target) == Target::current().ok()
            })
            .map(|(id, _)| id.clone())
            .expect("second compatible PHP descriptor")
    } else {
        php
    };
    let failure = w
        .execute(
            Command::Configure {
                id: p.id.clone(),
                revision: expected.revision,
                name: if debug_php_change {
                    p.name.clone()
                } else {
                    "\ninvalid".into()
                },
                php: selected_php,
                node,
                domain: domain.into(),
                https: false,
                vite: false,
                queue: false,
            },
            &Cancellation::default(),
        )
        .unwrap_err();
    assert_eq!(
        failure.code,
        if debug_php_change {
            "debug_runtime_change"
        } else {
            "invalid_project"
        }
    );
    let session = w
        .sessions
        .get(&p.id)
        .expect("invalid input must not stop the running session");
    assert!(session.web.status().running);
    let after = session
        .web
        .process_snapshots()
        .into_iter()
        .map(|(name, snapshot)| (name, snapshot.pid.unwrap()))
        .collect::<BTreeMap<_, _>>();
    assert_eq!(
        after, pids,
        "invalid configure must neither stop nor restart children"
    );
    assert_eq!(response(), token);
    let actual = w.projects.get(&p.id).unwrap();
    assert_eq!(actual.revision, expected.revision);
    assert_eq!(actual.php, expected.php);
    assert_eq!(actual.debugger, expected.debugger);
    w.shutdown().unwrap();
}
