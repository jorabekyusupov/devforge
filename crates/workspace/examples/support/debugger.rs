use super::*;
use devforge_debugger::{self as debug, Breakpoint, Mode, Settings};
use devforge_workspace::debugging::Control;
use std::time::{Duration, Instant};
fn wait(
    w: &Workspace,
    id: &str,
    predicate: impl Fn(&serde_json::Value) -> bool,
) -> Result<serde_json::Value> {
    let until = Instant::now() + Duration::from_secs(30);
    loop {
        let v = w.debug_view(id)?;
        if predicate(&v) {
            return Ok(v);
        }
        if Instant::now() > until {
            return Err(format!("debug wait expired: {v}").into());
        }
        std::thread::sleep(Duration::from_millis(30));
    }
}
fn mode(w: &mut Workspace, id: &str, runtime: Option<String>, mode: Mode) -> Result<()> {
    let p = w.projects.get(id)?;
    w.execute(
        Command::DebugConfigure {
            id: id.into(),
            revision: p.revision,
            settings: Settings {
                runtime,
                mode,
                breakpoints: p.debugger.breakpoints,
            },
            consent: true,
        },
        &Default::default(),
    )?;
    Ok(())
}
fn control(w: &mut Workspace, id: &str, control: Control) -> Result<()> {
    w.execute(
        Command::DebugControl {
            id: id.into(),
            control,
        },
        &Default::default(),
    )?;
    Ok(())
}
fn request(w: &mut Workspace, id: &str) -> Result<()> {
    w.execute(
        Command::DebugRequest {
            id: id.into(),
            path: "/m7-debug".into(),
        },
        &Default::default(),
    )?;
    Ok(())
}
fn pids(w: &Workspace, id: &str) -> Result<Vec<(String, Option<u32>)>> {
    Ok(w.snapshot()?
        .projects
        .into_iter()
        .find(|p| p.project.id == id)
        .unwrap()
        .processes
        .into_iter()
        .map(|(id, s)| (id, s.pid))
        .collect())
}
pub fn run(root: &Path, archives: &Path) -> Result<()> {
    let data = root.join("data");
    let secrets = Arc::new(devforge_workspace::testing::FixtureSecrets::open(
        data.join("e2e-secrets.json"),
    ));
    let mut w = Workspace::open(&data, secrets.clone())?;
    let cancel = Cancellation::default();
    let a = w
        .projects
        .list()?
        .into_iter()
        .find(|p| p.name == "Atlas")
        .unwrap();
    let b = w
        .projects
        .list()?
        .into_iter()
        .find(|p| p.name == "Studio")
        .unwrap();
    let env_before = fs::read(a.path.join(".env")).ok();
    let selected = a.php.clone();
    let normal = Context::resolve(
        &w.projects,
        &a.id,
        &w.catalog,
        &w.packages,
        secrets.as_ref(),
    )?
    .php_path()
    .to_owned();
    let mut runtimes = vec![];
    for version in ["8.4.27", "8.5.12"] {
        eprintln!("M7_XDEBUG_PHASE: install and probe {version}");
        let e = if Target::current()? == Target::WindowsX64 {
            debug::provider::install(
                w.projects.root(),
                version,
                &w.packages,
                secrets.as_ref(),
                true,
                &cancel,
            )?
        } else {
            debug::provider::install_fixture_archive(
                w.projects.root(),
                version,
                &archives.join(format!("debug-{version}.zip")),
                &w.packages,
                secrets.as_ref(),
                &cancel,
            )?
        };
        let resolved = debug::provider::resolve(
            w.projects.root(),
            &e.id,
            version,
            &w.packages,
            secrets.as_ref(),
        )?;
        let php = if let Some(php) = &resolved.php {
            php.path().to_owned()
        } else {
            let package = w
                .catalog
                .packages
                .values()
                .find(|p| {
                    p.kind == "php"
                        && p.target == Target::current().unwrap()
                        && p.version.starts_with(version)
                        && p.executables.contains_key("php")
                })
                .unwrap();
            w.packages.executable(package, "php")?.path().to_owned()
        };
        let output=std::process::Command::new(&php).args(["-n","-d"]).arg(format!("zend_extension={}",resolved.extension.path().display())).args(["-r","echo json_encode(['php'=>PHP_VERSION,'xdebug'=>phpversion('xdebug'),'pdo'=>PDO::getAvailableDrivers()]);"]).output()?;
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let probe: serde_json::Value = serde_json::from_slice(&output.stdout)?;
        assert_eq!(probe["php"], version);
        assert_eq!(probe["xdebug"], debug::provider::XDEBUG);
        if resolved.php.is_some() {
            for driver in ["mysql", "pgsql", "sqlite"] {
                assert!(probe["pdo"].as_array().unwrap().contains(&json!(driver)));
            }
        }
        runtimes.push((e, probe));
    }
    assert!(
        w.execute(
            Command::DebugInstall {
                id: a.id.clone(),
                trusted: false
            },
            &cancel
        )
        .is_err()
    );
    let reused = w.execute(
        Command::DebugInstall {
            id: a.id.clone(),
            trusted: true,
        },
        &cancel,
    )?;
    assert!(reused["installed"].as_bool().unwrap());
    let runtime = runtimes
        .iter()
        .find(|(e, _)| e.php == "8.5.12")
        .unwrap()
        .0
        .id
        .clone();
    fs::write(
        a.path.join("app/M7DebugFixture.php"),
        "<?php\nnamespace App;\nclass M7DebugFixture {\n public static function work($input) {\n  $api_token='M7_PRIVATE_SENTINEL';\n  $label='M7_VISIBLE_LOCAL';\n  $base=$input;for($i=0;$i<10000;$i++){$base+=0;}\n  $output=self::multiply($base);\n  return $output;\n }\n public static function multiply($value) {\n  $result=$value*3;\n  return $result;\n }\n}\n",
    )?;
    let mut routes = fs::read_to_string(a.path.join("routes/web.php"))?;
    if !routes.contains("/m7-debug") {
        routes.push_str("\nIlluminate\\Support\\Facades\\Route::get('/m7-debug',fn()=>response()->json(['result'=>App\\M7DebugFixture::work(7),'xdebug'=>phpversion('xdebug'),'database'=>Illuminate\\Support\\Facades\\DB::selectOne('SELECT current_database() AS value')->value]));\n");
        fs::write(a.path.join("routes/web.php"), routes)?;
    }
    let current = w.projects.get(&a.id)?;
    devforge_projects::environment::set(
        &w.projects,
        secrets.as_ref(),
        &a.id,
        current.revision,
        "M7_PRIVATE_VALUE",
        "M7_PRIVATE_SENTINEL",
    )?;
    w.start(&b.id)?;
    let other_pids = pids(&w, &b.id)?;
    mode(&mut w, &a.id, Some(runtime.clone()), Mode::Debug)?;
    let p = w.projects.get(&a.id)?;
    w.execute(
        Command::DebugBreakpoints {
            id: a.id.clone(),
            revision: p.revision,
            breakpoints: vec![Breakpoint {
                file: "app/M7DebugFixture.php".into(),
                line: 8,
            }],
        },
        &cancel,
    )?;
    w.start(&a.id)?;
    assert!(
        w.execute(
            Command::DebugConfigure {
                id: a.id.clone(),
                revision: w.projects.get(&a.id)?.revision,
                settings: Settings::default(),
                consent: true
            },
            &cancel
        )
        .is_err()
    );
    assert_eq!(w.debug_view(&b.id)?["settings"]["mode"], "off");
    request(&mut w, &a.id)?;
    wait(&w, &a.id, |v| v["session"]["state"] == "paused")?;
    control(&mut w, &a.id, Control::Run)?;
    let at = wait(&w, &a.id, |v| {
        v["session"]["state"] == "paused"
            && v["session"]["frames"].as_array().is_some_and(|f| {
                f.iter()
                    .any(|f| f["file"] == "app/M7DebugFixture.php" && f["line"] == 8)
            })
    })?;
    assert!(at.to_string().contains("M7_VISIBLE_LOCAL"));
    assert!(at.to_string().contains("REDACTED"));
    assert!(!at.to_string().contains("M7_PRIVATE_SENTINEL"));
    control(&mut w, &a.id, Control::StepInto)?;
    wait(&w, &a.id, |v| {
        v["session"]["state"] == "paused"
            && v["session"]["frames"][0]["function"]
                .as_str()
                .is_some_and(|s| s.contains("multiply"))
    })?;
    control(&mut w, &a.id, Control::StepOver)?;
    wait(&w, &a.id, |v| {
        v["session"]["state"] == "paused" && v["session"]["frames"][0]["line"] == 13
    })?;
    control(&mut w, &a.id, Control::StepOut)?;
    wait(&w, &a.id, |v| {
        v["session"]["state"] == "paused" && v["session"]["frames"][0]["line"] == 9
    })?;
    control(&mut w, &a.id, Control::Run)?;
    let completed = wait(&w, &a.id, |v| v["request"]["state"] == "completed")?;
    assert_eq!(completed["request"]["status"], 200);
    eprintln!("M7_XDEBUG_PHASE: stop a paused request and keep the web runtime alive");
    request(&mut w, &a.id)?;
    wait(&w, &a.id, |v| v["session"]["state"] == "paused")?;
    control(&mut w, &a.id, Control::Run)?;
    wait(&w, &a.id, |v| {
        v["session"]["state"] == "paused" && v["session"]["frames"][0]["line"] == 8
    })?;
    control(&mut w, &a.id, Control::Stop)?;
    wait(&w, &a.id, |v| {
        v["session"]["state"] == "listening" && v["request"]["state"] != "running"
    })?;
    let row = w
        .snapshot()?
        .projects
        .into_iter()
        .find(|r| r.project.id == a.id)
        .unwrap();
    assert_eq!(
        get(&(row.serving.unwrap().fallback_url + "/m7-debug"))?["result"],
        21
    );

    assert!(
        w.execute(
            Command::DebugControl {
                id: b.id.clone(),
                control: Control::Run
            },
            &cancel
        )
        .is_err()
    );
    assert!(w.debug_source(&a.id, "../outside.php").is_err());
    assert!(
        !w.debug_source(&a.id, "app/M7DebugFixture.php")?
            .to_string()
            .contains("M7_PRIVATE_SENTINEL")
    );
    assert_eq!(pids(&w, &b.id)?, other_pids);
    w.stop(&a.id)?;
    eprintln!("M7_XDEBUG_PHASE: actual HTTP and CLI function profiles");
    mode(&mut w, &a.id, Some(runtime.clone()), Mode::Profile)?;
    w.start(&a.id)?;
    request(&mut w, &a.id)?;
    let view = wait(&w, &a.id, |v| {
        v["request"]["state"] == "completed"
            && v["profiles"].as_array().is_some_and(|a| !a.is_empty())
    })?;
    assert_eq!(view["request"]["status"], 200);
    let key = view["profiles"][0]["id"].as_str().unwrap().to_owned();
    let profile = w.debug_profile(&a.id, &key)?;
    assert!(profile["total_ms"].as_f64().unwrap() > 0.0);
    assert!(
        profile["functions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f["name"].as_str().unwrap().contains("M7DebugFixture"))
    );
    assert!(w.debug_profile(&b.id, &key).is_err());
    let destination = root.join("m7-function-profile.out");
    if destination.exists() {
        fs::remove_file(&destination)?;
    }
    w.execute(
        Command::DebugProfileExport {
            id: a.id.clone(),
            profile: key.clone(),
            path: destination.clone(),
        },
        &cancel,
    )?;
    assert!(fs::metadata(&destination)?.len() > 0);
    assert!(
        w.execute(
            Command::DebugProfileExport {
                id: a.id.clone(),
                profile: key.clone(),
                path: destination.clone()
            },
            &cancel
        )
        .is_err()
    );
    fs::remove_file(destination)?;
    let invocation = Context::resolve(
        &w.projects,
        &a.id,
        &w.catalog,
        &w.packages,
        secrets.as_ref(),
    )?
    .debug_trigger()?
    .invocation(
        Tool::Php,
        vec![
            "-r".into(),
            "require 'vendor/autoload.php';echo App\\M7DebugFixture::work(7);".into(),
        ],
        &w.catalog,
        &w.packages,
    )?;
    let output = invocation.run(w.projects.root().join("m7-cli-profile"))?;
    assert!(output.success);
    assert!(output.logs.iter().any(|l| l.text.trim() == "21"));
    w.execute(
        Command::DebugProfileDelete {
            id: a.id.clone(),
            profile: key.clone(),
            consent: true,
        },
        &cancel,
    )?;
    assert!(w.debug_profile(&a.id, &key).is_err());
    assert_eq!(pids(&w, &b.id)?, other_pids);
    w.stop(&a.id)?;
    mode(&mut w, &a.id, None, Mode::Off)?;
    assert_eq!(
        Context::resolve(
            &w.projects,
            &a.id,
            &w.catalog,
            &w.packages,
            secrets.as_ref()
        )?
        .php_path(),
        normal
    );
    assert_eq!(w.projects.get(&a.id)?.php, selected);
    assert_eq!(fs::read(a.path.join(".env")).ok(), env_before);
    eprintln!("M7_XDEBUG_PHASE: PHP8.4 native web runtime and recovery");
    let php84 = w
        .catalog
        .packages
        .iter()
        .find(|(_, p)| {
            p.kind == "php"
                && p.version.starts_with("8.4.27")
                && p.target == Target::current().unwrap()
                && p.executables.contains_key("php")
        })
        .unwrap()
        .0
        .clone();
    let configure_php = |w: &mut Workspace, php: String| -> Result<()> {
        let p = w.projects.get(&a.id)?;
        let row = w
            .snapshot()?
            .projects
            .into_iter()
            .find(|r| r.project.id == a.id)
            .unwrap();
        let site = row.site.unwrap();
        w.execute(
            Command::Configure {
                id: a.id.clone(),
                revision: p.revision,
                name: p.name,
                php,
                node: p.node,
                domain: site.domain,
                https: site.https,
                vite: site.vite,
                queue: row.development.queue,
            },
            &cancel,
        )?;
        Ok(())
    };
    configure_php(&mut w, php84)?;
    let runtime84 = runtimes
        .iter()
        .find(|(e, _)| e.php == "8.4.27")
        .unwrap()
        .0
        .id
        .clone();
    mode(&mut w, &a.id, Some(runtime84), Mode::Profile)?;
    w.start(&a.id)?;
    request(&mut w, &a.id)?;
    let v = wait(&w, &a.id, |v| v["request"]["state"] == "completed")?;
    assert_eq!(v["request"]["status"], 200);
    assert_eq!(pids(&w, &b.id)?, other_pids);
    w.stop(&a.id)?;
    mode(&mut w, &a.id, None, Mode::Off)?;
    configure_php(&mut w, selected.clone())?;
    w.start(&a.id)?;
    let row = w
        .snapshot()?
        .projects
        .into_iter()
        .find(|p| p.project.id == a.id)
        .unwrap();
    assert_eq!(
        get(&(row.serving.unwrap().fallback_url + "/m7-debug"))?["result"],
        21
    );
    assert_eq!(pids(&w, &b.id)?, other_pids);
    w.stop(&a.id)?;
    mode(&mut w, &a.id, Some(runtime.clone()), Mode::Debug)?;
    let source = a.path.join("app/M7DebugFixture.php");
    let moved = a.path.join("app/M7DebugFixture.php.disabled");
    fs::rename(&source, &moved)?;
    mode(&mut w, &a.id, None, Mode::Off)?;
    fs::rename(&moved, &source)?;
    w.shutdown()?;
    let report = json!({"passed":true,"host":{"os":std::env::consts::OS,"arch":std::env::consts::ARCH},"project_id":a.id,"other_project_id":b.id,"runtime_id":runtime,"php_probes":runtimes.iter().map(|(_,p)|p).collect::<Vec<_>>(),"checks":{"verified_provider_install":true,"real_laravel_http_breakpoint":true,"step_into_over_out":true,"stop_paused_request_preserves_web_runtime":true,"locals_and_source_redacted":true,"request_http_200":true,"profile_http_and_cli":true,"php84_native_web":true,"profile_parse_export_delete":true,"cross_project_denial":true,"other_process_pids_unchanged":true,"off_restores_normal_php":true,"off_after_source_deleted":true,"project_php_pin_and_env_unchanged":true,"shutdown_cleans_own_sessions":true}});
    fs::write(
        root.join("m7-xdebug-native-result.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("M7_XDEBUG_REPORT:{report}");
    Ok(())
}

pub fn cli(root: &Path, executable: &Path) -> Result<()> {
    let native: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join("m7-xdebug-native-result.json"))?)?;
    let id = native["project_id"].as_str().unwrap();
    let runtime = native["runtime_id"].as_str().unwrap();
    let run = |args: &[&str]| -> Result<serde_json::Value> {
        let output = std::process::Command::new(executable)
            .args(args)
            .env("DEVFORGE_DATA_DIR", root.join("data"))
            .env("DEVFORGE_TEST_SECRETS", "1")
            .output()?;
        assert!(
            output.status.success(),
            "CLI: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(!String::from_utf8_lossy(&output.stdout).contains("M7_PRIVATE_SENTINEL"));
        Ok(serde_json::from_slice(&output.stdout)?)
    };
    let status = run(&["debug", "status", id])?;
    assert_eq!(status["schema"], 1);
    assert_eq!(status["result"]["settings"]["mode"], "off");
    run(&["debug", "source", id, "app/M7DebugFixture.php"])?;
    let profile = status["result"]["profiles"][0]["id"]
        .as_str()
        .ok_or("retained CLI profile missing")?;
    let parsed = run(&["debug", "profile", id, profile])?;
    assert!(!parsed["result"]["functions"].as_array().unwrap().is_empty());
    run(&["debug", "mode", id, "debug", runtime])?;
    assert_eq!(
        run(&["debug", "status", id])?["result"]["settings"]["mode"],
        "debug"
    );
    run(&["debug", "mode", id, "off", "none"])?;
    let report = json!({"target":Target::current()?,"stable_json_status_source_profile":true,"project_mode_and_off_recovery":true,"known_secret_absent":true});
    fs::write(
        root.join("m7-xdebug-cli-result.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("M7_XDEBUG_CLI_REPORT:{report}");
    Ok(())
}
