//! Actual owned native-test child; synthetic context declarations do not prove
//! PHP/Node compatibility. No native credentials, downloads or application DB.
use devforge_core::{ErrorCode, SecretStore};
use devforge_projects::project::Catalog;
use devforge_runtime::{
    Cancellation,
    package::{Archive, Package, Target, hash_file},
};
use devforge_workspace::{
    Workspace,
    execution_control::{ExecutionRequest, ExecutionTool},
};
use std::{collections::BTreeMap, fs, path::Path, sync::Arc};
struct NoSecrets;
impl SecretStore for NoSecrets {
    fn get(&self, _: &str) -> Result<Option<String>, ErrorCode> {
        panic!("data-free fixture must not probe native credentials")
    }
    fn set(&self, _: &str, _: &str) -> Result<(), ErrorCode> {
        panic!("execution must not set native credentials")
    }
    fn delete(&self, _: &str) -> Result<(), ErrorCode> {
        panic!("execution must not delete native credentials")
    }
}
fn fixture() -> (tempfile::TempDir, Workspace, String) {
    let temp = tempfile::tempdir().unwrap();
    let mut workspace = Workspace::open(temp.path().join("data"), Arc::new(NoSecrets)).unwrap();
    let source = std::env::current_exe().unwrap();
    let mut catalog = Catalog {
        packages: BTreeMap::new(),
        superseded: BTreeMap::new(),
        defaults: BTreeMap::from([("composer".into(), "2.11.2".into())]),
    };
    for (id, kind, version) in [
        ("php", "php", "8.5.12"),
        ("node", "node", "24.22.2"),
        ("composer", "composer", "2.11.2"),
    ] {
        let resource = kind == "composer";
        let package = Package {
            kind: kind.into(),
            version: version.into(),
            variant: "native-test-fixture".into(),
            target: Target::current().unwrap(),
            url: "https://example.invalid/never-downloaded".into(),
            sha256: hash_file(&source).unwrap(),
            archive: Archive::File {
                path: "fixture.exe".into(),
            },
            executables: if resource {
                BTreeMap::new()
            } else {
                BTreeMap::from([(kind.into(), "fixture.exe".into())])
            },
            resources: if resource {
                BTreeMap::from([("composer".into(), "fixture.exe".into())])
            } else {
                BTreeMap::new()
            },
        };
        workspace
            .packages
            .install_cached(&package, &source, &Cancellation::default(), |_| {})
            .unwrap();
        catalog.packages.insert(id.into(), package);
    }
    workspace.catalog = catalog;
    let project = temp.path().join("project");
    fs::create_dir_all(project.join("public")).unwrap();
    fs::write(project.join("public/index.php"), "<?php").unwrap();
    fs::write(
        project.join("composer.json"),
        r#"{"require":{"php":"^8.3","laravel/framework":"^13"}}"#,
    )
    .unwrap();
    fs::write(project.join(".execution-child-fixture"), "owned").unwrap();
    let record = workspace
        .projects
        .import(
            &project,
            "Execution fixture",
            "php",
            "node",
            &workspace.catalog,
        )
        .unwrap();
    (temp, workspace, record.id)
}
fn request() -> ExecutionRequest {
    ExecutionRequest {
        tool: ExecutionTool::Node,
        args: vec![
            "--exact".into(),
            "execution_child_fixture".into(),
            "--nocapture".into(),
            "--skip".into(),
            "PRIVATE_ARG_CANARY".into(),
        ],
        timeout_ms: 30000,
        managed_test: true,
    }
}
#[test]
fn execution_child_fixture() {
    if Path::new(".execution-child-fixture").is_file() {
        fs::write("child-started", "yes").unwrap();
        for arg in std::env::args().skip(1) {
            println!("{arg}");
        }
        match fs::read_to_string("mode").unwrap_or_default().as_str() {
            "fail" => std::process::exit(7),
            "sleep" => std::thread::sleep(std::time::Duration::from_secs(10)),
            "flood" => {
                for _ in 0..100 {
                    println!("{}", "x".repeat(4096));
                }
            }
            _ => println!("owned-success"),
        }
    }
}
#[test]
fn completed_nonzero_and_success_are_typed_outcomes_and_supplied_argv_never_reach_logs() {
    let (root, mut workspace, id) = fixture();
    let project = workspace.projects.get(&id).unwrap().path;
    let request = request();
    fs::write(project.join("mode"), "fail").unwrap();
    let facts = workspace.execution_preflight(&id, &request).unwrap();
    assert!(facts.native_summary().test_isolated);
    assert!(!facts.sensitive_context());
    let failed = workspace
        .execute_managed_execution(&id, &request, &facts, &Cancellation::default(), &|| Ok(()))
        .unwrap();
    assert!(!failed.success);
    assert_eq!(failed.exit_code, Some(7));
    assert!(
        !serde_json::to_string(&failed)
            .unwrap()
            .contains("PRIVATE_ARG_CANARY")
    );
    fs::write(project.join("mode"), "success").unwrap();
    let passed = workspace
        .execute_managed_execution(&id, &request, &facts, &Cancellation::default(), &|| Ok(()))
        .unwrap();
    assert!(passed.success);
    assert_eq!(passed.exit_code, Some(0));
    let logs = root.path().join("data/projects/managed-execution-logs");
    for entry in fs::read_dir(logs).unwrap() {
        let path = entry.unwrap().path();
        if path.is_file() {
            assert!(
                !String::from_utf8_lossy(&fs::read(path).unwrap()).contains("PRIVATE_ARG_CANARY")
            );
        }
    }
}
#[test]
fn captured_context_cannot_be_rebased_and_managed_test_refuses_env_and_testing_overrides() {
    let (_root, mut workspace, id) = fixture();
    let project = workspace.projects.get(&id).unwrap().path;
    let mut request = request();
    let facts = workspace.execution_preflight(&id, &request).unwrap();
    fs::write(project.join(".env"), "DB_DATABASE=development\n").unwrap();
    assert_eq!(
        workspace
            .execution_preflight(&id, &request)
            .err()
            .unwrap()
            .code,
        "execution_test_context_unverified"
    );
    request.managed_test = false;
    assert_eq!(
        workspace
            .execute_managed_execution(&id, &request, &facts, &Cancellation::default(), &|| Ok(()))
            .err()
            .unwrap()
            .code,
        "execution_context_changed"
    );
    assert!(!project.join("child-started").exists());
    fs::remove_file(project.join(".env")).unwrap();
    fs::write(project.join("phpunit.xml"), "<phpunit/>").unwrap();
    request.managed_test = true;
    assert_eq!(
        workspace
            .execution_preflight(&id, &request)
            .err()
            .unwrap()
            .code,
        "execution_test_context_unverified"
    );
}
#[test]
fn native_argument_preview_masks_inline_and_next_secret_values_even_verified_scripts() {
    let (_root, workspace, id) = fixture();
    let project = workspace.projects.get(&id).unwrap().path;
    fs::write(project.join("safe.php"), "<?php").unwrap();
    let mut request = request();
    request.args = vec![
        "--password".into(),
        "safe.php".into(),
        "-r".into(),
        "echo 'PRIVATE_INLINE';".into(),
        "--help".into(),
        "safe.php".into(),
    ];
    let facts = workspace.execution_preflight(&id, &request).unwrap();
    let preview = facts.argument_preview(&request.args);
    assert_eq!(preview[0], "[REDACTED_ARGUMENT]");
    assert_eq!(preview[1], "[REDACTED_ARGUMENT]");
    assert_eq!(preview[3], "[REDACTED_INLINE_CODE]");
    assert_eq!(preview[4], "--help");
    assert_eq!(preview[5], "safe.php");
    assert!(
        !serde_json::to_string(&preview)
            .unwrap()
            .contains("PRIVATE_INLINE")
    );
}
#[test]
fn native_git_context_is_sensitive_without_executing_host_git_or_exposing_agent_path() {
    let (_root, workspace, id) = fixture();
    let request = ExecutionRequest {
        tool: ExecutionTool::Git,
        args: vec!["status".into()],
        timeout_ms: 30000,
        managed_test: false,
    };
    let facts = workspace.execution_preflight(&id, &request).unwrap();
    assert!(facts.sensitive_context());
    assert!(facts.native_summary().environment);
    if let Some(path) = std::env::var_os("SSH_AUTH_SOCK") {
        let encoded = serde_json::to_string(&facts).unwrap();
        if !path.is_empty() {
            assert!(!encoded.contains(path.to_string_lossy().as_ref()));
        }
    }
}
#[test]
fn output_is_bounded_and_prelaunch_cancel_and_timeout_are_truthful() {
    let (_root, mut workspace, id) = fixture();
    let project = workspace.projects.get(&id).unwrap().path;
    let mut request = request();
    fs::write(project.join("mode"), "flood").unwrap();
    let facts = workspace.execution_preflight(&id, &request).unwrap();
    let output = workspace
        .execute_managed_execution(&id, &request, &facts, &Cancellation::default(), &|| Ok(()))
        .unwrap();
    assert!(output.success && output.truncated && output.dropped_bytes > 0);
    assert!(serde_json::to_vec(&output).unwrap().len() <= 32768);
    assert!(output.logs.iter().map(|log| log.text.len()).sum::<usize>() <= 16384);
    let cancel = Cancellation::default();
    cancel.cancel();
    assert_eq!(
        workspace
            .execute_managed_execution(&id, &request, &facts, &cancel, &|| Ok(()))
            .err()
            .unwrap()
            .code,
        "cancelled"
    );
    request.timeout_ms = 1;
    let facts = workspace.execution_preflight(&id, &request).unwrap();
    assert_eq!(
        workspace
            .execute_managed_execution(&id, &request, &facts, &Cancellation::default(), &|| Ok(()))
            .err()
            .unwrap()
            .code,
        "timed_out"
    );
}

