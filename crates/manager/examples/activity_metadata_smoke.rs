//! One owned Laravel/SMTP capture, public diagnostics SDK + existing CLI/stdio.
//! No source project processes, downloads, native key store or global settings.
use devforge_control::{
    Id, OperationState, Request, Scope,
    tools::{HealthInput, InspectorInput, LogInput, MailInput, ToolInput},
};
use devforge_core::{ErrorCode, SecretStore};
use devforge_manager::{
    Owner,
    auth::{DesktopAuth, Policy, TrustedDesktopAction},
    controller::{Controller, StartIntent, Stores, local_owner_user},
    external_auth::ExternalCredential,
    external_client::{DiagnosticsClient, ExternalClient},
    native_client::NativeClient,
};
use devforge_projects::project::Catalog;
use devforge_runtime::{Cancellation, package::Target};
use devforge_workspace::{Command, Workspace};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::Write,
    path::PathBuf,
    process::{Command as Process, Stdio},
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
#[derive(Default)]
struct Memory(Mutex<BTreeMap<String, String>>);
impl SecretStore for Memory {
    fn get(&self, key: &str) -> std::result::Result<Option<String>, ErrorCode> {
        Ok(self.0.lock().unwrap().get(key).cloned())
    }
    fn set(&self, key: &str, value: &str) -> std::result::Result<(), ErrorCode> {
        self.0.lock().unwrap().insert(key.into(), value.into());
        Ok(())
    }
    fn delete(&self, key: &str) -> std::result::Result<(), ErrorCode> {
        self.0.lock().unwrap().remove(key);
        Ok(())
    }
}
fn id(value: &str) -> Id {
    Id::new(value).unwrap()
}
struct OwnedManager {
    quit: devforge_manager::controller::OsQuitHandle,
    worker: Option<thread::JoinHandle<devforge_manager::controller::Result<()>>>,
}
impl Drop for OwnedManager {
    fn drop(&mut self) {
        let _ = self.quit.request_explicit();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
fn native(native: &mut NativeClient, project: &str, key: &str, command: Command) -> Result<Value> {
    let receipt = native.admit(&id(project), &id(key), &command)?;
    let until = Instant::now() + Duration::from_secs(10);
    loop {
        let current = native.status(&receipt.project_id, &receipt.operation_id)?;
        if current.state == OperationState::Succeeded {
            return Ok(native.result(&current.project_id, &current.operation_id)?);
        }
        if current.state.terminal() || Instant::now() > until {
            return Err("native fixture operation failed".into());
        }
        thread::sleep(Duration::from_millis(10));
    }
}
fn process(
    mut command: Process,
    input: Option<&[u8]>,
    timeout: Duration,
) -> Result<(bool, Vec<u8>, Vec<u8>)> {
    let stdout = tempfile::NamedTempFile::new()?;
    let stderr = tempfile::NamedTempFile::new()?;
    command
        .stdout(File::create(stdout.path())?)
        .stderr(File::create(stderr.path())?)
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        });
    let mut child = command.spawn()?;
    if let Some(input) = input {
        child
            .stdin
            .take()
            .ok_or("stdin missing")?
            .write_all(input)?;
    }
    let until = Instant::now() + timeout;
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if Instant::now() > until || fs::metadata(stdout.path())?.len() > 1024 * 1024 {
            let _ = child.kill();
            let _ = child.wait();
            return Err("owned child timeout/output bound".into());
        }
        thread::sleep(Duration::from_millis(10));
    };
    Ok((
        status.success(),
        fs::read(stdout.path())?,
        fs::read(stderr.path())?,
    ))
}
fn main() {
    if let Err(error) = run() {
        eprintln!("activity metadata fixture: {error}");
        std::process::exit(1);
    }
}
fn run() -> Result<()> {
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    if args.len() != 7 {
        return Err("usage: PROJECT PHP SCRIPT CLI REPORT NODE INSPECTOR_LAUNCHER".into());
    }
    let project = fs::canonicalize(&args[0])?;
    let php = PathBuf::from(&args[1]);
    let script = PathBuf::from(&args[2]);
    let cli = PathBuf::from(&args[3]);
    let report = PathBuf::from(&args[4]);
    let parent = project.parent().ok_or("missing owned parent")?;
    if !project.join(".devforge-e10-fixture").is_file() {
        return Err("owned fixture marker required".into());
    }
    let fixture_root = devforge_workspace::testing::FixtureRoot::create_new(&parent.join("data"))?;
    let root = fixture_root.root();
    let secrets = Arc::new(Memory::default());
    let control = Arc::new(Memory::default());
    let mut workspace = Workspace::open(root, secrets.clone())?;
    let catalog = Catalog::bundled();
    let target = Target::current()?;
    let php_id = catalog
        .packages
        .iter()
        .find(|(_, p)| {
            p.kind == "php"
                && p.target == target
                && p.executables.contains_key("php")
                && p.version.starts_with("8.4.27-staticphp")
        })
        .ok_or("native PHP metadata absent")?
        .0
        .clone();
    let node_id = catalog
        .packages
        .iter()
        .find(|(_, p)| p.kind == "node" && p.target == target && p.executables.contains_key("node"))
        .ok_or("native Node metadata absent")?
        .0
        .clone();
    let imported = workspace.execute(
        Command::Import {
            path: project.clone(),
            name: "Metadata Laravel fixture".into(),
            php: php_id,
            node: node_id,
            domain: "metadata.devforge.test".into(),
            https: false,
            vite: false,
            queue: false,
        },
        &Cancellation::default(),
    )?;
    let project_id = imported["project_id"]
        .as_str()
        .ok_or("project ID missing")?
        .to_string();
    drop(workspace);
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
            workspace: secrets.clone(),
            desktop: control,
        },
    )?;
    let endpoint = controller.endpoint().clone();
    let quit = controller.quitter();
    let worker = thread::spawn(move || controller.serve(None));
    let _owned = OwnedManager {
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
    let mut native_client = NativeClient::authenticate_explicit(endpoint.clone(), &key)?;
    native(
        &mut native_client,
        &project_id,
        "capture-enable",
        Command::InspectorCapture {
            id: project_id.clone(),
            enabled: true,
        },
    )?;
    native(
        &mut native_client,
        &project_id,
        "mail-enable",
        Command::MailConfigure {
            id: project_id.clone(),
            enabled: true,
        },
    )?;
    let session: Value = serde_json::from_slice(&fs::read(
        root.join("projects/inspection")
            .join(format!("{project_id}.json")),
    )?)?;
    let smtp: Value = serde_json::from_slice(&fs::read(
        root.join("mail").join(&project_id).join("session.json"),
    )?)?;
    let password = secrets
        .get(&devforge_projects::mail::reference(&project_id))?
        .ok_or("fixture mail password absent")?;
    let mut php_command = Process::new(&php);
    php_command
        .env_clear()
        .args([
            "-n",
            "-d",
            "date.timezone=UTC",
            "-d",
            "display_errors=stderr",
        ])
        .arg(&script)
        .arg(&project)
        .arg("main")
        .current_dir(&project)
        .env("APP_ENV", "testing")
        .env(
            "DEVFORGE_INSPECTOR_ENDPOINT",
            session["endpoint"].as_str().ok_or("endpoint")?,
        )
        .env(
            "DEVFORGE_INSPECTOR_TOKEN",
            session["token"].as_str().ok_or("token")?,
        )
        .env("DEVFORGE_INSPECTOR_PROJECT", &project_id)
        .env(
            "E10_SMTP_PORT",
            smtp["port"].as_u64().ok_or("smtp")?.to_string(),
        )
        .env("E10_SMTP_PASSWORD", &password);
    for key in ["SystemRoot", "WINDIR", "TEMP", "TMP", "TMPDIR"] {
        if let Some(value) = std::env::var_os(key) {
            php_command.env(key, value);
        }
    }
    let (success, output, errors) = process(php_command, None, Duration::from_secs(30))?;
    if !success {
        let diagnostic = String::from_utf8_lossy(&errors)
            .replace(session["token"].as_str().unwrap(), "[REDACTED]")
            .replace(&password, "[REDACTED]")
            .replace(project.to_string_lossy().as_ref(), "<fixture>");
        eprintln!("{}", diagnostic.chars().take(3000).collect::<String>());
        return Err("actual Laravel kernel fixture failed".into());
    }
    let framework: Value = serde_json::from_slice(&output)?;
    assert!(
        framework["framework"]
            .as_str()
            .is_some_and(|v| v.starts_with("13."))
    );
    // Boolean-only private fixture validation proves the canaries really landed.
    let db = rusqlite::Connection::open_with_flags(
        root.join("inspector/events.sqlite3"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )?;
    let stored_log:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM events WHERE project=?1 AND instr(body,'M13_RAW_LOG_PRIVATE_CANARY')>0)",[&project_id],|row|row.get(0))?;
    assert!(stored_log);
    let inbox = rusqlite::Connection::open_with_flags(
        root.join("mail").join(&project_id).join("inbox.sqlite3"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )?;
    let stored_mail:bool=inbox.query_row("SELECT EXISTS(SELECT 1 FROM messages WHERE instr(summary,'M13_MAIL_PRIVATE_CANARY')>0 AND instr(CAST(raw AS TEXT),'M13_BODY_PRIVATE_CANARY')>0)",[],|row|row.get(0))?;
    assert!(stored_mail);
    let grant = native_client.issue_external_grant(
        &id(&project_id),
        &[Scope::Observer],
        Duration::from_secs(600),
    )?;
    let mut client = DiagnosticsClient::connect(
        endpoint.clone(),
        ExternalCredential::from_wire(grant.credential.expose_secret())?,
    )?;
    let inputs = [
        ToolInput::Health(HealthInput { page: None }),
        ToolInput::Inspector(InspectorInput {
            page: None,
            context_id: None,
        }),
        ToolInput::Log(LogInput {
            page: None,
            query: None,
            level: None,
        }),
        ToolInput::Mail(MailInput {
            page: None,
            subject: None,
            recipient: None,
        }),
    ];
    let mut safe_outputs = Vec::new();
    for (index, input) in inputs.into_iter().enumerate() {
        let request = Request {
            api_version: Default::default(),
            request_id: id(&format!("actual-read-{index}")),
            project_id: id(&project_id),
            expected_revision: None,
            idempotency_key: None,
            input,
        };
        let response = client.read(&request)?;
        assert!(response.is_ok());
        let value = serde_json::to_value(response)?;
        if index > 0 {
            assert!(
                value["data"]["data"]["items"]
                    .as_array()
                    .is_some_and(|rows| !rows.is_empty())
            );
        }
        safe_outputs.push(value);
    }
    for words in [
        ["health", "doctor"],
        ["inspector", "request"],
        ["logs", "search"],
        ["mail", "list"],
    ] {
        let mut command = Process::new(&cli);
        command
            .args(["api", words[0], words[1], &project_id, "--credential-stdin"])
            .env("DEVFORGE_DATA_DIR", root);
        let (success, output, _) = process(
            command,
            Some(grant.credential.expose_secret().as_bytes()),
            Duration::from_secs(15),
        )?;
        assert!(success);
        let value: Value = serde_json::from_slice(&output)?;
        assert!(value["error"].is_null());
        safe_outputs.push(value);
    }
    let credential = parent.join("observer.json");
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&credential)?;
    serde_json::to_writer(
        &mut file,
        &json!({"schema":1,"project_id":project_id,"credential":grant.credential.expose_secret()}),
    )?;
    file.sync_all()?;
    drop(file);
    let node = PathBuf::from(&args[5]);
    let launcher = PathBuf::from(&args[6]);
    let mut official = false;
    if node.is_file() && launcher.is_file() {
        let config = parent.join("inspector-config.json");
        fs::write(
            &config,
            serde_json::to_vec(
                &json!({"mcpServers":{"owned":{"command":cli,"args":["mcp","serve","--credential-file",credential],"env":{"DEVFORGE_DATA_DIR":root}}}}),
            )?,
        )?;
        let mut command = Process::new(node);
        command
            .arg(launcher)
            .args(["--cli", "--config"])
            .arg(config)
            .args([
                "--server",
                "owned",
                "--method",
                "tools/call",
                "--tool-name",
                "inspector.request",
                "--tool-args-json",
                "{}",
            ])
            .env("MCP_INSPECTOR_SECRET_STORE", "memory")
            .env(
                "MCP_CLIENT_CONFIG_PATH",
                parent.join("inspector-client-settings.json"),
            );
        let (success, output, _) = process(command, None, Duration::from_secs(30))?;
        if !success {
            return Err("cached official MCP client failed".into());
        }
        safe_outputs.push(Value::String(String::from_utf8(output)?));
        official = true;
    }
    let serialized = serde_json::to_string(&safe_outputs)?;
    for canary in [
        "M13_RAW_LOG_PRIVATE_CANARY",
        "M13_MAIL_PRIVATE_CANARY",
        "M13_BODY_PRIVATE_CANARY",
        "private-sender@example.test",
        "private-recipient@example.test",
        "PRIVATE_RESET_TOKEN",
        "E10_BINDING_PRIVATE_CANARY",
        "RuntimeException",
    ] {
        assert!(
            !serialized.contains(canary),
            "private fixture content escaped metadata projection"
        );
    }
    assert!(ExternalClient::observe_owner(&endpoint).is_ok());
    fs::write(
        report,
        serde_json::to_vec_pretty(
            &json!({"passed":true,"host":{"os":std::env::consts::OS,"arch":std::env::consts::ARCH},"framework":framework["framework"],"php":framework["php"],
        "actual_foundation_kernel_collector_and_smtp":true,"stored_private_canaries_verified":true,"observer_public_diagnostics_sdk_tools":4,"actual_stdio_cli_metadata_tools":4,"cached_official_mcp_inspector":official,
        "public_outputs_exclude_content_canaries":true,"mcp_eof_preserves_owned_manager":true,"native_credentials":false,"source_project_process_used":false,"downloads":false,"application_http_server":false}),
        )?,
    )?;
    println!("owned Laravel/mail metadata public SDK and stdio passed");
    Ok(())
}
