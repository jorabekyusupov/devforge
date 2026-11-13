use super::*;
use std::time::{Duration, Instant};
fn php(w: &Workspace, s: &dyn devforge_core::SecretStore, id: &str, body: &str) -> Result<String> {
    exec(w,s,id,Tool::Php,vec!["-r".into(),format!("require 'vendor/autoload.php';$app=require 'bootstrap/app.php';$app->make(Illuminate\\Contracts\\Console\\Kernel::class)->bootstrap();{body}").into()])
}
pub fn run(root: &Path) -> Result<()> {
    let data = root.join("data");
    let s = Arc::new(devforge_workspace::testing::FixtureSecrets::open(
        data.join("e2e-secrets.json"),
    ));
    let mut w = Workspace::open(&data, s.clone())?;
    let cancel = Cancellation::default();
    let projects = w.projects.list()?;
    let a = projects.iter().find(|p| p.name == "Atlas").unwrap().clone();
    let b = projects
        .iter()
        .find(|p| p.name == "Studio")
        .unwrap()
        .clone();
    let before = fs::read(a.path.join(".env")).ok();
    for id in [&a.id, &b.id] {
        w.execute(Command::TestDatabase { id: id.clone() }, &cancel)?;
        w.execute(
            Command::MailDelete {
                id: id.clone(),
                message: None,
                consent: true,
            },
            &cancel,
        )?;
        w.execute(Command::InspectorClear { id: id.clone() }, &cancel)?;
        w.execute(Command::InspectorInstall { id: id.clone() }, &cancel)?;
        w.execute(
            Command::InspectorCapture {
                id: id.clone(),
                enabled: true,
            },
            &cancel,
        )?;
        w.execute(
            Command::MailConfigure {
                id: id.clone(),
                enabled: true,
            },
            &cancel,
        )?;
    }
    eprintln!("M11_PHASE: Laravel SMTP MIME with HTML plain text and attachment");
    let send = r#"Illuminate\Support\Facades\Mail::send([],[],function($message){$message->to('developer@example.test');$message->from('hello@example.test','DevForge fixture');$message->subject('M11 Welcome Русский O\'zbek');$message->getSymfonyMessage()->text('M11_PLAIN Русский O\'zbek')->html('<h1>M11_HTML_RENDERED</h1><script>parent.__M11_EVIL=1</script><img src="http://127.0.0.1:9/REMOTE_M11" onerror="parent.__M11_EVIL=2"><a href="https://example.test/REMOTE_M11">Remote link</a>');$message->attachData('M11_ATTACHMENT_UTF8','note.txt',['mime'=>'text/plain']);$message->attachData(base64_decode('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGPgL8sDAAGLAPQ8xSAqAAAAAElFTkSuQmCC'),'pixel.png',['mime'=>'image/png']);});Illuminate\Support\Facades\Log::warning('M11_LOG_WARNING',['order'=>'fixture','api_token'=>'DO_NOT_STORE_CONTEXT']);Illuminate\Support\Facades\Log::error('M11_EXCEPTION_LOG',['exception'=>(function(){return new RuntimeException('M11_EXCEPTION token=DO_NOT_STORE_EXCEPTION');})(),'order'=>'fixture']);"#;
    php(&w, s.as_ref(), &a.id, send)?;
    php(
        &w,
        s.as_ref(),
        &b.id,
        r#"Illuminate\Support\Facades\Mail::raw('Other project',function($m){$m->to('second@example.test')->subject('M11_OTHER_PROJECT');});"#,
    )?;
    let inbox = w.mail_view(&a.id, "M11 Welcome", 0)?;
    assert_eq!(inbox["inbox"]["total"], 1);
    let message = inbox["inbox"]["messages"][0]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let detail = w.mail_message(&a.id, &message)?;
    assert!(
        detail["html"]
            .as_str()
            .unwrap()
            .contains("M11_HTML_RENDERED")
    );
    assert!(detail["text"].as_str().unwrap().contains("M11_PLAIN"));
    assert_eq!(detail["attachments"].as_array().unwrap().len(), 2);
    let part = w.mail_part(&a.id, &message, 0)?;
    assert_eq!(part["attachment"]["name"], "note.txt");
    assert!(w.mail_message(&b.id, &message).is_err());
    let saved = root.join("m11-note.txt");
    w.execute(
        Command::MailSave {
            id: a.id.clone(),
            message: message.clone(),
            index: 0,
            path: saved.clone(),
        },
        &cancel,
    )?;
    assert_eq!(fs::read(&saved)?, b"M11_ATTACHMENT_UTF8");
    fs::remove_file(saved)?;
    let result = Context::resolve(&w.projects, &a.id, &w.catalog, &w.packages, s.as_ref())?
        .frontend()
        .invocation(
            Tool::Node,
            vec![
                "-p".into(),
                "JSON.stringify({mail:process.env.MAIL_PASSWORD===undefined})".into(),
            ],
            &w.catalog,
            &w.packages,
        )?
        .run(w.projects.root().join("m11-frontend-logs"))?;
    assert!(result.success);
    let node = result
        .logs
        .iter()
        .map(|r| r.text.as_str())
        .collect::<String>();
    assert!(node.contains("\"mail\":true"));
    eprintln!("M11_PHASE: structured logs exceptions and request/job correlation");
    let logs = w.logs_view(
        &a.id,
        &devforge_workspace::LogFilter {
            query: "M11_".into(),
            ..Default::default()
        },
    )?;
    assert!(logs["total"].as_u64().unwrap() >= 3);
    let encoded = logs.to_string();
    assert!(
        !encoded.contains("DO_NOT_STORE_CONTEXT") && !encoded.contains("DO_NOT_STORE_EXCEPTION")
    );
    assert!(encoded.contains("[REDACTED]"));
    assert!(
        logs["entries"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["kind"] == "exception" && e["data"]["line"].as_u64().is_some())
    );
    fs::write(
        a.path.join("app/Jobs/M11MailJob.php"),
        r#"<?php namespace App\Jobs; class M11MailJob implements \Illuminate\Contracts\Queue\ShouldQueue {use \Illuminate\Foundation\Bus\Dispatchable,\Illuminate\Queue\InteractsWithQueue,\Illuminate\Bus\Queueable,\Illuminate\Queue\SerializesModels;public function handle():void{\Illuminate\Support\Facades\Mail::raw('queue body',function($m){$m->to('queue@example.test')->subject('M11_QUEUE');});\Illuminate\Support\Facades\Log::notice('M11_QUEUE_LOG',['source'=>'queue']);}}"#,
    )?;
    php(&w, s.as_ref(), &a.id, r#"App\Jobs\M11MailJob::dispatch();"#)?;
    w.start(&a.id)?;
    let until = Instant::now() + Duration::from_secs(35);
    loop {
        if w.mail_view(&a.id, "M11_QUEUE", 0)?["inbox"]["total"] == 1 {
            break;
        }
        if Instant::now() > until {
            return Err("queue mail timed out".into());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(
        w.execute(
            Command::MailConfigure {
                id: a.id.clone(),
                enabled: false
            },
            &cancel
        )
        .is_err()
    );
    w.stop(&a.id)?;
    let filtered = w.logs_view(
        &a.id,
        &devforge_workspace::LogFilter {
            query: "M11_QUEUE_LOG".into(),
            level: "notice".into(),
            kind: "log".into(),
            ..Default::default()
        },
    )?;
    assert_eq!(filtered["total"], 1);
    assert!(filtered["entries"][0]["context"].as_str().is_some());
    let port = w.mail_view(&a.id, "", 0)?["port"].as_u64().unwrap();
    w.execute(
        Command::MailConfigure {
            id: a.id.clone(),
            enabled: false,
        },
        &cancel,
    )?;
    assert_eq!(w.mail_view(&a.id, "", 0)?["running"], false);
    w.execute(
        Command::MailConfigure {
            id: a.id.clone(),
            enabled: true,
        },
        &cancel,
    )?;
    assert_eq!(w.mail_view(&a.id, "", 0)?["port"], port);
    assert!(
        w.mail_view(&a.id, "", 0)?["inbox"]["total"]
            .as_u64()
            .unwrap()
            >= 2
    );
    assert_eq!(fs::read(a.path.join(".env")).ok(), before);
    let export = root.join("m11-devforge.yaml");
    let m = w.manifest_export(&a.id, Some(&export))?;
    assert_eq!(m.mail, Some(true));
    let manifest = fs::read_to_string(&export)?;
    let password =
        devforge_core::SecretStore::get(s.as_ref(), &devforge_projects::mail::reference(&a.id))?
            .unwrap();
    assert!(!manifest.contains(&password));
    fs::remove_file(export)?;
    let result = json!({"target":Target::current()?,"laravel_smtp_html_plain_attachments":true,"smtp_project_isolation":true,"attachment_export":true,"mail_restart_preserves_port_and_inbox":true,"frontend_credentials_filtered":true,"standalone_log_context_and_exception":true,"sensitive_context_redacted":true,"real_queue_mail_and_correlated_logs":true,"running_project_mail_toggle_refused":true,"environment_unchanged":true,"manifest_mail_boolean_no_credential":true,"project_id":a.id,"message_id":message});
    w.shutdown()?;
    fs::write(
        root.join("m11-native-result.json"),
        serde_json::to_vec_pretty(&result)?,
    )?;
    println!("M11_NATIVE_REPORT:{result}");
    Ok(())
}

pub fn send_ui(root: &Path, id: &str, port: &str) -> Result<()> {
    let data = root.join("data");
    let s = Arc::new(devforge_workspace::testing::FixtureSecrets::open(
        data.join("e2e-secrets.json"),
    ));
    let w = Workspace::open(&data, s.clone())?;
    let port: u16 = port.parse()?;
    let code=r#"Illuminate\Support\Facades\Mail::send([],[],function($m){$m->to('developer@example.test')->from('hello@example.test')->subject('M11_UI_ADDED');$m->getSymfonyMessage()->text('M11_UI_PLAIN')->html('<h1>M11_UI_HTML</h1><script>parent.__M11_EVIL=1</script><img src="http://127.0.0.1:TRAP/REMOTE_M11" onerror="parent.__M11_EVIL=2"><div style="background-image:url(http://127.0.0.1:TRAP/REMOTE_M11)">remote blocked</div>');$m->attachData('M11_UI_ATTACHMENT','note.txt',['mime'=>'text/plain']);});Illuminate\Support\Facades\Log::error('M11_UI_LOG',['api_token'=>'M11_UI_SECRET','order'=>'visible']);"#.replace("TRAP",&port.to_string());
    php(&w, s.as_ref(), id, &code)?;
    Ok(())
}
