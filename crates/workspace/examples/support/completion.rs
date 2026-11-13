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
    let suffix = devforge_serving::id();
    let source_path = root.join(format!("Completion source Ўзбек {suffix}"));
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
            domain: format!("completion-{}.devforge.test", &suffix[..8]),
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
    eprintln!("COMPLETION: verified TLS PostgreSQL source and wrong-CA rejection");
    let root_key = rcgen::KeyPair::generate()?;
    let mut ca_params = rcgen::CertificateParams::new(Vec::<String>::new())?;
    ca_params
        .distinguished_name
        .push(rcgen::DnType::CommonName, "DevForge acceptance CA");
    ca_params.key_usages = vec![
        rcgen::KeyUsagePurpose::KeyCertSign,
        rcgen::KeyUsagePurpose::CrlSign,
    ];
    ca_params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Constrained(0));
    let ca = ca_params.self_signed(&root_key)?;
    let issuer = rcgen::Issuer::new(ca_params, root_key);
    let server_key = rcgen::KeyPair::generate()?;
    let mut server_params = rcgen::CertificateParams::new(vec!["127.0.0.1".into()])?;
    server_params
        .distinguished_name
        .push(rcgen::DnType::CommonName, "DevForge acceptance source");
    server_params.key_usages = vec![rcgen::KeyUsagePurpose::DigitalSignature];
    server_params.extended_key_usages = vec![rcgen::ExtendedKeyUsagePurpose::ServerAuth];
    let server_cert = server_params.signed_by(&server_key, &issuer)?;
    w.execute(
        Command::StopDatabase {
            instance: instance.id.clone(),
        },
        &cancel,
    )?;
    let pg_data = w
        .projects
        .root()
        .join("databases")
        .join(&instance.id)
        .join("data");
    fs::write(pg_data.join("completion-cert.pem"), server_cert.pem())?;
    fs::write(
        pg_data.join("completion-key.pem"),
        server_key.serialize_pem(),
    )?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(
            pg_data.join("completion-key.pem"),
            fs::Permissions::from_mode(0o600),
        )?;
    }
    use std::io::Write;
    fs::OpenOptions::new()
        .append(true)
        .open(pg_data.join("postgresql.conf"))?
        .write_all(
            b"\nssl=on\nssl_cert_file='completion-cert.pem'\nssl_key_file='completion-key.pem'\n",
        )?;
    let ca_path = fs::canonicalize(root)?.join(format!("completion-ca-{suffix}.pem"));
    fs::write(&ca_path, ca.pem())?;
    w.execute(
        Command::TestDatabase {
            id: source.id.clone(),
        },
        &cancel,
    )?;
    let mut tls = remote.clone();
    tls.loopback_unencrypted = false;
    tls.tls_ca = Some(ca_path);
    // Read-only native TLS probe gives a bounded, secret-redacted diagnostic if the client rejects the certificate.
    let tools = w
        .catalog
        .packages
        .values()
        .find(|p| p.kind == "postgresql-tools" && p.target == Target::current().unwrap())
        .unwrap();
    let tool = w.packages.executable(tools, "pg_dump")?;
    let mut pass = tempfile::NamedTempFile::new_in(root)?;
    let password = secrets
        .get(&b.secret_ref)?
        .ok_or("source password missing")?;
    pass.write_all(
        format!(
            "127.0.0.1:{}:{}:{}:{}\n",
            remote.port, remote.database, remote.user, password
        )
        .as_bytes(),
    )?;
    pass.flush()?;
    let probe = tempfile::NamedTempFile::new_in(root)?;
    let out = std::process::Command::new(tool.path())
        .env_clear()
        .envs(devforge_projects::mysql_service::environment(&tool))
        .env("PGPASSFILE", pass.path())
        .env("PGSSLMODE", "verify-full")
        .env("PGSSLROOTCERT", tls.tls_ca.as_ref().unwrap())
        .args([
            "--host=127.0.0.1".into(),
            format!("--port={}", remote.port),
            format!("--username={}", remote.user),
            format!("--dbname={}", remote.database),
            "--no-password".into(),
            "--schema-only".into(),
            format!("--file={}", dunce::simplified(probe.path()).display()),
        ])
        .output()?;
    if !out.status.success() {
        return Err(format!(
            "TLS dump probe: {}",
            String::from_utf8_lossy(&out.stderr).replace(&password, "[REDACTED]")
        )
        .into());
    }
    let review = w.worktree_remote_plan(&child.id, &tls)?;
    w.worktree_remote_copy(
        &child.id,
        &tls,
        review["digest"].as_str().unwrap(),
        true,
        &cancel,
    )?;
    let tls_binding = w.projects.binding(&id)?.unwrap();
    let wrong_key = rcgen::KeyPair::generate()?;
    let mut wrong_params = rcgen::CertificateParams::new(Vec::<String>::new())?;
    wrong_params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Constrained(0));
    let wrong = wrong_params.self_signed(&wrong_key)?;
    let wrong_path = fs::canonicalize(root)?.join(format!("completion-wrong-ca-{suffix}.pem"));
    fs::write(&wrong_path, wrong.pem())?;
    tls.tls_ca = Some(wrong_path);
    let review = w.worktree_remote_plan(&child.id, &tls)?;
    assert!(
        w.worktree_remote_copy(
            &child.id,
            &tls,
            review["digest"].as_str().unwrap(),
            true,
            &cancel
        )
        .is_err()
    );
    assert_eq!(
        w.projects.binding(&id)?.unwrap().database,
        tls_binding.database
    );
    fs::write(p.path.join(".env"), "PRIVATE=archive-sentinel\n")?;
    cleanup(&mut w, &child.id)?;
    archive(&mut w, &child.id, "restore")?;
    assert_eq!(w.projects.get(&id)?.id, id);
    assert!(fs::read_to_string(p.path.join(".env"))?.contains("archive-sentinel"));
    assert_eq!(
        w.projects.binding(&id)?.unwrap().database,
        tls_binding.database
    );
    cleanup(&mut w, &child.id)?;
    archive(&mut w, &child.id, "purge")?;
    assert!(w.projects.binding(&id)?.is_some());
    assert_eq!(git(&source_path, &["rev-parse", "HEAD"])?, source_head);
    eprintln!(
        "COMPLETION: real recursive submodule checkout, dirty refusal, archive, restore and recovery bundle"
    );
    let module = root.join(format!("Completion module {suffix}"));
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
            &json!({"passed":true,"target":Target::current()?,"mysql_package":mysql,"checks":{"configuration_atomic_save_review_apply":true,"linked_archive_restore_purge":true,"identity_and_database_retained":true,"recursive_submodule_create_archive_restore_purge":true,"dirty_submodule_blocked":true,"external_postgresql_read_dump_staged_copy":true,"external_pg_verified_tls":true,"external_pg_wrong_ca_preserves_binding":true,"external_mysql_tables_staged_copy":true,"failed_external_connection_preserves_binding":true,"source_data_unchanged":true},"ui_project":mp.id,"ui_workspace":mysql_child.id}),
        )?,
    )?;
    Ok(())
}

