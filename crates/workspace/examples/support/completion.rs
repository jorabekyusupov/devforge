//! Remaining M7–M12 acceptance against isolated, real native runtimes.
use super::*;
use devforge_core::SecretStore;
use devforge_projects::{backup::remote::Source, database::Engine};
use devforge_workspace::worktrees::{DatabaseMode, Input};
fn git(path: &Path, args: &[&str]) -> Result<String> {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(path)
        .args(args)
        .output()?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).into_owned().into());
    }
    Ok(String::from_utf8(out.stdout)?.trim().into())
}
fn archive(w: &mut Workspace, id: &str, operation: &str) -> Result<()> {
    let plan = w.worktree_archive_plan(id, operation)?;
    assert!(plan.blockers.is_empty(), "{:?}", plan.blockers);
    w.worktree_archive(id, operation, &plan.digest, true, &Default::default())?;
    Ok(())
}
fn cleanup(w: &mut Workspace, id: &str) -> Result<()> {
    let plan = w.worktree_cleanup_plan(id)?;
    assert!(plan.blockers.is_empty(), "{:?}", plan.blockers);
    w.worktree_cleanup(id, &plan.digest, true, &Default::default())?;
    Ok(())
}
pub fn run(root: &Path) -> Result<()> {
    let data = root.join("data");
    let secrets = Arc::new(devforge_workspace::testing::FixtureSecrets::open(
        data.join("e2e-secrets.json"),
    ));
    let mut w = Workspace::open(&data, secrets.clone())?;
    let cancel = Cancellation::default();
    let atlas = w
        .projects
        .list()?
        .into_iter()
        .find(|p| p.name == "Atlas")
        .ok_or("Atlas missing")?;
    let source_path = root.join("Completion source Ўзбек");
    fs::create_dir_all(source_path.join("public"))?;
    fs::create_dir_all(source_path.join("database"))?;
    fs::write(
        source_path.join("public/index.php"),
        "<?php echo 'completion';",
    )?;
    fs::write(
        source_path.join("composer.json"),
        r#"{"require":{"php":"^8.3","laravel/framework":"^13"}}"#,
    )?;
    fs::write(source_path.join(".env.example"), "APP_KEY=\n")?;
    fs::write(source_path.join(".gitignore"), ".env\n/database/*.sqlite\n")?;
    fs::write(source_path.join("database/.gitkeep"), "")?;
    let source = w.projects.import(
        &source_path,
        "Completion source",
        &atlas.php,
        &atlas.node,
        &w.catalog,
    )?;
    w.sites.configure(
        &w.projects,
        devforge_serving::site::Site {
            project_id: source.id.clone(),
            domain: "completion-source.devforge.test".into(),
            https: false,
            vite: false,
        },
    )?;
    let pg = w.projects.binding(&atlas.id)?.unwrap().instance_id;
    w.execute(
        Command::SwitchDatabase {
            id: source.id.clone(),
            instance: pg,
            consent: true,
        },
        &cancel,
    )?;
    git(&source_path, &["init", "-q"])?;
    git(&source_path, &["config", "user.name", "DevForge fixture"])?;
    git(
        &source_path,
        &["config", "user.email", "fixture@example.test"],
    )?;
    git(&source_path, &["add", "."])?;
    git(&source_path, &["commit", "-qm", "source"])?;
    let source_head = git(&source_path, &["rev-parse", "HEAD"])?;
    let input = |branch: &str| Input {
        source_id: source.id.clone(),
        name: format!("Completion {branch}"),
        branch: branch.into(),
        base: "HEAD".into(),
        new_branch: true,
        database: DatabaseMode::Empty,
        dependencies: false,
        copy_environment: false,
    };
    eprintln!("COMPLETION: actual linked workspace creation, config save and reviewed apply");
    let request = input("feature/completion-linked");
    let plan = w.worktree_plan(&request)?;
    let child = w.worktree_create(request, &plan.digest, true, &cancel)?;
    let id = child.project_id.clone().unwrap();
    let p = w.projects.get(&id)?;
    let original = w.configuration_document(&id)?;
    let text = format!(
        "{}\n# GUI-compatible native save\n",
        original["text"].as_str().unwrap()
    );
    let saved = w.configuration_save(&id, original["expected"].as_str(), &text, true)?;
    assert!(
        w.configuration_save(&id, original["expected"].as_str(), &text, true)
            .is_err()
    );
    let plan = w.configuration_plan(&id, false)?;
    assert!(plan["blockers"].as_array().unwrap().is_empty(), "{plan}");
    w.configuration_apply(&id, plan["digest"].as_str().unwrap(), true, false, &cancel)?;
    assert_eq!(
        w.configuration_document(&id)?["expected"],
        saved["expected"]
    );
    // The portable YAML is an explicit project file change. Commit before archive, as required.
    git(&p.path, &["add", "devforge.yaml"])?;
    git(
        &p.path,
        &[
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.test",
            "commit",
            "-qm",
            "portable configuration",
        ],
    )?;
    eprintln!("COMPLETION: read-only PostgreSQL external dump and staged local restore");
    w.execute(
        Command::TestDatabase {
            id: source.id.clone(),
        },
        &cancel,
    )?;
    w.projects.credentials(&source.id,secrets.as_ref())?.connect()?.batch_execute("CREATE TABLE completion_values(value INTEGER);INSERT INTO completion_values VALUES(73)")?;
    let b = w.projects.binding(&source.id)?.unwrap();
    let instance = w.projects.instance(&b.instance_id)?;
    w.manifest_secret(
        &p.path,
        "remote-password",
        &secrets
            .get(&b.secret_ref)?
            .ok_or("missing source password")?,
    )?;
    let remote = Source {
        host: "127.0.0.1".into(),
        port: instance.port,
        database: b.database.clone(),
        user: b.role,
        password_ref: "remote-password".into(),
        tls_ca: None,
        loopback_unencrypted: true,
    };
    let plan = w.worktree_remote_plan(&child.id, &remote)?;
    let old = w.projects.binding(&id)?.unwrap();
    w.worktree_remote_copy(
        &child.id,
        &remote,
        plan["digest"].as_str().unwrap(),
        true,
        &cancel,
    )?;
    let copied: i32 = w
        .projects
        .credentials(&id, secrets.as_ref())?
        .connect()?
        .query_one("SELECT value FROM completion_values", &[])?
        .get(0);
    assert_eq!(copied, 73);
    assert_ne!(w.projects.binding(&id)?.unwrap().database, old.database);
    assert!(
        w.projects
            .binding_history(&id)?
            .iter()
            .any(|b| b.database == old.database)
    );
    assert_eq!(
        w.projects
            .credentials(&source.id, secrets.as_ref())?
            .connect()?
            .query_one("SELECT value FROM completion_values", &[])?
            .get::<_, i32>(0),
        73
    );
    let current = w.projects.binding(&id)?.unwrap();
    let mut rejected = remote.clone();
    rejected.port = 1;
    let bad = w.worktree_remote_plan(&child.id, &rejected)?;
    assert!(
        w.worktree_remote_copy(
            &child.id,
            &rejected,
            bad["digest"].as_str().unwrap(),
            true,
            &cancel
        )
        .is_err()
    );
    assert_eq!(w.projects.binding(&id)?.unwrap().database, current.database);
    fs::write(p.path.join(".env"), "PRIVATE=archive-sentinel\n")?;
    cleanup(&mut w, &child.id)?;
    archive(&mut w, &child.id, "restore")?;
    assert_eq!(w.projects.get(&id)?.id, id);
    assert!(fs::read_to_string(p.path.join(".env"))?.contains("archive-sentinel"));
    assert_eq!(w.projects.binding(&id)?.unwrap().database, current.database);
    cleanup(&mut w, &child.id)?;
    archive(&mut w, &child.id, "purge")?;
    assert!(w.projects.binding(&id)?.is_some());
    assert_eq!(git(&source_path, &["rev-parse", "HEAD"])?, source_head);
    eprintln!(
        "COMPLETION: real recursive submodule checkout, dirty refusal, archive, restore and recovery bundle"
    );
    let module = root.join("Completion module");
    fs::create_dir_all(&module)?;
    git(&module, &["init", "-q"])?;
    git(&module, &["config", "user.name", "Fixture"])?;
    git(&module, &["config", "user.email", "fixture@example.test"])?;
    fs::write(module.join("module.txt"), "owned-module\n")?;
    git(&module, &["add", "."])?;
    git(&module, &["commit", "-qm", "module"])?;
    git(
        &source_path,
        &[
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "add",
            module.to_str().unwrap(),
            "modules/shared",
        ],
    )?;
    git(&source_path, &["commit", "-qam", "submodule source"])?;
    let request = input("feature/completion-submodules");
    let plan = w.worktree_plan(&request)?;
    assert!(plan.isolated);
    assert!(plan.submodule_sources.contains("modules/shared"));
    let isolated = w.worktree_create(request, &plan.digest, true, &cancel)?;
    let ip = w.projects.get(isolated.project_id.as_deref().unwrap())?;
    assert_eq!(
        fs::read_to_string(ip.path.join("modules/shared/module.txt"))?,
        "owned-module\n"
    );
    fs::write(ip.path.join("modules/shared/module.txt"), "dirty\n")?;
    assert!(w.worktree_cleanup_plan(&isolated.id)?.dirty);
    git(
        &ip.path.join("modules/shared"),
        &["checkout", "--", "module.txt"],
    )?;
    cleanup(&mut w, &isolated.id)?;
    archive(&mut w, &isolated.id, "restore")?;
    assert_eq!(
        fs::read_to_string(ip.path.join("modules/shared/module.txt"))?,
        "owned-module\n"
    );
    cleanup(&mut w, &isolated.id)?;
    archive(&mut w, &isolated.id, "purge")?;
    eprintln!("COMPLETION: native MySQL provider on every target and external table copy");
    let mysql = w
        .catalog
        .packages
        .iter()
        .find(|(_, p)| p.kind == "mysql" && p.target == Target::current().unwrap())
        .unwrap()
        .0
        .clone();
    w.packages
        .download(w.catalog.get(&mysql)?, &cancel, |_| {})?;
    let instance = w.execute(
        Command::CreateDatabase {
            package: mysql.clone(),
        },
        &cancel,
    )?;
    let mid = instance["id"].as_str().unwrap().to_string();
    w.execute(
        Command::SwitchDatabase {
            id: source.id.clone(),
            instance: mid.clone(),
            consent: true,
        },
        &cancel,
    )?;
    let request = input("feature/completion-mysql");
    let plan = w.worktree_plan(&request)?;
    let mysql_child = w.worktree_create(request, &plan.digest, true, &cancel)?;
    let mp = w.projects.get(mysql_child.project_id.as_deref().unwrap())?;
    use mysql::prelude::Queryable;
    w.projects
        .credentials(&source.id, secrets.as_ref())?
        .mysql()?
        .query_drop("CREATE TABLE completion_values(value INT) ENGINE=InnoDB")?;
    w.projects
        .credentials(&source.id, secrets.as_ref())?
        .mysql()?
        .query_drop("INSERT INTO completion_values VALUES(91)")?;
    let b = w.projects.binding(&source.id)?.unwrap();
    let db = w.projects.instance(&mid)?;
    w.manifest_secret(
        &mp.path,
        "remote-password",
        &secrets.get(&b.secret_ref)?.unwrap(),
    )?;
    let remote = Source {
        host: "127.0.0.1".into(),
        port: db.port,
        database: b.database.clone(),
        user: b.role,
        password_ref: "remote-password".into(),
        tls_ca: None,
        loopback_unencrypted: true,
    };
    let plan = w.worktree_remote_plan(&mysql_child.id, &remote)?;
    w.worktree_remote_copy(
        &mysql_child.id,
        &remote,
        plan["digest"].as_str().unwrap(),
        true,
        &cancel,
    )?;
    let value: Option<i32> = w
        .projects
        .credentials(&mp.id, secrets.as_ref())?
        .mysql()?
        .query_first("SELECT value FROM completion_values")?;
    assert_eq!(value, Some(91));
    assert_eq!(w.projects.instance(&mid)?.engine, Engine::Mysql);
    w.shutdown()?;
    fs::write(
        root.join("completion-native-result.json"),
        serde_json::to_vec_pretty(
            &json!({"passed":true,"target":Target::current()?,"mysql_package":mysql,"checks":{"configuration_atomic_save_review_apply":true,"linked_archive_restore_purge":true,"identity_and_database_retained":true,"recursive_submodule_create_archive_restore_purge":true,"dirty_submodule_blocked":true,"external_postgresql_read_dump_staged_copy":true,"external_mysql_tables_staged_copy":true,"failed_external_connection_preserves_binding":true,"source_data_unchanged":true},"ui_project":mp.id,"ui_workspace":mysql_child.id}),
        )?,
    )?;
    Ok(())
}
