//! Logical backups are private, bounded archives. Restores run as a freshly
//! provisioned project role and activate only after success; old data is retained.
use crate::{
    ProjectError as E, Result,
    database::{Binding, Credentials, Engine, RunningDatabase},
    project::{Catalog, ProjectStore},
};
use devforge_core::SecretStore;
use devforge_runtime::{
    Cancellation,
    package::PackageStore,
    process::{LogPolicy, Observed, ProcessSpec, Readiness, RestartPolicy, Supervisor},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::{Read, Write},
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant},
};
use zeroize::Zeroizing;
pub mod drill;
pub mod remote;
const LIMIT: u64 = 1024 * 1024 * 1024;
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub schema: u32,
    pub id: String,
    pub project: String,
    pub engine: Engine,
    pub version: String,
    pub created: u64,
    pub sha256: String,
    pub bytes: u64,
}
fn hash(path: &Path) -> Result<String> {
    let mut file = File::open(path)?;
    let mut h = Sha256::new();
    let mut b = [0; 65536];
    loop {
        let n = file.read(&mut b)?;
        if n == 0 {
            break;
        }
        h.update(&b[..n]);
    }
    Ok(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
}
fn valid_id(id: &str) -> bool {
    id.len() == 32 && id.bytes().all(|b| b.is_ascii_hexdigit())
}
fn options(c: &Credentials, dir: &Path) -> Result<tempfile::NamedTempFile> {
    let mut file = tempfile::NamedTempFile::new_in(dir)?;
    let text = if c.engine == Engine::Postgresql {
        format!(
            "127.0.0.1:{}:{}:{}:{}\n",
            c.port,
            c.binding.database,
            c.binding.role,
            c.password.as_str()
        )
    } else {
        // Generated credentials contain only ASCII alphanumerics; no config injection.
        format!(
            "[client]\nhost=127.0.0.1\nport={}\nuser={}\npassword={}\nprotocol=TCP\nssl-mode=DISABLED\nserver-public-key-path={}\n",
            c.port,
            c.binding.role,
            c.password.as_str(),
            c.root
                .join("data/public_key.pem")
                .to_string_lossy()
                .replace('\\', "/")
        )
    };
    let text = Zeroizing::new(text);
    file.write_all(text.as_bytes())?;
    file.as_file().sync_all()?;
    Ok(file)
}
#[allow(
    clippy::too_many_arguments,
    reason = "One process boundary carries credentials, verified packages and cancellation explicitly"
)]
fn run(
    c: &Credentials,
    package: &str,
    tool: &str,
    args: Vec<std::ffi::OsString>,
    input: Option<PathBuf>,
    catalog: &Catalog,
    packages: &PackageStore,
    dir: &Path,
    cancel: &Cancellation,
) -> Result<()> {
    run_connection(
        c, package, tool, args, input, catalog, packages, dir, cancel, None,
    )
}
#[allow(
    clippy::too_many_arguments,
    reason = "Existing backup process boundary with an optional reviewed external source"
)]
fn run_connection(
    c: &Credentials,
    package: &str,
    tool: &str,
    mut args: Vec<std::ffi::OsString>,
    input: Option<PathBuf>,
    catalog: &Catalog,
    packages: &PackageStore,
    dir: &Path,
    cancel: &Cancellation,
    source: Option<&remote::Source>,
) -> Result<()> {
    let descriptor = if c.engine == Engine::Postgresql {
        catalog
            .packages
            .values()
            .find(|p| {
                p.kind == "postgresql-tools"
                    && p.target == devforge_runtime::package::Target::current().unwrap()
            })
            .ok_or(E::RuntimeUnavailable)?
    } else {
        catalog.get(package)?
    };
    let executable = packages.executable(descriptor, tool)?;
    let config = if let Some(source) = source {
        remote::options(source, c, dir)?
    } else {
        options(c, dir)?
    };
    let mut env = crate::mysql_service::environment(&executable);
    if c.engine == Engine::Postgresql {
        env.insert(
            "PGPASSFILE".into(),
            dunce::simplified(config.path()).as_os_str().to_owned(),
        );
        env.insert("PGCONNECT_TIMEOUT".into(), "5".into());
        args.extend([
            format!(
                "--host={}",
                source.map(|s| s.host.as_str()).unwrap_or("127.0.0.1")
            )
            .into(),
            format!("--port={}", c.port).into(),
            format!("--username={}", c.binding.role).into(),
            "--no-password".into(),
        ]);
    } else {
        args.insert(
            0,
            format!(
                "--defaults-file={}",
                dunce::simplified(config.path()).display()
            )
            .into(),
        );
        args.insert(1, "--no-login-paths".into());
    }
    if let Some(source) = source {
        env.insert(
            "PGSSLMODE".into(),
            if source.loopback_unencrypted {
                "disable"
            } else {
                "verify-full"
            }
            .into(),
        );
        if let Some(ca) = &source.tls_ca {
            env.insert(
                "PGSSLROOTCERT".into(),
                dunce::simplified(ca).as_os_str().into(),
            );
        }
        if c.engine == Engine::Postgresql {
            env.insert(
                "PGOPTIONS".into(),
                "-c default_transaction_read_only=on".into(),
            );
        }
    }
    let mut supervisor = Supervisor::open(dir.join("logs"))?;
    supervisor.start_redacted(
        ProcessSpec {
            input,
            id: "backup-tool".into(),
            executable,
            args,
            directory: dir.into(),
            environment: env,
            readiness: Readiness::SuccessfulExit,
            readiness_timeout: Duration::from_secs(300),
            stop_grace: Duration::from_secs(5),
            restart: RestartPolicy::default(),
            logs: LogPolicy {
                capture: false,
                ..Default::default()
            },
        },
        vec![c.password.to_string()],
    )?;
    let until = Instant::now() + Duration::from_secs(310);
    loop {
        let state = supervisor.snapshot("backup-tool")?;
        match state.observed {
            Observed::Succeeded => {
                supervisor.stop("backup-tool")?;
                return Ok(());
            }
            Observed::Failed | Observed::Stopped => return Err(E::ExecutionFailed),
            _ => {}
        }
        if fs::metadata(dir.join("payload")).is_ok_and(|m| m.len() > LIMIT) {
            supervisor.stop("backup-tool")?;
            return Err(E::InvalidBackup);
        }
        if cancel.is_cancelled() || Instant::now() > until {
            supervisor.stop("backup-tool")?;
            return Err(E::ExecutionFailed);
        }
        thread::sleep(Duration::from_millis(50));
    }
}
impl ProjectStore {
    fn backup_root(&self, project: &str) -> Result<PathBuf> {
        self.get(project)?;
        let dir = self.root.join("backups").join(project);
        crate::private_dir(&dir)?;
        Ok(dir)
    }
    pub fn backups(&self, project: &str) -> Result<Vec<Manifest>> {
        let root = self.backup_root(project)?;
        let mut items = vec![];
        for entry in fs::read_dir(root)? {
            let path = entry?.path();
            if path.extension().and_then(|s| s.to_str()) == Some("dfbackup")
                && let Ok((manifest, _)) = inspect_header(&path)
            {
                items.push(manifest);
            }
        }
        items.sort_by_key(|m| std::cmp::Reverse(m.created));
        Ok(items)
    }
    pub fn backup_path(&self, project: &str, id: &str) -> Result<PathBuf> {
        if !valid_id(id) {
            return Err(E::InvalidBackup);
        }
        Ok(self.backup_root(project)?.join(format!("{id}.dfbackup")))
    }
    pub fn create_backup(
        &self,
        project: &str,
        catalog: &Catalog,
        packages: &PackageStore,
        secrets: &dyn SecretStore,
        cancel: &Cancellation,
    ) -> Result<Manifest> {
        let c = self.credentials(project, secrets)?;
        let instance = self.instance(&c.binding.instance_id)?;
        let package = catalog.get(&instance.package)?;
        let root = self.backup_root(project)?;
        let temp = tempfile::tempdir_in(&root)?;
        let payload = temp.path().join("payload");
        if c.engine == Engine::Mysql {
            use mysql::prelude::Queryable;
            let mut conn = crate::mysql_service::connect(
                &c.binding.role,
                &c.password,
                Some(&c.binding.database),
                c.port,
                &c.root,
            )?;
            let count:Option<u64>=conn.exec_first("SELECT (SELECT COUNT(*) FROM information_schema.TRIGGERS WHERE TRIGGER_SCHEMA=?)+(SELECT COUNT(*) FROM information_schema.ROUTINES WHERE ROUTINE_SCHEMA=?)+(SELECT COUNT(*) FROM information_schema.EVENTS WHERE EVENT_SCHEMA=?)+(SELECT COUNT(*) FROM information_schema.VIEWS WHERE TABLE_SCHEMA=?)+(SELECT COUNT(*) FROM information_schema.TABLES WHERE TABLE_SCHEMA=? AND TABLE_TYPE='BASE TABLE' AND ENGINE<>'InnoDB')",(&c.binding.database,&c.binding.database,&c.binding.database,&c.binding.database,&c.binding.database)).map_err(|_|E::DatabaseUnavailable)?;
            if count != Some(0) {
                return Err(E::BackupUnsupported);
            }
            run(
                &c,
                &instance.package,
                "mysqldump",
                vec![
                    "--single-transaction".into(),
                    "--no-tablespaces".into(),
                    "--set-gtid-purged=OFF".into(),
                    "--skip-triggers".into(),
                    format!("--result-file={}", dunce::simplified(&payload).display()).into(),
                    c.binding.database.clone().into(),
                ],
                None,
                catalog,
                packages,
                temp.path(),
                cancel,
            )?;
        } else {
            run(
                &c,
                &instance.package,
                "pg_dump",
                vec![
                    "--format=custom".into(),
                    "--no-owner".into(),
                    "--no-privileges".into(),
                    format!("--file={}", dunce::simplified(&payload).display()).into(),
                    c.binding.database.clone().into(),
                ],
                None,
                catalog,
                packages,
                temp.path(),
                cancel,
            )?;
        }
        self.publish_payload(project, &payload, c.engine, &package.version)
    }
    fn publish_payload(
        &self,
        project: &str,
        payload: &Path,
        engine: Engine,
        version: &str,
    ) -> Result<Manifest> {
        let bytes = fs::metadata(payload)?.len();
        if bytes > LIMIT {
            return Err(E::InvalidBackup);
        }
        let manifest = Manifest {
            schema: 1,
            id: crate::id(),
            project: project.into(),
            engine,
            version: version.into(),
            created: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            sha256: hash(payload)?,
            bytes,
        };
        let mut archive = tempfile::NamedTempFile::new_in(self.backup_root(project)?)?;
        {
            let mut zip = zip::ZipWriter::new(archive.as_file_mut());
            let options = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored)
                .unix_permissions(0o600);
            zip.start_file("manifest.json", options)
                .map_err(|_| E::InvalidBackup)?;
            zip.write_all(&serde_json::to_vec(&manifest).map_err(|_| E::InvalidBackup)?)?;
            zip.start_file("payload", options)
                .map_err(|_| E::InvalidBackup)?;
            std::io::copy(&mut File::open(payload)?, &mut zip)?;
            zip.finish().map_err(|_| E::InvalidBackup)?;
        }
        archive.as_file().sync_all()?;
        archive
            .persist_noclobber(self.backup_path(project, &manifest.id)?)
            .map_err(|_| E::StorageUnavailable)?;
        Ok(manifest)
    }
    pub fn export_backup(&self, project: &str, id: &str, destination: &Path) -> Result<()> {
        let source = self.backup_path(project, id)?;
        inspect(&source)?;
        let parent = destination.parent().ok_or(E::InvalidBackup)?;
        let mut out = tempfile::NamedTempFile::new_in(parent)?;
        std::io::copy(&mut File::open(source)?, out.as_file_mut())?;
        out.as_file().sync_all()?;
        out.persist_noclobber(destination).map_err(|e| {
            if e.error.kind() == std::io::ErrorKind::AlreadyExists {
                E::BackupDestinationExists
            } else {
                E::StorageUnavailable
            }
        })?;
        Ok(())
    }
    pub fn import_backup(&self, project: &str, source: &Path) -> Result<Manifest> {
        // Copy before inspecting: no verification/use race against an external file.
        let root = self.backup_root(project)?;
        let mut temp = tempfile::NamedTempFile::new_in(&root)?;
        let input = File::open(source)?;
        if input.metadata()?.len() > LIMIT + 65536 {
            return Err(E::InvalidBackup);
        }
        std::io::copy(&mut input.take(LIMIT + 65537), temp.as_file_mut())?;
        let (manifest, _) = inspect(temp.path())?;
        let id = manifest.id.clone();
        temp.persist_noclobber(self.backup_path(project, &id)?)
            .map_err(|_| E::StorageUnavailable)?;
        Ok(manifest)
    }
    #[allow(
        clippy::too_many_arguments,
        reason = "Restore coordinates a live owned database and explicit runtime and secret stores"
    )]
    pub fn restore_backup(
        &self,
        project: &str,
        id: &str,
        server: &mut RunningDatabase,
        catalog: &Catalog,
        packages: &PackageStore,
        secrets: &dyn SecretStore,
        cancel: &Cancellation,
    ) -> Result<Binding> {
        let (manifest, mut archive) = inspect(&self.backup_path(project, id)?)?;
        let active = self.binding(project)?.ok_or(E::DatabaseUnavailable)?;
        let instance = self.instance(&active.instance_id)?;
        server.validate_store_instance(self, &instance)?;
        if instance.engine != manifest.engine
            || catalog.get(&instance.package)?.version != manifest.version
        {
            return Err(E::BackupUnsupported);
        }
        let dir = tempfile::tempdir_in(self.backup_root(project)?)?;
        let payload = dir.path().join("payload");
        let mut output = File::create(&payload)?;
        std::io::copy(
            &mut archive.by_name("payload").map_err(|_| E::InvalidBackup)?,
            &mut output,
        )?;
        output.sync_all()?;
        drop(output);
        if cancel.is_cancelled() {
            return Err(E::ExecutionFailed);
        }
        let staged = server.provision_staged(self, project, secrets)?;
        let mut pending = staged.clone();
        pending.ready = false;
        self.retain_binding(&pending)?;
        self.restore_staged_payload(
            &staged, &instance, &payload, catalog, packages, secrets, cancel,
        )?;
        self.retain_binding(&staged)?;
        self.activate_binding(project, Some(&staged))?;
        Ok(staged)
    }
    #[allow(
        clippy::too_many_arguments,
        reason = "Existing bounded restore process boundary"
    )]
    fn restore_staged_payload(
        &self,
        staged: &Binding,
        instance: &crate::database::Instance,
        payload: &Path,
        catalog: &Catalog,
        packages: &PackageStore,
        secrets: &dyn SecretStore,
        cancel: &Cancellation,
    ) -> Result<()> {
        if cancel.is_cancelled() {
            return Err(E::ExecutionFailed);
        }
        let c = self.credentials_for(staged.clone(), secrets)?;
        if c.engine == Engine::Postgresql {
            run(
                &c,
                &instance.package,
                "pg_restore",
                vec![
                    format!("--dbname={}", staged.database).into(),
                    "--no-owner".into(),
                    "--no-privileges".into(),
                    "--exit-on-error".into(),
                    dunce::simplified(payload).as_os_str().to_owned(),
                ],
                None,
                catalog,
                packages,
                payload.parent().ok_or(E::InvalidBackup)?,
                cancel,
            )?;
        } else {
            run(
                &c,
                &instance.package,
                "mysql",
                vec![
                    "--binary-mode".into(),
                    "--batch".into(),
                    "--local-infile=0".into(),
                    staged.database.clone().into(),
                ],
                Some(payload.to_owned()),
                catalog,
                packages,
                payload.parent().ok_or(E::InvalidBackup)?,
                cancel,
            )?;
        }
        if cancel.is_cancelled() || !c.connection_test()? {
            return Err(E::ExecutionFailed);
        }
        Ok(())
    }
}
fn inspect_header(path: &Path) -> Result<(Manifest, zip::ZipArchive<File>)> {
    let meta = fs::symlink_metadata(path)?;
    if !meta.is_file() || meta.file_type().is_symlink() || meta.len() > LIMIT + 65536 {
        return Err(E::InvalidBackup);
    }
    let mut zip = zip::ZipArchive::new(File::open(path)?).map_err(|_| E::InvalidBackup)?;
    if zip.len() != 2 {
        return Err(E::InvalidBackup);
    }
    let mut names = std::collections::BTreeSet::new();
    for index in 0..2 {
        let f = zip.by_index(index).map_err(|_| E::InvalidBackup)?;
        let name = f.name().map_err(|_| E::InvalidBackup)?.into_owned();
        if !names.insert(name.clone())
            || !["manifest.json", "payload"].contains(&name.as_str())
            || f.is_symlink()
            || f.is_dir()
        {
            return Err(E::InvalidBackup);
        }
    }
    let manifest: Manifest = {
        let f = zip.by_name("manifest.json").map_err(|_| E::InvalidBackup)?;
        if f.size() > 8192 {
            return Err(E::InvalidBackup);
        }
        serde_json::from_reader(f.take(8193)).map_err(|_| E::InvalidBackup)?
    };
    if manifest.schema != 1
        || !valid_id(&manifest.id)
        || !valid_id(&manifest.project)
        || manifest.bytes > LIMIT
        || manifest.created > 253402300799
        || manifest.version.is_empty()
        || manifest.version.len() > 64
    {
        return Err(E::InvalidBackup);
    }
    Ok((manifest, zip))
}
pub fn inspect(path: &Path) -> Result<(Manifest, zip::ZipArchive<File>)> {
    let (manifest, mut zip) = inspect_header(path)?;
    {
        let mut f = zip.by_name("payload").map_err(|_| E::InvalidBackup)?;
        if f.size() != manifest.bytes {
            return Err(E::InvalidBackup);
        }
        let mut h = Sha256::new();
        let mut buffer = [0; 65536];
        let mut total = 0;
        loop {
            let n = f.read(&mut buffer)?;
            if n == 0 {
                break;
            }
            total += n as u64;
            if total > LIMIT {
                return Err(E::InvalidBackup);
            }
            h.update(&buffer[..n]);
        }
        let digest = h
            .finalize()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        if total != manifest.bytes || digest != manifest.sha256 {
            return Err(E::InvalidBackup);
        }
    }
    Ok((manifest, zip))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn archive(path: &Path, extra: &str, checksum: &str) {
        let mut z = zip::ZipWriter::new(File::create(path).unwrap());
        let o = zip::write::SimpleFileOptions::default();
        z.start_file("manifest.json", o).unwrap();
        z.write_all(
            serde_json::to_string(&Manifest {
                schema: 1,
                id: "a".repeat(32),
                project: "b".repeat(32),
                engine: Engine::Mysql,
                version: "8.4.13".into(),
                created: 1,
                sha256: checksum.into(),
                bytes: 3,
            })
            .unwrap()
            .as_bytes(),
        )
        .unwrap();
        z.start_file(extra, o).unwrap();
        z.write_all(b"SQL").unwrap();
        z.finish().unwrap();
    }
    #[test]
    fn checksum_and_path_boundaries_are_enforced() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("test.dfbackup");
        let digest = Sha256::digest(b"SQL")
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        archive(&path, "payload", &digest);
        assert!(inspect(&path).is_ok());
        archive(&path, "../payload", &digest);
        assert_eq!(inspect(&path).err(), Some(E::InvalidBackup));
        archive(&path, "payload", &"0".repeat(64));
        assert_eq!(inspect(&path).err(), Some(E::InvalidBackup));
    }
    #[test]
    fn link_backups_are_refused() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("not-file");
        fs::create_dir(&path).unwrap();
        assert_eq!(inspect(&path).err(), Some(E::InvalidBackup));
    }
}
