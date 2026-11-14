//! E10: real Laravel Foundation kernel -> companion -> production Collector/SMTP.
//! Invocation is restricted by the Python runner to a disposable marked fixture.
use devforge_inspector::{Collector, Kind, mail, read_session};
use serde_json::{Value, json};
use std::{
    error::Error,
    fs::{self, File},
    path::Path,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

fn require(condition: bool, code: &'static str) -> Result<()> {
    if condition { Ok(()) } else { Err(code.into()) }
}

fn php(
    executable: &Path,
    script: &Path,
    fixture: &Path,
    mode: &str,
    session: &devforge_inspector::Session,
    smtp: &mail::Session,
    password: &str,
) -> Result<(bool, Vec<u8>, Vec<u8>)> {
    let output = tempfile::NamedTempFile::new()?;
    let errors = tempfile::NamedTempFile::new()?;
    let mut command = Command::new(executable);
    command
        .args([
            "-n",
            "-d",
            "date.timezone=UTC",
            "-d",
            "display_errors=stderr",
        ])
        .arg(script)
        .arg(fixture)
        .arg(mode)
        .current_dir(fixture)
        .env_clear()
        .env(
            "APP_ENV",
            if mode == "disabled" {
                "production"
            } else {
                "testing"
            },
        )
        .env("DEVFORGE_INSPECTOR_ENDPOINT", &session.endpoint)
        .env("DEVFORGE_INSPECTOR_TOKEN", &session.token)
        .env("DEVFORGE_INSPECTOR_PROJECT", &session.project_id)
        .env("E10_SMTP_PORT", smtp.port.to_string())
        .env("E10_SMTP_PASSWORD", password)
        .stdin(Stdio::null())
        .stdout(File::create(output.path())?)
        .stderr(File::create(errors.path())?);
    for key in ["SystemRoot", "WINDIR", "TEMP", "TMP", "TMPDIR"] {
        if let Some(value) = std::env::var_os(key) {
            command.env(key, value);
        }
    }
    let mut child = command.spawn()?;
    let until = Instant::now() + Duration::from_secs(30);
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if Instant::now() >= until
            || output.as_file().metadata()?.len() > 1_048_576
            || errors.as_file().metadata()?.len() > 1_048_576
        {
            let _ = child.kill();
            let _ = child.wait();
            return Err("php_fixture_limit".into());
        }
        thread::sleep(Duration::from_millis(10));
    };
    Ok((
        status.success(),
        fs::read(output.path())?,
        fs::read(errors.path())?,
    ))
}

