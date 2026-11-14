#![cfg(feature = "test-support")]
//! Narrow service CLI/SDK/native approval fixtures. No native credential store,
//! downloaded runtime, UI or unrelated mutation tool is involved.
#[path = "../../manager/tests/common/mod.rs"]
mod common;
use devforge_control::{Digest, Id, Scope};
use devforge_core::{ErrorCode, SecretStore, SettingsPatch};
use devforge_manager::{
    Owner,
    auth::{DesktopAuth, Policy, TrustedDesktopAction},
    controller::{Controller, StartIntent, Stores, local_owner_user},
    external_auth::IssuedGrant,
    native_client::NativeClient,
};
use devforge_projects::project::{Catalog, ProjectStore};
use devforge_runtime::package::Target;
use devforge_workspace::testing::FixtureRoot;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    path::PathBuf,
    process::{Command, Stdio},
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
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
        panic!("no OS/workspace secret reads")
    }
    fn set(&self, _: &str, _: &str) -> Result<(), ErrorCode> {
        panic!("no secret writes")
    }
    fn delete(&self, _: &str) -> Result<(), ErrorCode> {
        panic!("no secret deletes")
    }
}
struct Fixture {
    parent: tempfile::TempDir,
    root: PathBuf,
    project: String,
    other: String,
    revision: u64,
    native: NativeClient,
    full: IssuedGrant,
    manage: IssuedGrant,
    observer: IssuedGrant,
    quit: devforge_manager::controller::OsQuitHandle,
    worker: Option<thread::JoinHandle<devforge_manager::controller::Result<()>>>,
}
impl Fixture {
    fn new() -> Self {
        let parent = common::root("df-service-");
        let root = FixtureRoot::create_new(&parent.path().join("data"))
            .unwrap()
            .root()
            .to_owned();
        let catalog = Catalog::bundled();
        let target = Target::current().unwrap();
        let scan = parent.path().join("scan");
        fs::create_dir(&scan).unwrap();
        devforge_platform::SqliteSettings::open(&root)
            .unwrap()
            .patch(
                SettingsPatch {
                    project_folder: Some(scan.to_string_lossy().into_owned()),
                    ..SettingsPatch::default()
                },
                &devforge_core::LocaleRegistry::from_json(include_str!(
                    "../../../shared/locales/registry.json"
                ))
                .unwrap(),
            )
            .unwrap();
        let php = catalog
            .packages
            .iter()
            .find(|(id, p)| {
                p.kind == "php"
                    && p.executables.contains_key("php")
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
        for name in ["Service fixture", "Other service fixture"] {
            let app = parent.path().join(name);
            fs::create_dir_all(app.join("public")).unwrap();
            fs::write(app.join("public/index.php"),"<?php header('Content-Type: application/json'); echo json_encode(['fixture'=>'m134-service']);").unwrap();
            fs::write(
                app.join("composer.json"),
                br#"{"require":{"laravel/framework":"^13.0","php":">=8.0"}}"#,
            )
            .unwrap();
            rows.push(projects.import(&app, name, php, node, &catalog).unwrap());
        }
        devforge_serving::site::SiteStore::open(root.join("serving"))
            .unwrap()
            .configure(
                &projects,
                devforge_serving::site::Site {
                    project_id: rows[0].id.clone(),
                    domain: "m134-service.devforge.test".into(),
                    https: false,
                    vite: false,
                },
            )
            .unwrap();
        drop(projects);
        let store = Arc::new(Memory::default());
        let owner = Owner::acquire(&root).unwrap();
        let auth = DesktopAuth::new(
            owner.endpoint().clone(),
            local_owner_user().unwrap(),
            Policy::default(),
        )
        .unwrap();
        let mut auth = auth;
        let key = auth
            .rotate_desktop_key(
                &TrustedDesktopAction::from_explicit_desktop_action(),
                store.as_ref(),
                Instant::now(),
            )
            .unwrap();
        drop(auth);
        drop(owner);
        let controller = Controller::start(
            &root,
            StartIntent::ExplicitUserStart,
            Stores {
                workspace: Arc::new(Deny),
                desktop: store,
            },
        )
        .unwrap();
        let quit = controller.quitter();
        let endpoint = controller.endpoint().clone();
        let worker = thread::spawn(move || controller.serve(None));
        let start = Instant::now();
        while devforge_manager::Client::new(endpoint.clone())
            .request(&json!({"method":"health"}))
            .is_err()
        {
            assert!(start.elapsed() < Duration::from_secs(5));
            thread::sleep(Duration::from_millis(10));
        }
        let mut native = NativeClient::authenticate_explicit(endpoint, &key).unwrap();
        let project = Id::new(&rows[0].id).unwrap();
        let full = native
            .issue_external_grant(
                &project,
                &[Scope::Observer, Scope::ProjectManage, Scope::ExecutionTest],
                Duration::from_secs(300),
            )
            .unwrap();
        let manage = native
            .issue_external_grant(
                &project,
                &[Scope::Observer, Scope::ProjectManage],
                Duration::from_secs(300),
            )
            .unwrap();
        let observer = native
            .issue_external_grant(&project, &[Scope::Observer], Duration::from_secs(300))
            .unwrap();
        Self {
            parent,
            root,
            project: rows[0].id.clone(),
            other: rows[1].id.clone(),
            revision: rows[0].revision,
            native,
            full,
            manage,
            observer,
            quit,
            worker: Some(worker),
        }
    }
    fn cli(&self, args: &[&str], grant: &IssuedGrant) -> (i32, Value) {
        let mut child = Command::new(env!("CARGO_BIN_EXE_devforge"))
            .env("DEVFORGE_DATA_DIR", &self.root)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(grant.credential.expose_secret().as_bytes())
            .unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(output.stderr.is_empty());
        let text = String::from_utf8(output.stdout).unwrap();
        assert!(!text.contains(grant.credential.expose_secret()));
        let value = serde_json::from_str(&text).unwrap();
        (output.status.code().unwrap(), value)
    }
    fn action(
        &self,
        plan: bool,
        action: &str,
        key: &str,
        review: Option<&str>,
        grant: &IssuedGrant,
    ) -> (i32, Value) {
        let revision = self.revision.to_string();
        let mut args = vec!["api", "service"];
        if plan {
            args.push("plan");
        }
        args.extend([
            action,
            &self.project,
            "--revision",
            &revision,
            "--idempotency-key",
            key,
            "--credential-stdin",
        ]);
        if let Some(review) = review {
            args.extend(["--review", review]);
        }
        self.cli(&args, grant)
    }
    fn approve(&mut self, plan: &Value) {
        assert_eq!(plan["data"]["blocked"], true);
        let digest = Digest::new(plan["data"]["digest"].as_str().unwrap().to_owned()).unwrap();
        assert!(!self.native.approve_service_plan(&digest).unwrap().blocked);
    }
    fn change_revision(&mut self) -> u64 {
        let snapshot = self.native.snapshot().unwrap();
        let project = &snapshot.snapshot["projects"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["project"]["id"] == self.project)
            .unwrap()["project"];
        let command = devforge_workspace::Command::Configure {
            id: self.project.clone(),
            revision: self.revision,
            name: "Changed by native owner".into(),
            php: project["php"].as_str().unwrap().into(),
            node: project["node"].as_str().unwrap().into(),
            domain: "m134-service.devforge.test".into(),
            https: false,
            vite: false,
            queue: false,
        };
        let receipt = self
            .native
            .admit(
                &Id::new(&self.project).unwrap(),
                &Id::new("native-config-change").unwrap(),
                &command,
            )
            .unwrap();
        let started = Instant::now();
        loop {
            let state = self
                .native
                .status(&receipt.project_id, &receipt.operation_id)
                .unwrap()
                .state;
            match state {
                devforge_control::OperationState::Succeeded => break,
                devforge_control::OperationState::Queued
                | devforge_control::OperationState::Running => {}
                _ => panic!("native fixture revision change failed"),
            };
            assert!(started.elapsed() < Duration::from_secs(5));
            thread::sleep(Duration::from_millis(20));
        }
        let snapshot = self.native.snapshot().unwrap();
        snapshot.snapshot["projects"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["project"]["id"] == self.project)
            .unwrap()["project"]["revision"]
            .as_u64()
            .unwrap()
    }
    fn complete(&self, key: &str, grant: &IssuedGrant) -> Value {
        let start = Instant::now();
        loop {
            let (code, value) = self.cli(
                &[
                    "api",
                    "operation",
                    "lookup",
                    key,
                    "--project",
                    &self.project,
                    "--credential-stdin",
                ],
                grant,
            );
            assert_eq!(code, 0);
            let state = value["data"]["state"].as_str().unwrap();
            match state {
                "succeeded" => return value["data"].clone(),
                "queued" | "running" => {}
                _ => panic!("service operation {state}"),
            };
            assert!(start.elapsed() < Duration::from_secs(40));
            thread::sleep(Duration::from_millis(40));
        }
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
fn cli_native_review_stop_replay_and_denials_preserve_existing_envelope() {
    let mut fixture = Fixture::new();
    let (code, none) = fixture.cli(
        &[
            "api",
            "operation",
            "lookup",
            "not-admitted",
            "--project",
            &fixture.project,
            "--credential-stdin",
        ],
        &fixture.full,
    );
    assert_eq!(code, 0);
    assert!(none.get("data").unwrap().is_null());
    assert!(none.get("error").is_none());
    let (code, _) = fixture.action(true, "stop", "denied", None, &fixture.observer);
    assert_eq!(code, 3);
    let (code, _) = fixture.action(true, "start", "no-execution", None, &fixture.manage);
    assert_eq!(code, 3);
    let (code, missing) = fixture.action(true, "start", "missing-cache", None, &fixture.full);
    assert_eq!(code, 5);
    assert_eq!(missing["error"]["code"], "precondition_failed");
    let (code, plan) = fixture.action(true, "stop", "stop-once", None, &fixture.manage);
    assert_eq!(code, 0);
    let digest = plan["data"]["digest"].as_str().unwrap();
    let (code, _) = fixture.action(false, "stop", "stop-once", Some(digest), &fixture.manage);
    assert_eq!(code, 3);
    fixture.approve(&plan);
    let (code, accepted) =
        fixture.action(false, "stop", "stop-once", Some(digest), &fixture.manage);
    assert_eq!(code, 0, "{accepted}");
    let receipt = fixture.complete("stop-once", &fixture.manage);
    assert_eq!(receipt["operation_id"], accepted["data"]["operation_id"]);
    let (code, replay) = fixture.action(false, "stop", "stop-once", Some(digest), &fixture.manage);
    assert_eq!(code, 0);
    assert_eq!(replay["data"]["operation_id"], receipt["operation_id"]);
    let (code, _) = fixture.action(false, "stop", "changed-key", Some(digest), &fixture.manage);
    assert_eq!(code, 5);
    let revision = fixture.revision.to_string();
    let (code, denied) = fixture.cli(
        &[
            "api",
            "service",
            "plan",
            "stop",
            &fixture.other,
            "--revision",
            &revision,
            "--idempotency-key",
            "foreign",
            "--credential-stdin",
        ],
        &fixture.full,
    );
    assert_eq!(code, 3);
    assert_eq!(denied["error"]["code"], "project_mismatch");
    // An existing native owner changes configuration, not an automation tool.
    // Approved old intent must fail while the original receipt stays readable.
    let (code, stale) = fixture.action(true, "stop", "stale", None, &fixture.full);
    assert_eq!(code, 0);
    fixture.approve(&stale);
    let next_revision = fixture.change_revision();
    let (code, value) = fixture.action(
        false,
        "stop",
        "stale",
        stale["data"]["digest"].as_str(),
        &fixture.full,
    );
    assert_eq!(code, 5);
    assert_eq!(value["error"]["code"], "revision_conflict");
    let (code, old) = fixture.cli(
        &[
            "api",
            "operation",
            "lookup",
            "stop-once",
            "--project",
            &fixture.project,
            "--credential-stdin",
        ],
        &fixture.manage,
    );
    assert_eq!(code, 0);
    assert_eq!(old["data"]["revision"], fixture.revision);
    fixture.revision = next_revision;
    let (code, revoke_plan) = fixture.action(true, "stop", "revoked", None, &fixture.full);
    assert_eq!(code, 0);
    fixture.approve(&revoke_plan);
    fixture
        .native
        .revoke_external_grant(&fixture.full.record.grant_id)
        .unwrap();
    let (code, value) = fixture.action(
        false,
        "stop",
        "revoked",
        revoke_plan["data"]["digest"].as_str(),
        &fixture.full,
    );
    assert_eq!(code, 3);
    assert_eq!(value["error"]["code"], "grant_revoked");
}

struct Mcp {
    child: std::process::Child,
    input: Option<std::process::ChildStdin>,
    output: std::sync::mpsc::Receiver<Value>,
    counter: u64,
}
impl Mcp {
    fn start(fixture: &Fixture, service: bool) -> Self {
        Self::with_flags(fixture, service, false)
    }
    fn with_flags(fixture: &Fixture, service: bool, runtime: bool) -> Self {
        let file = fixture
            .parent
            .path()
            .join(format!("grant-service-{service}-runtime-{runtime}.json"));
        let bytes=zeroize::Zeroizing::new(serde_json::to_vec(&json!({"schema":1,"project_id":fixture.project,"credential":fixture.full.credential.expose_secret()})).unwrap());
        common::credential_file(&file, &bytes);
        let mut command = Command::new(env!("CARGO_BIN_EXE_devforge"));
        command
            .env("DEVFORGE_DATA_DIR", &fixture.root)
            .args(["mcp", "serve", "--credential-file"])
            .arg(&file);
        if service {
            command.arg("--allow-service-actions");
        }
        if runtime {
            command.arg("--allow-runtime-actions");
        }
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let input = child.stdin.take();
        let stdout = child.stdout.take().unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        thread::spawn(move || {
            use std::io::BufRead;
            for line in std::io::BufReader::new(stdout).lines() {
                if tx
                    .send(serde_json::from_str(&line.unwrap()).unwrap())
                    .is_err()
                {
                    break;
                }
            }
        });
        Self {
            child,
            input,
            output: rx,
            counter: 0,
        }
    }
    fn call(&mut self, method: &str, mut params: Value) -> Value {
        self.counter += 1;
        params["_meta"] = json!({"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{}});
        let request = json!({"jsonrpc":"2.0","id":self.counter,"method":method,"params":params});
        writeln!(self.input.as_mut().unwrap(), "{request}").unwrap();
        self.input.as_mut().unwrap().flush().unwrap();
        let response = self.output.recv_timeout(Duration::from_secs(8)).unwrap();
        assert_eq!(response["id"], self.counter);
        response
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
    }
}
impl Drop for Mcp {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}
#[test]
fn mcp_service_surface_is_explicit_and_native_review_remains_required() {
    let mut fixture = Fixture::new();
    let mut readonly = Mcp::start(&fixture, false);
    assert_eq!(
        readonly.call("tools/list", json!({}))["result"]["tools"]
            .as_array()
            .unwrap()
            .len(),
        5
    );
    let denied=readonly.call("tools/call",json!({"name":"services.stop","arguments":{"expected_revision":fixture.revision,"idempotency_key":"no-optin","review":"a".repeat(64)}}));
    assert_eq!(denied["error"]["code"], -32602);
    readonly.close();
    let mut mcp = Mcp::start(&fixture, true);
    let tools = mcp.call("tools/list", json!({}));
    assert_eq!(tools["result"]["tools"].as_array().unwrap().len(), 9);
    assert!(
        tools["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .any(|tool| tool["name"] == "services.start"
                && tool["annotations"]["readOnlyHint"] == false)
    );
    let args =
        json!({"action":"stop","expected_revision":fixture.revision,"idempotency_key":"mcp-stop"});
    let plan = mcp.call(
        "tools/call",
        json!({"name":"services.plan","arguments":args}),
    );
    assert_eq!(plan["result"]["isError"], false);
    let envelope = plan["result"]["structuredContent"].clone();
    let digest = envelope["data"]["digest"].as_str().unwrap().to_owned();
    let apply_args =
        json!({"expected_revision":fixture.revision,"idempotency_key":"mcp-stop","review":digest});
    let denied = mcp.call(
        "tools/call",
        json!({"name":"services.stop","arguments":apply_args}),
    );
    assert_eq!(denied["result"]["isError"], true);
    fixture.approve(&envelope);
    let apply = mcp.call(
        "tools/call",
        json!({"name":"services.stop","arguments":apply_args}),
    );
    assert_eq!(apply["result"]["isError"], false);
    let receipt = fixture.complete("mcp-stop", &fixture.full);
    assert_eq!(
        receipt["operation_id"],
        apply["result"]["structuredContent"]["data"]["operation_id"]
    );
    let lookup = mcp.call(
        "tools/call",
        json!({"name":"operations.status","arguments":{"idempotency_key":"mcp-stop"}}),
    );
    assert_eq!(
        lookup["result"]["structuredContent"]["data"]["operation_id"],
        receipt["operation_id"]
    );
    let invalid=mcp.call("tools/call",json!({"name":"services.plan","arguments":{"action":"stop","expected_revision":fixture.revision,"idempotency_key":"bypass","trusted":true}}));
    assert_eq!(invalid["error"]["code"], -32602);
    mcp.close();
    assert!(fixture.native.snapshot().is_ok());
}

#[test]
fn service_sdk_lost_admission_reply_reconciles_original_key_without_hidden_retry() {
    use devforge_control::{
        Request,
        mutations::{MutationInput, MutationLimits, ServiceActionInput},
    };
    use devforge_manager::{
        Client, Endpoint, Server, StopSignal, external_auth::ExternalCredential,
        external_client::ServiceClient,
    };
    use std::sync::atomic::{AtomicUsize, Ordering};
    use zeroize::Zeroize;
    let mut fixture = Fixture::new();
    let proxy = common::root("df-service-relay-");
    let owner = Owner::acquire(proxy.path()).unwrap();
    let endpoint = owner.endpoint().clone();
    let actual = Endpoint::existing(&fixture.root).unwrap();
    let forward = Client::new(actual.clone());
    let (real_peer, _) = forward
        .request_with_peer(&json!({"method":"health"}))
        .unwrap();
    let stop = StopSignal::default();
    let server_stop = stop.clone();
    let calls = Arc::new(AtomicUsize::new(0));
    let server_calls = calls.clone();
    let relay = thread::spawn(move || {
        Server::new(owner).run(server_stop, move |_, mut wire| {
            let apply = wire["method"] == "service_apply";
            let response = forward.request_with_expected_peer(&real_peer, &wire);
            if let Some(Value::String(token)) = wire.get_mut("credential") {
                token.zeroize();
            }
            let response = response?;
            if apply && response["ok"] == true {
                server_calls.fetch_add(1, Ordering::SeqCst);
                return Err(devforge_manager::Error::HandlerFailed);
            }
            Ok(response)
        })
    });
    let started = Instant::now();
    while Client::new(endpoint.clone())
        .request(&json!({"method":"health"}))
        .is_err()
    {
        assert!(started.elapsed() < Duration::from_secs(5));
        thread::sleep(Duration::from_millis(10));
    }
    let token = || ExternalCredential::from_wire(fixture.full.credential.expose_secret()).unwrap();
    let mut client = ServiceClient::connect(endpoint, token()).unwrap();
    let mut request = Request {
        api_version: Default::default(),
        request_id: Id::new("lost-admission").unwrap(),
        project_id: Id::new(&fixture.project).unwrap(),
        expected_revision: Some(fixture.revision),
        idempotency_key: Some(Id::new("lost-once").unwrap()),
        input: MutationInput::Stop(ServiceActionInput { limits: None }),
    };
    let preview = client.plan(&request).unwrap().data().unwrap().clone();
    fixture
        .native
        .approve_service_plan(&preview.digest)
        .unwrap();
    assert_eq!(
        client.apply(&request, &preview.digest).unwrap_err(),
        devforge_control::ErrorCode::AdmissionUnconfirmed
    );
    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "SDK never automatically retries admission"
    );
    let lookup = client
        .lookup(
            Id::new("reconcile").unwrap(),
            &request.project_id,
            request.idempotency_key.as_ref().unwrap(),
        )
        .unwrap();
    let admitted = lookup
        .data()
        .unwrap()
        .as_ref()
        .unwrap()
        .operation_id
        .clone();
    assert_eq!(
        fixture.complete("lost-once", &fixture.full)["operation_id"],
        admitted.as_str()
    );
    let mut direct = ServiceClient::connect(actual, token()).unwrap();
    let replay = direct.apply(&request, &preview.digest).unwrap();
    assert_eq!(replay.data().unwrap().operation_id, admitted);
    request.input = MutationInput::Stop(ServiceActionInput {
        limits: Some(MutationLimits::default()),
    });
    let changed = direct.apply(&request, &preview.digest).unwrap();
    assert_eq!(
        changed.error().unwrap().code,
        devforge_control::ErrorCode::IdempotencyConflict
    );
    stop.request();
    relay.join().unwrap().unwrap();
}

#[test]
fn runtime_projection_scope_review_and_preserved_field_guards_have_no_install_effect() {
    use devforge_control::{
        Request,
        mutations::{MutationInput, PackageInput, ProjectName},
    };
    use devforge_manager::{
        Endpoint, external_auth::ExternalCredential, external_client::RuntimeClient,
    };
    let mut fixture = Fixture::new();
    let endpoint = Endpoint::existing(&fixture.root).unwrap();
    let token = |grant: &IssuedGrant| {
        ExternalCredential::from_wire(grant.credential.expose_secret()).unwrap()
    };
    let mut full = RuntimeClient::connect(endpoint.clone(), token(&fixture.full)).unwrap();
    let project = Id::new(&fixture.project).unwrap();
    let before = fixture.native.snapshot().unwrap().snapshot["projects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["project"]["id"] == fixture.project)
        .unwrap()["project"]
        .clone();
    let selection = full
        .selection(Id::new("runtime-current").unwrap(), &project)
        .unwrap();
    let input = selection.data().unwrap().input.clone();
    assert_eq!(input.name.as_str(), "Service fixture");
    assert_eq!(input.domain.as_str(), "m134-service.devforge.test");
    assert!(!input.https && !input.vite && !input.queue);
    let revision = fixture.revision.to_string();
    let (code, cli_plan) = fixture.cli(
        &[
            "api",
            "runtime",
            "plan",
            "install",
            &fixture.project,
            "--package",
            input.php.as_str(),
            "--revision",
            &revision,
            "--idempotency-key",
            "cli-runtime-install",
            "--credential-stdin",
        ],
        &fixture.manage,
    );
    assert_eq!(code, 0);
    assert_eq!(cli_plan["data"]["blocked"], true);
    let (code, cli_select) = fixture.cli(
        &[
            "api",
            "runtime",
            "plan",
            "select",
            &fixture.project,
            "--php",
            input.php.as_str(),
            "--node",
            input.node.as_str(),
            "--revision",
            &revision,
            "--idempotency-key",
            "cli-runtime-select",
            "--credential-stdin",
        ],
        &fixture.full,
    );
    assert_eq!(code, 5);
    assert_eq!(cli_select["error"]["code"], "precondition_failed");
    let mut request = full
        .select_request(
            Id::new("select-plan").unwrap(),
            project.clone(),
            fixture.revision,
            Id::new("select-intent").unwrap(),
            (input.php.clone(), input.node.clone()),
            None,
        )
        .unwrap();
    assert!(
        matches!(&request.input,MutationInput::Configure(selected) if selected.name.as_str()==input.name.as_str()&&selected.domain.as_str()==input.domain.as_str()&&selected.php==input.php&&selected.node==input.node)
    );
    let response = full.plan(&request).unwrap();
    assert_eq!(
        response.error().unwrap().code,
        devforge_control::ErrorCode::PreconditionFailed
    );
    if let MutationInput::Configure(selected) = &mut request.input {
        selected.name = ProjectName::new("Not runtime selection".into()).unwrap();
    }
    let altered = full.plan(&request).unwrap();
    assert_eq!(
        altered.error().unwrap().code,
        devforge_control::ErrorCode::InvalidInput
    );
    let other = full
        .selection(
            Id::new("other-current").unwrap(),
            &Id::new(&fixture.other).unwrap(),
        )
        .unwrap();
    assert_eq!(
        other.error().unwrap().code,
        devforge_control::ErrorCode::ProjectMismatch
    );
    let mut manage = RuntimeClient::connect(endpoint.clone(), token(&fixture.manage)).unwrap();
    let mut observer = RuntimeClient::connect(endpoint, token(&fixture.observer)).unwrap();
    let request = Request {
        api_version: Default::default(),
        request_id: Id::new("install-plan").unwrap(),
        project_id: project,
        expected_revision: Some(fixture.revision),
        idempotency_key: Some(Id::new("install-intent").unwrap()),
        input: MutationInput::RuntimeInstall(PackageInput {
            package_id: input.php.clone(),
            limits: None,
        }),
    };
    let denied = observer.plan(&request).unwrap();
    assert_eq!(
        denied.error().unwrap().code,
        devforge_control::ErrorCode::Unauthorized
    );
    let preview = manage.plan(&request).unwrap();
    assert!(preview.data().unwrap().blocked);
    let digest = preview.data().unwrap().digest.clone();
    assert_eq!(
        manage
            .apply(&request, &digest)
            .unwrap()
            .error()
            .unwrap()
            .code,
        devforge_control::ErrorCode::Unauthorized
    );
    fixture.native.approve_service_plan(&digest).unwrap();
    fixture
        .native
        .revoke_external_grant(&fixture.manage.record.grant_id)
        .unwrap();
    assert_eq!(
        manage
            .apply(&request, &digest)
            .unwrap()
            .error()
            .unwrap()
            .code,
        devforge_control::ErrorCode::GrantRevoked
    );
    let lookup = full
        .lookup(
            Id::new("runtime-lookup").unwrap(),
            &request.project_id,
            request.idempotency_key.as_ref().unwrap(),
        )
        .unwrap();
    assert!(lookup.data().unwrap().is_none());
    assert!(
        fs::read_dir(fixture.root.join("runtimes/packages"))
            .unwrap()
            .next()
            .is_none()
    );
    let after = fixture.native.snapshot().unwrap().snapshot["projects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["project"]["id"] == fixture.project)
        .unwrap()["project"]
        .clone();
    assert_eq!(before, after);
}

#[test]
fn runtime_mcp_opt_in_is_independent_and_only_exposes_install_select() {
    let mut fixture = Fixture::new();
    let row = fixture.native.snapshot().unwrap().snapshot["projects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["project"]["id"] == fixture.project)
        .unwrap()["project"]
        .clone();
    for (service, runtime, count) in [
        (false, false, 5),
        (true, false, 9),
        (false, true, 8),
        (true, true, 12),
    ] {
        let mut mcp = Mcp::with_flags(&fixture, service, runtime);
        let tools = mcp.call("tools/list", json!({}));
        let tools = tools["result"]["tools"].as_array().unwrap();
        assert_eq!(tools.len(), count);
        assert!(
            !tools
                .iter()
                .any(|tool| tool["name"] == "runtimes.remove" || tool["name"] == "runtimes.probe")
        );
        let plan=mcp.call("tools/call",json!({"name":"runtimes.plan","arguments":{"action":"install","package_id":row["php"],"expected_revision":fixture.revision,"idempotency_key":format!("mcp-install-{service}-{runtime}")}}));
        if runtime {
            assert_eq!(plan["result"]["isError"], false);
            assert_eq!(plan["result"]["structuredContent"]["data"]["blocked"], true);
            let invalid=mcp.call("tools/call",json!({"name":"runtimes.select","arguments":{"expected_revision":fixture.revision,"idempotency_key":"illegal","review":"a".repeat(64),"php":row["php"],"node":row["node"],"name":"not-runtime"}}));
            assert_eq!(invalid["error"]["code"], -32602);
            let lookup=mcp.call("tools/call",json!({"name":"operations.status","arguments":{"idempotency_key":"never-admitted"}}));
            assert!(lookup["result"]["structuredContent"]["data"].is_null());
        } else {
            assert_eq!(plan["error"]["code"], -32602);
        }
        mcp.close();
    }
    assert!(fixture.native.snapshot().is_ok());
    assert!(
        fs::read_dir(fixture.root.join("runtimes/packages"))
            .unwrap()
            .next()
            .is_none()
    );
}
