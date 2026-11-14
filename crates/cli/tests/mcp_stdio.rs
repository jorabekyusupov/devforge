//! Actual stdio child/manager fixtures. Tokens stay in fresh protected files;
//! stdout assertions read JSON-RPC only. No native credential store or window.
#[path = "../../manager/tests/common/mod.rs"]
mod common;
use serde_json::{Value, json};
use std::{
    fs,
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    sync::mpsc::{self, Receiver},
    thread,
    time::{Duration, Instant},
};

struct StdioClient {
    child: Child,
    input: Option<ChildStdin>,
    output: Receiver<Value>,
    stderr: PathBuf,
}
impl StdioClient {
    fn start(root: &Path, file: &Path, stderr: PathBuf) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_devforge"))
            .env("DEVFORGE_DATA_DIR", root)
            .args(["mcp", "serve", "--credential-file"])
            .arg(file)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(fs::File::create(&stderr).unwrap())
            .spawn()
            .unwrap();
        let input = child.stdin.take();
        let output = child.stdout.take().unwrap();
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            for line in BufReader::new(output).lines() {
                let value: Value = serde_json::from_str(&line.unwrap())
                    .expect("stdout must contain JSON-RPC only");
                if tx.send(value).is_err() {
                    break;
                }
            }
        });
        Self {
            child,
            input,
            output: rx,
            stderr,
        }
    }
    fn send(&mut self, value: Value) {
        self.raw(format!("{value}\n").as_bytes());
    }
    fn raw(&mut self, bytes: &[u8]) {
        self.input.as_mut().unwrap().write_all(bytes).unwrap();
        self.input.as_mut().unwrap().flush().unwrap();
    }
    fn receive(&self) -> Value {
        self.output
            .recv_timeout(Duration::from_secs(8))
            .expect("bounded MCP response")
    }
    fn call(&mut self, id: i64, method: &str, params: Value, modern: bool) -> Value {
        let mut params = params;
        if modern {
            params["_meta"] = meta();
        }
        self.send(json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}));
        let reply = self.receive();
        assert_eq!(reply["id"], id);
        reply
    }
    fn close(mut self) {
        drop(self.input.take());
        let started = Instant::now();
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                assert!(status.success());
                break;
            }
            assert!(started.elapsed() < Duration::from_secs(8));
            thread::sleep(Duration::from_millis(20));
        }
        assert!(fs::read(&self.stderr).unwrap().is_empty());
    }
}
impl Drop for StdioClient {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}
fn meta() -> Value {
    json!({"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{}})
}
#[test]
fn help_and_invalid_startup_never_create_state_or_mix_stdout() {
    let parent = common::root("df-mcp-help-");
    let root = parent.path().join("absent");
    for args in [vec!["mcp", "--help"], vec!["mcp", "serve", "--help"]] {
        let output = Command::new(env!("CARGO_BIN_EXE_devforge"))
            .env("DEVFORGE_DATA_DIR", &root)
            .args(args)
            .output()
            .unwrap();
        assert!(output.status.success());
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(value["default_read_only"], true);
        assert!(!root.exists());
    }
    for args in [
        vec!["mcp", "serve"],
        vec!["mcp", "serve", "--credential", "token-canary"],
        vec!["mcp", "serve", "--credential-file", "relative"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_devforge"))
            .env("DEVFORGE_DATA_DIR", &root)
            .args(args)
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert!(
            !String::from_utf8(output.stderr)
                .unwrap()
                .contains("token-canary")
        );
        assert!(!root.exists());
    }
}
#[test]
fn dual_era_metadata_framing_and_eof_work_without_a_manager() {
    let parent = common::root("df-mcp-meta-");
    let root = parent.path().join("absent");
    let file = parent.path().join("credential.json");
    common::credential_file(
        &file,
        serde_json::to_string(
            &json!({"schema":1,"project_id":"requested-project","credential":"a".repeat(64)}),
        )
        .unwrap()
        .as_bytes(),
    );
    let mut client = StdioClient::start(&root, &file, parent.path().join("stderr"));
    let discovery = client.call(1, "server/discover", json!({}), true);
    assert_eq!(discovery["result"]["resultType"], "complete");
    assert_eq!(discovery["result"]["ttlMs"], 0);
    let tools = client.call(2, "tools/list", json!({}), true);
    assert_eq!(tools["result"]["tools"].as_array().unwrap().len(), 5);
    assert!(
        tools["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .all(|tool| tool["annotations"]["readOnlyHint"] == true)
    );
    let missing = client.call(3, "ping", json!({}), false);
    assert_eq!(missing["error"]["code"], -32602);
    let unsupported=client.call(4,"ping",json!({"_meta":{"io.modelcontextprotocol/protocolVersion":"1900-01-01","io.modelcontextprotocol/clientCapabilities":{}}}),false);
    assert_eq!(unsupported["error"]["code"], -32022);
    client.raw(b"not json\n");
    assert_eq!(client.receive()["error"]["code"], -32700);
    client.raw(&vec![b'x'; 65_537]);
    client.raw(b"\n");
    assert_eq!(client.receive()["error"]["code"], -32600);
    assert_eq!(
        client.call(5, "ping", json!({}), true)["result"]["resultType"],
        "complete"
    );
    let cursor = client.call(6, "tools/list", json!({"cursor":"stale"}), true);
    assert_eq!(cursor["error"]["code"], -32602);
    let denial = client.call(
        7,
        "tools/call",
        json!({"name":"projects.get","arguments":{}}),
        true,
    );
    assert_eq!(denial["result"]["isError"], true);
    assert_eq!(
        denial["result"]["structuredContent"]["error"]["code"],
        "storage_unavailable"
    );
    assert!(!root.exists());
    client.close();
    assert!(!root.exists());
    let mut legacy = StdioClient::start(&root, &file, parent.path().join("legacy-stderr"));
    let initialized=legacy.call(8,"initialize",json!({"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"legacy-fixture","version":"1"}}),false);
    assert_eq!(initialized["result"]["protocolVersion"], "2025-11-25");
    legacy.send(json!({"jsonrpc":"2.0","method":"notifications/initialized"}));
    assert_eq!(
        legacy.call(9, "tools/list", json!({}), false)["result"]["tools"]
            .as_array()
            .unwrap()
            .len(),
        5
    );
    legacy.raw(b"{");
    drop(legacy.input.take());
    assert_eq!(legacy.receive()["error"]["code"], -32700);
    legacy.close();
    assert!(!root.exists());
}

#[cfg(feature = "test-support")]
mod live {
    use super::*;
    use devforge_control::{Id, ProjectBinding, Scope};
    use devforge_core::{ErrorCode, LocaleRegistry, SecretStore, SettingsPatch};
    use devforge_manager::{
        Owner, PeerIdentity,
        auth::{DesktopAuth, Policy, TrustedDesktopAction},
        controller::{Controller, StartIntent, Stores, local_owner_user},
        external_auth::ExternalRegistry,
    };
    use devforge_projects::project::{Catalog, ProjectStore};
    use devforge_runtime::package::Target;
    use devforge_workspace::testing::FixtureRoot;
    use std::{
        collections::BTreeMap,
        sync::{Arc, Mutex},
    };
    #[derive(Default)]
    struct Memory(Mutex<BTreeMap<String, String>>);
    impl SecretStore for Memory {
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
    struct Deny;
    impl SecretStore for Deny {
        fn get(&self, _: &str) -> Result<Option<String>, ErrorCode> {
            panic!("no native/workspace credential reads")
        }
        fn set(&self, _: &str, _: &str) -> Result<(), ErrorCode> {
            panic!("no credential writes")
        }
        fn delete(&self, _: &str) -> Result<(), ErrorCode> {
            panic!("no credential deletes")
        }
    }
    struct Fixture {
        parent: tempfile::TempDir,
        root: PathBuf,
        project: String,
        file: PathBuf,
        wrong_file: PathBuf,
        revoked_file: PathBuf,
        quit: devforge_manager::controller::OsQuitHandle,
        worker: Option<thread::JoinHandle<devforge_manager::controller::Result<()>>>,
    }
    impl Fixture {
        fn new() -> Self {
            let parent = common::root("df-mcp-live-");
            let root = FixtureRoot::create_new(&parent.path().join("data"))
                .unwrap()
                .root()
                .to_owned();
            let discovery = parent.path().join("scan");
            fs::create_dir(&discovery).unwrap();
            devforge_platform::SqliteSettings::open(&root)
                .unwrap()
                .patch(
                    SettingsPatch {
                        project_folder: Some(discovery.to_string_lossy().into_owned()),
                        ..SettingsPatch::default()
                    },
                    &LocaleRegistry::from_json(include_str!(
                        "../../../shared/locales/registry.json"
                    ))
                    .unwrap(),
                )
                .unwrap();
            let catalog = Catalog::bundled();
            let target = Target::current().unwrap();
            let php = catalog
                .packages
                .iter()
                .find(|(id, p)| {
                    p.kind == "php"
                        && p.variant == "cli"
                        && p.target == target
                        && !catalog.superseded.contains_key(*id)
                })
                .unwrap()
                .0;
            let node = catalog
                .packages
                .iter()
                .find(|(id, p)| {
                    p.kind == "node" && p.target == target && !catalog.superseded.contains_key(*id)
                })
                .unwrap()
                .0;
            let projects = ProjectStore::open(root.join("projects")).unwrap();
            let mut rows = vec![];
            for name in ["MCP Granted", "MCP Other"] {
                let path = parent.path().join(name);
                fs::create_dir_all(path.join("public")).unwrap();
                fs::write(path.join("public/index.php"), "<?php").unwrap();
                fs::write(
                    path.join("composer.json"),
                    br#"{"require":{"laravel/framework":"^13.0","php":">=8.0"}}"#,
                )
                .unwrap();
                rows.push(projects.import(&path, name, php, node, &catalog).unwrap());
            }
            drop(projects);
            let owner = Owner::acquire(&root).unwrap();
            let peer = PeerIdentity {
                user: local_owner_user().unwrap(),
                process_id: Some(std::process::id()),
            };
            let mut auth = DesktopAuth::new(
                owner.endpoint().clone(),
                peer.user.clone(),
                Policy::default(),
            )
            .unwrap();
            let memory = Memory::default();
            let key = auth
                .rotate_desktop_key(
                    &TrustedDesktopAction::from_explicit_desktop_action(),
                    &memory,
                    Instant::now(),
                )
                .unwrap();
            let session = auth
                .authenticate(&peer, &key, &memory, Instant::now())
                .unwrap();
            let proof = auth.authorize(&peer, &session, Instant::now()).unwrap();
            let binding = ProjectBinding::from_registered(
                Id::new(&rows[0].id).unwrap(),
                &rows[0].path,
                rows[0].revision,
            )
            .unwrap();
            let mut grants = ExternalRegistry::open(&owner).unwrap();
            let allowed = grants
                .issue(
                    &proof,
                    &binding,
                    &[Scope::Observer],
                    &[],
                    Duration::from_secs(3600),
                )
                .unwrap();
            let revoked = grants
                .issue(
                    &proof,
                    &binding,
                    &[Scope::Observer],
                    &[],
                    Duration::from_secs(3600),
                )
                .unwrap();
            grants.revoke(&proof, &revoked.record.grant_id).unwrap();
            let file = parent.path().join("grant.json");
            let wrong_file = parent.path().join("wrong.json");
            let revoked_file = parent.path().join("revoked.json");
            for (path, project, credential) in [
                (&file, &rows[0].id, &allowed.credential),
                (&wrong_file, &rows[1].id, &allowed.credential),
                (&revoked_file, &rows[0].id, &revoked.credential),
            ] {
                let bytes=zeroize::Zeroizing::new(serde_json::to_vec(&json!({"schema":1,"project_id":project,"credential":credential.expose_secret()})).unwrap());
                common::credential_file(path, &bytes);
            }
            drop(grants);
            drop(auth);
            drop(owner);
            let controller = Controller::start(
                &root,
                StartIntent::ExplicitUserStart,
                Stores {
                    workspace: Arc::new(Deny),
                    desktop: Arc::new(Deny),
                },
            )
            .unwrap();
            let quit = controller.quitter();
            let endpoint = controller.endpoint().clone();
            let worker = thread::spawn(move || controller.serve(None));
            let started = Instant::now();
            while devforge_manager::Client::new(endpoint.clone())
                .request(&json!({"method":"health"}))
                .is_err()
            {
                assert!(started.elapsed() < Duration::from_secs(5));
                thread::sleep(Duration::from_millis(10));
            }
            Self {
                parent,
                root,
                project: rows.remove(0).id,
                file,
                wrong_file,
                revoked_file,
                quit,
                worker: Some(worker),
            }
        }
        fn healthy(&self) {
            let endpoint = devforge_manager::Endpoint::existing(&self.root).unwrap();
            assert_eq!(
                devforge_manager::Client::new(endpoint)
                    .request(&json!({"method":"health"}))
                    .unwrap()["ok"],
                true
            );
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = self.quit.request_explicit();
            if let Some(worker) = self.worker.take() {
                assert!(worker.join().unwrap().is_ok());
            }
        }
    }
    #[test]
    fn tools_and_bound_summary_are_live_scoped_and_eof_preserves_owner() {
        let fixture = Fixture::new();
        let mut client = StdioClient::start(
            &fixture.root,
            &fixture.file,
            fixture.parent.path().join("stdio-errors"),
        );
        for (id, name) in [
            (1, "health.health"),
            (2, "services.status"),
            (3, "projects.list"),
            (4, "projects.get"),
        ] {
            let value = client.call(id, "tools/call", json!({"name":name,"arguments":{}}), true);
            assert_eq!(value["result"]["isError"], false);
            let text = value.to_string();
            assert!(!text.contains("MCP Other"));
            assert!(!text.contains(fixture.root.to_str().unwrap()));
        }
        let resources = client.call(5, "resources/list", json!({}), true);
        let uri = format!("devforge://project/{}/summary", fixture.project);
        assert_eq!(
            resources["result"]["resources"].as_array().unwrap().len(),
            1
        );
        assert_eq!(resources["result"]["resources"][0]["uri"], uri);
        assert_eq!(resources["result"]["cacheScope"], "private");
        let read = client.call(6, "resources/read", json!({"uri":uri}), true);
        let summary: Value =
            serde_json::from_str(read["result"]["contents"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(summary["items"][0]["project_id"], fixture.project);
        let denied = client.call(
            7,
            "resources/read",
            json!({"uri":"file:///etc/passwd"}),
            true,
        );
        assert_eq!(denied["error"]["code"], -32602);
        let foreign = client.call(
            8,
            "tools/call",
            json!({"name":"projects.get","arguments":{"project_id":"foreign"}}),
            true,
        );
        assert_eq!(foreign["error"]["code"], -32602);
        let cancel = client.call(
            9,
            "tools/call",
            json!({"name":"operations.cancel","arguments":{"operation_id":"op"}}),
            true,
        );
        assert_eq!(cancel["error"]["code"], -32602);
        let operation = client.call(
            10,
            "tools/call",
            json!({"name":"operations.status","arguments":{"operation_id":"not-owned"}}),
            true,
        );
        assert_eq!(operation["result"]["isError"], true);
        client.close();
        fixture.healthy();
        for (file, code) in [
            (&fixture.wrong_file, "project_mismatch"),
            (&fixture.revoked_file, "grant_revoked"),
        ] {
            let mut denied = StdioClient::start(
                &fixture.root,
                file,
                fixture.parent.path().join(format!("{code}-errors")),
            );
            let result = denied.call(11, "resources/list", json!({}), true);
            assert!(result["error"]["data"]["code"] == code);
            assert!(result.get("result").is_none());
            denied.close();
            fixture.healthy();
        }
    }
    #[test]
    #[ignore = "manual actual-client acceptance fixture; no native store or GUI"]
    fn actual_client_fixture() {
        let report = std::env::var_os("DEVFORGE_MCP_FIXTURE_REPORT")
            .expect("explicit isolated report path required");
        let fixture = Fixture::new();
        let release = fixture.parent.path().join("release");
        let value = json!({"root":fixture.root,"credential_file":fixture.file,"project_id":fixture.project,"cli":env!("CARGO_BIN_EXE_devforge"),"release":release});
        let mut output = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&report)
            .unwrap();
        output.write_all(value.to_string().as_bytes()).unwrap();
        output.sync_all().unwrap();
        println!("MCP_FIXTURE_READY");
        let started = Instant::now();
        while !release.exists() {
            assert!(started.elapsed() < Duration::from_secs(900));
            thread::sleep(Duration::from_millis(100));
        }
        fixture.healthy();
    }
}
