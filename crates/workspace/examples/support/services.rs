use super::*;
use devforge_projects::cache::{CacheDriver, QueueDriver, ServiceSelection};

fn php(
    w: &Workspace,
    secrets: &dyn devforge_core::SecretStore,
    id: &str,
    code: &str,
) -> Result<String> {
    exec(w,secrets,id,Tool::Php,vec!["-r".into(),format!("require 'vendor/autoload.php';$app=require 'bootstrap/app.php';$app->make(Illuminate\\Contracts\\Console\\Kernel::class)->bootstrap();{code}").into()])
}
pub fn run(root: &Path) -> Result<()> {
    let data = root.join("data");
    let secrets = Arc::new(devforge_workspace::testing::FixtureSecrets::open(
        data.join("e2e-secrets.json"),
    ));
    let mut w = Workspace::open(&data, secrets.clone())?;
    let cancel = Cancellation::default();
    let target = Target::current()?;
    let a = w
        .projects
        .list()?
        .into_iter()
        .find(|p| p.name == "Atlas")
        .ok_or("Atlas missing")?;
    let b = w
        .projects
        .list()?
        .into_iter()
        .find(|p| p.name == "Studio")
        .ok_or("Studio missing")?;
    for id in [&a.id, &b.id] {
        let p = w.projects.get(id)?;
        w.execute(
            Command::ConfigureServices {
                id: id.clone(),
                revision: p.revision,
                selection: ServiceSelection::default(),
            },
            &cancel,
        )?;
    }
    let pg = w
        .projects
        .instances()?
        .into_iter()
        .find(|i| i.engine == devforge_projects::database::Engine::Postgresql)
        .unwrap();
    w.execute(
        Command::SwitchDatabase {
            id: a.id.clone(),
            instance: pg.id,
            consent: true,
        },
        &cancel,
    )?;
    let old = w.projects.binding(&a.id)?.unwrap();
    let old_env = fs::read(a.path.join(".env")).ok();
    let selected = w
        .catalog
        .packages
        .iter()
        .filter(|(_, p)| {
            p.target == target
                && ["garnet", "dotnet", "mysql", "postgresql-tools"].contains(&p.kind.as_str())
        })
        .map(|(id, _)| id.clone())
        .collect::<Vec<_>>();
    for id in selected {
        eprintln!("M9_INSTALL:{id}");
        let p = w.catalog.get(&id)?;
        let local = match p.kind.as_str() {
            "mysql" => Some("/tmp/devforge-m9-spike/mysql-8.4.13-macos15-arm64.tar.gz"),
            "dotnet" => Some("/tmp/devforge-m9-spike/dotnet-arm64.tar.gz"),
            "garnet" if p.version == "2.2.11" => Some("/tmp/devforge-m9-spike/garnet-arm64.tar.xz"),
            "postgresql-tools" => Some("/tmp/devforge-m9-spike/postgresql-18.6-1-osx-binaries.zip"),
            _ => None,
        };
        if target == Target::MacosArm64 && local.is_some_and(|s| Path::new(s).exists()) {
            w.packages
                .install_cached(p, Path::new(local.unwrap()), &cancel, |_| {})?;
        } else {
            w.packages.download(p, &cancel, |_| {})?;
        }
    }
    w.refresh_runtimes()?;
    let choose = |kind: &str, version: &str| {
        w.catalog
            .packages
            .iter()
            .find(|(_, p)| p.target == target && p.kind == kind && p.version == version)
            .map(|(id, _)| id.clone())
    };
    let garnet = choose("garnet", "2.2.11").unwrap();
    let older = choose("garnet", "2.2.10").unwrap();
    let mysql = choose("mysql", "8.4.13");
    // Real PostgreSQL custom dump and staged restore using a separate managed client package.
    eprintln!("M9_PHASE: PostgreSQL backup");
    let backup = w.execute(Command::CreateBackup { id: a.id.clone() }, &cancel)?;
    let original = old.database.clone();
    w.execute(
        Command::RestoreBackup {
            id: a.id.clone(),
            backup: backup["id"].as_str().unwrap().into(),
            consent: true,
        },
        &cancel,
    )?;
    assert_ne!(w.projects.binding(&a.id)?.unwrap().database, original);
    assert!(
        w.projects
            .binding_history(&a.id)?
            .iter()
            .any(|b| b.database == original)
    );
    let mut mysql_instance = None;
    if let Some(package) = mysql {
        eprintln!("M9_PHASE: MySQL project switch and migrations");
        let i = w.execute(Command::CreateDatabase { package }, &cancel)?;
        let id = i["id"].as_str().unwrap().to_string();
        assert!(
            w.execute(
                Command::SwitchDatabase {
                    id: a.id.clone(),
                    instance: id.clone(),
                    consent: false
                },
                &cancel
            )
            .is_err()
        );
        w.execute(
            Command::SwitchDatabase {
                id: a.id.clone(),
                instance: id.clone(),
                consent: true,
            },
            &cancel,
        )?;
        exec(
            &w,
            secrets.as_ref(),
            &a.id,
            Tool::Artisan,
            vec!["migrate:fresh".into(), "--force".into()],
        )?;
        php(
            &w,
            secrets.as_ref(),
            &a.id,
            "Illuminate\\Support\\Facades\\DB::statement('CREATE TABLE m9_sentinel (value varchar(100))');Illuminate\\Support\\Facades\\DB::insert(\"INSERT INTO m9_sentinel VALUES ('A')\");",
        )?;
        let backup = w.execute(Command::CreateBackup { id: a.id.clone() }, &cancel)?;
        php(
            &w,
            secrets.as_ref(),
            &a.id,
            "Illuminate\\Support\\Facades\\DB::update(\"UPDATE m9_sentinel SET value='changed'\");",
        )?;
        let before = w.projects.binding(&a.id)?.unwrap();
        w.execute(
            Command::RestoreBackup {
                id: a.id.clone(),
                backup: backup["id"].as_str().unwrap().into(),
                consent: true,
            },
            &cancel,
        )?;
        assert!(
            php(
                &w,
                secrets.as_ref(),
                &a.id,
                "echo Illuminate\\Support\\Facades\\DB::table('m9_sentinel')->value('value');"
            )?
            .contains('A')
        );
        assert!(
            w.projects
                .credentials_for(before.clone(), secrets.as_ref())?
                .connection_test()?
        );
        mysql_instance = Some(id);
    }
    eprintln!("M9_PHASE: dedicated cache, Predis, real Laravel Lua queue");
    w.execute(
        Command::InstallRedisClient {
            id: a.id.clone(),
            consent: true,
        },
        &cancel,
    )?;
    w.execute(
        Command::InstallRedisClient {
            id: b.id.clone(),
            consent: true,
        },
        &cancel,
    )?;
    let ac = w.execute(
        Command::CreateCache {
            id: a.id.clone(),
            package: garnet,
        },
        &cancel,
    )?;
    let bc = w.execute(
        Command::CreateCache {
            id: b.id.clone(),
            package: older,
        },
        &cancel,
    )?;
    let ac = ac["id"].as_str().unwrap().to_string();
    let bc = bc["id"].as_str().unwrap().to_string();
    for (id, cache) in [(&a.id, &ac), (&b.id, &bc)] {
        let p = w.projects.get(id)?;
        w.execute(
            Command::ConfigureServices {
                id: id.clone(),
                revision: p.revision,
                selection: ServiceSelection {
                    cache: CacheDriver::Redis,
                    queue: QueueDriver::Redis,
                    redis_instance: Some(cache.clone()),
                },
            },
            &cancel,
        )?;
        w.execute(
            Command::StartCache {
                instance: cache.clone(),
            },
            &cancel,
        )?;
    }
    let p = w.projects.get(&a.id)?;
    assert!(
        w.execute(
            Command::ConfigureServices {
                id: a.id.clone(),
                revision: p.revision,
                selection: ServiceSelection {
                    cache: CacheDriver::Redis,
                    queue: QueueDriver::Redis,
                    redis_instance: Some(bc.clone())
                }
            },
            &cancel
        )
        .is_err()
    );
    php(
        &w,
        secrets.as_ref(),
        &a.id,
        "Illuminate\\Support\\Facades\\Cache::put('m9','A',600);$lock=Illuminate\\Support\\Facades\\Cache::lock('m9-lock',10);if(!$lock->get())exit(23);$lock->release();",
    )?;
    php(
        &w,
        secrets.as_ref(),
        &b.id,
        "Illuminate\\Support\\Facades\\Cache::put('m9','B',600);",
    )?;
    php(
        &w,
        secrets.as_ref(),
        &a.id,
        "Illuminate\\Support\\Facades\\Cache::flush();",
    )?;
    assert!(
        php(
            &w,
            secrets.as_ref(),
            &b.id,
            "echo Illuminate\\Support\\Facades\\Cache::get('m9');"
        )?
        .contains('B')
    );
    fs::write(
        a.path.join("app/Jobs/M9ServiceJob.php"),
        r#"<?php namespace App\Jobs; class M9ServiceJob implements \Illuminate\Contracts\Queue\ShouldQueue {use \Illuminate\Foundation\Bus\Dispatchable,\Illuminate\Queue\InteractsWithQueue,\Illuminate\Bus\Queueable,\Illuminate\Queue\SerializesModels;public function handle():void{\Illuminate\Support\Facades\Cache::put('m9-queue','completed',600);}}"#,
    )?;
    php(
        &w,
        secrets.as_ref(),
        &a.id,
        "App\\Jobs\\M9ServiceJob::dispatch();",
    )?;
    // Execute Laravel's real pop/reserve/delete Lua path as a one-shot worker.
    exec(
        &w,
        secrets.as_ref(),
        &a.id,
        Tool::Artisan,
        vec![
            "queue:work".into(),
            "--once".into(),
            "--tries=1".into(),
            "--timeout=15".into(),
        ],
    )?;
    assert!(
        php(
            &w,
            secrets.as_ref(),
            &a.id,
            "echo Illuminate\\Support\\Facades\\Cache::get('m9-queue');"
        )?
        .contains("completed")
    );
    w.execute(
        Command::StopCache {
            instance: bc.clone(),
        },
        &cancel,
    )?;
    w.execute(
        Command::StartCache {
            instance: bc.clone(),
        },
        &cancel,
    )?;
    assert!(
        php(
            &w,
            secrets.as_ref(),
            &b.id,
            "echo Illuminate\\Support\\Facades\\Cache::get('m9');"
        )?
        .contains('B')
    );
    // Simultaneous PHP serving, PostgreSQL, MySQL and two Garnet versions.
    w.start(&a.id)?;
    w.start(&b.id)?;
    assert!(
        w.execute(
            Command::StopCache {
                instance: ac.clone()
            },
            &cancel
        )
        .is_err()
    );
    assert!(
        w.execute(
            Command::SwitchDatabase {
                id: a.id.clone(),
                instance: old.instance_id.clone(),
                consent: true
            },
            &cancel
        )
        .is_err()
    );
    w.stop(&a.id)?;
    w.stop(&b.id)?;
    assert_eq!(fs::read(a.path.join(".env")).ok(), old_env);
    verify_retention(&mut w, &a.id, &cancel)?;
    let report = json!({"retained_connection_reactivation":true,"project_default_preserves_history":true,"target":target,"postgresql_staged_restore":true,"mysql":mysql_instance.is_some(),"intel_mysql":"no_current_official_binary; not selected","predis":"^3.7.1","laravel_cache_lock_flush_isolation":true,"laravel_queue_lua":true,"cache_restart_persistence":true,"parallel_garnet_versions":["2.2.11","2.2.10"],"parallel_php_database_services":true,"cross_project_cache_rejected":true,"environment_unchanged":true});
    w.shutdown()?;
    fs::write(
        root.join("m9-native-result.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("M9_NATIVE_REPORT:{report}");
    Ok(())
}

fn verify_retention(w: &mut Workspace, id: &str, cancel: &Cancellation) -> Result<()> {
    let current = w.projects.binding(id)?.unwrap();
    let previous = w
        .projects
        .binding_history(id)?
        .into_iter()
        .find(|b| b.ready && b.database != current.database)
        .ok_or("retained generation missing")?;
    w.execute(
        Command::ActivateDatabaseGeneration {
            id: id.into(),
            instance: previous.instance_id.clone(),
            database: previous.database.clone(),
            consent: true,
        },
        cancel,
    )?;
    assert_eq!(w.projects.binding(id)?.unwrap().database, previous.database);
    w.execute(
        Command::UnbindDatabase {
            id: id.into(),
            consent: true,
        },
        cancel,
    )?;
    assert!(w.projects.binding(id)?.is_none());
    w.execute(
        Command::ActivateDatabaseGeneration {
            id: id.into(),
            instance: current.instance_id.clone(),
            database: current.database.clone(),
            consent: true,
        },
        cancel,
    )?;
    assert_eq!(w.projects.binding(id)?.unwrap().database, current.database);
    assert!(
        w.execute(
            Command::ActivateDatabaseGeneration {
                id: id.into(),
                instance: current.instance_id,
                database: "not-owned".into(),
                consent: true
            },
            cancel
        )
        .is_err()
    );
    Ok(())
}
pub fn retention(root: &Path) -> Result<()> {
    let data = root.join("data");
    let secrets = Arc::new(devforge_workspace::testing::FixtureSecrets::open(
        data.join("e2e-secrets.json"),
    ));
    let mut w = Workspace::open(&data, secrets)?;
    let id = w
        .projects
        .list()?
        .into_iter()
        .find(|p| p.name == "Atlas")
        .unwrap()
        .id;
    verify_retention(&mut w, &id, &Cancellation::default())?;
    w.shutdown()?;
    println!(
        "M9_RETENTION_REPORT:{{\"retained_connection_reactivation\":true,\"project_default_preserves_history\":true}}"
    );
    Ok(())
}