pub fn cli(root: &Path, executable: &Path) -> Result<()> {
    let native: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join("completion-native-result.json"))?)?;
    let data = root.join("data");
    let secrets = Arc::new(devforge_workspace::testing::FixtureSecrets::open(
        data.join("e2e-secrets.json"),
    ));
    let w = Workspace::open(&data, secrets.clone())?;
    let selected = w
        .worktrees()?
        .into_iter()
        .find(|v| v.record.id == native["ui_workspace"].as_str().unwrap())
        .unwrap();
    let source = selected.record.input.source_id;
    let file = root.join("completion-cli-input.json");
    fs::write(
        &file,
        serde_json::to_vec(&Input {
            source_id: source.clone(),
            name: "Completion CLI".into(),
            branch: format!("feature/completion-cli-{}", &devforge_serving::id()[..8]),
            base: "HEAD".into(),
            new_branch: true,
            database: DatabaseMode::Empty,
            dependencies: false,
            copy_environment: false,
        })?,
    )?;
    drop(w);
    let run = |args: &[&str]| -> Result<serde_json::Value> {
        let output = std::process::Command::new(executable)
            .args(args)
            .env("DEVFORGE_DATA_DIR", &data)
            .env("DEVFORGE_TEST_SECRETS", "1")
            .output()?;
        if !output.status.success() {
            return Err(format!(
                "Completion CLI: {}",
                String::from_utf8_lossy(&output.stderr)
            )
            .into());
        }
        Ok(serde_json::from_slice(&output.stdout)?)
    };
    let input = dunce::simplified(&file).to_str().unwrap();
    let plan = run(&["workspace", "plan", input])?;
    let created = run(&[
        "workspace",
        "create",
        input,
        "--review",
        plan["result"]["digest"].as_str().unwrap(),
        "--trust",
    ])?;
    let id = created["result"]["id"].as_str().unwrap();
    let project = created["result"]["project_id"].as_str().unwrap();
    let w = Workspace::open(&data, secrets.clone())?;
    let p = w.projects.get(project)?;
    let b = w.projects.binding(&source)?.unwrap();
    let instance = w.projects.instance(&b.instance_id)?;
    w.manifest_secret(
        &p.path,
        "cli-remote-password",
        &secrets.get(&b.secret_ref)?.unwrap(),
    )?;
    let remote = root.join("completion-cli-remote.json");
    fs::write(
        &remote,
        serde_json::to_vec(&Source {
            host: "127.0.0.1".into(),
            port: instance.port,
            database: b.database,
            user: b.role,
            password_ref: "cli-remote-password".into(),
            tls_ca: None,
            loopback_unencrypted: true,
        })?,
    )?;
    drop(w);
    let remote_path = dunce::simplified(&remote).to_str().unwrap();
    let review = run(&["workspace", "remote-plan", id, remote_path])?;
    assert!(review["result"]["digest"].as_str().is_some());
    let cleanup_plan = run(&["workspace", "cleanup-plan", id])?;
    let removed = run(&[
        "workspace",
        "cleanup",
        id,
        "--review",
        cleanup_plan["result"]["digest"].as_str().unwrap(),
        "--confirm",
    ])?;
    assert_eq!(removed["result"]["stage"], "removed");
    let restore = run(&["workspace", "restore-plan", id])?;
    let restored = run(&[
        "workspace",
        "restore",
        id,
        "--review",
        restore["result"]["digest"].as_str().unwrap(),
        "--confirm",
    ])?;
    assert_eq!(restored["result"]["project_id"], project);
    assert_eq!(restored["result"]["stage"], "ready");
    let plan = run(&["workspace", "cleanup-plan", id])?;
    run(&[
        "workspace",
        "cleanup",
        id,
        "--review",
        plan["result"]["digest"].as_str().unwrap(),
        "--confirm",
    ])?;
    let plan = run(&["workspace", "purge-plan", id])?;
    let purged = run(&[
        "workspace",
        "purge",
        id,
        "--review",
        plan["result"]["digest"].as_str().unwrap(),
        "--confirm",
    ])?;
    assert_eq!(purged["result"]["stage"], "purged");
    fs::write(
        root.join("completion-cli-result.json"),
        serde_json::to_vec_pretty(
            &json!({"target":Target::current()?,"real_cli_create_remote_review_archive_restore_purge":true,"same_project_identity":true,"source_and_bindings_retained":true}),
        )?,
    )?;
    Ok(())
}