fn run() -> Result<()> {
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    require(args.len() == 4, "usage_php_fixture_script_report")?;
    let executable = Path::new(&args[0]);
    let fixture = Path::new(&args[1]);
    let script = Path::new(&args[2]);
    require(
        fixture.join(".devforge-e10-fixture").is_file(),
        "isolated_fixture_required",
    )?;
    let storage = tempfile::tempdir()?;
    let projects = storage.path().join("projects");
    fs::create_dir_all(&projects)?;
    let project = uuid::Uuid::new_v4().simple().to_string();
    let other = uuid::Uuid::new_v4().simple().to_string();
    let mut collector = Collector::open(&storage.path().join("inspector"), &projects)?;
    collector.enable(&project)?;
    collector.enable(&other)?;
    let session = read_session(&projects, &project)?.ok_or("session_missing")?;
    let password = format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    );
    let smtp = mail::MailServer::start(&projects, &project, password.clone())?;
    let disabled = php(
        executable,
        script,
        fixture,
        "disabled",
        &session,
        &smtp.session,
        &password,
    )?;
    require(
        disabled.0 && serde_json::from_slice::<Value>(&disabled.1)?["disabled"] == true,
        "production_opt_out_failed",
    )?;
    require(
        collector.view(&project)?.events.is_empty(),
        "production_emitted_events",
    )?;
    let main = php(
        executable,
        script,
        fixture,
        "main",
        &session,
        &smtp.session,
        &password,
    )?;
    if !main.0 {
        let detail = String::from_utf8_lossy(&main.2)
            .replace(&session.token, "[REDACTED]")
            .replace(&password, "[REDACTED]")
            .replace(&fixture.to_string_lossy().to_string(), "<fixture>");
        eprintln!("{}", detail.chars().take(6000).collect::<String>());
        return Err("framework_fixture_failed".into());
    }
    let framework: Value = serde_json::from_slice(&main.1)?;
    let dd = php(
        executable,
        script,
        fixture,
        "dd",
        &session,
        &smtp.session,
        &password,
    )?;
    require(!dd.0, "dd_did_not_exit_nonzero")?;
    let view = collector.view(&project)?;
    for kind in [
        Kind::Request,
        Kind::Query,
        Kind::Job,
        Kind::Log,
        Kind::Dump,
        Kind::Exception,
    ] {
        require(
            view.events.iter().any(|record| record.event.kind == kind),
            "capture_kind_missing",
        )?;
    }
    require(
        view.events.iter().any(|r| {
            r.event.kind == Kind::Request
                && r.event.name == "GET compatibility"
                && r.event.data["status"] == 200
        }),
        "request_summary_missing",
    )?;
    require(
        view.events.iter().any(|r| {
            r.event.kind == Kind::Request
                && r.event.name == "GET compatibility-error"
                && r.event.data["status"] == 500
        }),
        "exception_request_summary_missing",
    )?;
    for status in ["succeeded", "failed"] {
        require(
            view.events
                .iter()
                .any(|r| r.event.kind == Kind::Job && r.event.data["status"] == status),
            "queue_summary_missing",
        )?;
    }
    for marker in ["request-dump", "job-dump", "dump-exit"] {
        require(
            view.events
                .iter()
                .any(|r| r.event.kind == Kind::Dump && r.event.data["marker"] == marker),
            "framework_dump_missing",
        )?;
    }
    let serialized = serde_json::to_string(&view)?;
    for secret in [
        "E10_BINDING_PRIVATE_CANARY",
        "E10_SQL_PRIVATE_CANARY",
        "E10_LOG_PRIVATE_CANARY",
        "E10_CONTEXT_PRIVATE_CANARY",
        "E10_EXCEPTION_PRIVATE_CANARY",
        &session.token,
        &password,
    ] {
        require(!serialized.contains(secret), "private_value_in_capture")?;
    }
    require(
        collector.view(&other)?.events.is_empty(),
        "cross_project_events",
    )?;
    let inbox = mail::MailStore::open(&projects, &project)?;
    let page = inbox.list("E10 compatibility", 0)?;
    require(page.total == 1, "smtp_message_missing")?;
    let detail = inbox.detail(&page.messages[0].id)?;
    require(
        detail.text.contains("E10 local-only body") && detail.attachments.len() == 1,
        "mime_contract_failed",
    )?;
    let part = inbox.part(&detail.summary.id, 0)?;
    use base64::{Engine, engine::general_purpose::STANDARD};
    require(
        STANDARD.decode(&part.base64)? == b"E10 attachment bytes",
        "attachment_bytes_changed",
    )?;
    require(
        mail::MailStore::open(&projects, &other)?.list("", 0)?.total == 0,
        "cross_project_mail",
    )?;
    let report = json!({
        "schema":1,"passed":true,"framework":framework["framework"],"php":framework["php"],
        "host":{"os":std::env::consts::OS,"architecture":std::env::consts::ARCH},
        "scope":"synthetic_foundation_http_kernel_sqlite_sync_queue_production_collector_smtp",
        "native_desktop_checked":false,"full_skeleton_or_web_server_checked":false,
        "checks":{
            "provider_boot_and_request_middleware":true,"http_kernel_200_and_500":true,
            "sqlite_query_listener":true,"sync_queue_success_and_failure_events":true,
            "framework_logging_and_exception":true,"request_job_and_dd_dumps":true,
            "private_binding_literal_context_and_message_redaction":true,
            "production_environment_opt_out":true,"other_project_capture_empty":true,
            "actual_authenticated_smtp_and_mime_store":true,"attachment_exact_bytes":true,
            "other_project_inbox_empty":true
        },"event_count":view.events.len()
    });
    fs::write(Path::new(&args[3]), serde_json::to_vec_pretty(&report)?)?;
    println!("E10 companion framework contract passed");
    Ok(())
}
fn main() {
    if let Err(error) = run() {
        eprintln!("E10: {error}");
        std::process::exit(1);
    }
}
