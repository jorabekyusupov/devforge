//! M10 real CLI acceptance across two isolated machine stores and repository paths.
use devforge_core::SecretStore;
use devforge_projects::{
    execution::{Context, Tool},
    manifest::Manifest,
    project::Catalog,
};
use devforge_runtime::{Cancellation, package::Target};
use devforge_workspace::{Command, Workspace, testing::FixtureSecrets};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command as Process, Stdio},
    sync::Arc,
};
#[cfg(windows)]
#[path = "../../projects/examples/support/windows_unprivileged.rs"]
mod windows_unprivileged;
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
fn cli(
    exe: &Path,
    data: &Path,
    args: &[String],
    input: Option<&str>,
    success: bool,
) -> Result<serde_json::Value> {
    let mut child = Process::new(exe)
        .args(args)
        .env("DEVFORGE_DATA_DIR", data)
        .env("DEVFORGE_TEST_SECRETS", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    if let Some(v) = input {
        child.stdin.take().unwrap().write_all(v.as_bytes())?;
    } else {
        child.stdin.take();
    }
    let output = child.wait_with_output()?;
    if output.status.success() != success {
        return Err(format!(
            "CLI {} failed unexpectedly: {}",
            args[0..2].join(" "),
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    Ok(serde_json::from_slice(if success {
        &output.stdout
    } else {
        &output.stderr
    })?)
}
fn command(
    exe: &Path,
    data: &Path,
    action: &str,
    path: &Path,
    tail: &[&str],
    input: Option<&str>,
    success: bool,
) -> Result<serde_json::Value> {
    let mut args = vec![
        "config".into(),
        action.into(),
        dunce::simplified(path).to_string_lossy().into_owned(),
    ];
    args.extend(tail.iter().map(|s| s.to_string()));
    cli(exe, data, &args, input, success)
}
fn open(data: &Path) -> Result<(Workspace, Arc<FixtureSecrets>)> {
    let s = Arc::new(FixtureSecrets::open(data.join("e2e-secrets.json")));
    Ok((Workspace::open(data, s.clone())?, s))
}
fn php(w: &Workspace, secrets: &dyn SecretStore, id: &str, code: &str) -> Result<String> {
    let output = Context::resolve(&w.projects, id, &w.catalog, &w.packages, secrets)?
        .invocation(
            Tool::Php,
            vec!["-r".into(), code.into()],
            &w.catalog,
            &w.packages,
        )?
        .run(w.projects.root().join("m10-test-logs"))?;
    if !output.success {
        return Err("PHP acceptance failed (payload suppressed)".into());
    }
    Ok(output.logs.iter().map(|l| l.text.clone()).collect())
}
fn copy(from: &Path, to: &Path, repository: bool) -> Result<()> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let e = entry?;
        let name = e.file_name();
        if repository
            && ["vendor", "node_modules", ".env", ".git"]
                .iter()
                .any(|v| name == *v)
        {
            continue;
        }
        if e.path().is_dir() {
            copy(&e.path(), &to.join(name), repository)?;
        } else {
            fs::copy(e.path(), to.join(name))?;
        }
    }
    Ok(())
}
fn key(path: &Path) -> Result<String> {
    Ok(fs::read_to_string(path.join(".env"))?
        .lines()
        .find(|l| l.starts_with("APP_KEY="))
        .ok_or("key missing")?
        .to_string())
}
fn main() -> Result<()> {
    #[cfg(windows)]
    if let Some(code) = windows_unprivileged::relaunch()? {
        std::process::exit(code);
    }
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let exe = fs::canonicalize(args.first().ok_or("CLI required")?)?;
    let cache = PathBuf::from(args.get(1).ok_or("cache required")?);
    let report = PathBuf::from(args.get(2).ok_or("report required")?);
    let temp = tempfile::tempdir()?;
    let result = run(temp.path(), &exe, &cache);
    match result {
        Ok(value) => {
            fs::write(report, serde_json::to_vec_pretty(&value)?)?;
            println!("M10_NATIVE_REPORT:{value}");
            Ok(())
        }
        Err(e) => Err(e),
    }
}
fn run(root: &Path, exe: &Path, cache: &Path) -> Result<serde_json::Value> {
    let a = root.join("pc-a");
    let b = root.join("pc-b");
    let (mut w, secrets) = open(&a)?;
    let target = Target::current()?;
    let catalog = Catalog::bundled();
    let choose = |kind: &str, version: &str| {
        catalog
            .packages
            .iter()
            .find(|(_, p)| {
                p.target == target
                    && p.kind == kind
                    && (if kind == "php" {
                        p.version.split('-').next().unwrap()
                    } else {
                        p.version.trim_start_matches('v')
                    }) == version
                    && (kind != "php" || p.executables.contains_key("php"))
                    && (kind != "postgresql" || p.variant == "server")
            })
            .unwrap()
            .0
            .clone()
    };
    let php_id = choose("php", "8.5.12");
    let node_id = choose("node", "24.22.2");
    let candidates: Vec<serde_json::Value> =
        serde_json::from_str(include_str!("../../../runtime/m0-candidates.json"))?;
    let selected = catalog
        .packages
        .iter()
        .filter(|(_, p)| {
            p.target == target
                && ((p.kind == "php" && p.version.split('-').next().unwrap() == "8.5.12")
                    || (p.kind == "node" && p.version == "v24.22.2")
                    || p.kind == "composer"
                    || p.kind == "caddy"
                    || (p.kind == "postgresql" && p.variant == "server")
                    || (p.kind == "garnet" && p.version == "2.2.11")
                    || p.kind == "dotnet")
        })
        .map(|(id, p)| (id.clone(), p.clone()))
        .collect::<Vec<_>>();
    for (id, p) in selected {
        eprintln!("M10_INSTALL:{id}");
        let archived = candidates
            .iter()
            .find(|c| c["url"].as_str() == Some(&p.url))
            .map(|c| {
                cache.join(format!(
                    "{}-{}",
                    c["id"].as_str().unwrap(),
                    p.url.rsplit('/').next().unwrap()
                ))
            });
        if let Some(path) = archived.filter(|p| p.is_file()) {
            w.packages
                .install_cached(&p, &path, &Cancellation::default(), |_| {})?;
        } else {
            w.packages.download(&p, &Cancellation::default(), |_| {})?;
        }
    }
    let tools = root.join("tools");
    fs::create_dir_all(&tools)?;
    let project = root.join("Source Ўзбек & ' repo");
    let bootstrap = Context::bootstrap(
        &w.projects,
        &tools,
        &php_id,
        &node_id,
        &w.catalog,
        &w.packages,
        secrets.as_ref(),
    )?;
    let created = bootstrap
        .invocation(
            Tool::Composer,
            vec![
                "create-project".into(),
                "laravel/laravel".into(),
                dunce::simplified(&project).as_os_str().into(),
                "v13.11.0".into(),
                "--no-dev".into(),
                "--no-scripts".into(),
                "--no-plugins".into(),
                "--no-interaction".into(),
            ],
            &w.catalog,
            &w.packages,
        )?
        .run(w.projects.root().join("create-logs"))?;
    assert!(created.success, "Laravel creation failed");
    let p = w
        .projects
        .import(&project, "Portable", &php_id, &node_id, &w.catalog)?;
    w.sites.configure(
        &w.projects,
        devforge_serving::site::Site {
            project_id: p.id.clone(),
            domain: "portable.devforge.test".into(),
            https: false,
            vite: false,
        },
    )?;
    fs::write(project.join(".env"), "UNRELATED=preserve\nAPP_KEY=\n")?;
    devforge_projects::env_file::EnvPlan::application(&w.projects, &p.id, false, true)?
        .unwrap()
        .apply(&w.projects)?;
    devforge_projects::environment::set(
        &w.projects,
        secrets.as_ref(),
        &p.id,
        p.revision,
        "API_TOKEN",
        "SOURCE_PRIVATE_TOKEN",
    )?;
    w.manifest_export(&p.id, None)?;
    let config = project.join("devforge.yaml");
    let mut m = Manifest::parse(&fs::read(&config)?)?;
    m.database = Some(devforge_projects::manifest::Database {
        engine: devforge_projects::database::Engine::Postgresql,
        version: "18.6.0".into(),
    });
    m.redis = Some(devforge_projects::manifest::Redis {
        version: "2.2.11".into(),
        dotnet: "10.0.15".into(),
    });
    m.services.cache = devforge_projects::cache::CacheDriver::Redis;
    m.services.queue = devforge_projects::cache::QueueDriver::Redis;
    m.php_settings.memory_limit = Some("512M".into());
    m.setup.composer = true;
    m.setup.npm = true;
    m.setup.create_env = true;
    m.setup.generate_key = true;
    m.setup.predis = true;
    m.pin_all(&catalog)?;
    fs::write(&config, m.yaml()?)?;
    let yaml = fs::read_to_string(&config)?;
    assert!(
        !yaml.contains("SOURCE_PRIVATE_TOKEN")
            && !yaml.contains(&p.id)
            && !yaml.contains(&root.to_string_lossy().to_string())
    );
    let key_before = key(&project)?;
    w.shutdown()?;
    drop(w);
    drop(secrets);
    eprintln!("M10_PHASE: reviewed setup and no-script dependency execution");
    let plan = command(exe, &a, "plan", &project, &["--write-env"], None, true)?;
    assert!(!plan.to_string().contains("SOURCE_PRIVATE_TOKEN"));
    let digest = plan["digest"].as_str().unwrap();
    let setup = command(
        exe,
        &a,
        "setup",
        &project,
        &["--review", digest, "--write-env", "--trust"],
        None,
        true,
    )?;
    assert_eq!(setup["completed"], true);
    assert_eq!(key(&project)?, key_before);
    let (mut w, s) = open(&a)?;
    let active = w.projects.binding(&p.id)?.unwrap();
    let cache_id = w.projects.get(&p.id)?.services.redis_instance.unwrap();
    w.execute(
        Command::TestDatabase { id: p.id.clone() },
        &Default::default(),
    )?;
    w.execute(
        Command::StartCache {
            instance: cache_id.clone(),
        },
        &Default::default(),
    )?;
    let code = r#"require 'vendor/autoload.php';$app=require 'bootstrap/app.php';$app->make(Illuminate\Contracts\Console\Kernel::class)->bootstrap();Illuminate\Support\Facades\DB::statement('CREATE TABLE m10_sentinel (value text)');Illuminate\Support\Facades\DB::insert("INSERT INTO m10_sentinel VALUES ('keep')");Illuminate\Support\Facades\Cache::put('m10','cache',60);echo json_encode(['secret'=>getenv('API_TOKEN')==='SOURCE_PRIVATE_TOKEN','cache'=>Illuminate\Support\Facades\Cache::get('m10')==='cache','driver'=>Illuminate\Support\Facades\DB::connection()->getDriverName()]);"#;
    let output = php(&w, s.as_ref(), &p.id, code)?;
    assert!(output.contains("\"secret\":true") && output.contains("pgsql"));
    fs::write(
        project.join("public/m10.php"),
        r#"<?php header('Content-Type: application/json');echo json_encode(['php'=>PHP_VERSION,'token'=>getenv('API_TOKEN')==='SOURCE_PRIVATE_TOKEN','memory'=>ini_get('memory_limit')]);"#,
    )?;
    w.start(&p.id)?;
    let served = w
        .snapshot()?
        .projects
        .into_iter()
        .find(|r| r.project.id == p.id)
        .unwrap()
        .serving
        .unwrap();
    let url = url::Url::parse(&format!(
        "{}/m10.php",
        served.http_url.trim_end_matches('/')
    ))?;
    let client = reqwest::blocking::Client::builder()
        .no_proxy()
        .resolve(
            url.host_str().unwrap(),
            ([127, 0, 0, 1], url.port_or_known_default().unwrap()).into(),
        )
        .build()?;
    let web: serde_json::Value = client.get(url).send()?.error_for_status()?.json()?;
    assert_eq!(web["token"], true);
    assert_eq!(web["memory"], "512M");
    assert!(web["php"].as_str().unwrap().starts_with("8.5.12"));
    w.stop(&p.id)?;
    fs::remove_file(project.join("public/m10.php"))?;
    w.shutdown()?;
    drop(w);
    drop(s);
    let marker = project.join(".m10-script-ran");
    let mut composer: serde_json::Value =
        serde_json::from_slice(&fs::read(project.join("composer.json"))?)?;
    composer["scripts"]["post-install-cmd"] =
        serde_json::json!(["@php -r \"file_put_contents('.m10-script-ran','unsafe');\""]);
    fs::write(
        project.join("composer.json"),
        serde_json::to_vec_pretty(&composer)?,
    )?;
    let mut package: serde_json::Value =
        serde_json::from_slice(&fs::read(project.join("package.json"))?)?;
    package["scripts"]["postinstall"] = serde_json::json!(
        "node -e \"require('fs').writeFileSync('.m10-npm-script-ran','unsafe')\""
    );
    fs::write(
        project.join("package.json"),
        serde_json::to_vec_pretty(&package)?,
    )?;
    eprintln!("M10_PHASE: rerun preserves database cache instance and APP_KEY");
    let plan = command(exe, &a, "plan", &project, &["--write-env"], None, true)?;
    command(
        exe,
        &a,
        "setup",
        &project,
        &[
            "--review",
            plan["digest"].as_str().unwrap(),
            "--write-env",
            "--trust",
        ],
        None,
        true,
    )?;
    assert!(!marker.exists());
    assert!(!project.join(".m10-npm-script-ran").exists());
    let (mut w, s) = open(&a)?;
    assert_eq!(
        w.projects.binding(&p.id)?.unwrap().database,
        active.database
    );
    assert_eq!(
        w.projects.get(&p.id)?.services.redis_instance.as_deref(),
        Some(cache_id.as_str())
    );
    w.execute(
        Command::TestDatabase { id: p.id.clone() },
        &Default::default(),
    )?;
    assert!(php(&w,s.as_ref(),&p.id,r#"require 'vendor/autoload.php';$app=require 'bootstrap/app.php';$app->make(Illuminate\Contracts\Console\Kernel::class)->bootstrap();echo Illuminate\Support\Facades\DB::table('m10_sentinel')->value('value');"#)?.contains("keep"));
    w.shutdown()?;
    drop(w);
    drop(s);
    eprintln!("M10_PHASE: stale reviews, cancellation and dependency recovery");
    let stale = command(exe, &a, "plan", &project, &["--write-env"], None, true)?;
    command(
        exe,
        &a,
        "secret",
        &project,
        &["api_token"],
        Some("SOURCE_ROTATED"),
        true,
    )?;
    let rejected = command(
        exe,
        &a,
        "setup",
        &project,
        &[
            "--review",
            stale["digest"].as_str().unwrap(),
            "--write-env",
            "--trust",
        ],
        None,
        false,
    )?;
    assert_eq!(rejected["error"]["code"], "manifest_review_changed");
    let lock = project.join("composer.lock");
    let lock_before = fs::read(&lock)?;
    fs::write(&lock, "INVALID JSON")?;
    let plan = command(exe, &a, "plan", &project, &["--write-env"], None, true)?;
    assert_eq!(
        command(
            exe,
            &a,
            "setup",
            &project,
            &[
                "--review",
                plan["digest"].as_str().unwrap(),
                "--write-env",
                "--trust"
            ],
            None,
            false
        )?["error"]["code"],
        "manifest_dependency_failed"
    );
    assert_eq!(
        command(exe, &a, "status", &project, &[], None, true)?["stage"],
        "dependencies"
    );
    fs::write(&lock, lock_before)?;
    let (mut w, _) = open(&a)?;
    let plan = w.manifest_plan(&project, true)?;
    let cancel = Cancellation::default();
    cancel.cancel();
    assert_eq!(
        w.manifest_setup(&project, &plan.digest, true, true, &cancel)
            .err()
            .unwrap()
            .code,
        "setup_cancelled"
    );
    assert_eq!(
        w.projects.binding(&p.id)?.unwrap().database,
        active.database
    );
    drop(w);
    let plan = command(exe, &a, "plan", &project, &["--write-env"], None, true)?;
    command(
        exe,
        &a,
        "setup",
        &project,
        &[
            "--review",
            plan["digest"].as_str().unwrap(),
            "--write-env",
            "--trust",
        ],
        None,
        true,
    )?;
    eprintln!("M10_PHASE: second machine uses the same team pins and different local secrets");
    let clone = root.join("Clone Русский & ' repo");
    copy(&project, &clone, true)?;
    fs::create_dir_all(&b)?;
    copy(&a.join("runtimes"), &b.join("runtimes"), false)?;
    let plan = command(exe, &b, "plan", &clone, &["--write-env"], None, true)?;
    assert_eq!(plan["blockers"][0], "manifest_secret_missing");
    command(
        exe,
        &b,
        "secret",
        &clone,
        &["api_token"],
        Some("TARGET_PRIVATE_TOKEN"),
        true,
    )?;
    let no_write = command(exe, &b, "plan", &clone, &[], None, true)?;
    command(
        exe,
        &b,
        "setup",
        &clone,
        &["--review", no_write["digest"].as_str().unwrap(), "--trust"],
        None,
        true,
    )?;
    assert!(!clone.join(".env").exists());
    let plan = command(exe, &b, "plan", &clone, &["--write-env"], None, true)?;
    command(
        exe,
        &b,
        "setup",
        &clone,
        &[
            "--review",
            plan["digest"].as_str().unwrap(),
            "--write-env",
            "--trust",
        ],
        None,
        true,
    )?;
    let (mut target_w, target_s) = open(&b)?;
    let target_p = target_w.projects.list()?.remove(0);
    assert_ne!(target_p.id, p.id);
    assert_eq!(target_p.php, php_id);
    assert_eq!(target_p.node, node_id);
    target_w.execute(
        Command::TestDatabase {
            id: target_p.id.clone(),
        },
        &Default::default(),
    )?;
    assert!(
        php(
            &target_w,
            target_s.as_ref(),
            &target_p.id,
            "echo getenv('API_TOKEN')==='TARGET_PRIVATE_TOKEN'?'LOCAL_OK':'BAD';"
        )?
        .contains("LOCAL_OK")
    );
    assert!(!fs::read_to_string(clone.join(".env"))?.contains("SOURCE_PRIVATE_TOKEN"));
    target_w.shutdown()?;
    let lease = devforge_platform::distribution::OperationLease::acquire(&b)?;
    assert_eq!(
        command(
            exe,
            &b,
            "secret",
            &clone,
            &["api_token"],
            Some("BLOCKED"),
            false
        )?["error"]["code"],
        "manifest_busy"
    );
    drop(lease);
    Ok(
        serde_json::json!({"target":target,"http_php_environment":true,"real_cli":true,"schema_roundtrip":true,"pinned_runtimes_two_machine_stores":true,"local_secret_isolation":true,"redacted_preview":true,"env_untouched_without_flag":true,"stale_secret_review_rejected":true,"repeated_setup_keeps_db_cache_and_key":true,"real_postgresql_and_garnet_php":true,"no_composer_scripts":true,"no_npm_scripts":true,"dependency_failure_and_resume":true,"cancel_preserves_data":true,"cli_operation_lease":true,"private_fixture_removed_on_exit":true,"native_gui":"M10 CLI-only scope; desktop UI unchanged"}),
    )
}
