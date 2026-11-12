use super::*;
fn late_ack_summary(
    w: &Workspace,
    secrets: &dyn devforge_core::SecretStore,
    id: &str,
) -> Result<()> {
    use std::{
        io::{BufRead, BufReader, Read, Write},
        net::TcpListener,
        time::{Duration, Instant},
    };
    let listener = TcpListener::bind("127.0.0.1:0")?;
    listener.set_nonblocking(true)?;
    let endpoint = format!(
        "http://127.0.0.1:{}/v1/events",
        listener.local_addr()?.port()
    );
    let server = std::thread::spawn(move || -> std::io::Result<Vec<serde_json::Value>> {
        let mut until = Instant::now() + Duration::from_secs(120);
        let mut batches = Vec::new();
        while batches.len() < 2 && Instant::now() < until {
            let (mut socket, _) = match listener.accept() {
                Ok(s) => s,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(5));
                    continue;
                }
                Err(e) => return Err(e),
            };
            socket.set_read_timeout(Some(Duration::from_secs(1)))?;
            let mut reader = BufReader::new(&mut socket);
            let mut size = 0;
            for _ in 0..64 {
                let mut line = String::new();
                reader.read_line(&mut line)?;
                if line == "\r\n" {
                    break;
                }
                if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                    size = value
                        .trim()
                        .parse::<usize>()
                        .map_err(std::io::Error::other)?;
                }
            }
            if size == 0 || size > 128 * 1024 {
                return Err(std::io::Error::other("invalid test batch"));
            }
            let mut body = vec![0; size];
            reader.read_exact(&mut body)?;
            batches.push(serde_json::from_slice(&body).map_err(std::io::Error::other)?);
            drop(reader);
            if batches.len() == 1 {
                until = Instant::now() + Duration::from_secs(5);
                std::thread::sleep(Duration::from_millis(120));
            }
            let _ = socket.write_all(
                b"HTTP/1.1 202 Accepted\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            );
        }
        Ok(batches)
    });
    let source = w
        .projects
        .get(id)?
        .path
        .join(".devforge/laravel-inspector/src/Capture.php");
    let result = exec(w,secrets,id,Tool::Php,vec!["-r".into(),r#"function config($key){return null;} require $argv[1]; $c=new DevForge\Inspector\Capture($argv[2],str_repeat('a',64),str_repeat('b',32)); $c->begin('request','late-ack'); $c->dump(['marker'=>'first-batch']); $c->finish(['status'=>200]);"#.into(), source.into_os_string(), endpoint.into()]);
    let batches = server
        .join()
        .map_err(|_| "late ACK fixture thread failed")??;
    result?;
    assert_eq!(batches.len(), 2, "lost final summary after late ACK");
    let summary = &batches[1]["events"][0];
    assert_eq!(summary["kind"], "request");
    assert_eq!(summary["data"]["truncated"], true);
    eprintln!("M7_PHASE: late acknowledgement retains a marked final summary");
    Ok(())
}
pub fn run(root: &Path) -> Result<()> {
    let data = root.join("data");
    let secrets = Arc::new(devforge_workspace::testing::FixtureSecrets::open(
        data.join("e2e-secrets.json"),
    ));
    let mut w = Workspace::open(&data, secrets.clone())?;
    let all = w.projects.list()?;
    let a = all
        .iter()
        .find(|p| p.name == "Atlas")
        .ok_or("Atlas fixture missing")?
        .clone();
    let b = all
        .iter()
        .find(|p| p.name == "Studio")
        .ok_or("Studio fixture missing")?
        .clone();
    for project in [&a, &b] {
        eprintln!("M7_PHASE: install companion {}", project.name);
        w.execute(
            Command::InspectorInstall {
                id: project.id.clone(),
            },
            &Cancellation::default(),
        )?;
        use std::io::Write;
        let mut routes = fs::OpenOptions::new()
            .append(true)
            .open(project.path.join("routes/web.php"))?;
        routes.write_all(br#"
\Illuminate\Support\Facades\Route::get('/inspector/{value}',function(){
 for($i=0;$i<6;$i++){\Illuminate\Support\Facades\DB::select('select * from m5_markers where value=?',['M7_PRIVATE_SENTINEL']);}
 \Illuminate\Support\Facades\DB::select('select pg_sleep(0.12)');
 \Illuminate\Support\Facades\DB::select("select 'M7_PRIVATE_SENTINEL' as value");
 \Illuminate\Support\Facades\Log::warning('token=M7_PRIVATE_SENTINEL');
 dump(['password'=>'M7_PRIVATE_SENTINEL','items'=>[1,2,3]]);
 return response()->json(['ok'=>true]);
});
\Illuminate\Support\Facades\Route::get('/inspector-dd',function(){dd(['secret'=>'M7_PRIVATE_SENTINEL','marker'=>'dump-exit']);});
\Illuminate\Support\Facades\Route::get('/inspector-error',function(){throw new \RuntimeException('token=M7_PRIVATE_SENTINEL');});
"#)?;
    }
    late_ack_summary(&w, secrets.as_ref(), &a.id)?;
    fs::write(a.path.join("app/Jobs/M7InspectorJob.php"),br#"<?php namespace App\Jobs;class M7InspectorJob implements \Illuminate\Contracts\Queue\ShouldQueue{use \Illuminate\Foundation\Bus\Dispatchable,\Illuminate\Queue\InteractsWithQueue,\Illuminate\Bus\Queueable,\Illuminate\Queue\SerializesModels;public function handle():void{\Illuminate\Support\Facades\DB::select('select 1');\Illuminate\Support\Facades\Log::info('M7 job completed');dump(['token'=>'M7_PRIVATE_SENTINEL','marker'=>'job-dump']);}}"#)?;
    w.execute(
        Command::InspectorCapture {
            id: a.id.clone(),
            enabled: true,
        },
        &Cancellation::default(),
    )?;
    w.start(&a.id)?;
    w.start(&b.id)?;
    let url = |w: &Workspace, id: &str| {
        w.snapshot()
            .unwrap()
            .projects
            .into_iter()
            .find(|r| r.project.id == id)
            .unwrap()
            .serving
            .unwrap()
            .preferred_url
    };
    let au = url(&w, &a.id);
    let bu = url(&w, &b.id);
    let get = |base: &str, path: &str| -> Result<(u16, String)> {
        let u = url::Url::parse(base)?;
        let client = reqwest::blocking::Client::builder()
            .no_proxy()
            .resolve(
                u.host_str().unwrap(),
                ([127, 0, 0, 1], u.port_or_known_default().unwrap()).into(),
            )
            .build()?;
        let response = client
            .get(format!("{}{path}", base.trim_end_matches('/')))
            .send()?;
        Ok((response.status().as_u16(), response.text()?))
    };
    assert_eq!(get(&au, "/inspector/M7_PRIVATE_SENTINEL")?.0, 200);
    assert_eq!(get(&bu, "/inspector/other")?.0, 200);
    let captured = w.inspector_view(&a.id)?;
    assert!(
        w.inspector_view(&b.id)?["capture"]["events"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    for kind in ["request", "query", "dump", "log"] {
        assert!(
            captured["capture"]["events"]
                .as_array()
                .unwrap()
                .iter()
                .any(|e| e["kind"] == kind),
            "missing {kind}"
        );
    }
    assert!(!captured.to_string().contains("M7_PRIVATE_SENTINEL"));
    let profiles = captured["profiles"].as_object().unwrap();
    assert!(profiles.values().any(|p| {
        p["patterns"]
            .as_array()
            .unwrap()
            .iter()
            .any(|q| q["repeated"] == true)
    }));
    assert!(profiles.values().any(|p| {
        p["patterns"]
            .as_array()
            .unwrap()
            .iter()
            .any(|q| q["slow"] == true)
    }));
    let _ = get(&au, "/inspector-dd")?;
    assert_eq!(get(&au, "/inspector-error")?.0, 500);
    exec(&w,secrets.as_ref(),&a.id,Tool::Php,vec!["-r".into(),r#"require 'vendor/autoload.php';$app=require 'bootstrap/app.php';$app->make(Illuminate\Contracts\Console\Kernel::class)->bootstrap();dispatch(new App\Jobs\M7InspectorJob);"#.into()])?;
    let until = std::time::Instant::now() + std::time::Duration::from_secs(20);
    loop {
        let view = w.inspector_view(&a.id)?;
        if view["capture"]["events"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["kind"] == "job")
        {
            break;
        }
        if std::time::Instant::now() > until {
            return Err("M7 job event timeout".into());
        }
        std::thread::sleep(std::time::Duration::from_millis(150));
    }
    let view = w.inspector_view(&a.id)?;
    assert!(!view.to_string().contains("M7_PRIVATE_SENTINEL"));
    assert!(
        view["capture"]["events"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["kind"] == "exception")
    );
    assert!(
        view["capture"]["events"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["kind"] == "dump" && e["data"]["marker"] == "dump-exit")
    );
    let a_pids = w
        .snapshot()?
        .projects
        .into_iter()
        .find(|r| r.project.id == a.id)
        .unwrap()
        .processes
        .into_iter()
        .map(|(n, s)| (n, s.pid))
        .collect::<Vec<_>>();
    w.execute(
        Command::InspectorCapture {
            id: b.id.clone(),
            enabled: true,
        },
        &Cancellation::default(),
    )?;
    assert_eq!(
        a_pids,
        w.snapshot()?
            .projects
            .into_iter()
            .find(|r| r.project.id == a.id)
            .unwrap()
            .processes
            .into_iter()
            .map(|(n, s)| (n, s.pid))
            .collect::<Vec<_>>()
    );
    assert_eq!(get(&url(&w, &b.id), "/inspector/other")?.0, 200);
    assert!(
        !w.inspector_view(&b.id)?["capture"]["events"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let capability = w.execute(
        Command::InspectorProbe { id: a.id.clone() },
        &Cancellation::default(),
    )?;
    w.execute(
        Command::InspectorClear { id: a.id.clone() },
        &Cancellation::default(),
    )?;
    assert!(
        w.inspector_view(&a.id)?["capture"]["events"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(
        !w.inspector_view(&b.id)?["capture"]["events"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    w.execute(
        Command::InspectorCapture {
            id: a.id.clone(),
            enabled: false,
        },
        &Cancellation::default(),
    )?;
    assert_eq!(get(&url(&w, &a.id), "/inspector/off")?.0, 200);
    assert!(
        w.inspector_view(&a.id)?["capture"]["events"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    w.shutdown()?;
    drop(w);
    let reopened = Workspace::open(&data, secrets)?;
    assert!(
        !reopened.inspector_view(&b.id)?["capture"]["events"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(reopened.inspector_view(&b.id)?["capture"]["enabled"], false);
    let report = json!({"host":{"os":std::env::consts::OS,"arch":std::env::consts::ARCH},"late_ack_preserves_marked_final_summary":true,"real_laravel_http_sql_log_dump_job_exception":true,"dd_preserves_exit_and_records_dump":true,"normalized_sql_slow_and_repeated_hints":true,"sentinel_redacted":true,"project_isolation_and_scoped_clear":true,"enable_B_preserves_A_pids":true,"pause_stops_capture":true,"history_persists_capture_off_after_reopen":true,"php_capability":capability,"passed":true});
    fs::write(
        root.join("m7-native-result.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("M7_NATIVE_REPORT:{report}");
    Ok(())
}
