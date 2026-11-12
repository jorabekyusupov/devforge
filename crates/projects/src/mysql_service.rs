//! Owned portable MySQL processes. Never installs an OS service or attaches to
//! another listener; passwords stay in SecretStore/private bootstrap files.
use crate::{
    ProjectError as E, Result,
    database::{Binding, Credentials, Engine, Instance},
    private_dir,
    project::{Catalog, Project, ProjectStore},
};
use devforge_core::SecretStore;
use devforge_runtime::{
    package::{PackageStore, VerifiedExecutable},
    process::{LogChunk, LogPolicy, Observed, ProcessSpec, Readiness, RestartPolicy, Supervisor},
};
use mysql::{Conn, OptsBuilder, prelude::Queryable};
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    net::TcpListener,
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant},
};
use zeroize::Zeroizing;
pub fn connect(
    user: &str,
    password: &str,
    database: Option<&str>,
    port: u16,
    root: &Path,
) -> Result<Conn> {
    let opts = OptsBuilder::new()
        .ip_or_hostname(Some("127.0.0.1"))
        .tcp_port(port)
        .prefer_socket(false)
        .user(Some(user))
        .pass(Some(password))
        .db_name(database)
        .tcp_connect_timeout(Some(Duration::from_secs(3)))
        .read_timeout(Some(Duration::from_secs(10)))
        .write_timeout(Some(Duration::from_secs(10)))
        .server_key_path(Some(root.join("data/public_key.pem")));
    Conn::new(opts).map_err(|_| E::DatabaseUnavailable)
}
pub fn connection_test(c: &Credentials) -> Result<bool> {
    let mut conn = connect(
        &c.binding.role,
        &c.password,
        Some(&c.binding.database),
        c.port,
        &c.root,
    )?;
    let found: Option<(String, String)> = conn
        .query_first("SELECT DATABASE(),SUBSTRING_INDEX(CURRENT_USER(),'@',1)")
        .map_err(|_| E::DatabaseUnavailable)?;
    Ok(found.is_some_and(|(db, user)| db == c.binding.database && user == c.binding.role))
}
pub fn rejects_database(c: &Credentials, name: &str) -> bool {
    connect(&c.binding.role, &c.password, Some(name), c.port, &c.root).is_err()
}
pub fn literal(value: &str) -> String {
    format!("'{}'", value.replace('\\', "\\\\").replace('\'', "''"))
}
fn ident(value: &str) -> Result<String> {
    if value.is_empty()
        || value.len() > 64
        || !value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'_')
    {
        return Err(E::DatabaseConflict);
    }
    Ok(format!("`{value}`"))
}
pub fn environment(
    executable: &VerifiedExecutable,
) -> std::collections::BTreeMap<std::ffi::OsString, std::ffi::OsString> {
    let mut env = crate::environment::base();
    let mut path = vec![executable.path().parent().unwrap().to_path_buf()];
    if let Some(root) = std::env::var_os("SystemRoot") {
        path.push(PathBuf::from(root).join("System32"));
    }
    if !cfg!(windows) {
        path.extend([PathBuf::from("/usr/bin"), PathBuf::from("/bin")]);
    }
    env.insert(
        "PATH".into(),
        std::env::join_paths(path.iter().map(|p| dunce::simplified(p))).unwrap(),
    );
    env
}
fn base(executable: &VerifiedExecutable) -> PathBuf {
    executable
        .path()
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}
pub fn initialize(
    store: &ProjectStore,
    package_id: &str,
    port: u16,
    catalog: &Catalog,
    packages: &PackageStore,
    secrets: &dyn SecretStore,
) -> Result<Instance> {
    if crate::database::administrator_enabled()? {
        return Err(E::AdministratorUnsupported);
    }
    let _operation = store.operation_lock()?;
    let package = catalog.get(package_id)?;
    if package.kind != "mysql" {
        return Err(E::RuntimeUnavailable);
    }
    let executable = packages.executable(package, "mysqld")?;
    let reserved = TcpListener::bind(("127.0.0.1", port)).map_err(|_| E::InstanceBusy)?;
    let port = reserved.local_addr()?.port();
    drop(reserved);
    let id = crate::id();
    let instance = Instance {
        engine: Engine::Mysql,
        id: id.clone(),
        package: package_id.into(),
        port,
        admin: format!("df_admin_{}", &id[..16]),
        secret_ref: format!("instance/{id}/admin"),
        ready: false,
    };
    let root = store.root.join("databases").join(&id);
    private_dir(&root)?;
    let password = crate::password();
    secrets
        .set(&instance.secret_ref, &password)
        .map_err(|_| E::SecretUnavailable)?;
    store.save_instance(&instance)?;
    let mut supervisor = Supervisor::open(root.join("logs"))?;
    supervisor.start_redacted(
        ProcessSpec {
            input: None,
            id: "mysql-init".into(),
            executable: executable.clone(),
            args: vec![
                "--no-defaults".into(),
                "--initialize-insecure".into(),
                format!(
                    "--basedir={}",
                    dunce::simplified(&base(&executable)).display()
                )
                .into(),
                format!(
                    "--datadir={}",
                    dunce::simplified(&root.join("data")).display()
                )
                .into(),
                "--lower-case-table-names=1".into(),
            ],
            directory: root.clone(),
            environment: environment(&executable),
            readiness: Readiness::SuccessfulExit,
            readiness_timeout: Duration::from_secs(120),
            stop_grace: Duration::from_secs(15),
            restart: RestartPolicy::default(),
            logs: LogPolicy::default(),
        },
        vec![password.to_string()],
    )?;
    let state = supervisor.wait_terminal("mysql-init", Duration::from_secs(130))?;
    supervisor.stop("mysql-init")?;
    if state.observed != Observed::Succeeded {
        return Err(E::RecoveryRequired);
    }
    fs::write(root.join("initialized"), b"devforge-mysql-v1\n")?;
    drop(supervisor);
    drop(_operation);
    let mut running = RunningMySql::start_inner(store, &id, catalog, packages, secrets, true)?;
    let mut ready = instance;
    ready.ready = true;
    store.save_instance(&ready)?;
    running.stop()?;
    Ok(ready)
}
pub struct RunningMySql {
    pub instance: Instance,
    pub(crate) root: PathBuf,
    supervisor: Supervisor,
    password: Zeroizing<String>,
    _lock: File,
    _executable: VerifiedExecutable,
    stopped: bool,
    _socket: Option<tempfile::TempDir>,
}
impl RunningMySql {
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
        bootstrap: bool,
    ) -> Result<Self> {
        if crate::database::administrator_enabled()? {
            return Err(E::AdministratorUnsupported);
        }
        let instance = store.instance(id)?;
        let root = store.root.join("databases").join(id);
        if instance.engine != Engine::Mysql
            || (!instance.ready && !bootstrap)
            || fs::read(root.join("initialized")).ok().as_deref() != Some(b"devforge-mysql-v1\n")
        {
            return Err(E::RecoveryRequired);
        }
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(root.join("instance.lock"))?;
        lock.try_lock().map_err(|_| E::InstanceBusy)?;
        let available =
            TcpListener::bind(("127.0.0.1", instance.port)).map_err(|_| E::InstanceBusy)?;
        drop(available);
        let executable = packages.executable(catalog.get(&instance.package)?, "mysqld")?;
        let password = Zeroizing::new(
            secrets
                .get(&instance.secret_ref)
                .map_err(|_| E::SecretUnavailable)?
                .ok_or(E::SecretUnavailable)?,
        );
        let mut bootstrap_file = None;
        if bootstrap {
            let mut file = tempfile::NamedTempFile::new_in(&root)?;
            let sql = Zeroizing::new(format!(
                "ALTER USER 'root'@'localhost' IDENTIFIED BY {};\nCREATE USER IF NOT EXISTS {}@'127.0.0.1' IDENTIFIED BY {};\nALTER USER {}@'127.0.0.1' IDENTIFIED BY {};\nGRANT ALL PRIVILEGES ON *.* TO {}@'127.0.0.1' WITH GRANT OPTION;\n",
                literal(&password),
                literal(&instance.admin),
                literal(&password),
                literal(&instance.admin),
                literal(&password),
                literal(&instance.admin)
            ));
            file.write_all(sql.as_bytes())?;
            file.as_file().sync_all()?;
            bootstrap_file = Some(file);
        }
        let socket_dir = if cfg!(windows) {
            None
        } else {
            Some(
                tempfile::Builder::new()
                    .prefix("df-mysql-")
                    .tempdir_in("/tmp")?,
            )
        };
        let mut args = vec![
            "--no-defaults".into(),
            format!(
                "--basedir={}",
                dunce::simplified(&base(&executable)).display()
            )
            .into(),
            format!(
                "--datadir={}",
                dunce::simplified(&root.join("data")).display()
            )
            .into(),
            "--bind-address=127.0.0.1".into(),
            format!("--port={}", instance.port).into(),
            "--mysqlx=0".into(),
            "--skip-log-bin".into(),
            "--skip-name-resolve".into(),
            "--local-infile=0".into(),
            "--lower-case-table-names=1".into(),
            "--innodb-buffer-pool-size=64M".into(),
            "--innodb-redo-log-capacity=64M".into(),
            format!(
                "--pid-file={}",
                dunce::simplified(&root.join("mysql.pid")).display()
            )
            .into(),
            format!(
                "--socket={}",
                if cfg!(windows) {
                    format!("devforge-{id}")
                } else {
                    dunce::simplified(&socket_dir.as_ref().unwrap().path().join("mysql.sock"))
                        .display()
                        .to_string()
                }
            )
            .into(),
        ];
        if let Some(file) = &bootstrap_file {
            args.push(format!("--init-file={}", dunce::simplified(file.path()).display()).into());
        }
        let mut supervisor = Supervisor::open(root.join("logs"))?;
        supervisor.start_redacted(
            ProcessSpec {
                input: None,
                id: "mysqld".into(),
                executable: executable.clone(),
                args,
                directory: root.clone(),
                environment: environment(&executable),
                readiness: Readiness::Tcp {
                    address: ([127, 0, 0, 1], instance.port).into(),
                },
                readiness_timeout: Duration::from_secs(45),
                stop_grace: Duration::from_secs(15),
                restart: RestartPolicy::default(),
                logs: LogPolicy::default(),
            },
            vec![password.to_string()],
        )?;
        let running = Self {
            instance,
            root,
            supervisor,
            password,
            _lock: lock,
            _executable: executable,
            stopped: false,
            _socket: socket_dir,
        };
        let until = Instant::now() + Duration::from_secs(50);
        loop {
            if let Ok(mut admin) = running.admin() {
                running.verify(&mut admin)?;
                if bootstrap {
                    running.initialize_metadata(&mut admin)?;
                }
                break;
            }
            if running.supervisor.snapshot("mysqld")?.observed == Observed::Failed
                || Instant::now() > until
            {
                return Err(E::DatabaseUnavailable);
            }
            thread::sleep(Duration::from_millis(50));
        }
        drop(bootstrap_file);
        Ok(running)
    }
    fn admin(&self) -> Result<Conn> {
        connect(
            &self.instance.admin,
            &self.password,
            Some("mysql"),
            self.instance.port,
            &self.root,
        )
    }
    fn verify(&self, conn: &mut Conn) -> Result<()> {
        let directory: Option<String> = conn
            .query_first("SELECT @@datadir")
            .map_err(|_| E::DatabaseUnavailable)?;
        if fs::canonicalize(directory.ok_or(E::DatabaseConflict)?)?
            != fs::canonicalize(self.root.join("data"))?
        {
            return Err(E::DatabaseConflict);
        }
        let pid: u32 = fs::read_to_string(self.root.join("mysql.pid"))?
            .trim()
            .parse()
            .map_err(|_| E::DatabaseConflict)?;
        if self.supervisor.snapshot("mysqld")?.pid != Some(pid) {
            return Err(E::DatabaseConflict);
        }
        Ok(())
    }
    pub fn provision(
        &mut self,
        store: &ProjectStore,
        project: &Project,
        secrets: &dyn SecretStore,
    ) -> Result<Binding> {
        let _lock = store.operation_lock()?;
        let binding = if let Some(b) = store.binding(&project.id)? {
            if b.instance_id != self.instance.id {
                return Err(E::DatabaseConflict);
            }
            b
        } else {
            let nonce = crate::id();
            let b = Binding {
                project_id: project.id.clone(),
                instance_id: self.instance.id.clone(),
                database: format!("df_{}", project.id),
                role: format!("df_{}_{}", &project.id[..20], &nonce[..6]),
                secret_ref: format!(
                    "project/{}/database/{}/{}",
                    project.id, self.instance.id, nonce
                ),
                ready: false,
            };
            secrets
                .set(&b.secret_ref, &crate::password())
                .map_err(|_| E::SecretUnavailable)?;
            store.connection.execute(
                "INSERT INTO database_bindings VALUES(?1,?2,?3)",
                rusqlite::params![
                    b.project_id,
                    b.instance_id,
                    serde_json::to_string(&b).map_err(|_| E::StorageUnavailable)?
                ],
            )?;
            b
        };
        let binding = self.provision_binding(store, binding, secrets)?;
        store.connection.execute(
            "UPDATE database_bindings SET record=?1 WHERE project_id=?2",
            rusqlite::params![
                serde_json::to_string(&binding).map_err(|_| E::StorageUnavailable)?,
                project.id
            ],
        )?;
        Ok(binding)
    }
    pub fn provision_binding(
        &mut self,
        store: &ProjectStore,
        mut binding: Binding,
        secrets: &dyn SecretStore,
    ) -> Result<Binding> {
        if binding.instance_id != self.instance.id {
            return Err(E::DatabaseConflict);
        }
        let mut admin = self.admin()?;
        self.verify(&mut admin)?;
        let db = ident(&binding.database)?;
        ident(&binding.role)?;
        let password = Zeroizing::new(
            secrets
                .get(&binding.secret_ref)
                .map_err(|_| E::SecretUnavailable)?
                .ok_or(E::SecretUnavailable)?,
        );
        let marker = format!("devforge:{}:{}", self.instance.id, binding.project_id);
        let attribute:Option<Option<String>>=admin.exec_first("SELECT JSON_UNQUOTE(JSON_EXTRACT(User_attributes,'$.devforge')) FROM mysql.user WHERE User=? AND Host='127.0.0.1'",(&binding.role,)).map_err(|_|E::DatabaseUnavailable)?;
        if let Some(value) = attribute {
            if value.as_deref() != Some(marker.as_str()) {
                return Err(E::DatabaseConflict);
            }
        } else {
            admin
                .query_drop(format!(
                    "CREATE USER {}@'127.0.0.1' IDENTIFIED BY {} ATTRIBUTE {}",
                    literal(&binding.role),
                    literal(&password),
                    literal(&serde_json::json!({"devforge":marker}).to_string())
                ))
                .map_err(|_| E::DatabaseUnavailable)?;
        }
        let table = self.metadata_table()?;
        let owner: Option<(String, String)> = admin
            .exec_first(
                format!("SELECT project_id,role_name FROM {table} WHERE database_name=?"),
                (&binding.database,),
            )
            .map_err(|_| E::DatabaseConflict)?;
        let exists: Option<String> = admin
            .exec_first(
                "SELECT SCHEMA_NAME FROM information_schema.SCHEMATA WHERE SCHEMA_NAME=?",
                (&binding.database,),
            )
            .map_err(|_| E::DatabaseUnavailable)?;
        if let Some((project, role)) = owner {
            if project != binding.project_id || role != binding.role || exists.is_none() {
                return Err(E::DatabaseConflict);
            }
        } else {
            if exists.is_some() {
                return Err(E::DatabaseConflict);
            }
            admin
                .query_drop(format!(
                    "CREATE DATABASE {db} CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci"
                ))
                .map_err(|_| E::DatabaseUnavailable)?;
            admin
                .exec_drop(
                    format!("INSERT INTO {table} VALUES(?,?,?)"),
                    (&binding.database, &binding.project_id, &binding.role),
                )
                .map_err(|_| E::DatabaseUnavailable)?;
        }
        admin
            .query_drop(format!(
                "GRANT ALL PRIVILEGES ON {db}.* TO {}@'127.0.0.1'",
                literal(&binding.role)
            ))
            .map_err(|_| E::DatabaseUnavailable)?;
        binding.ready = true;
        store
            .credentials_for(binding.clone(), secrets)?
            .connection_test()?;
        Ok(binding)
    }
    fn metadata_table(&self) -> Result<String> {
        Ok(format!(
            "{}.`binding_owners`",
            ident(&format!("df_meta_{}", &self.instance.id[..20]))?
        ))
    }
    fn initialize_metadata(&self, admin: &mut Conn) -> Result<()> {
        let name = ident(&format!("df_meta_{}", &self.instance.id[..20]))?;
        admin
            .query_drop(format!("CREATE DATABASE {name}"))
            .map_err(|_| E::RecoveryRequired)?;
        admin.query_drop(format!("CREATE TABLE {} (database_name VARCHAR(64) PRIMARY KEY,project_id VARCHAR(32) NOT NULL,role_name VARCHAR(32) NOT NULL)",self.metadata_table()?)).map_err(|_|E::RecoveryRequired)?;
        Ok(())
    }
    pub fn stop(&mut self) -> Result<()> {
        if self.stopped {
            return Ok(());
        }
        if let Ok(mut admin) = self.admin() {
            self.verify(&mut admin)?;
            self.supervisor.expect_exit("mysqld")?;
            let _ = admin.query_drop("SHUTDOWN");
            let _ = self
                .supervisor
                .wait_terminal("mysqld", Duration::from_secs(20));
        }
        self.supervisor.stop("mysqld")?;
        self.stopped = true;
        Ok(())
    }
    pub fn status(&self) -> Option<devforge_runtime::process::Snapshot> {
        self.supervisor.snapshot("mysqld").ok()
    }
    pub fn logs(&self) -> Vec<LogChunk> {
        self.supervisor
            .logs("mysqld")
            .map(|v| v.0)
            .unwrap_or_default()
    }
}
impl Drop for RunningMySql {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}
