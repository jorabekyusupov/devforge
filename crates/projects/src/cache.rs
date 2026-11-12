//! Dedicated per-project RESP service. Cache and queue use separate logical DBs;
//! a project's FLUSHDB cannot erase another project's cache or queued jobs.
use crate::{
    ProjectError as E, Result, private_dir,
    project::{Catalog, Project, ProjectStore},
};
use devforge_core::SecretStore;
use devforge_runtime::{
    package::{PackageStore, VerifiedExecutable},
    process::{LogChunk, LogPolicy, Observed, ProcessSpec, Readiness, RestartPolicy, Supervisor},
};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    ffi::OsString,
    fs::{File, OpenOptions},
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    thread,
    time::{Duration, Instant},
};
use zeroize::Zeroizing;
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CacheDriver {
    #[default]
    Project,
    File,
    Database,
    Redis,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QueueDriver {
    #[default]
    Project,
    Sync,
    Database,
    Redis,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ServiceSelection {
    pub cache: CacheDriver,
    pub queue: QueueDriver,
    pub redis_instance: Option<String>,
}
impl ServiceSelection {
    pub fn uses_redis(&self) -> bool {
        self.cache == CacheDriver::Redis || self.queue == QueueDriver::Redis
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheInstance {
    pub id: String,
    pub project_id: String,
    pub package: String,
    pub runtime: String,
    pub port: u16,
    pub username: String,
    pub secret_ref: String,
    pub admin_ref: String,
    pub ready: bool,
}
#[derive(Debug)]
pub enum Response {
    Text(String),
    Bytes(Vec<u8>),
    Integer(i64),
    Null,
    Array(Vec<Response>),
}
pub struct Client {
    socket: BufReader<TcpStream>,
}
impl Client {
    pub fn connect(port: u16, user: &str, password: &str) -> Result<Self> {
        let stream =
            TcpStream::connect_timeout(&([127, 0, 0, 1], port).into(), Duration::from_secs(2))
                .map_err(|_| E::DatabaseUnavailable)?;
        stream.set_read_timeout(Some(Duration::from_secs(10)))?;
        stream.set_write_timeout(Some(Duration::from_secs(5)))?;
        let mut client = Self {
            socket: BufReader::new(stream),
        };
        client.command(&["AUTH", user, password])?;
        Ok(client)
    }
    pub fn command(&mut self, args: &[&str]) -> Result<Response> {
        let mut bytes = Zeroizing::new(format!("*{}\r\n", args.len()).into_bytes());
        for arg in args {
            bytes.extend_from_slice(format!("${}\r\n", arg.len()).as_bytes());
            bytes.extend_from_slice(arg.as_bytes());
            bytes.extend_from_slice(b"\r\n");
        }
        self.socket
            .get_mut()
            .write_all(&bytes)
            .map_err(|_| E::DatabaseUnavailable)?;
        read_response(&mut self.socket, 0)
    }
}
fn line(reader: &mut impl BufRead) -> Result<String> {
    let mut bytes = vec![];
    reader
        .take(4097)
        .read_until(b'\n', &mut bytes)
        .map_err(|_| E::DatabaseUnavailable)?;
    if bytes.len() > 4096 || !bytes.ends_with(b"\r\n") {
        return Err(E::DatabaseUnavailable);
    }
    bytes.truncate(bytes.len() - 2);
    String::from_utf8(bytes).map_err(|_| E::DatabaseUnavailable)
}
fn read_response(reader: &mut impl BufRead, depth: usize) -> Result<Response> {
    if depth > 8 {
        return Err(E::DatabaseUnavailable);
    }
    let mut kind = [0];
    reader
        .read_exact(&mut kind)
        .map_err(|_| E::DatabaseUnavailable)?;
    match kind[0] {
        b'+' => Ok(Response::Text(line(reader)?)),
        b'-' => {
            let _ = line(reader)?;
            Err(E::DatabaseUnavailable)
        }
        b':' => Ok(Response::Integer(
            line(reader)?.parse().map_err(|_| E::DatabaseUnavailable)?,
        )),
        b'$' => {
            let len: i64 = line(reader)?.parse().map_err(|_| E::DatabaseUnavailable)?;
            if len == -1 {
                return Ok(Response::Null);
            }
            if !(0..=1_048_576).contains(&len) {
                return Err(E::DatabaseUnavailable);
            }
            let mut data = vec![0; len as usize];
            reader.read_exact(&mut data)?;
            let mut end = [0; 2];
            reader.read_exact(&mut end)?;
            if end != *b"\r\n" {
                return Err(E::DatabaseUnavailable);
            }
            Ok(Response::Bytes(data))
        }
        b'*' => {
            let count: i64 = line(reader)?.parse().map_err(|_| E::DatabaseUnavailable)?;
            if count == -1 {
                return Ok(Response::Null);
            }
            if !(0..=1024).contains(&count) {
                return Err(E::DatabaseUnavailable);
            }
            Ok(Response::Array(
                (0..count)
                    .map(|_| read_response(reader, depth + 1))
                    .collect::<Result<_>>()?,
            ))
        }
        _ => Err(E::DatabaseUnavailable),
    }
}
impl ProjectStore {
    pub fn cache_instances(&self) -> Result<Vec<CacheInstance>> {
        let mut q = self
            .connection
            .prepare("SELECT record FROM cache_instances ORDER BY id")?;
        q.query_map([], |r| r.get::<_, String>(0))?
            .map(|r| serde_json::from_str(&r?).map_err(|_| E::StorageUnavailable))
            .collect()
    }
    pub fn cache_instance(&self, id: &str) -> Result<CacheInstance> {
        let raw = self
            .connection
            .query_row(
                "SELECT record FROM cache_instances WHERE id=?1",
                [id],
                |r| r.get::<_, String>(0),
            )
            .optional()?
            .ok_or(E::NotFound)?;
        serde_json::from_str(&raw).map_err(|_| E::StorageUnavailable)
    }
    fn save_cache(&self, i: &CacheInstance) -> Result<()> {
        self.connection.execute(
            "INSERT OR REPLACE INTO cache_instances VALUES(?1,?2)",
            params![
                i.id,
                serde_json::to_string(i).map_err(|_| E::StorageUnavailable)?
            ],
        )?;
        Ok(())
    }
    pub fn initialize_cache(
        &self,
        project: &str,
        package: &str,
        runtime: &str,
        catalog: &Catalog,
        packages: &PackageStore,
        secrets: &dyn SecretStore,
    ) -> Result<CacheInstance> {
        self.get(project)?;
        let server = catalog.get(package)?;
        let dotnet = catalog.get(runtime)?;
        if server.kind != "garnet" || dotnet.kind != "dotnet" || server.target != dotnet.target {
            return Err(E::RuntimeUnavailable);
        }
        packages.executable(server, "garnet")?;
        packages.executable(dotnet, "dotnet")?;
        let bind = TcpListener::bind(("127.0.0.1", 0))?;
        let port = bind.local_addr()?.port();
        drop(bind);
        let id = crate::id();
        let mut instance = CacheInstance {
            id: id.clone(),
            project_id: project.into(),
            package: package.into(),
            runtime: runtime.into(),
            port,
            username: format!("df_{project}"),
            secret_ref: format!("cache/{id}/project"),
            admin_ref: format!("cache/{id}/admin"),
            ready: false,
        };
        secrets
            .set(&instance.secret_ref, &crate::password())
            .map_err(|_| E::SecretUnavailable)?;
        secrets
            .set(&instance.admin_ref, &crate::password())
            .map_err(|_| E::SecretUnavailable)?;
        self.save_cache(&instance)?;
        let mut service = RunningCache::start_inner(self, &id, catalog, packages, secrets, true)?;
        instance.ready = true;
        self.save_cache(&instance)?;
        service.stop()?;
        Ok(instance)
    }
    pub fn save_services(
        &self,
        id: &str,
        revision: u64,
        selection: ServiceSelection,
    ) -> Result<()> {
        let _lock = self.operation_lock()?;
        let mut p = self.get(id)?;
        if selection.uses_redis() {
            let instance = self.cache_instance(
                selection
                    .redis_instance
                    .as_deref()
                    .ok_or(E::DatabaseUnavailable)?,
            )?;
            if !instance.ready || instance.project_id != id {
                return Err(E::DatabaseConflict);
            }
        }
        p.services = selection;
        self.save(&mut p, revision)?;
        Ok(())
    }
    pub fn cache_client(&self, id: &str, secrets: &dyn SecretStore) -> Result<Client> {
        let i = self.cache_instance(id)?;
        if !i.ready {
            return Err(E::RecoveryRequired);
        }
        let p = Zeroizing::new(
            secrets
                .get(&i.secret_ref)
                .map_err(|_| E::SecretUnavailable)?
                .ok_or(E::SecretUnavailable)?,
        );
        Client::connect(i.port, &i.username, &p)
    }
}
pub(crate) fn apply_environment(
    store: &ProjectStore,
    p: &Project,
    secrets: &dyn SecretStore,
    environment: &mut BTreeMap<OsString, OsString>,
    sensitive: &mut Vec<String>,
) -> Result<()> {
    let s = &p.services;
    let cache = match s.cache {
        CacheDriver::Project => None,
        CacheDriver::File => Some("file"),
        CacheDriver::Database => Some("database"),
        CacheDriver::Redis => Some("redis"),
    };
    let queue = match s.queue {
        QueueDriver::Project => None,
        QueueDriver::Sync => Some("sync"),
        QueueDriver::Database => Some("database"),
        QueueDriver::Redis => Some("redis"),
    };
    if let Some(value) = cache {
        environment.insert("CACHE_STORE".into(), value.into());
        environment.insert("CACHE_DRIVER".into(), value.into());
    }
    if let Some(value) = queue {
        environment.insert("QUEUE_CONNECTION".into(), value.into());
    }
    if s.uses_redis() {
        let i = store.cache_instance(s.redis_instance.as_deref().ok_or(E::DatabaseUnavailable)?)?;
        if !i.ready || i.project_id != p.id {
            return Err(E::DatabaseConflict);
        }
        if !p.path.join("vendor/predis/predis/src/Client.php").is_file() {
            return Err(E::RuntimeUnavailable);
        }
        let password = Zeroizing::new(
            secrets
                .get(&i.secret_ref)
                .map_err(|_| E::SecretUnavailable)?
                .ok_or(E::SecretUnavailable)?,
        );
        sensitive.push(password.to_string());
        for (key, value) in [
            ("REDIS_CLIENT", "predis".into()),
            ("REDIS_HOST", "127.0.0.1".into()),
            ("REDIS_PORT", i.port.to_string()),
            ("REDIS_USERNAME", i.username),
            ("REDIS_PASSWORD", password.to_string()),
            ("REDIS_DB", "0".into()),
            ("REDIS_CACHE_DB", "1".into()),
            ("REDIS_PREFIX", format!("df_{}:", p.id)),
            ("CACHE_PREFIX", format!("cache_{}:", p.id)),
            ("REDIS_URL", String::new()),
        ] {
            environment.insert(key.into(), value.into());
        }
    }
    Ok(())
}
pub struct RunningCache {
    pub instance: CacheInstance,
    supervisor: Supervisor,
    _server: VerifiedExecutable,
    _runtime: VerifiedExecutable,
    _lock: File,
    password: Zeroizing<String>,
    stopped: bool,
}
impl RunningCache {
    pub fn start(
        store: &ProjectStore,
        id: &str,
        catalog: &Catalog,
        packages: &PackageStore,
        secrets: &dyn SecretStore,
    ) -> Result<Self> {
        Self::start_inner(store, id, catalog, packages, secrets, false)
    }
    fn start_inner(
        store: &ProjectStore,
        id: &str,
        catalog: &Catalog,
        packages: &PackageStore,
        secrets: &dyn SecretStore,
        initial: bool,
    ) -> Result<Self> {
        use sha2::{Digest, Sha256};
        let instance = store.cache_instance(id)?;
        if !initial && !instance.ready {
            return Err(E::RecoveryRequired);
        }
        let root = store.root.join("cache-services").join(id);
        private_dir(&root)?;
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(root.join("instance.lock"))?;
        lock.try_lock().map_err(|_| E::InstanceBusy)?;
        let bind = TcpListener::bind(("127.0.0.1", instance.port)).map_err(|_| E::InstanceBusy)?;
        drop(bind);
        let server = packages.executable(catalog.get(&instance.package)?, "garnet")?;
        let runtime = packages.executable(catalog.get(&instance.runtime)?, "dotnet")?;
        let admin = Zeroizing::new(
            secrets
                .get(&instance.admin_ref)
                .map_err(|_| E::SecretUnavailable)?
                .ok_or(E::SecretUnavailable)?,
        );
        let password = Zeroizing::new(
            secrets
                .get(&instance.secret_ref)
                .map_err(|_| E::SecretUnavailable)?
                .ok_or(E::SecretUnavailable)?,
        );
        let hash = |value: &str| {
            Sha256::digest(value.as_bytes())
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>()
        };
        let acl = format!(
            "user default off\nuser df_admin on #{} ~* +@all\nuser {} on #{} ~* +@all -@admin\n",
            hash(&admin),
            instance.username,
            hash(&password)
        );
        let mut file = tempfile::NamedTempFile::new_in(&root)?;
        file.write_all(acl.as_bytes())?;
        file.as_file().sync_all()?;
        let acl_path = root.join("users.acl");
        file.persist(&acl_path).map_err(|_| E::StorageUnavailable)?;
        let mut env = crate::environment::base();
        env.insert("DOTNET_MULTILEVEL_LOOKUP".into(), "0".into());
        let home = runtime.path().parent().unwrap();
        for key in ["DOTNET_ROOT", "DOTNET_ROOT_ARM64", "DOTNET_ROOT_X64"] {
            env.insert(key.into(), dunce::simplified(home).as_os_str().to_owned());
        }
        let args = vec![
            "--bind".into(),
            "127.0.0.1".into(),
            "--port".into(),
            instance.port.to_string().into(),
            "--auth".into(),
            "ACL".into(),
            "--acl-file".into(),
            dunce::simplified(&acl_path).as_os_str().to_owned(),
            "--lua".into(),
            "--memory".into(),
            "64m".into(),
            "--page".into(),
            "1m".into(),
            "--index".into(),
            "1m".into(),
            "--device-type".into(),
            "FileStream".into(),
            "--aof".into(),
            "--aof-memory".into(),
            "8m".into(),
            "--aof-page-size".into(),
            "2m".into(),
            "--aof-segment-size".into(),
            "16m".into(),
            "--aof-commit-wait".into(),
            "--storage-tier".into(),
            "--logdir".into(),
            dunce::simplified(&root.join("data")).as_os_str().to_owned(),
            "--checkpointdir".into(),
            dunce::simplified(&root.join("checkpoints"))
                .as_os_str()
                .to_owned(),
            "--recover".into(),
        ];
        let mut supervisor = Supervisor::open(root.join("logs"))?;
        supervisor.start_redacted(
            ProcessSpec {
                input: None,
                id: "garnet".into(),
                executable: server.clone(),
                args,
                directory: root,
                environment: env,
                readiness: Readiness::Tcp {
                    address: ([127, 0, 0, 1], instance.port).into(),
                },
                readiness_timeout: Duration::from_secs(30),
                stop_grace: Duration::from_secs(10),
                restart: RestartPolicy::default(),
                logs: LogPolicy::default(),
            },
            vec![admin.to_string(), password.to_string()],
        )?;
        let service = Self {
            instance,
            supervisor,
            _server: server,
            _runtime: runtime,
            _lock: lock,
            password: admin,
            stopped: false,
        };
        let until = Instant::now() + Duration::from_secs(35);
        loop {
            if let Ok(mut c) = service.admin()
                && matches!(c.command(&["PING"]),Ok(Response::Text(v))if v=="PONG")
            {
                break;
            }
            if service.supervisor.snapshot("garnet")?.observed == Observed::Failed
                || Instant::now() > until
            {
                return Err(E::DatabaseUnavailable);
            }
            thread::sleep(Duration::from_millis(50));
        }
        Ok(service)
    }
    fn admin(&self) -> Result<Client> {
        Client::connect(self.instance.port, "df_admin", &self.password)
    }
    pub fn stop(&mut self) -> Result<()> {
        if self.stopped {
            return Ok(());
        }
        if let Ok(mut c) = self.admin() {
            let _ = c.command(&["SAVE"]);
            self.supervisor.expect_exit("garnet")?;
            let _ = c.command(&["SHUTDOWN"]);
            let _ = self
                .supervisor
                .wait_terminal("garnet", Duration::from_secs(10));
        }
        self.supervisor.stop("garnet")?;
        self.stopped = true;
        Ok(())
    }
    pub fn status(&self) -> Option<devforge_runtime::process::Snapshot> {
        self.supervisor.snapshot("garnet").ok()
    }
    pub fn logs(&self) -> Vec<LogChunk> {
        self.supervisor
            .logs("garnet")
            .map(|v| v.0)
            .unwrap_or_default()
    }
}
impl Drop for RunningCache {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}