fn git(path: &Path, args: &[&str]) -> String {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(path)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().into()
}
fn owned_laravel_fixture() -> (tempfile::TempDir, Workspace, String, String) {
    use devforge_workspace::worktrees::{DatabaseMode, Input, Record};
    let (temp, workspace, source_id) = fixture();
    let source = workspace.projects.get(&source_id).unwrap();
    fs::create_dir_all(source.path.join("config")).unwrap();
    fs::create_dir_all(source.path.join("tests")).unwrap();
    fs::write(
        source.path.join("config/database.php"),
        "<?php return [];\n",
    )
    .unwrap();
    fs::write(source.path.join("tests/job.php"), "<?php echo 'test';\n").unwrap();
    fs::write(
        source.path.join(".gitignore"),
        ".env\n/vendor/\n/database/\n",
    )
    .unwrap();
    git(&source.path, &["init", "-q"]);
    git(
        &source.path,
        &[
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.test",
            "add",
            ".",
        ],
    );
    git(
        &source.path,
        &[
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.test",
            "commit",
            "-qm",
            "fixture",
        ],
    );
    let wid = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    let dir = temp.path().join("data/worktrees").join(wid);
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("tree");
    git(
        &source.path,
        &[
            "worktree",
            "add",
            "-qb",
            "a01",
            path.to_str().unwrap(),
            "HEAD",
        ],
    );
    let child = workspace
        .projects
        .import(&path, "Owned tests", "php", "node", &workspace.catalog)
        .unwrap();
    workspace
        .projects
        .claim_worktree(&child.id, child.revision, wid)
        .unwrap();
    let configuration = serde_json::from_value(serde_json::json!({
        "schema":1,"project":{"name":"Owned tests","domain":"tests.devforge.test"},
        "runtimes":{"php":"8.5.12","node":"24.22.2","composer":"2.11.2"},"pins":{}
    }))
    .unwrap();
    let record = Record {
        isolated: false,
        schema: 2,
        id: wid.into(),
        input: Input {
            source_id: source_id.clone(),
            name: "Owned tests".into(),
            branch: "a01".into(),
            base: "HEAD".into(),
            new_branch: true,
            database: DatabaseMode::Empty,
            dependencies: false,
            copy_environment: false,
        },
        source_path: source.path.clone(),
        common_dir: fs::canonicalize(source.path.join(".git")).unwrap(),
        git_dir: Some(fs::canonicalize(git(&path, &["rev-parse", "--absolute-git-dir"])).unwrap()),
        commit: git(&path, &["rev-parse", "HEAD"]),
        domain: "tests.devforge.test".into(),
        review_digest: "fixture".into(),
        project_id: Some(child.id.clone()),
        stage: "ready".into(),
        last_error: None,
        configuration,
    };
    fs::write(
        dir.join("receipt.json"),
        serde_json::to_vec(&record).unwrap(),
    )
    .unwrap();
    (temp, workspace, source_id, child.id)
}
fn laravel_request() -> ExecutionRequest {
    ExecutionRequest {
        tool: ExecutionTool::Php,
        args: vec!["tests/job.php".into()],
        timeout_ms: 30000,
        managed_test: true,
    }
}
#[test]
fn laravel_jobs_require_owned_child_and_reject_overrides_before_launch() {
    let (_temp, mut workspace, source, child) = owned_laravel_fixture();
    let request = laravel_request();
    assert!(workspace.execution_preflight(&source, &request).is_err());
    let path = workspace.projects.get(&child).unwrap().path;
    fs::write(
        path.join(".env"),
        "APP_KEY=source-key\nDB_DATABASE=development.sqlite\n",
    )
    .unwrap();
    assert!(workspace.execution_preflight(&child, &request).is_err());
    fs::remove_file(path.join(".env")).unwrap();
    let facts = workspace.execution_preflight(&child, &request).unwrap();
    assert!(facts.native_summary().test_isolated);
    assert!(!facts.sensitive_context());
    for name in [
        ".env.testing",
        ".env.test",
        "phpunit.xml",
        "phpunit.xml.dist",
        "bootstrap/cache/config.php",
    ] {
        fs::create_dir_all(path.join(name).parent().unwrap()).unwrap();
        fs::write(path.join(name), "conflicting configuration").unwrap();
        assert!(
            workspace.execution_preflight(&child, &request).is_err(),
            "{name}"
        );
        assert!(
            workspace
                .execute_managed_execution(
                    &child,
                    &request,
                    &facts,
                    &Cancellation::default(),
                    &|| Ok(())
                )
                .is_err()
        );
        fs::remove_file(path.join(name)).unwrap();
    }
    for args in [
        vec!["-r", "echo 'unsafe';"],
        vec!["tests/../public/index.php"],
        vec!["tests/job.php", "--env=production"],
    ] {
        let mut hostile = laravel_request();
        hostile.args = args.into_iter().map(Into::into).collect();
        assert!(workspace.execution_preflight(&child, &hostile).is_err());
    }
    let mut receipt: serde_json::Value =
        serde_json::from_slice(&fs::read(path.parent().unwrap().join("receipt.json")).unwrap())
            .unwrap();
    receipt["project_id"] = serde_json::json!(source);
    fs::write(
        path.parent().unwrap().join("receipt.json"),
        serde_json::to_vec(&receipt).unwrap(),
    )
    .unwrap();
    assert!(workspace.execution_preflight(&child, &request).is_err());
    assert!(!fs::read_dir(path.parent().unwrap()).unwrap().any(|e| {
        e.unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with("test-job-")
    }));
}

