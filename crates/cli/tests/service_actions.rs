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
        Self::with_environment(fixture, service, runtime, false)
    }
    fn with_environment(
        fixture: &Fixture,
        service: bool,
        runtime: bool,
        environment: bool,
    ) -> Self {
        Self::with_database(fixture, service, runtime, environment, false)
    }
    fn with_database(
        fixture: &Fixture,
        service: bool,
        runtime: bool,
        environment: bool,
        database: bool,
    ) -> Self {
        Self::with_domain(fixture, service, runtime, environment, database, false)
    }
    fn with_domain(
        fixture: &Fixture,
        service: bool,
        runtime: bool,
        environment: bool,
        database: bool,
        domain: bool,
    ) -> Self {
        let file = fixture.parent.path().join(format!(
            "grant-service-{service}-runtime-{runtime}-environment-{environment}-database-{database}-domain-{domain}.json"
        ));
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
        if environment {
            command.arg("--allow-environment-actions");
        }
        if database {
            command.arg("--allow-database-actions");
        }
        if domain {
            command.arg("--allow-domain-actions");
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
        9
    );
    let denied=readonly.call("tools/call",json!({"name":"services.stop","arguments":{"expected_revision":fixture.revision,"idempotency_key":"no-optin","review":"a".repeat(64)}}));
    assert_eq!(denied["error"]["code"], -32602);
    readonly.close();
    let mut mcp = Mcp::start(&fixture, true);
    let tools = mcp.call("tools/list", json!({}));
    assert_eq!(tools["result"]["tools"].as_array().unwrap().len(), 13);
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
        (false, false, 9),
        (true, false, 13),
        (false, true, 12),
        (true, true, 16),
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

fn env_cli(
    fixture: &Fixture,
    file: &std::path::Path,
    args: &[&str],
    input: Option<&[u8]>,
) -> (i32, Value) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_devforge"))
        .env("DEVFORGE_DATA_DIR", &fixture.root)
        .args(args)
        .arg("--credential-file")
        .arg(file)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    if let Some(input) = input {
        child.stdin.take().unwrap().write_all(input).unwrap();
    } else {
        drop(child.stdin.take());
    }
    let output = child.wait_with_output().unwrap();
    assert!(output.stderr.is_empty());
    (
        output.status.code().unwrap(),
        serde_json::from_slice(&output.stdout).unwrap(),
    )
}
#[test]
fn environment_masked_preview_and_yaml_stdin_do_not_export_invalid_document_content() {
    use devforge_control::{
        Request,
        mutations::{EnvironmentPlanInput, MutationInput},
    };
    use devforge_manager::{
        Endpoint, external_auth::ExternalCredential, external_client::EnvClient,
    };
    use sha2::{Digest as _, Sha256};
    let mut fixture = Fixture::new();
    let project = Id::new(&fixture.project).unwrap();
    let snapshot = fixture.native.snapshot().unwrap();
    let row = snapshot.snapshot["projects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["project"]["id"] == fixture.project)
        .unwrap();
    let path = PathBuf::from(row["project"]["path"].as_str().unwrap()).join("devforge.yaml");
    let canary = "yaml-private-comment-canary";
    let invalid = format!("schema: [\n# {canary}\n");
    fs::write(&path, &invalid).unwrap();
    let raw_hash = Sha256::digest(invalid.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let mut observer = EnvClient::connect(
        Endpoint::existing(&fixture.root).unwrap(),
        ExternalCredential::from_wire(fixture.observer.credential.expose_secret()).unwrap(),
    )
    .unwrap();
    let request = Request {
        api_version: Default::default(),
        request_id: Id::new("masked-preview").unwrap(),
        project_id: project,
        expected_revision: None,
        idempotency_key: None,
        input: MutationInput::EnvironmentPlan(EnvironmentPlanInput {
            write_env: false,
            limits: None,
        }),
    };
    let preview = observer.plan(&request).unwrap();
    let data = preview.data().unwrap();
    assert!(data.document.yaml_exists && data.document.version.is_some());
    assert!(data.receipt.is_none());
    assert!(!data.blockers.is_empty());
    let wire = serde_json::to_string(&preview).unwrap();
    assert!(
        !wire.contains(canary)
            && !wire.contains(&raw_hash)
            && !wire.contains(fixture.root.to_str().unwrap())
    );
    let credential = fixture.parent.path().join("env-observer.json");
    let bytes=zeroize::Zeroizing::new(serde_json::to_vec(&json!({"schema":1,"project_id":fixture.project,"credential":fixture.observer.credential.expose_secret()})).unwrap());
    common::credential_file(&credential, &bytes);
    let (code, cli_preview) = env_cli(
        &fixture,
        &credential,
        &["api", "env", "plan", &fixture.project],
        None,
    );
    assert_eq!(code, 0);
    assert_eq!(cli_preview["data"]["document"]["yaml_exists"], true);
    assert!(
        !cli_preview.to_string().contains(canary) && !cli_preview.to_string().contains(&raw_hash)
    );
    let revision = fixture.revision.to_string();
    let candidate=zeroize::Zeroizing::new(serde_json::to_vec(&json!({"expected":data.document.version,"text":"# supplied-candidate-canary\nschema: 1\n"})).unwrap());
    let (code, denied) = env_cli(
        &fixture,
        &credential,
        &[
            "api",
            "env",
            "plan",
            "yaml-save",
            &fixture.project,
            "--revision",
            &revision,
            "--idempotency-key",
            "denied-yaml",
            "--input-stdin",
        ],
        Some(&candidate),
    );
    assert_eq!(code, 3);
    assert_eq!(denied["error"]["code"], "unauthorized");
    assert!(!denied.to_string().contains("supplied-candidate-canary"));
    let large = zeroize::Zeroizing::new(
        serde_json::to_vec(&json!({"expected":null,"text":"x".repeat(16385)})).unwrap(),
    );
    let (code, limited) = env_cli(
        &fixture,
        &credential,
        &[
            "api",
            "env",
            "plan",
            "yaml-save",
            &fixture.project,
            "--revision",
            &revision,
            "--idempotency-key",
            "oversized-yaml",
            "--input-stdin",
        ],
        Some(&large),
    );
    assert_eq!(code, 6);
    assert_eq!(limited["error"]["code"], "output_limit_exceeded");
    assert_eq!(fs::read_to_string(path).unwrap(), invalid);
}
#[test]
fn environment_mcp_surface_is_masked_and_flag_never_grants_sensitive_authority() {
    let mut fixture = Fixture::new();
    let snapshot = fixture.native.snapshot().unwrap();
    let row = snapshot.snapshot["projects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["project"]["id"] == fixture.project)
        .unwrap();
    let path = PathBuf::from(row["project"]["path"].as_str().unwrap()).join("devforge.yaml");
    fs::write(&path, "schema: [\n# mcp-yaml-private-canary\n").unwrap();
    for (service, runtime, environment, count) in [
        (false, false, false, 9),
        (false, false, true, 12),
        (true, true, true, 19),
    ] {
        let mut mcp = Mcp::with_environment(&fixture, service, runtime, environment);
        let tools = mcp.call("tools/list", json!({}));
        assert_eq!(tools["result"]["tools"].as_array().unwrap().len(), count);
        assert!(
            !tools["result"]["tools"]
                .as_array()
                .unwrap()
                .iter()
                .any(|tool| tool["name"] == "env_document_state"
                    || tool["name"] == "environment.raw_get")
        );
        let preview = mcp.call(
            "tools/call",
            json!({"name":"environment.plan","arguments":{}}),
        );
        if environment {
            assert_eq!(preview["result"]["isError"], false);
            let document = &preview["result"]["structuredContent"]["data"]["document"];
            assert_eq!(document["yaml_exists"], true);
            assert!(document["version"].is_string());
            assert!(!preview.to_string().contains("mcp-yaml-private-canary"));
            let denied=mcp.call("tools/call",json!({"name":"environment.plan","arguments":{"action":"yaml_save","expected_revision":fixture.revision,"idempotency_key":"no-sensitive","expected":document["version"],"text":"schema: 1\n"}}));
            assert_eq!(denied["result"]["isError"], true);
            assert_eq!(
                denied["result"]["structuredContent"]["error"]["code"],
                "unauthorized"
            );
            let oversized=mcp.call("tools/call",json!({"name":"environment.plan","arguments":{"action":"yaml_save","expected_revision":fixture.revision,"idempotency_key":"oversized","text":"x".repeat(16385)}}));
            assert_eq!(oversized["error"]["code"], -32602);
            let bypass=mcp.call("tools/call",json!({"name":"environment.plan","arguments":{"action":"yaml_save","expected_revision":fixture.revision,"idempotency_key":"bypass","text":"schema: 1\n","trusted":true}}));
            assert_eq!(bypass["error"]["code"], -32602);
        } else {
            assert_eq!(preview["error"]["code"], -32602);
        }
        mcp.close();
    }
    assert!(fixture.native.snapshot().is_ok());
    assert!(
        fs::read_to_string(path)
            .unwrap()
            .contains("mcp-yaml-private-canary")
    );
}

#[test]
fn database_choices_and_mutation_denials_do_not_start_a_database_or_expose_private_parameters() {
    use devforge_control::{
        Request,
        mutations::{DatabaseBindInput, DatabaseRestoreInput, MutationInput},
    };
    use devforge_manager::{
        Endpoint, external_auth::ExternalCredential, external_client::DatabaseClient,
    };
    let mut fixture = Fixture::new();
    let endpoint = Endpoint::existing(&fixture.root).unwrap();
    let project = Id::new(&fixture.project).unwrap();
    let token = |grant: &IssuedGrant| {
        ExternalCredential::from_wire(grant.credential.expose_secret()).unwrap()
    };
    let mut observer = DatabaseClient::connect(endpoint.clone(), token(&fixture.observer)).unwrap();
    let choices = observer
        .choices(Id::new("db-choices").unwrap(), &project)
        .unwrap();
    let data = choices.data().unwrap();
    assert!(data.stopped);
    assert!(
        data.current_instance_id.is_none()
            && data.instance_ids.is_empty()
            && data.backup_ids.is_empty()
    );
    let value = serde_json::to_value(&choices).unwrap();
    let keys = value["data"]
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect::<Vec<_>>();
    assert_eq!(
        keys,
        vec![
            "backup_ids",
            "current_instance_id",
            "instance_ids",
            "project_id",
            "revision",
            "stopped"
        ]
    );
    assert!(!value.to_string().contains(fixture.root.to_str().unwrap()));
    let (code, cli) = fixture.cli(
        &[
            "api",
            "database",
            "plan",
            &fixture.project,
            "--credential-stdin",
        ],
        &fixture.observer,
    );
    assert_eq!(code, 0);
    assert_eq!(cli["data"]["stopped"], true);
    let request = Request {
        api_version: Default::default(),
        request_id: Id::new("bind-plan").unwrap(),
        project_id: project.clone(),
        expected_revision: Some(fixture.revision),
        idempotency_key: Some(Id::new("bind-denied").unwrap()),
        input: MutationInput::DatabaseBind(DatabaseBindInput {
            instance_id: Id::new("not-owned-instance").unwrap(),
            limits: None,
        }),
    };
    let denied = observer.plan(&request).unwrap();
    assert_eq!(
        denied.error().unwrap().code,
        devforge_control::ErrorCode::Unauthorized
    );
    let mut manage = DatabaseClient::connect(endpoint.clone(), token(&fixture.manage)).unwrap();
    assert_eq!(
        manage.plan(&request).unwrap().error().unwrap().code,
        devforge_control::ErrorCode::Unauthorized
    );
    let grant = fixture
        .native
        .issue_external_grant(
            &project,
            &[
                Scope::Observer,
                Scope::ProjectManage,
                Scope::SensitiveOperation,
            ],
            Duration::from_secs(300),
        )
        .unwrap();
    let mut scoped = DatabaseClient::connect(endpoint, token(&grant)).unwrap();
    let unknown = scoped.plan(&request).unwrap();
    assert!(!unknown.is_ok());
    let mut restore = request.clone();
    restore.input = MutationInput::DatabaseRestore(DatabaseRestoreInput {
        backup_id: Id::new("not-owned-backup").unwrap(),
        limits: None,
    });
    assert!(!scoped.plan(&restore).unwrap().is_ok());
    let lookup = scoped
        .lookup(
            Id::new("db-lookup").unwrap(),
            &project,
            request.idempotency_key.as_ref().unwrap(),
        )
        .unwrap();
    assert!(lookup.data().unwrap().is_none());
    let foreign = observer
        .choices(
            Id::new("db-foreign").unwrap(),
            &Id::new(&fixture.other).unwrap(),
        )
        .unwrap();
    assert_eq!(
        foreign.error().unwrap().code,
        devforge_control::ErrorCode::ProjectMismatch
    );
    assert!(
        fixture.native.snapshot().unwrap().snapshot["projects"]
            .as_array()
            .unwrap()
            .iter()
            .all(|row| row["processes"].as_array().unwrap().is_empty())
    );
}
#[test]
fn database_mcp_opt_in_only_adds_choices_reviewed_bind_and_restore() {
    let mut fixture = Fixture::new();
    for (service, runtime, environment, database, count) in [
        (false, false, false, false, 9),
        (false, false, false, true, 12),
        (true, true, true, true, 22),
    ] {
        let mut mcp = Mcp::with_database(&fixture, service, runtime, environment, database);
        let listed = mcp.call("tools/list", json!({}));
        let tools = listed["result"]["tools"].as_array().unwrap();
        assert_eq!(tools.len(), count);
        assert!(!tools.iter().any(|tool| {
            [
                "database.query",
                "database.clone",
                "database.import",
                "database.export",
                "db_choices",
            ]
            .iter()
            .any(|name| tool["name"] == *name)
        }));
        let choices = mcp.call("tools/call", json!({"name":"database.plan","arguments":{}}));
        if database {
            assert_eq!(choices["result"]["isError"], false);
            assert!(
                choices["result"]["structuredContent"]["data"]["instance_ids"]
                    .as_array()
                    .unwrap()
                    .is_empty()
            );
            let denied=mcp.call("tools/call",json!({"name":"database.plan","arguments":{"action":"bind","instance_id":"not-owned","expected_revision":fixture.revision,"idempotency_key":"db-no-sensitive"}}));
            assert_eq!(
                denied["result"]["structuredContent"]["error"]["code"],
                "unauthorized"
            );
            let invalid=mcp.call("tools/call",json!({"name":"database.restore","arguments":{"backup_id":"id","expected_revision":fixture.revision,"idempotency_key":"bad-restore","review":"a".repeat(64),"path":"/tmp/not-accepted","sql":"private-sql-canary","consent":true}}));
            assert_eq!(invalid["error"]["code"], -32602);
            assert!(!invalid.to_string().contains("private-sql-canary"));
            let clone=mcp.call("tools/call",json!({"name":"database.plan","arguments":{"action":"clone","expected_revision":fixture.revision,"idempotency_key":"no-clone"}}));
            assert_eq!(clone["error"]["code"], -32602);
        } else {
            assert_eq!(choices["error"]["code"], -32602);
        }
        mcp.close();
    }
    assert!(fixture.native.snapshot().is_ok());
}

#[test]
fn domain_observer_facts_and_missing_ca_plans_never_initialize_authority_or_export_global_state() {
    use devforge_control::{
        Request,
        mutations::{DomainCleanupInput, MutationInput},
    };
    use devforge_manager::{
        Endpoint,
        external_auth::ExternalCredential,
        external_client::DomainClient,
        service_control::{DomainPlanKind, DomainReadKind},
    };
    let fixture = Fixture::new();
    let ca = fixture.root.join("serving/authority");
    assert!(!ca.exists());
    let endpoint = Endpoint::existing(&fixture.root).unwrap();
    let project = Id::new(&fixture.project).unwrap();
    let token = |grant: &IssuedGrant| {
        ExternalCredential::from_wire(grant.credential.expose_secret()).unwrap()
    };
    let mut observer = DomainClient::connect(endpoint.clone(), token(&fixture.observer)).unwrap();
    for operation in [
        DomainReadKind::Url,
        DomainReadKind::Ports,
        DomainReadKind::CertificateStatus,
    ] {
        let reply = observer
            .facts(Id::new("domain-read").unwrap(), &project, operation)
            .unwrap();
        let facts = reply.data().unwrap();
        assert!(!facts.ca_prepared);
        assert!(facts.ca_fingerprint.is_none());
        assert!(!facts.domain.as_str().contains("Other"));
        let value = serde_json::to_value(&reply).unwrap();
        assert!(!value.to_string().contains(fixture.root.to_str().unwrap()));
        let keys = value["data"].as_object().unwrap();
        for private in [
            "owner",
            "domains",
            "hosts_path",
            "before_sha256",
            "certificate_der",
            "key",
            "root",
        ] {
            assert!(!keys.contains_key(private));
        }
    }
    let trust = observer
        .prepare(
            Id::new("trust-preview").unwrap(),
            &project,
            DomainPlanKind::Trust,
        )
        .unwrap();
    let trust = trust.data().unwrap();
    assert!(trust.ticket.is_none() && trust.blocked.is_some());
    assert!(!trust.ca_prepared);
    let cleanup = observer
        .prepare(
            Id::new("cleanup-denied").unwrap(),
            &project,
            DomainPlanKind::Cleanup,
        )
        .unwrap();
    assert_eq!(
        cleanup.error().unwrap().code,
        devforge_control::ErrorCode::Unauthorized
    );
    let (code, cli) = fixture.cli(
        &[
            "api",
            "domain",
            "certificate-status",
            &fixture.project,
            "--credential-stdin",
        ],
        &fixture.observer,
    );
    assert_eq!(code, 0);
    assert_eq!(cli["data"]["ca_prepared"], false);
    let foreign = observer
        .facts(
            Id::new("foreign-domain").unwrap(),
            &Id::new(&fixture.other).unwrap(),
            DomainReadKind::Url,
        )
        .unwrap();
    assert_eq!(
        foreign.error().unwrap().code,
        devforge_control::ErrorCode::ProjectMismatch
    );
    let mut full = DomainClient::connect(endpoint, token(&fixture.full)).unwrap();
    let request = Request {
        api_version: Default::default(),
        request_id: Id::new("unapproved-machine-plan").unwrap(),
        project_id: project,
        expected_revision: Some(fixture.revision),
        idempotency_key: Some(Id::new("no-system-permission").unwrap()),
        input: MutationInput::DomainCleanup(DomainCleanupInput {
            plan: Digest::new("a".repeat(64)).unwrap(),
            limits: None,
        }),
    };
    assert_eq!(
        full.plan(&request).unwrap().error().unwrap().code,
        devforge_control::ErrorCode::Unauthorized
    );
    assert!(!ca.exists());
}
#[test]
fn domain_mcp_flag_exposes_exact_seven_operations_without_elevation_or_configure() {
    let mut fixture = Fixture::new();
    let ca = fixture.root.join("serving/authority");
    for (service, runtime, environment, database, domain, count) in [
        (false, false, false, false, false, 9),
        (false, false, false, false, true, 16),
        (true, true, true, true, true, 29),
    ] {
        let mut mcp = Mcp::with_domain(&fixture, service, runtime, environment, database, domain);
        let listed = mcp.call("tools/list", json!({}));
        let tools = listed["result"]["tools"].as_array().unwrap();
        assert_eq!(tools.len(), count);
        assert!(!tools.iter().any(|tool| {
            [
                "domains.configure",
                "domains.confirm",
                "domains.elevate",
                "domain_prepare",
            ]
            .iter()
            .any(|name| tool["name"] == *name)
        }));
        let facts = mcp.call(
            "tools/call",
            json!({"name":"domains.certificate_status","arguments":{}}),
        );
        if domain {
            assert_eq!(facts["result"]["isError"], false);
            assert_eq!(
                facts["result"]["structuredContent"]["data"]["ca_prepared"],
                false
            );
            let prepared = mcp.call(
                "tools/call",
                json!({"name":"domains.trust_plan","arguments":{}}),
            );
            assert!(prepared["result"]["structuredContent"]["data"]["ticket"].is_null());
            let denied = mcp.call(
                "tools/call",
                json!({"name":"domains.cleanup","arguments":{}}),
            );
            assert_eq!(
                denied["result"]["structuredContent"]["error"]["code"],
                "unauthorized"
            );
            let bypass=mcp.call("tools/call",json!({"name":"domains.apply","arguments":{"action":"plan","plan":"a".repeat(64),"expected_revision":fixture.revision,"idempotency_key":"no-elevation","consent":true,"confirm":true,"elevate":true,"helper":"/tmp/not-accepted"}}));
            assert_eq!(bypass["error"]["code"], -32602);
            let global=mcp.call("tools/call",json!({"name":"domains.hosts_plan","arguments":{"domains":["other.devforge.test"],"owner":"not-authority"}}));
            assert_eq!(global["error"]["code"], -32602);
        } else {
            assert_eq!(facts["error"]["code"], -32602);
        }
        mcp.close();
    }
    assert!(!ca.exists());
    assert!(fixture.native.snapshot().is_ok());
}

#[test]
fn diagnostics_sdk_rejects_content_filters_before_any_read_transport() {
    use devforge_control::{
        Request,
        limits::Text,
        tools::{LogInput, MailInput, ToolInput},
    };
    use devforge_manager::{
        Owner, Server, StopSignal, external_auth::ExternalCredential,
        external_client::DiagnosticsClient,
    };
    use std::sync::atomic::{AtomicUsize, Ordering};
    let root = common::root("df-diag-guard-");
    let owner = Owner::acquire(root.path()).unwrap();
    let endpoint = owner.endpoint().clone();
    let stop = StopSignal::default();
    let server_stop = stop.clone();
    let reads = Arc::new(AtomicUsize::new(0));
    let server_reads = reads.clone();
    let server = thread::spawn(move || {
        Server::new(owner).run(server_stop,move|_,request|{if request["method"]=="health"{Ok(json!({"ok":true,"data":{"api_version":1,"owner_pid":std::process::id(),"state":"ready"}}))}else{server_reads.fetch_add(1,Ordering::SeqCst);Err(devforge_manager::Error::HandlerFailed)}})
    });
    let started = Instant::now();
    while devforge_manager::Client::new(endpoint.clone())
        .request(&json!({"method":"health"}))
        .is_err()
    {
        assert!(started.elapsed() < Duration::from_secs(5));
        thread::sleep(Duration::from_millis(10));
    }
    let mut client = DiagnosticsClient::connect(
        endpoint,
        ExternalCredential::from_wire(&"a".repeat(64)).unwrap(),
    )
    .unwrap();
    for input in [
        ToolInput::Log(LogInput {
            page: None,
            query: Some(Text::new(String::new()).unwrap()),
            level: None,
        }),
        ToolInput::Mail(MailInput {
            page: None,
            subject: Some(Text::new(String::new()).unwrap()),
            recipient: None,
        }),
        ToolInput::Mail(MailInput {
            page: None,
            subject: None,
            recipient: Some(Text::new("private-content-filter".into()).unwrap()),
        }),
    ] {
        let request = Request {
            api_version: Default::default(),
            request_id: Id::new("local-denial").unwrap(),
            project_id: Id::new("project").unwrap(),
            expected_revision: None,
            idempotency_key: None,
            input,
        };
        assert_eq!(
            client.read(&request).unwrap_err(),
            devforge_control::ErrorCode::Unauthorized
        );
    }
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    stop.request();
    server.join().unwrap().unwrap();
}

#[test]
fn diagnostics_mcp_defaults_are_readonly_metadata_and_legacy_health_is_preserved() {
    let mut fixture = Fixture::new();
    let mut mcp = Mcp::start(&fixture, false);
    let tools = mcp.call("tools/list", json!({}));
    let tools = tools["result"]["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 9);
    for name in [
        "health.doctor",
        "logs.search",
        "inspector.request",
        "mail.list",
    ] {
        let tool = tools.iter().find(|tool| tool["name"] == name).unwrap();
        assert_eq!(tool["annotations"]["readOnlyHint"], true);
        for forbidden in ["query", "subject", "recipient"] {
            assert!(tool["inputSchema"]["properties"].get(forbidden).is_none());
        }
    }
    let doctor = mcp.call("tools/call", json!({"name":"health.doctor","arguments":{}}));
    assert_eq!(doctor["result"]["isError"], false);
    assert_eq!(
        doctor["result"]["structuredContent"]["data"]["tool"],
        "health.health"
    );
    for name in ["logs.search", "inspector.request", "mail.list"] {
        let reply = mcp.call("tools/call", json!({"name":name,"arguments":{}}));
        assert_eq!(reply["result"]["isError"], false, "{name}: {reply}");
        let data = &reply["result"]["structuredContent"]["data"]["data"];
        assert!(data["items"].as_array().unwrap().is_empty());
        assert_eq!(data["freshness"]["window"]["source_missing"], true);
        assert_eq!(data["freshness"]["origin"], "cached");
    }
    for (name, args) in [
        ("logs.search", json!({"query":""})),
        ("logs.search", json!({"query":null})),
        ("mail.list", json!({"subject":"private-mail-filter-canary"})),
        ("mail.list", json!({"recipient":null})),
    ] {
        let reply = mcp.call("tools/call", json!({"name":name,"arguments":args}));
        assert_eq!(reply["error"]["code"], -32602);
        assert!(!reply.to_string().contains("private-mail-filter-canary"));
    }
    let (code, mail) = fixture.cli(
        &[
            "api",
            "mail",
            "list",
            &fixture.project,
            "--limit",
            "1",
            "--credential-stdin",
        ],
        &fixture.observer,
    );
    assert_eq!(code, 0);
    assert_eq!(mail["data"]["tool"], "mail.list");
    assert_eq!(
        mail["data"]["data"]["freshness"]["window"]["source_missing"],
        true
    );
    let output = Command::new(env!("CARGO_BIN_EXE_devforge"))
        .env("DEVFORGE_DATA_DIR", &fixture.root)
        .args(["api", "health"])
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(output.status.success());
    let owner: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(owner["data"].get("owner").is_some());
    assert!(owner["data"].get("tool").is_none());
    mcp.close();
    assert!(fixture.native.snapshot().is_ok());
}
