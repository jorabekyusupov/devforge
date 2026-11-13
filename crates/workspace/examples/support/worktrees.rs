fn optional_env(path: &Path) -> Result<Option<Vec<u8>>> {
    match fs::read(path.join(".env")) {
        Ok(v) => Ok(Some(v)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}
use super::*;
use devforge_core::SecretStore;
use devforge_projects::cache::{CacheDriver, QueueDriver, ServiceSelection};
use devforge_workspace::worktrees::{DatabaseMode, Input};
fn php(w: &Workspace, s: &dyn devforge_core::SecretStore, id: &str, body: &str) -> Result<String> {
    exec(w,s,id,Tool::Php,vec!["-r".into(),format!("require 'vendor/autoload.php';$app=require 'bootstrap/app.php';$app->make(Illuminate\\Contracts\\Console\\Kernel::class)->bootstrap();{body}").into()])
}
fn git(path: &Path, args: &[&str]) -> Result<String> {
    let out = std::process::Command::new(devforge_projects::execution::find_git()?)
        .arg("-C")
        .arg(dunce::simplified(path))
        .args(args)
        .output()?;
    if !out.status.success() {
        return Err(format!(
            "fixture Git failed: {}",
            String::from_utf8_lossy(&out.stderr)
        )
        .into());
    }
    Ok(String::from_utf8(out.stdout)?.trim().into())
}
fn repository(path: &Path) -> Result<()> {
    if !path.join(".git").exists() {
        git(path, &["init", "-b", "main"])?;
    }
    git(path, &["config", "user.name", "DevForge fixture"])?;
    git(path, &["config", "user.email", "fixture@example.test"])?;
    let mut ignore = fs::read_to_string(path.join(".gitignore")).unwrap_or_default();
    ignore.push_str("\n/.devforge\n/.env\n/vendor\n/node_modules\n/database/*.sqlite*\n");
    fs::write(path.join(".gitignore"), ignore)?;
    let route = path.join("routes/web.php");
    let mut code = fs::read_to_string(&route)?;
    code.push_str("\nIlluminate\\Support\\Facades\\Route::get('/m12-workspace',fn()=>response()->json(['value'=>Illuminate\\Support\\Facades\\DB::table('m12_values')->value('value'),'database'=>Illuminate\\Support\\Facades\\DB::connection()->getDatabaseName()]));\n");
    fs::write(route, code)?;
    git(path, &["add", "."])?;
    git(path, &["commit", "-m", "M12 isolated workspace fixture"])?;
    Ok(())
}
fn input(id: &str, name: &str, branch: &str, mode: DatabaseMode, deps: bool) -> Input {
    Input {
        source_id: id.into(),
        name: name.into(),
        branch: branch.into(),
        base: "HEAD".into(),
        new_branch: true,
        database: mode,
        dependencies: deps,
        copy_environment: true,
    }
}
fn make(w: &mut Workspace, input: Input) -> Result<devforge_workspace::worktrees::Record> {
    let plan = w.worktree_plan(&input)?;
    assert!(plan.blockers.is_empty(), "{:?}", plan.blockers);
    Ok(w.worktree_create(input, &plan.digest, true, &Default::default())?)
}
fn value(w: &Workspace, s: &dyn devforge_core::SecretStore, id: &str) -> Result<i64> {
    Ok(php(
        w,
        s,
        id,
        "echo Illuminate\\Support\\Facades\\DB::table('m12_values')->value('value');",
    )?
    .trim()
    .parse()?)
}
pub fn run(root: &Path) -> Result<()> {
    let data = root.join("data");
    let s = Arc::new(devforge_workspace::testing::FixtureSecrets::open(
        data.join("e2e-secrets.json"),
    ));
    let mut w = Workspace::open(&data, s.clone())?;
    let cancel = Cancellation::default();
    let target = Target::current()?;
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
    let env_a = optional_env(&a.path)?;
    let env_b = optional_env(&b.path)?;
    for id in [&a.id, &b.id] {
        w.execute(Command::TestDatabase { id: id.clone() }, &cancel)?;
    }
    eprintln!("M12_PHASE: source service profile and committed repository");
    let choose = |kind: &str, w: &Workspace| {
        w.catalog
            .packages
            .iter()
            .find(|(_, p)| {
                p.kind == kind && p.target == target && (kind != "garnet" || p.version == "2.2.11")
            })
            .map(|(id, _)| id.clone())
    };
    let garnet = choose("garnet", &w).unwrap();
    let dotnet = choose("dotnet", &w).unwrap();
    for id in [&garnet, &dotnet] {
        w.packages.download(w.catalog.get(id)?, &cancel, |_| {})?;
    }
    w.execute(
        Command::InstallRedisClient {
            id: a.id.clone(),
            consent: true,
        },
        &cancel,
    )?;
    let cache = w.execute(
        Command::CreateCache {
            id: a.id.clone(),
            package: garnet.clone(),
        },
        &cancel,
    )?;
    let cache_id = cache["id"].as_str().unwrap().to_owned();
    let p = w.projects.get(&a.id)?;
    w.execute(
        Command::ConfigureServices {
            id: a.id.clone(),
            revision: p.revision,
            selection: ServiceSelection {
                cache: CacheDriver::Redis,
                queue: QueueDriver::Redis,
                redis_instance: Some(cache_id.clone()),
            },
        },
        &cancel,
    )?;
    w.execute(
        Command::StartCache {
            instance: cache_id.clone(),
        },
        &cancel,
    )?;
    w.execute(
        Command::MailConfigure {
            id: a.id.clone(),
            enabled: true,
        },
        &cancel,
    )?;
    let p = w.projects.get(&a.id)?;
    devforge_projects::environment::set(
        &w.projects,
        s.as_ref(),
        &a.id,
        p.revision,
        "API_TOKEN",
        "DO_NOT_EXPORT_M12_SECRET",
    )?;
    php(
        &w,
        s.as_ref(),
        &a.id,
        "Illuminate\\Support\\Facades\\Schema::create('m12_values',function($t){$t->integer('value');});Illuminate\\Support\\Facades\\DB::table('m12_values')->insert(['value'=>7]);Illuminate\\Support\\Facades\\Cache::put('m12-isolation','source',600);",
    )?;
    repository(&a.path)?;
    let source_head = git(&a.path, &["rev-parse", "HEAD"])?;
    eprintln!("M12_PHASE: PostgreSQL empty and copied databases");
    let empty = make(
        &mut w,
        input(
            &a.id,
            "M12 Empty",
            "feature/m12-empty",
            DatabaseMode::Empty,
            true,
        ),
    )?;
    let eid = empty.project_id.clone().unwrap();
    w.execute(
        Command::StartCache {
            instance: w.projects.get(&eid)?.services.redis_instance.unwrap(),
        },
        &cancel,
    )?;
    assert_eq!(
        php(
            &w,
            s.as_ref(),
            &eid,
            "echo Illuminate\\Support\\Facades\\Schema::hasTable('m12_values')?'yes':'no';"
        )?
        .trim(),
        "no"
    );
    exec(
        &w,
        s.as_ref(),
        &eid,
        Tool::Artisan,
        vec![
            "migrate".into(),
            "--force".into(),
            "--no-interaction".into(),
        ],
    )?;
    php(
        &w,
        s.as_ref(),
        &eid,
        "Illuminate\\Support\\Facades\\Schema::create('m12_values',function($t){$t->integer('value');});Illuminate\\Support\\Facades\\DB::table('m12_values')->insert(['value'=>13]);",
    )?;
    let clone = make(
        &mut w,
        input(
            &a.id,
            "M12 Copy",
            "feature/m12-copy",
            DatabaseMode::Clone,
            true,
        ),
    )?;
    let cid = clone.project_id.clone().unwrap();
    assert_eq!(value(&w, s.as_ref(), &cid)?, 7);
    php(
        &w,
        s.as_ref(),
        &cid,
        "Illuminate\\Support\\Facades\\DB::table('m12_values')->update(['value'=>11]);",
    )?;
    assert_eq!(value(&w, s.as_ref(), &a.id)?, 7);
    let parent = w.projects.binding(&a.id)?.unwrap();
    let child = w.projects.binding(&cid)?.unwrap();
    assert_ne!(parent.database, child.database);
    assert_ne!(parent.secret_ref, child.secret_ref);
    assert!(
        w.projects
            .credentials(&cid, s.as_ref())?
            .rejects_database(&parent.database)
    );
    let cp = w.projects.get(&cid)?;
    assert_ne!(
        cp.services.redis_instance.as_deref(),
        Some(cache_id.as_str())
    );
    w.execute(
        Command::StartCache {
            instance: cp.services.redis_instance.clone().unwrap(),
        },
        &cancel,
    )?;
    assert_eq!(
        php(
            &w,
            s.as_ref(),
            &cid,
            "echo Illuminate\\Support\\Facades\\Cache::get('m12-isolation','empty');"
        )?
        .trim(),
        "empty"
    );
    php(
        &w,
        s.as_ref(),
        &cid,
        "Illuminate\\Support\\Facades\\Cache::put('m12-isolation','child',600);",
    )?;
    assert_eq!(
        php(
            &w,
            s.as_ref(),
            &a.id,
            "echo Illuminate\\Support\\Facades\\Cache::get('m12-isolation');"
        )?
        .trim(),
        "source"
    );
    let parent_mail = w.mail_view(&a.id, "", 0)?["port"].as_u64().unwrap();
    let child_mail = w.mail_view(&cid, "", 0)?["port"].as_u64().unwrap();
    assert_ne!(parent_mail, child_mail);
    php(
        &w,
        s.as_ref(),
        &cid,
        "Illuminate\\Support\\Facades\\Mail::raw('M12 child mail',function($m){$m->to('test@example.test')->subject('M12_CHILD');});",
    )?;
    assert_eq!(w.mail_view(&a.id, "M12_CHILD", 0)?["inbox"]["total"], 0);
    assert_eq!(w.mail_view(&cid, "M12_CHILD", 0)?["inbox"]["total"], 1);
    let source_key = devforge_projects::environment::key(&w.projects.get(&a.id)?, "API_TOKEN");
    let child_key = devforge_projects::environment::key(&cp, "API_TOKEN");
    assert_ne!(source_key, child_key);
    assert_eq!(s.get(&source_key)?, s.get(&child_key)?);
    devforge_projects::environment::set(
        &w.projects,
        s.as_ref(),
        &cid,
        cp.revision,
        "API_TOKEN",
        "M12_CHILD_ROTATION",
    )?;
    assert_eq!(
        s.get(&source_key)?.as_deref(),
        Some("DO_NOT_EXPORT_M12_SECRET")
    );
    eprintln!("M12_PHASE: parallel Laravel PHP Node queue and HTTP isolation");
    w.start(&a.id)?;
    w.start(&cid)?;
    w.start(&eid)?;
    let snap = w.snapshot()?;
    let rows = [&a.id, &cid, &eid]
        .iter()
        .map(|id| snap.projects.iter().find(|r| &r.project.id == *id).unwrap())
        .collect::<Vec<_>>();
    let urls = rows
        .iter()
        .map(|r| r.serving.as_ref().unwrap().fallback_url.clone())
        .collect::<Vec<_>>();
    assert_ne!(urls[0], urls[1]);
    assert_ne!(urls[1], urls[2]);
    let client = reqwest::blocking::Client::builder().no_proxy().build()?;
    for (url, expected) in urls.iter().zip([7, 11, 13]) {
        let response: serde_json::Value = client
            .get(format!("{url}/m12-workspace"))
            .send()?
            .error_for_status()?
            .json()?;
        assert_eq!(response["value"].as_i64(), Some(expected));
    }
    let parent_pid = rows[0]
        .processes
        .iter()
        .find(|(name, _)| name.contains("php"))
        .unwrap()
        .1
        .pid;
    assert!(parent_pid.is_some());
    assert_ne!(
        parent_pid,
        rows[1]
            .processes
            .iter()
            .find(|(name, _)| name.contains("php"))
            .unwrap()
            .1
            .pid
    );
    // Real independent Redis queues consume their own jobs and write only their own databases.
    for id in [&a.id, &cid] {
        let p = w.projects.get(id)?;
        fs::write(
            p.path.join("app/Jobs/M12WorkspaceJob.php"),
            r#"<?php namespace App\Jobs;class M12WorkspaceJob implements \Illuminate\Contracts\Queue\ShouldQueue {use \Illuminate\Foundation\Bus\Dispatchable,\Illuminate\Queue\InteractsWithQueue,\Illuminate\Bus\Queueable,\Illuminate\Queue\SerializesModels;public function handle():void{\Illuminate\Support\Facades\Cache::put('m12-job-done','done',600);}}"#,
        )?;
        php(
            &w,
            s.as_ref(),
            id,
            "App\\Jobs\\M12WorkspaceJob::dispatch();",
        )?;
    }
    let until = std::time::Instant::now() + std::time::Duration::from_secs(35);
    loop {
        if php(
            &w,
            s.as_ref(),
            &cid,
            "echo Illuminate\\Support\\Facades\\Cache::get('m12-job-done','waiting');",
        )?
        .trim()
            == "done"
            && php(
                &w,
                s.as_ref(),
                &a.id,
                "echo Illuminate\\Support\\Facades\\Cache::get('m12-job-done','waiting');",
            )?
            .trim()
                == "done"
        {
            break;
        }
        if std::time::Instant::now() > until {
            return Err("M12 independent queue timed out".into());
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    // User source changes remain in source. Child job is fixture-only and removed before cleanup.
    fs::remove_file(
        w.projects
            .get(&cid)?
            .path
            .join("app/Jobs/M12WorkspaceJob.php"),
    )?;
    let path = w.projects.get(&cid)?.path;
    fs::write(path.join("untracked-user.txt"), "preserve-user-work")?;
    let dirty = w.worktree_cleanup_plan(&clone.id)?;
    assert!(dirty.dirty);
    assert_eq!(
        w.worktree_cleanup(&clone.id, &dirty.digest, true, &cancel)
            .err()
            .unwrap()
            .code,
        "worktree_dirty"
    );
    assert!(path.join("untracked-user.txt").exists());
    fs::remove_file(path.join("untracked-user.txt"))?;
    let connection = w.projects.credentials(&cid, s.as_ref())?;
    let original_env = fs::read(path.join(".env"))?;
    let clean = w.worktree_cleanup_plan(&clone.id)?;
    assert!(clean.blockers.is_empty(), "{:?}", clean.blockers);
    w.worktree_cleanup(&clone.id, &clean.digest, true, &cancel)?;
    assert_eq!(
        fs::read(clean.workspace.retained_path.join(".env"))?,
        original_env
    );
    assert_eq!(
        connection
            .connect()?
            .query_one("SELECT value FROM m12_values", &[])?
            .get::<_, i32>(0),
        11
    );
    assert!(
        git(
            &a.path,
            &["show-ref", "--verify", "refs/heads/feature/m12-copy"]
        )
        .is_ok()
    );
    assert!(
        !git(&a.path, &["worktree", "list", "--porcelain"])?.contains(&path.display().to_string())
    );
    let now = w.snapshot()?;
    assert_eq!(
        now.projects
            .iter()
            .find(|r| r.project.id == a.id)
            .unwrap()
            .processes
            .iter()
            .find(|(name, _)| name.contains("php"))
            .unwrap()
            .1
            .pid,
        parent_pid
    );
    assert!(
        client
            .get(format!("{}/m12-workspace", urls[0]))
            .send()?
            .status()
            .is_success()
    );
    w.stop(&a.id)?;
    w.stop(&eid)?;
    existing_branch(&mut w, s.as_ref(), root)?;
    eprintln!("M12_PHASE: SQLite snapshot from a real wizard-created project");
    let created = w.execute(
        Command::CreateProject {
            input: devforge_workspace::setup::CreateInput {
                source: devforge_workspace::setup::Source::Laravel {
                    version: "13.11.0".into(),
                },
                parent: root.into(),
                folder: "SQLite-source".into(),
                name: "SQLite source".into(),
                php: a.php.clone(),
                node: a.node.clone(),
                domain: "sqlite-m12.devforge.test".into(),
                https: false,
                options: devforge_workspace::setup::PrepareOptions {
                    composer: true,
                    npm: false,
                    create_env: true,
                    generate_key: true,
                    run_scripts: false,
                    initialize_sqlite: true,
                    trusted: true,
                },
            },
        },
        &cancel,
    )?;
    let sid = created["project_id"].as_str().unwrap().to_string();
    let sp = w.projects.get(&sid)?;
    php(
        &w,
        s.as_ref(),
        &sid,
        "Illuminate\\Support\\Facades\\Schema::create('m12_values',function($t){$t->integer('value');});Illuminate\\Support\\Facades\\DB::table('m12_values')->insert(['value'=>23]);",
    )?;
    repository(&sp.path)?;
    let sq = make(
        &mut w,
        input(
            &sid,
            "M12 SQLite copy",
            "feature/m12-sqlite",
            DatabaseMode::Clone,
            false,
        ),
    )?;
    let sqid = sq.project_id.clone().unwrap();
    let output=exec(&w,s.as_ref(),&sqid,Tool::Php,vec!["-r".into(),"$p=new PDO('sqlite:'.getcwd().'/database/database.sqlite');echo $p->query('select value from m12_values')->fetchColumn();".into()])?;
    assert_eq!(output.trim(), "23");
    let plan = w.worktree_cleanup_plan(&sq.id)?;
    w.worktree_cleanup(&sq.id, &plan.digest, true, &cancel)?;
    assert!(
        plan.workspace
            .retained_path
            .join("database/database.sqlite")
            .exists()
    );
    sqlite_empty(root)?;
    let mut mysql_checked = false;
    if let Some(mysql) = choose("mysql", &w) {
        eprintln!("M12_PHASE: MySQL isolated copied database");
        w.packages
            .download(w.catalog.get(&mysql)?, &cancel, |_| {})?;
        let instance = w.execute(Command::CreateDatabase { package: mysql }, &cancel)?;
        w.execute(
            Command::SwitchDatabase {
                id: b.id.clone(),
                instance: instance["id"].as_str().unwrap().into(),
                consent: true,
            },
            &cancel,
        )?;
        exec(
            &w,
            s.as_ref(),
            &b.id,
            Tool::Artisan,
            vec![
                "migrate".into(),
                "--force".into(),
                "--no-interaction".into(),
            ],
        )?;
        php(
            &w,
            s.as_ref(),
            &b.id,
            "Illuminate\\Support\\Facades\\Schema::create('m12_values',function($t){$t->integer('value');});Illuminate\\Support\\Facades\\DB::table('m12_values')->insert(['value'=>31]);",
        )?;
        repository(&b.path)?;
        let my = make(
            &mut w,
            input(
                &b.id,
                "M12 MySQL copy",
                "feature/m12-mysql",
                DatabaseMode::Clone,
                true,
            ),
        )?;
        let mid = my.project_id.clone().unwrap();
        assert_eq!(value(&w, s.as_ref(), &mid)?, 31);
        php(
            &w,
            s.as_ref(),
            &mid,
            "Illuminate\\Support\\Facades\\DB::table('m12_values')->update(['value'=>37]);",
        )?;
        assert_eq!(value(&w, s.as_ref(), &b.id)?, 31);
        let pc = w.projects.binding(&b.id)?.unwrap();
        let cc = w.projects.credentials(&mid, s.as_ref())?;
        assert!(cc.rejects_database(&pc.database));
        let my_path = w.projects.get(&mid)?.path;
        let generated = git(&my_path, &["status", "--porcelain"])?;
        if generated == "?? package-lock.json" {
            git(&my_path, &["add", "package-lock.json"])?;
            git(
                &my_path,
                &[
                    "commit",
                    "-m",
                    "Keep generated dependency lock on workspace branch",
                ],
            )?;
        } else {
            assert!(
                generated.is_empty(),
                "unexpected fixture changes: {generated}"
            );
        }
        let saved_commit = git(&my_path, &["rev-parse", "HEAD"])?;
        let plan = w.worktree_cleanup_plan(&my.id)?;
        w.worktree_cleanup(&my.id, &plan.digest, true, &cancel)?;
        assert!(cc.connection_test()?);
        assert_eq!(
            git(&b.path, &["rev-parse", "refs/heads/feature/m12-mysql"])?,
            saved_commit
        );
        mysql_checked = true;
    }
    if !b.path.join(".git").exists() {
        repository(&b.path)?;
    }
    // The empty workspace remains for the native UI to inspect; its source is untouched.
    assert_eq!(optional_env(&a.path)?, env_a);
    assert_eq!(optional_env(&b.path)?, env_b);
    assert_eq!(git(&a.path, &["rev-parse", "HEAD"])?, source_head);
    let report = json!({"target":target,"source_id":a.id,"ui_seed_workspace":empty.id,"ui_create_source_id":b.id,"postgresql_empty_and_clone":true,"existing_local_branch_reuse_isolated":true,"database_credentials_and_permissions_isolated":true,"parallel_laravel_php_node_queue_http":true,"redis_cache_queue_isolated":true,"mail_listener_and_inbox_isolated":true,"copied_environment_secret_entries_independent":true,"dirty_cleanup_refused":true,"archive_preserves_ignored_env_and_branch":true,"retained_database_data_accessible":true,"parent_process_survives_child_cleanup":true,"sqlite_wizard_snapshot_and_archive":true,"sqlite_empty_then_explicit_prepare":true,"mysql_clone":mysql_checked,"mysql_capability":if mysql_checked{"verified"}else{"unavailable current Intel package"},"source_environment_and_head_preserved":true});
    w.shutdown()?;
    fs::write(
        root.join("m12-native-result.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("M12_NATIVE_REPORT:{report}");
    Ok(())
}

pub fn cli(root: &Path, executable: &Path) -> Result<()> {
    let native: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join("m12-native-result.json"))?)?;
    let data = root.join("data");
    let request = input(
        native["ui_create_source_id"]
            .as_str()
            .ok_or("UI source missing")?,
        "M12 CLI workspace",
        "feature/m12-cli",
        DatabaseMode::Empty,
        false,
    );
    let file = root.join("m12-cli-input.json");
    fs::write(&file, serde_json::to_vec(&request)?)?;
    let run = |args: &[&str]| -> Result<serde_json::Value> {
        let output = std::process::Command::new(executable)
            .args(args)
            .env("DEVFORGE_DATA_DIR", &data)
            .env("DEVFORGE_TEST_SECRETS", "1")
            .output()?;
        if !output.status.success() {
            return Err(format!(
                "M12 CLI failed: {}",
                String::from_utf8_lossy(&output.stderr)
            )
            .into());
        }
        Ok(serde_json::from_slice(&output.stdout)?)
    };
    let path = dunce::simplified(&file)
        .to_str()
        .ok_or("UTF8 input path required")?;
    let list = run(&["workspace", "list"])?;
    assert_eq!(list["schema"], 1);
    let plan = run(&["workspace", "plan", path])?;
    let digest = plan["result"]["digest"].as_str().unwrap();
    let created = run(&["workspace", "create", path, "--review", digest, "--trust"])?;
    let id = created["result"]["id"].as_str().unwrap();
    assert_eq!(created["result"]["stage"], "ready");
    let review = run(&["workspace", "cleanup-plan", id])?;
    let digest = review["result"]["digest"].as_str().unwrap();
    let retained = PathBuf::from(
        review["result"]["workspace"]["retained_path"]
            .as_str()
            .unwrap(),
    );
    let removed = run(&["workspace", "cleanup", id, "--review", digest, "--confirm"])?;
    assert_eq!(removed["result"]["stage"], "removed");
    assert!(retained.join(".env").is_file());
    let report = json!({"target":Target::current()?,"real_cli_json_plan_create_cleanup":true,"review_and_explicit_trust":true,"archived_local_files":true,"fixture_secret_store":"test-support only"});
    fs::write(
        root.join("m12-cli-result.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("M12_CLI_REPORT:{report}");
    Ok(())
}

pub fn sqlite_empty(root: &Path) -> Result<()> {
    let data = root.join("data");
    let secrets = Arc::new(devforge_workspace::testing::FixtureSecrets::open(
        data.join("e2e-secrets.json"),
    ));
    let mut w = Workspace::open(&data, secrets.clone())?;
    let source = w
        .projects
        .list()?
        .into_iter()
        .find(|p| p.name == "SQLite source")
        .ok_or("SQLite source missing")?;
    let empty = make(
        &mut w,
        input(
            &source.id,
            "M12 SQLite empty",
            "feature/m12-sqlite-empty",
            DatabaseMode::Empty,
            false,
        ),
    )?;
    let id = empty.project_id.clone().unwrap();
    let child = w.projects.get(&id)?;
    assert_eq!(
        fs::metadata(child.path.join("database/database.sqlite"))?.len(),
        0
    );
    w.prepare_project(
        &id,
        &devforge_workspace::setup::PrepareOptions {
            composer: true,
            npm: false,
            create_env: true,
            generate_key: true,
            run_scripts: false,
            initialize_sqlite: true,
            trusted: true,
        },
        &Default::default(),
    )?;
    assert_eq!(
        php(
            &w,
            secrets.as_ref(),
            &id,
            "echo Illuminate\\Support\\Facades\\Schema::hasTable('users')?'ready':'missing';"
        )?
        .trim(),
        "ready"
    );
    assert_eq!(php(&w,secrets.as_ref(),&id,"echo Illuminate\\Support\\Facades\\Schema::hasTable('m12_values')?'unexpected':'empty';")?.trim(),"empty");
    assert_eq!(value(&w, secrets.as_ref(), &source.id)?, 23);
    let plan = w.worktree_cleanup_plan(&empty.id)?;
    w.worktree_cleanup(&empty.id, &plan.digest, true, &Default::default())?;
    assert!(
        plan.workspace
            .retained_path
            .join("database/database.sqlite")
            .is_file()
    );
    w.shutdown()?;
    let result = json!({"target":Target::current()?,"sqlite_empty_starts_without_source_tables":true,"explicit_prepare_runs_migrations":true,"source_sqlite_data_preserved":true,"sqlite_file_archived":true});
    fs::write(
        root.join("m12-sqlite-empty-result.json"),
        serde_json::to_vec_pretty(&result)?,
    )?;
    println!("M12_SQLITE_EMPTY_REPORT:{result}");
    Ok(())
}

fn existing_branch(
    w: &mut Workspace,
    secrets: &dyn devforge_core::SecretStore,
    root: &Path,
) -> Result<()> {
    let source = w
        .projects
        .list()?
        .into_iter()
        .find(|p| p.name == "Atlas")
        .ok_or("Atlas missing")?;
    let mut attached = input(
        &source.id,
        "Existing branch conflict",
        "main",
        DatabaseMode::Empty,
        true,
    );
    attached.new_branch = false;
    let blocked = w.worktree_plan(&attached)?;
    assert!(blocked.blockers.contains(&"worktree_branch_in_use".into()));
    assert_eq!(
        w.worktree_create(attached, &blocked.digest, true, &Default::default())
            .err()
            .unwrap()
            .code,
        "worktree_branch_in_use"
    );
    let mut request = input(
        &source.id,
        "Existing branch workspace",
        "feature/m12-copy",
        DatabaseMode::Empty,
        true,
    );
    request.new_branch = false;
    let old_head = git(&source.path, &["rev-parse", "refs/heads/feature/m12-copy"])?;
    let created = make(w, request)?;
    let child = created.project_id.clone().unwrap();
    assert_ne!(child, source.id);
    assert_eq!(created.commit, old_head);
    let child_project = w.projects.get(&child)?;
    assert!(
        !fs::read_to_string(child_project.path.join(".env"))?.contains("DO_NOT_EXPORT_M12_SECRET")
    );
    php(
        w,
        secrets,
        &child,
        "exit(getenv('API_TOKEN')==='DO_NOT_EXPORT_M12_SECRET'?0:1);",
    )?;
    let binding = w.projects.binding(&child)?.unwrap();
    assert_ne!(
        binding.database,
        w.projects.binding(&source.id)?.unwrap().database
    );
    assert!(
        w.projects
            .credentials(&child, secrets)?
            .rejects_database(&w.projects.binding(&source.id)?.unwrap().database)
    );
    let plan = w.worktree_cleanup_plan(&created.id)?;
    w.worktree_cleanup(&created.id, &plan.digest, true, &Default::default())?;
    assert_eq!(
        git(&source.path, &["rev-parse", "refs/heads/feature/m12-copy"])?,
        old_head
    );
    let result = json!({"target":Target::current()?,"existing_local_branch_attached":true,"copied_custom_secret_in_store_not_env":true,"checked_out_branch_refused":true,"independent_project_and_database":true,"branch_preserved_after_cleanup":true});
    fs::write(
        root.join("m12-existing-branch-result.json"),
        serde_json::to_vec_pretty(&result)?,
    )?;
    println!("M12_EXISTING_BRANCH_REPORT:{result}");
    Ok(())
}
pub fn existing_branch_run(root: &Path) -> Result<()> {
    let data = root.join("data");
    let secrets = Arc::new(devforge_workspace::testing::FixtureSecrets::open(
        data.join("e2e-secrets.json"),
    ));
    let mut w = Workspace::open(data, secrets.clone())?;
    existing_branch(&mut w, secrets.as_ref(), root)?;
    w.shutdown()?;
    Ok(())
}