#[test]
#[ignore = "owned offline PHP/Laravel fixture required; never downloads or opens user data"]
fn actual_laravel_job_migrates_only_its_job_database() {
    let php = std::env::var_os("DEVFORGE_A01_PHP").expect("explicit cached PHP path");
    let vendor =
        std::env::var_os("DEVFORGE_A01_VENDOR").expect("explicit offline vendor directory");
    let (temp, mut workspace, source_id, child_id) = owned_laravel_fixture();
    let archive = temp.path().join("actual-php");
    fs::copy(php, &archive).unwrap();
    let package = Package {
        kind: "php".into(),
        version: "8.4.27".into(),
        variant: "a01-offline".into(),
        target: Target::current().unwrap(),
        url: "https://example.invalid/never-downloaded".into(),
        sha256: hash_file(&archive).unwrap(),
        archive: Archive::File { path: "php".into() },
        executables: BTreeMap::from([("php".into(), "php".into())]),
        resources: BTreeMap::new(),
    };
    workspace
        .packages
        .install_cached(&package, &archive, &Cancellation::default(), |_| {})
        .unwrap();
    workspace.catalog.packages.insert("php".into(), package);
    let source = workspace.projects.get(&source_id).unwrap().path;
    let child = workspace.projects.get(&child_id).unwrap().path;
    // Copy only the explicit offline dependency tree; no shared vendor writes.
    fn copy(from: &Path, to: &Path) {
        fs::create_dir_all(to).unwrap();
        for entry in fs::read_dir(from).unwrap() {
            let entry = entry.unwrap();
            let kind = entry.file_type().unwrap();
            assert!(
                !kind.is_symlink(),
                "fixture dependency links are not adopted"
            );
            if kind.is_dir() {
                copy(&entry.path(), &to.join(entry.file_name()));
            } else {
                fs::copy(entry.path(), to.join(entry.file_name())).unwrap();
            }
        }
    }
    copy(Path::new(&vendor), &child.join("vendor"));
    for path in [&source, &child] {
        fs::create_dir_all(path.join("database")).unwrap();
        let db = path.join("database/development.sqlite");
        let connection = rusqlite::Connection::open(&db).unwrap();
        connection
            .execute_batch("CREATE TABLE canary (value INTEGER); INSERT INTO canary VALUES(701);")
            .unwrap();
        drop(connection);
        if path == &source {
            fs::write(
                path.join(".env"),
                format!(
                    "APP_KEY=base64:DEVELOPMENT_CANARY\nDB_CONNECTION=sqlite\nDB_DATABASE={}\n",
                    dunce::simplified(&db).display()
                ),
            )
            .unwrap();
        }
    }
    fs::write(source.join("dirty-note"), "preserve dirty file").unwrap();
    fs::write(
        child.join("tests/job.php"),
        include_str!("fixtures/a01-laravel-job.php"),
    )
    .unwrap();
    fs::create_dir_all(child.join("bootstrap/cache")).unwrap();
    fs::write(
        child.join("config/app.php"),
        r#"<?php return [
        'env'=>env('APP_ENV'), 'key'=>env('APP_KEY'), 'providers'=>[
            \Illuminate\Database\DatabaseServiceProvider::class,
            \Illuminate\Database\MigrationServiceProvider::class,
            \Illuminate\Filesystem\FilesystemServiceProvider::class,
        ],
    ];"#,
    )
    .unwrap();
    fs::write(child.join("config/database.php"), r#"<?php return [
        'default'=>env('DB_CONNECTION'), 'connections'=>[
            'sqlite'=>['driver'=>'sqlite','url'=>env('DB_URL'),'database'=>env('DB_DATABASE'),'prefix'=>'','foreign_key_constraints'=>true],
        ], 'migrations'=>['table'=>'migrations','update_date_on_publish'=>true],
    ];"#).unwrap();
    let protected = [
        source.join(".env"),
        source.join("database/development.sqlite"),
        child.join("database/development.sqlite"),
        source.join("dirty-note"),
    ];
    let before: Vec<_> = protected.iter().map(|p| fs::read(p).unwrap()).collect();
    let heads = [
        git(&source, &["rev-parse", "HEAD"]),
        git(&child, &["rev-parse", "HEAD"]),
    ];
    let states = [
        git(&source, &["status", "--porcelain"]),
        git(&child, &["status", "--porcelain"]),
    ];
    let request = laravel_request();
    let facts = workspace.execution_preflight(&child_id, &request).unwrap();
    let result = workspace
        .execute_managed_execution(
            &child_id,
            &request,
            &facts,
            &Cancellation::default(),
            &|| Ok(()),
        )
        .unwrap();
    assert!(
        result.success,
        "{}",
        serde_json::to_string(&result).unwrap()
    );
    assert_eq!(result.exit_code, Some(0));
    assert_eq!(
        before,
        protected
            .iter()
            .map(|p| fs::read(p).unwrap())
            .collect::<Vec<_>>()
    );
    assert_eq!(
        heads,
        [
            git(&source, &["rev-parse", "HEAD"]),
            git(&child, &["rev-parse", "HEAD"])
        ]
    );
    assert_eq!(
        states,
        [
            git(&source, &["status", "--porcelain"]),
            git(&child, &["status", "--porcelain"])
        ]
    );
    let jobs: Vec<_> = fs::read_dir(child.parent().unwrap())
        .unwrap()
        .filter_map(|e| {
            let p = e.unwrap().path();
            p.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("test-job-")
                .then_some(p)
        })
        .collect();
    assert_eq!(jobs.len(), 1);
    let receipt: serde_json::Value =
        serde_json::from_slice(&fs::read(jobs[0].join("receipt.json")).unwrap()).unwrap();
    assert_eq!(receipt["project_id"], child_id);
    assert_eq!(receipt["state"], "completed");
    let db = rusqlite::Connection::open(jobs[0].join("database.sqlite")).unwrap();
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM migrations", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM a01_records", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
    // One rejected hostile config does not create a second job or start PHP.
    fs::write(
        child.join(".env.testing"),
        "DB_DATABASE=development.sqlite\n",
    )
    .unwrap();
    assert!(workspace.execution_preflight(&child_id, &request).is_err());
    assert!(
        workspace
            .execute_managed_execution(
                &child_id,
                &request,
                &facts,
                &Cancellation::default(),
                &|| Ok(())
            )
            .is_err()
    );
    if let Some(report) = std::env::var_os("DEVFORGE_A01_REPORT") {
        fs::write(report, serde_json::to_vec_pretty(&serde_json::json!({
            "packet":"A01","passed":true,"os":std::env::consts::OS,"arch":std::env::consts::ARCH,
            "php":"8.4.27","laravel":"13.42.0","normal_environment_and_configuration_bootstrap":true,
            "owned_worktree":true,"per_job_sqlite":true,"migration_count":1,"create_delete":true,
            "two_development_canaries_unchanged":true,"source_env_app_key_unchanged":true,
            "source_child_git_head_and_dirty_state_unchanged":true,"conflicting_configuration_refused":true,
            "child_env_refused":true,"native_secret_store":false,"downloads":false,"os_sandbox":false,
            "windows_actual_laravel":"not_verified"
        })).unwrap()).unwrap();
    }
    println!(
        "A01: actual Laravel migration/create/delete; two dev DBs, env/APP_KEY, HEAD/dirty state preserved; conflicting config refused; no downloads/native credentials/OS sandbox"
    );
}
