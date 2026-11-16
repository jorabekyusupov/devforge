//! One cached pure-PHP data-free execution flow; no Laravel/collector/DB startup.
use devforge_control::{
    Id, OperationState, Request, Scope,
    mutations::{
        ExecutionArguments, ExecutionTool, ManagedTestInput, MutationInput, MutationReceipt,
    },
    tools::{LogInput, ToolInput},
};
use devforge_core::{ErrorCode, SecretStore};
use devforge_manager::{
    Owner,
    auth::{DesktopAuth, Policy, TrustedDesktopAction},
    controller::{Controller, StartIntent, Stores, local_owner_user},
    external_auth::{ExternalCredential, IssuedGrant},
    external_client::{DiagnosticsClient, ExecutionClient, ExternalClient},
    external_control::{ExternalInput, ExternalOutput},
    native_client::NativeClient,
};
use devforge_projects::project::Catalog;
use devforge_runtime::{Cancellation, package::Target};
use devforge_workspace::{Command, Workspace};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    path::PathBuf,
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
#[derive(Default)]
struct Memory(Mutex<BTreeMap<String, String>>);
impl SecretStore for Memory {
    fn get(&self, k: &str) -> std::result::Result<Option<String>, ErrorCode> {
        Ok(self.0.lock().unwrap().get(k).cloned())
    }
    fn set(&self, k: &str, v: &str) -> std::result::Result<(), ErrorCode> {
        self.0.lock().unwrap().insert(k.into(), v.into());
        Ok(())
    }
    fn delete(&self, k: &str) -> std::result::Result<(), ErrorCode> {
        self.0.lock().unwrap().remove(k);
        Ok(())
    }
}
struct Owned {
    quit: devforge_manager::controller::OsQuitHandle,
    worker: Option<thread::JoinHandle<devforge_manager::controller::Result<()>>>,
}
impl Drop for Owned {
    fn drop(&mut self) {
        let _ = self.quit.request_explicit();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
fn id(v: &str) -> Id {
    Id::new(v).unwrap()
}
fn client(endpoint: devforge_manager::Endpoint, grant: &IssuedGrant) -> Result<ExecutionClient> {
    Ok(ExecutionClient::connect(
        endpoint,
        ExternalCredential::from_wire(grant.credential.expose_secret())?,
    )?)
}
fn complete(
    endpoint: devforge_manager::Endpoint,
    grant: &IssuedGrant,
    receipt: &MutationReceipt,
) -> Result<()> {
    let mut client = ExternalClient::connect(
        endpoint,
        ExternalCredential::from_wire(grant.credential.expose_secret())?,
    )?;
    let until = Instant::now() + Duration::from_secs(30);
    loop {
        let reply = client.request(
            id("observe-execution"),
            ExternalInput::OperationStatus {
                project_id: receipt.project_id.clone(),
                operation_id: receipt.operation_id.clone(),
            },
            None,
        )?;
        if let Some(ExternalOutput::OperationStatus(output)) = reply.data {
            let state = output.items[0].state;
            if state.terminal() {
                assert_eq!(state, OperationState::Succeeded);
                return Ok(());
            }
        }
        if Instant::now() > until {
            return Err("owned execution did not finish".into());
        }
        thread::sleep(Duration::from_millis(20));
    }
}
fn main() {
    if let Err(error) = run() {
        eprintln!("execution fixture: {error}");
        std::process::exit(1);
    }
}
fn run() -> Result<()> {
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    if args.len() != 2 {
        return Err("usage RUNTIME_CACHE REPORT".into());
    }
    let source = PathBuf::from(&args[0]);
    let report = PathBuf::from(&args[1]);
    let parent = tempfile::Builder::new()
        .prefix("df-exec-")
        .tempdir_in("/tmp")?;
    let fixture =
        devforge_workspace::testing::FixtureRoot::create_new(&parent.path().join("data"))?;
    let root = fixture.root();
    let workspace_store = Arc::new(Memory::default());
    let control = Arc::new(Memory::default());
    let mut setup = Workspace::open(root, workspace_store.clone())?;
    let catalog = Catalog::bundled();
    let target = Target::current()?;
    let mut selected = BTreeMap::new();
    for kind in ["php", "node"] {
        let (name, package) = catalog
            .packages
            .iter()
            .find(|(_, p)| {
                p.kind == kind
                    && p.target == target
                    && if kind == "php" {
                        p.version.starts_with("8.4.27-staticphp")
                            && p.executables.contains_key("php")
                    } else {
                        p.version.contains("24.22.2")
                    }
            })
            .ok_or("cached native descriptor missing")?;
        let key = package.key()?;
        let from = source.join("packages").join(&key);
        let to = root.join("runtimes/packages").join(&key);
        fs::create_dir(&to)?;
        fs::copy(
            from.join(".devforge-receipt.json"),
            to.join(".devforge-receipt.json"),
        )?;
        for relative in package
            .executables
            .values()
            .chain(package.resources.values())
        {
            let file = to.join(relative);
            fs::create_dir_all(file.parent().unwrap())?;
            fs::copy(from.join(relative), file)?;
        }
        setup.packages.executable(package, kind)?;
        selected.insert(kind.to_string(), name.clone());
    }
    let project = parent.path().join("project");
    fs::create_dir_all(project.join("public"))?;
    fs::write(project.join("public/index.php"), "<?php")?;
    fs::write(
        project.join("composer.json"),
        r#"{"require":{"php":"^8.3","laravel/framework":"^13"}}"#,
    )?;
    let script = |value: u8| {
        format!(
            "<?php file_put_contents(__DIR__.'/run-count',(string)((int)@file_get_contents(__DIR__.'/run-count')+1)); echo $argv[1],\"\\n\"; $actual={value}; if($actual!==2){{echo \"owned-test-failed\\n\";exit(7);}} echo \"owned-test-passed\\n\";"
        )
    };
    fs::write(project.join("check.php"), script(1))?;
    let value = setup.execute(
        Command::Import {
            path: project.clone(),
            name: "Data-free execution fixture".into(),
            php: selected["php"].clone(),
            node: selected["node"].clone(),
            domain: "execution.devforge.test".into(),
            https: false,
            vite: false,
            queue: false,
        },
        &Cancellation::default(),
    )?;
    let project_id = value["project_id"]
        .as_str()
        .ok_or("missing project")?
        .to_string();
    let revision = setup.projects.get(&project_id)?.revision;
    drop(setup);
    let owner = Owner::acquire(root)?;
    let mut auth = DesktopAuth::new(
        owner.endpoint().clone(),
        local_owner_user()?,
        Policy::default(),
    )?;
    let key = auth.rotate_desktop_key(
        &TrustedDesktopAction::from_explicit_desktop_action(),
        control.as_ref(),
        Instant::now(),
    )?;
    drop(owner);
    let controller = Controller::start(
        root,
        StartIntent::ExplicitUserStart,
        Stores {
            workspace: workspace_store,
            desktop: control,
        },
    )?;
    let endpoint = controller.endpoint().clone();
    let quit = controller.quitter();
    let worker = thread::spawn(move || controller.serve(None));
    let _owned = Owned {
        quit,
        worker: Some(worker),
    };
    let until = Instant::now() + Duration::from_secs(5);
    while ExternalClient::observe_owner(&endpoint).is_err() {
        if Instant::now() > until {
            return Err("owned manager not ready".into());
        }
        thread::sleep(Duration::from_millis(10));
    }
    let mut native = NativeClient::authenticate_explicit(endpoint.clone(), &key)?;
    let grant = native.issue_external_grant(
        &id(&project_id),
        &[Scope::Observer, Scope::ProjectManage, Scope::ExecutionTest],
        Duration::from_secs(600),
    )?;
    let mut execution = client(endpoint.clone(), &grant)?;
    let arguments = vec![
        "check.php".to_string(),
        "PRIVATE_EXEC_ARG_CANARY".to_string(),
    ];
    let request = |key: &str| Request {
        api_version: Default::default(),
        request_id: id(key),
        project_id: id(&project_id),
        expected_revision: Some(revision),
        idempotency_key: Some(id(key)),
        input: MutationInput::ManagedTest(ManagedTestInput {
            tool: ExecutionTool::Php,
            args: ExecutionArguments::new(arguments.clone()).unwrap(),
            limits: None,
        }),
    };
    let first = request("owned-test-first");
    let planned = execution.plan(&first)?;
    let plan = planned.data().ok_or("first plan denied")?;
    native.approve_service_plan(&plan.digest)?;
    let applied = execution.apply(&first, &plan.digest)?;
    let receipt = applied.data().ok_or("first execution denied")?.clone();
    complete(endpoint.clone(), &grant, &receipt)?;
    let output = execution.output(id("failed-output"), &id(&project_id), &receipt.operation_id)?;
    let output = output.data().ok_or("failed output unavailable")?;
    assert!(!output.success);
    assert_eq!(output.exit_code, Some(7));
    assert!(!serde_json::to_string(output)?.contains("PRIVATE_EXEC_ARG_CANARY"));
    let replay = execution.apply(&first, &plan.digest)?;
    assert_eq!(
        replay.data().ok_or("replay unavailable")?.operation_id,
        receipt.operation_id
    );
    assert_eq!(fs::read_to_string(project.join("run-count"))?, "1");
    fs::write(project.join("check.php"), script(2))?;
    let second = request("owned-test-fixed");
    let planned = execution.plan(&second)?;
    let plan = planned.data().ok_or("fixed plan denied")?;
    native.approve_service_plan(&plan.digest)?;
    let applied = execution.apply(&second, &plan.digest)?;
    let fixed = applied.data().ok_or("fixed execution denied")?.clone();
    complete(endpoint.clone(), &grant, &fixed)?;
    let output = execution.output(id("fixed-output"), &id(&project_id), &fixed.operation_id)?;
    let output = output.data().ok_or("fixed output unavailable")?;
    assert!(output.success);
    assert_eq!(output.exit_code, Some(0));
    assert!(!serde_json::to_string(output)?.contains("PRIVATE_EXEC_ARG_CANARY"));
    assert_eq!(fs::read_to_string(project.join("run-count"))?, "2");
    let mut diagnostics = DiagnosticsClient::connect(
        endpoint.clone(),
        ExternalCredential::from_wire(grant.credential.expose_secret())?,
    )?;
    let metadata = diagnostics.read(&Request {
        api_version: Default::default(),
        request_id: id("after-test-metadata"),
        project_id: id(&project_id),
        expected_revision: None,
        idempotency_key: None,
        input: ToolInput::Log(LogInput {
            page: None,
            query: None,
            level: None,
        }),
    })?;
    assert!(metadata.is_ok());
    let other = native.issue_external_grant(
        &id(&project_id),
        &[Scope::Observer, Scope::ProjectManage, Scope::ExecutionTest],
        Duration::from_secs(600),
    )?;
    let denied = client(endpoint.clone(), &other)?.output(
        id("other-output"),
        &id(&project_id),
        &fixed.operation_id,
    )?;
    assert!(!denied.is_ok());
    native.revoke_external_grant(&grant.record.grant_id)?;
    let revoked = execution.output(id("revoked-output"), &id(&project_id), &fixed.operation_id)?;
    assert!(!revoked.is_ok());
    let ledger = fs::read(root.join("control/operations.sqlite3"))?;
    let text = String::from_utf8_lossy(&ledger);
    let canonical = serde_json::to_vec(&arguments)?;
    let raw_hash = Sha256::digest(&canonical)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    assert!(!text.contains("PRIVATE_EXEC_ARG_CANARY"));
    assert!(!text.contains(&raw_hash));
    fs::write(
        report,
        serde_json::to_vec_pretty(
            &json!({"passed":true,"host":{"os":std::env::consts::OS,"arch":std::env::consts::ARCH},"actual_cached_php":selected["php"],"public_execution_client":true,"native_exact_review":true,"initial_exit":7,"fixed_exit":0,"same_key_replay_no_rerun":true,
        "data_free_managed_test_context":true,"strong_database_isolation":false,"existing_metadata_read":true,"other_grant_output_denied":true,"revoked_output_denied":true,"raw_arg_and_canonical_arg_hash_absent_ledger":true,"supplied_arg_masked_output":true,"downloads":false,"native_secret_store":false,"source_cache_mutated":false,"laravel_producer":false}),
        )?,
    )?;
    println!("owned PHP failure/fix execution and metadata passed");
    Ok(())
}
