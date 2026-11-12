//! Isolated M9 native service lifecycle and recovery evidence.
use devforge_core::{ErrorCode, SecretStore};
use devforge_projects::{
    cache::{Response, RunningCache},
    database::{Engine, RunningDatabase},
    project::{Catalog, ProjectStore},
};
use devforge_runtime::{
    Cancellation,
    package::{Limits, PackageStore, Target},
};
use mysql::prelude::Queryable;
use std::{collections::BTreeMap, fs, path::Path, sync::Mutex};
#[cfg(windows)]
#[path = "support/windows_unprivileged.rs"]
mod windows_unprivileged;
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
    #[cfg(windows)]
    if let Some(code) = windows_unprivileged::relaunch()? {
        std::process::exit(code);
    }
    let root = tempfile::tempdir()?;
    let result = run(root.path());
    if result.is_err() {
        show(root.path());
    }
    result
}
fn show(root: &Path) {
    for e in fs::read_dir(root).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            show(&p);
        } else if p.extension().is_some_and(|s| s == "log")
            || p.file_name().is_some_and(|s| s == "ownership-check.json")
        {
            let b = fs::read(&p).unwrap_or_default();
            eprintln!(
                "{}: {}",
                p.display(),
                String::from_utf8_lossy(&b[b.len().saturating_sub(4000)..])
            );
        }
    }
}
fn run(root: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let catalog = Catalog::bundled();
    let target = Target::current()?;
    let secrets = Secrets::default();
    let store = ProjectStore::open(root.join("metadata"))?;
    let packages = PackageStore::open(root.join("packages"), Limits::default())?;
    let cancel = Cancellation::default();
    let select = |kind: &str| {
        catalog
            .packages
            .iter()
            .find(|(_, p)| {
                (p.kind == kind && (kind != "garnet" || p.version == "2.2.11"))
                    && p.target == target
                    && (kind != "postgresql" || p.variant == "server")
            })
            .map(|(id, _)| id.clone())
    };
    let php = catalog
        .packages
        .iter()
        .find(|(_, p)| p.kind == "php" && p.target == target && p.executables.contains_key("php"))
        .unwrap()
        .0;
    let node = select("node").unwrap();
    let path = root.join("A Ўзбек Русский");
    fs::create_dir_all(path.join("public"))?;
    fs::write(path.join("public/index.php"), "<?php")?;
    fs::write(path.join("artisan"), "<?php")?;
    fs::write(
        path.join("composer.json"),
        r#"{"require":{"php":"^8.3","laravel/framework":"^13"}}"#,
    )?;
    let a = store.import(&path, "A", php, &node, &catalog)?;
    for kind in ["garnet", "dotnet", "mysql"] {
        let Some(id) = select(kind) else { continue };
        let p = catalog.get(&id)?;
        eprintln!("INSTALL {id}");
        let local = match kind {
            "garnet" => Some("/tmp/devforge-m9-spike/garnet-arm64.tar.xz"),
            "dotnet" => Some("/tmp/devforge-m9-spike/dotnet-arm64.tar.gz"),
            "mysql" => Some("/tmp/devforge-m9-spike/mysql-8.4.13-macos15-arm64.tar.gz"),
            _ => None,
        };
        if target == Target::MacosArm64 && local.is_some_and(|s| Path::new(s).is_file()) {
            packages.install_cached(p, Path::new(local.unwrap()), &cancel, |_| {})?;
        } else {
            packages.download(p, &cancel, |_| {})?;
        }
    }
    eprintln!("CACHE INIT");
    let cache = store.initialize_cache(
        &a.id,
        &select("garnet").unwrap(),
        &select("dotnet").unwrap(),
        &catalog,
        &packages,
        &secrets,
    )?;
    let mut server = RunningCache::start(&store, &cache.id, &catalog, &packages, &secrets)?;
    eprintln!("PROJECT CLIENT");
    let mut c = store.cache_client(&cache.id, &secrets)?;
    eprintln!("SELECT");
    c.command(&["SELECT", "1"])?;
    eprintln!("SET");
    c.command(&["SET", "sentinel", "Ўзбек Русский"])?;
    assert!(
        matches!(c.command(&["EVAL","return redis.call('GET', KEYS[1])","1","sentinel"] )?,Response::Bytes(v)if v=="Ўзбек Русский".as_bytes())
    );
    eprintln!("STOP");
    server.stop()?;
    drop(server);
    let mut server = RunningCache::start(&store, &cache.id, &catalog, &packages, &secrets)?;
    eprintln!("PROJECT CLIENT");
    let mut c = store.cache_client(&cache.id, &secrets)?;
    eprintln!("SELECT");
    c.command(&["SELECT", "1"])?;
    assert!(
        matches!(c.command(&["GET","sentinel"] )?,Response::Bytes(v)if v=="Ўзбек Русский".as_bytes())
    );
    eprintln!("FLUSH");
    c.command(&["FLUSHDB"])?;
    server.stop()?;
    let Some(mysql) = select("mysql") else {
        println!("{{\"cache_persistence\":true,\"mysql\":\"unavailable_for_target\"}}");
        return Ok(());
    };
    eprintln!("MYSQL INIT");
    let i = store.initialize_instance(&mysql, 0, &catalog, &packages, &secrets)?;
    assert_eq!(i.engine, Engine::Mysql);
    let mut server = RunningDatabase::start(&store, &i.id, &catalog, &packages, &secrets)?;
    let original = server.provision(&store, &a, &secrets)?;
    let credentials = store.credentials(&a.id, &secrets)?;
    assert!(credentials.connection_test()?);
    assert!(credentials.rejects_database("mysql"));
    let mut conn = credentials.mysql()?;
    conn.query_drop("CREATE TABLE sentinel (value varchar(100))")?;
    conn.query_drop("INSERT INTO sentinel VALUES ('A')")?;
    let backup = store.create_backup(&a.id, &catalog, &packages, &secrets, &cancel)?;
    conn.query_drop("UPDATE sentinel SET value='changed'")?;
    let restored = store.restore_backup(
        &a.id,
        &backup.id,
        &mut server,
        &catalog,
        &packages,
        &secrets,
        &cancel,
    )?;
    assert_ne!(restored.database, original.database);
    let found: Option<String> = store
        .credentials(&a.id, &secrets)?
        .mysql()?
        .query_first("SELECT value FROM sentinel")?;
    assert_eq!(found.as_deref(), Some("A"));
    let old: Option<String> = conn.query_first("SELECT value FROM sentinel")?;
    assert_eq!(old.as_deref(), Some("changed"));
    // A checksum-valid SQL archive must not execute mysql client shell commands.
    use sha2::{Digest, Sha256};
    use std::io::Write;
    let marker = root.join("must-not-exist.txt");
    let payload = format!(
        "\\! echo compromised > \"{}\"\nTHIS IS INVALID SQL;\n",
        marker.display()
    );
    let mut bad = backup.clone();
    bad.id = uuid::Uuid::new_v4().simple().to_string();
    bad.bytes = payload.len() as u64;
    bad.sha256 = Sha256::digest(payload.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    {
        let mut zip = zip::ZipWriter::new(fs::File::create(store.backup_path(&a.id, &bad.id)?)?);
        let o = zip::write::SimpleFileOptions::default();
        zip.start_file("manifest.json", o)?;
        zip.write_all(&serde_json::to_vec(&bad)?)?;
        zip.start_file("payload", o)?;
        zip.write_all(payload.as_bytes())?;
        zip.finish()?;
    }
    assert!(
        store
            .restore_backup(
                &a.id,
                &bad.id,
                &mut server,
                &catalog,
                &packages,
                &secrets,
                &cancel
            )
            .is_err()
    );
    assert!(!marker.exists());
    assert_eq!(store.binding(&a.id)?.unwrap().database, restored.database);
    assert!(store.binding_history(&a.id)?.iter().any(|b| !b.ready));
    let export = root.join("copy.dfbackup");
    store.export_backup(&a.id, &backup.id, &export)?;
    assert!(devforge_projects::backup::inspect(&export).is_ok());
    assert_eq!(
        store.export_backup(&a.id, &backup.id, &export).err(),
        Some(devforge_projects::ProjectError::BackupDestinationExists)
    );
    let bpath = root.join("B project");
    fs::create_dir_all(bpath.join("public"))?;
    fs::copy(a.path.join("composer.json"), bpath.join("composer.json"))?;
    fs::write(bpath.join("public/index.php"), "<?php")?;
    fs::write(bpath.join("artisan"), "<?php")?;
    let b = store.import(&bpath, "B", php, &node, &catalog)?;
    let imported = store.import_backup(&b.id, &export)?;
    server.provision(&store, &b, &secrets)?;
    let copied = store.restore_backup(
        &b.id,
        &imported.id,
        &mut server,
        &catalog,
        &packages,
        &secrets,
        &cancel,
    )?;
    assert!(
        store
            .credentials(&a.id, &secrets)?
            .rejects_database(&copied.database)
    );
    let value: Option<String> = store
        .credentials(&b.id, &secrets)?
        .mysql()?
        .query_first("SELECT value FROM sentinel")?;
    assert_eq!(value.as_deref(), Some("A"));
    server.stop()?;
    println!(
        "{{\"cache_persistence\":true,\"mysql\":true,\"staged_restore\":true,\"original_preserved\":true,\"failed_restore_preserves_active\":true,\"client_shell_escape_rejected\":true,\"export_verified\":true,\"import_restore_isolated\":true}}"
    );
    Ok(())
}
