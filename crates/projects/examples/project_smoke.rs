//! Task-local M3 integration using real verified PHP/Node/Composer/PostgreSQL.
use devforge_core::{ErrorCode, SecretStore};
use devforge_projects::{
    database::RunningPostgres,
    env_file::EnvPlan,
    environment,
    execution::{Context, Tool},
    project::{Catalog, ProjectStore},
    terminal,
};
use devforge_runtime::{
    Cancellation,
    package::{Limits, PackageStore, Target},
};
use serde_json::json;
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::Mutex,
};
#[derive(Default)]
struct Secrets(Mutex<BTreeMap<String, String>>);
impl SecretStore for Secrets {
    fn get(&self, k: &str) -> Result<Option<String>, ErrorCode> {
        Ok(self.0.lock().unwrap().get(k).cloned())
    }
    fn set(&self, k: &str, v: &str) -> Result<(), ErrorCode> {
        self.0.lock().unwrap().insert(k.into(), v.into());
        Ok(())
    }
    fn delete(&self, k: &str) -> Result<(), ErrorCode> {
        self.0.lock().unwrap().remove(k);
        Ok(())
    }
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let root = tempfile::tempdir()?;
    let outcome = run(root.path(), &args);
    if outcome.is_err() {
        show_logs(root.path());
    }
    outcome
}
fn run(root: &Path, args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let cache = PathBuf::from(args.first().ok_or("cache required")?);
    let report = PathBuf::from(args.get(1).ok_or("report required")?);
    let catalog = Catalog::bundled();
    let secrets = Secrets::default();
    let store = ProjectStore::open(root.join("metadata"))?;
    let packages = PackageStore::open(root.join("packages"), Limits::default())?;
    let target = Target::current()?;
    let m0: Vec<serde_json::Value> =
        serde_json::from_str(include_str!("../../../runtime/m0-candidates.json"))?;
    let mut installed = 0;
    for (id, p) in catalog.packages.iter().filter(|(_, p)| {
        p.target == target
            && (p.kind == "php"
                || p.kind == "node"
                || p.kind == "composer"
                || (p.kind == "postgresql" && p.variant == "server"))
    }) {
        eprintln!("Installing {id}");
        let source = m0
            .iter()
            .find(|e| e["url"].as_str() == Some(p.url.as_str()))
            .ok_or("candidate not pinned in M0")?;
        let file = cache.join(format!(
            "{}-{}",
            source["id"].as_str().unwrap(),
            p.url.rsplit('/').next().unwrap()
        ));
        if file.is_file() {
            packages.install_cached(p, &file, &Cancellation::default(), |_| {})?;
        } else if args.iter().any(|a| a == "--download-missing") {
            packages.download(p, &Cancellation::default(), |_| {})?;
        } else {
            return Err(format!("missing cache {id}").into());
        }
        installed += 1;
    }
    let select = |kind: &str, version: &str| {
        catalog
            .packages
            .iter()
            .find(|(_, p)| {
                p.target == target
                    && p.kind == kind
                    && p.version.starts_with(version)
                    && (kind != "php" || p.executables.contains_key("php"))
            })
            .unwrap()
            .0
            .clone()
    };
    let a_path = root.join("A Ўзбек & ' project");
    let b_path = root.join("B Русский project");
    fixture(&a_path)?;
    fixture(&b_path)?;
    let a = store.import(
        &a_path,
        "A",
        &select("php", "8.4"),
        &select("node", "v22"),
        &catalog,
    )?;
    let b = store.import(
        &b_path,
        "B",
        &select("php", "8.5"),
        &select("node", "v24"),
        &catalog,
    )?;
    assert_eq!(
        fs::read(a_path.join(".env"))?,
        b"APP_KEY=keep-original\r\nUNRELATED=preserve\r\nDB_HOST=old\r\n"
    );
    assert!(!a_path.join("should-not-run").exists());
    assert!(!a_path.join("vendor").exists());
    assert!(!a_path.join("node_modules").exists());
    let pg = catalog
        .packages
        .iter()
        .find(|(_, p)| p.target == target && p.kind == "postgresql" && p.variant == "server")
        .unwrap()
        .0;
    let instance = store.initialize_instance(pg, 0, &catalog, &packages, &secrets)?;
    let mut server = RunningPostgres::start(&store, &instance.id, &catalog, &packages, &secrets)?;
    let ba = server.provision(&store, &a, &secrets)?;
    let bb = server.provision(&store, &b, &secrets)?;
    assert_eq!(
        server.provision(&store, &a, &secrets)?.database,
        ba.database
    );
    let ca = store.credentials(&a.id, &secrets)?;
    let cb = store.credentials(&b.id, &secrets)?;
    assert!(ca.connection_test()? && cb.connection_test()?);
    assert!(ca.rejects_database(&bb.database) && cb.rejects_database(&ba.database));
    assert!(ca.rejects_database("postgres"));
    ca.connect()?.batch_execute(
        "CREATE TABLE IF NOT EXISTS sentinel(value text); INSERT INTO sentinel VALUES('A')",
    )?;
    cb.connect()?.batch_execute(
        "CREATE TABLE IF NOT EXISTS sentinel(value text); INSERT INTO sentinel VALUES('B')",
    )?;
    let parallel_a = store.credentials(&a.id, &secrets)?;
    let parallel_b = store.credentials(&b.id, &secrets)?;
    let one = std::thread::spawn(
        move || -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
            Ok(parallel_a
                .connect()?
                .query_one("SELECT value FROM sentinel,pg_sleep(0.2) LIMIT 1", &[])?
                .get(0))
        },
    );
    let two = std::thread::spawn(
        move || -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
            Ok(parallel_b
                .connect()?
                .query_one("SELECT value FROM sentinel,pg_sleep(0.2) LIMIT 1", &[])?
                .get(0))
        },
    );
    assert_eq!(one.join().unwrap().unwrap(), "A");
    assert_eq!(two.join().unwrap().unwrap(), "B");
    let a = environment::set(
        &store,
        &secrets,
        &a.id,
        a.revision,
        "APP_TEST_SECRET",
        "redact-this-M3-fixture-secret",
    )?;
    let mut checks = vec![];
    for (p, php, node) in [(&a, "8.4", "v22"), (&b, "8.5", "v24")] {
        let mut results = vec![];
        for (tool, arguments) in [
            (
                Tool::Php,
                vec![
                    "-r",
                    r#"echo json_encode(['php'=>PHP_VERSION,'ini'=>php_ini_loaded_file(),'db'=>getenv('DB_DATABASE')]); $pdo=new PDO('pgsql:host='.getenv('DB_HOST').';port='.getenv('DB_PORT').';dbname='.getenv('DB_DATABASE'),getenv('DB_USERNAME'),getenv('DB_PASSWORD')); echo '\nPDO_OK';"#,
                ],
            ),
            (Tool::PhpWeb, vec!["-v"]),
            (
                Tool::Node,
                vec![
                    "-p",
                    "JSON.stringify({version:process.version,db:process.env.DB_DATABASE})",
                ],
            ),
            (Tool::Npm, vec!["--version"]),
            (Tool::Composer, vec!["--version"]),
            (Tool::Artisan, vec!["--version"]),
            (Tool::Queue, vec!["--once"]),
        ] {
            let result = Context::resolve(&store, &p.id, &catalog, &packages, &secrets)?
                .invocation(
                    tool,
                    arguments.into_iter().map(Into::into).collect(),
                    &catalog,
                    &packages,
                )?
                .run(root.join("command-logs"))?;
            if !result.success {
                return Err(format!(
                    "{}: {} failed: {}",
                    p.name,
                    tool.name(),
                    serde_json::to_string(&result)?
                )
                .into());
            }
            let output = result
                .logs
                .iter()
                .map(|l| l.text.clone())
                .collect::<String>();
            if matches!(tool, Tool::Php | Tool::PhpWeb | Tool::Artisan | Tool::Queue) {
                assert!(output.contains(php), "{output}");
            }
            if tool == Tool::Node {
                assert!(output.contains(node));
            }
            if tool == Tool::Php {
                assert!(output.contains("PDO_OK"));
            }
            results.push(json!({"tool":tool.name(),"passed":true,"exit_code":result.exit_code}));
        }
        let mut context = Context::resolve(&store, &p.id, &catalog, &packages, &secrets)?;
        let versions = terminal::probe(&mut context, &catalog, &packages)?;
        assert!(
            versions.contains(php) && versions.contains(node),
            "{versions}"
        );
        checks.push(json!({"project":p.name,"tools":results,"terminal_shell_probe":true}));
    }
    let result = Context::resolve(&store, &a.id, &catalog, &packages, &secrets)?
        .invocation(
            Tool::Php,
            vec![
                "-r".into(),
                "echo getenv('APP_TEST_SECRET'),PHP_EOL,getenv('DB_PASSWORD'),PHP_EOL;".into(),
            ],
            &catalog,
            &packages,
        )?
        .run(root.join("command-logs"))?;
    let output = result
        .logs
        .iter()
        .map(|l| l.text.clone())
        .collect::<String>();
    assert!(output.contains("[REDACTED]"));
    assert!(!output.contains("redact-this-M3-fixture-secret"));
    let changed = store.configure(
        &a.id,
        a.revision,
        "A",
        &select("php", "8.5"),
        &select("node", "v24"),
        &catalog,
    )?;
    assert_eq!(store.get(&b.id)?.php, b.php);
    for (tool, arg, version) in [
        (Tool::Php, vec!["-r", "echo PHP_VERSION;"], "8.5"),
        (Tool::Node, vec!["--version"], "v24"),
    ] {
        let result = Context::resolve(&store, &a.id, &catalog, &packages, &secrets)?
            .invocation(
                tool,
                arg.into_iter().map(Into::into).collect(),
                &catalog,
                &packages,
            )?
            .run(root.join("command-logs"))?;
        assert!(result.success && result.logs.iter().any(|l| l.text.contains(version)));
    }
    let laravel = if args.iter().any(|a| a == "--laravel") {
        let laravel_path = root.join("real-laravel");
        let exec = |id: &str,
                    tool: Tool,
                    args: Vec<std::ffi::OsString>|
         -> Result<String, Box<dyn std::error::Error>> {
            let result = Context::resolve(&store, id, &catalog, &packages, &secrets)?
                .invocation(tool, args, &catalog, &packages)?
                .run(root.join("command-logs"))?;
            let output = result
                .logs
                .iter()
                .map(|l| l.text.clone())
                .collect::<String>();
            if !result.success {
                return Err(format!("Real Laravel {} failed: {output}", tool.name()).into());
            }
            Ok(output)
        };
        exec(
            &a.id,
            Tool::Composer,
            vec![
                "create-project".into(),
                "laravel/laravel".into(),
                laravel_path.as_os_str().to_owned(),
                "v13.11.0".into(),
                "--no-dev".into(),
                "--no-scripts".into(),
                "--no-plugins".into(),
                "--no-interaction".into(),
                "--prefer-dist".into(),
            ],
        )?;
        let mut real = store.import(
            &laravel_path,
            "Laravel acceptance",
            &select("php", "8.4"),
            &select("node", "v22"),
            &catalog,
        )?;
        server.provision(&store, &real, &secrets)?;
        for (key, value) in [
            ("APP_ENV", "testing"),
            ("APP_KEY", "0123456789abcdef0123456789abcdef"),
            ("QUEUE_CONNECTION", "database"),
        ] {
            real = environment::set(&store, &secrets, &real.id, real.revision, key, value)?;
        }
        EnvPlan::build(&store, &real.id, &secrets)?.apply(&store)?;
        let version = exec(&real.id, Tool::Artisan, vec!["--version".into()])?;
        exec(
            &real.id,
            Tool::Artisan,
            vec![
                "migrate".into(),
                "--force".into(),
                "--no-interaction".into(),
            ],
        )?;
        exec(
            &real.id,
            Tool::Queue,
            vec![
                "--once".into(),
                "--stop-when-empty".into(),
                "--no-interaction".into(),
            ],
        )?;
        real = store.configure(
            &real.id,
            real.revision,
            &real.name,
            &select("php", "8.5"),
            &real.node,
            &catalog,
        )?;
        exec(
            &real.id,
            Tool::Artisan,
            vec!["migrate:status".into(), "--no-interaction".into()],
        )?;
        json!({"framework":version.trim(),"skeleton":"v13.11.0","composer_create_project_no_scripts_no_plugins":true,"migration":true,"empty_queue_worker":true,"php84_then_php85":true})
    } else {
        json!({"executed":false})
    };
    let plan = EnvPlan::build(&store, &changed.id, &secrets)?;
    fs::write(a_path.join(".env"), b"EXTERNAL_EDIT=keep\n")?;
    assert!(plan.apply(&store).is_err());
    assert_eq!(fs::read(a_path.join(".env"))?, b"EXTERNAL_EDIT=keep\n");
    fs::write(
        a_path.join(".env"),
        b"APP_KEY=keep-original\r\nUNRELATED=preserve\r\nDB_HOST=old\r\n",
    )?;
    let applied = EnvPlan::build(&store, &a.id, &secrets)?.apply(&store)?;
    assert!(fs::read_to_string(a_path.join(".env"))?.contains("UNRELATED=preserve\r\n"));
    assert!(fs::read_to_string(applied.backup.unwrap())?.contains("DB_HOST=old"));
    for secret in secrets.0.lock().unwrap().values().filter(|v| v.len() >= 16) {
        let bytes = fs::read(store.root().join("projects.sqlite3"))?;
        assert!(!String::from_utf8_lossy(&bytes).contains(secret));
    }
    assert_no_secret_logs(root, &secrets)?;
    server.stop()?;
    let mut server = RunningPostgres::start(&store, &instance.id, &catalog, &packages, &secrets)?;
    let a_value: String = ca
        .connect()?
        .query_one("SELECT value FROM sentinel LIMIT 1", &[])?
        .get(0);
    let b_value: String = cb
        .connect()?
        .query_one("SELECT value FROM sentinel LIMIT 1", &[])?
        .get(0);
    assert_eq!(a_value, "A");
    assert_eq!(b_value, "B");
    server.stop()?;
    store.forget(&b.id, b.revision)?;
    assert!(b_path.join("composer.json").exists());
    assert!(
        store
            .root()
            .join("databases")
            .join(&instance.id)
            .join("data/PG_VERSION")
            .exists()
    );
    let result = json!({"host":{"os":std::env::consts::OS,"architecture":std::env::consts::ARCH},"packages":installed,"projects":checks,"version_switch_A_does_not_change_B":true,"real_laravel":laravel,"database":{"isolated_roles_and_databases":true,"cross_database_denied":true,"idempotent_provisioning":true,"restart_preserves_rows":true,"graceful_stop":true,"data_preserved_after_forget":true},"environment":{"import_read_only":true,"secret_store_fixture":"in-memory test adapter; production uses native keyring","no_plaintext_in_sqlite_or_logs":true,"log_redaction":true,"env_explicit_apply_and_backup":true,"concurrent_file_edit_rejected":true},"gui_terminal_window":"not exercised; shell shims probed natively","passed":true});
    fs::write(report, serde_json::to_vec_pretty(&result)?)?;
    println!("M3_REPORT:{}", serde_json::to_string(&result)?);
    Ok(())
}
fn fixture(path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    fs::create_dir_all(path.join("public"))?;
    fs::write(path.join("public/index.php"), b"<?php")?;
    fs::write(path.join("composer.json"),br#"{"require":{"php":"^8.3","laravel/framework":"^13"},"scripts":{"post-install-cmd":"touch should-not-run"}}"#)?;
    fs::write(
        path.join("package.json"),
        br#"{"engines":{"node":"^22 || ^24"},"scripts":{"postinstall":"touch should-not-run"}}"#,
    )?;
    fs::write(
        path.join("artisan"),
        b"<?php echo PHP_VERSION, ' ', implode(' ', $argv), PHP_EOL;",
    )?;
    fs::write(
        path.join(".env"),
        b"APP_KEY=keep-original\r\nUNRELATED=preserve\r\nDB_HOST=old\r\n",
    )?;
    Ok(())
}
fn files(root: &Path) -> Vec<PathBuf> {
    let mut out = vec![];
    if let Ok(entries) = fs::read_dir(root) {
        for entry in entries.flatten() {
            if entry.path().is_dir() {
                out.extend(files(&entry.path()));
            } else {
                out.push(entry.path());
            }
        }
    }
    out
}
fn assert_no_secret_logs(root: &Path, secrets: &Secrets) -> Result<(), Box<dyn std::error::Error>> {
    for file in files(root)
        .into_iter()
        .filter(|p| p.to_string_lossy().ends_with(".log"))
    {
        let text = fs::read_to_string(file)?;
        for value in secrets.0.lock().unwrap().values().filter(|v| v.len() >= 16) {
            assert!(!text.contains(value));
        }
    }
    Ok(())
}
fn show_logs(root: &Path) {
    for file in files(root)
        .into_iter()
        .filter(|p| p.to_string_lossy().ends_with(".log"))
    {
        if let Ok(text) = fs::read_to_string(&file) {
            eprintln!(
                "LOG {}: {}",
                file.file_name().unwrap().to_string_lossy(),
                text.chars()
                    .rev()
                    .take(2000)
                    .collect::<String>()
                    .chars()
                    .rev()
                    .collect::<String>()
            );
        }
    }
}
