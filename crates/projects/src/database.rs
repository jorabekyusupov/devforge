use crate::{
    ProjectError as E, Result, private_dir,
    project::{Catalog, Project, ProjectStore},
};
use devforge_core::SecretStore;
use devforge_runtime::{
    package::{Package, PackageStore},
    process::{LogPolicy, Observed, ProcessSpec, Readiness, RestartPolicy, Supervisor},
};
use postgres::{Client, NoTls};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    net::TcpListener,
    path::PathBuf,
    thread,
    time::{Duration, Instant},
};
use zeroize::Zeroizing;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Instance {
    pub id: String,
    pub package: String,
    pub port: u16,
    pub admin: String,
    pub secret_ref: String,
    pub ready: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Binding {
    pub project_id: String,
    pub instance_id: String,
    pub database: String,
    pub role: String,
    pub secret_ref: String,
    pub ready: bool,
}
pub struct Credentials {
    pub(crate) binding: Binding,
    pub(crate) port: u16,
    pub(crate) password: Zeroizing<String>,
}
impl Credentials {
    pub fn connect(&self) -> Result<Client> {
        connect(
            &self.binding.role,
            &self.password,
            &self.binding.database,
            self.port,
        )
    }
    pub fn connection_test(&self) -> Result<bool> {
        Ok(self
            .connect()?
            .query_one(
                "SELECT current_database() = $1 AND current_user = $2",
                &[&self.binding.database, &self.binding.role],
            )
            .map_err(|_| E::DatabaseUnavailable)?
            .get(0))
    }
    pub fn rejects_database(&self, name: &str) -> bool {
        connect(&self.binding.role, &self.password, name, self.port).is_err()
    }
}
fn connect(user: &str, password: &str, database: &str, port: u16) -> Result<Client> {
    postgres::Config::new()
        .host("127.0.0.1")
        .port(port)
        .user(user)
        .password(password)
        .dbname(database)
        .connect_timeout(Duration::from_secs(3))
        .options("-c statement_timeout=5000 -c lock_timeout=3000")
        .connect(NoTls)
        .map_err(|_| E::DatabaseUnavailable)
}
impl ProjectStore {
    pub fn instance(&self, id: &str) -> Result<Instance> {
        let raw = self
            .connection
            .query_row("SELECT record FROM instances WHERE id=?1", [id], |r| {
                r.get::<_, String>(0)
            })
            .optional()?
            .ok_or(E::NotFound)?;
        serde_json::from_str(&raw).map_err(|_| E::StorageUnavailable)
    }
    pub fn instances(&self) -> Result<Vec<Instance>> {
        let mut q = self
            .connection
            .prepare("SELECT record FROM instances ORDER BY id")?;
        q.query_map([], |r| r.get::<_, String>(0))?
            .map(|r| serde_json::from_str(&r?).map_err(|_| E::StorageUnavailable))
            .collect()
    }
    pub fn binding(&self, project_id: &str) -> Result<Option<Binding>> {
        self.connection
            .query_row(
                "SELECT record FROM database_bindings WHERE project_id=?1",
                [project_id],
                |r| r.get::<_, String>(0),
            )
            .optional()?
            .map(|v| serde_json::from_str(&v).map_err(|_| E::StorageUnavailable))
            .transpose()
    }
    pub fn credentials(&self, project_id: &str, secrets: &dyn SecretStore) -> Result<Credentials> {
        let binding = self
            .binding(project_id)?
            .filter(|b| b.ready)
            .ok_or(E::DatabaseUnavailable)?;
        let instance = self.instance(&binding.instance_id)?;
        let password = Zeroizing::new(
            secrets
                .get(&binding.secret_ref)
                .map_err(|_| E::SecretUnavailable)?
                .ok_or(E::SecretUnavailable)?,
        );
        Ok(Credentials {
            binding,
            port: instance.port,
            password,
        })
    }
    pub fn initialize_instance(
        &self,
        package_id: &str,
        port: u16,
        catalog: &Catalog,
        packages: &PackageStore,
        secrets: &dyn SecretStore,
    ) -> Result<Instance> {
        let _operation = self.operation_lock()?;
        let package = catalog.get(package_id)?;
        if package.kind != "postgresql" || package.variant != "server" {
            return Err(E::RuntimeUnavailable);
        }
        let initdb = packages.executable(package, "initdb")?;
        let chosen = TcpListener::bind(("127.0.0.1", port)).map_err(|_| E::InstanceBusy)?;
        let id = crate::id();
        let port = chosen.local_addr()?.port();
        drop(chosen);
        let mut instance = Instance {
            id: id.clone(),
            package: package_id.into(),
            port,
            admin: format!("df_admin_{id}"),
            secret_ref: format!("instance/{id}/admin"),
            ready: false,
        };
        let root = self.root.join("databases").join(&id);
        private_dir(&root)?;
        let password = crate::password();
        secrets
            .set(&instance.secret_ref, &password)
            .map_err(|_| E::SecretUnavailable)?;
        self.connection.execute(
            "INSERT INTO instances VALUES(?1,?2)",
            params![id, serde_json::to_string(&instance).unwrap()],
        )?;
        // The pending row is durable before initdb. A failure preserves its data for
        // explicit recovery instead of deleting/reinitializing a possibly useful DB.
        let mut pw = tempfile::NamedTempFile::new_in(&root)?;
        pw.write_all(password.as_bytes())?;
        pw.as_file().sync_all()?;
        let mut supervisor = Supervisor::open(root.join("logs"))?;
        let env = server_env(&initdb);
        supervisor.start_redacted(
            ProcessSpec {
                id: "initdb".into(),
                executable: initdb,
                args: vec![
                    "-D".into(),
                    dunce::simplified(&root.join("data")).as_os_str().to_owned(),
                    "--encoding=UTF8".into(),
                    "--no-locale".into(),
                    format!("--username={}", instance.admin).into(),
                    format!("--pwfile={}", dunce::simplified(pw.path()).display()).into(),
                    "--auth-host=scram-sha-256".into(),
                    "--auth-local=scram-sha-256".into(),
                ],
                directory: root.clone(),
                environment: env,
                readiness: Readiness::SuccessfulExit,
                readiness_timeout: Duration::from_secs(60),
                stop_grace: Duration::from_secs(5),
                restart: RestartPolicy::default(),
                logs: LogPolicy::default(),
            },
            vec![password.to_string()],
        )?;
        let outcome = supervisor.wait_terminal("initdb", Duration::from_secs(70))?;
        if outcome.observed != Observed::Succeeded {
            return Err(E::DatabaseUnavailable);
        }
        drop(pw);
        let data = root.join("data");
        let hba = format!(
            "host all {} 127.0.0.1/32 scram-sha-256\nhost sameuser all 127.0.0.1/32 scram-sha-256\nhost all all 0.0.0.0/0 reject\nhost all all ::0/0 reject\nlocal all all reject\n",
            instance.admin
        );
        fs::write(data.join("pg_hba.conf"), hba)?;
        let mut conf = OpenOptions::new()
            .append(true)
            .open(data.join("postgresql.conf"))?;
        conf.write_all(format!("\n# DevForge owned instance\nlisten_addresses='127.0.0.1'\nport={}\nunix_socket_directories=''\nssl=off\npassword_encryption='scram-sha-256'\nlog_statement='none'\nlog_min_error_statement='panic'\nlogging_collector=off\nlc_messages='C'\ntimezone='UTC'\n",port).as_bytes())?;
        conf.sync_all()?;
        fs::write(root.join("initialized"), b"devforge-postgres-v1\n")?;
        instance.ready = true;
        self.connection.execute(
            "UPDATE instances SET record=?1 WHERE id=?2",
            params![serde_json::to_string(&instance).unwrap(), instance.id],
        )?;
        Ok(instance)
    }
}
/// Holds the instance's lifetime lock, verified package leases and process owner.
/// Drop requests pg_ctl fast shutdown before the generic process-tree fallback.
pub struct RunningPostgres {
    supervisor: Supervisor,
    instance: Instance,
    root: PathBuf,
    package: Package,
    pg_ctl: Option<devforge_runtime::package::VerifiedExecutable>,
    password: Zeroizing<String>,
    _lock: Option<File>,
    stopped: bool,
}
impl RunningPostgres {
    pub fn start(
        store: &ProjectStore,
        id: &str,
        catalog: &Catalog,
        packages: &PackageStore,
        secrets: &dyn SecretStore,
    ) -> Result<Self> {
        let instance = store.instance(id)?;
        let root = store.root.join("databases").join(id);
        if !instance.ready
            || fs::read(root.join("initialized")).ok().as_deref() != Some(b"devforge-postgres-v1\n")
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
        // A listener already on the port is foreign. Never stop/attach to it.
        let probe = TcpListener::bind(("127.0.0.1", instance.port)).map_err(|_| E::InstanceBusy)?;
        drop(probe);
        let package = catalog.get(&instance.package)?.clone();
        let executable = packages.executable(&package, "postgres")?;
        let pg_ctl = packages.executable(&package, "pg_ctl")?;
        let password = Zeroizing::new(
            secrets
                .get(&instance.secret_ref)
                .map_err(|_| E::SecretUnavailable)?
                .ok_or(E::SecretUnavailable)?,
        );
        let mut supervisor = Supervisor::open(root.join("logs"))?;
        let environment = server_env(&executable);
        supervisor.start_redacted(
            ProcessSpec {
                id: "postgres".into(),
                executable,
                args: vec![
                    "-D".into(),
                    dunce::simplified(&root.join("data")).as_os_str().to_owned(),
                ],
                directory: root.clone(),
                environment,
                readiness: Readiness::Output {
                    stream: "stderr",
                    needle: "database system is ready to accept connections".into(),
                },
                readiness_timeout: Duration::from_secs(20),
                stop_grace: Duration::from_secs(5),
                restart: RestartPolicy::default(),
                logs: LogPolicy::default(),
            },
            vec![password.to_string()],
        )?;
        let mut running = Self {
            supervisor,
            instance,
            root,
            package,
            pg_ctl: Some(pg_ctl),
            password,
            _lock: Some(lock),
            stopped: false,
        };
        let until = Instant::now() + Duration::from_secs(25);
        loop {
            let snapshot = running.supervisor.snapshot("postgres")?;
            if snapshot.observed == Observed::Ready {
                break;
            }
            if snapshot.observed == Observed::Failed || Instant::now() >= until {
                return Err(E::DatabaseUnavailable);
            }
            thread::sleep(Duration::from_millis(20));
        }
        let mut admin = running.admin()?;
        let location: String = admin
            .query_one("SHOW data_directory", &[])
            .map_err(|_| E::DatabaseUnavailable)?
            .get(0);
        if fs::canonicalize(location)? != fs::canonicalize(running.root.join("data"))? {
            return Err(E::DatabaseConflict);
        }
        admin
            .batch_execute("REVOKE CONNECT ON DATABASE postgres, template1 FROM PUBLIC")
            .map_err(|_| E::DatabaseUnavailable)?;
        Ok(running)
    }
    fn admin(&mut self) -> Result<Client> {
        connect(
            &self.instance.admin,
            &self.password,
            "postgres",
            self.instance.port,
        )
    }
    pub fn provision(
        &mut self,
        store: &ProjectStore,
        project: &Project,
        secrets: &dyn SecretStore,
    ) -> Result<Binding> {
        provision_on_running(store, &self.instance.id, project, secrets)
    }
    pub fn stop(&mut self) -> Result<()> {
        if self.stopped {
            return Ok(());
        }
        let snapshot = self.supervisor.snapshot("postgres")?;
        let pid = fs::read_to_string(self.root.join("data/postmaster.pid"))?
            .lines()
            .next()
            .and_then(|l| l.parse::<u32>().ok())
            .ok_or(E::DatabaseConflict)?;
        if snapshot.pid != Some(pid) {
            return Err(E::DatabaseConflict);
        }
        self.supervisor.expect_exit("postgres")?;
        let pg_ctl = self.pg_ctl.as_ref().ok_or(E::DatabaseUnavailable)?.clone();
        self.supervisor.start_redacted(
            ProcessSpec {
                id: "pg-stop".into(),
                executable: pg_ctl.clone(),
                args: vec![
                    "stop".into(),
                    "-D".into(),
                    dunce::simplified(&self.root.join("data"))
                        .as_os_str()
                        .to_owned(),
                    "-m".into(),
                    "fast".into(),
                    "-w".into(),
                    "-t".into(),
                    "10".into(),
                ],
                directory: self.root.clone(),
                environment: server_env(&pg_ctl),
                readiness: Readiness::SuccessfulExit,
                readiness_timeout: Duration::from_secs(15),
                stop_grace: Duration::from_secs(2),
                restart: RestartPolicy::default(),
                logs: LogPolicy::default(),
            },
            vec![self.password.to_string()],
        )?;
        let status = self
            .supervisor
            .wait_terminal("pg-stop", Duration::from_secs(20))?;
        if status.observed != Observed::Succeeded {
            return Err(E::DatabaseUnavailable);
        }
        self.supervisor
            .wait_terminal("postgres", Duration::from_secs(10))?;
        self.supervisor.stop("postgres")?;
        self.supervisor.stop("pg-stop")?;
        self.stopped = true;
        self.pg_ctl.take();
        self._lock.take();
        Ok(())
    }
    pub fn package(&self) -> &Package {
        &self.package
    }
}
impl Drop for RunningPostgres {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}
fn ident(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\"\""))
}
fn literal(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}
fn server_env(
    executable: &devforge_runtime::package::VerifiedExecutable,
) -> std::collections::BTreeMap<std::ffi::OsString, std::ffi::OsString> {
    let mut env = crate::environment::base();
    let mut paths = vec![executable.path().parent().unwrap().to_path_buf()];
    if cfg!(windows) {
        if let Some(root) = std::env::var_os("SystemRoot") {
            paths.push(PathBuf::from(root).join("System32"));
        }
    } else {
        paths.extend([PathBuf::from("/usr/bin"), PathBuf::from("/bin")]);
    }
    env.insert(
        "PATH".into(),
        std::env::join_paths(paths).expect("validated OS paths"),
    );
    env
}

pub fn provision_on_running(
    store: &ProjectStore,
    instance_id: &str,
    project: &Project,
    secrets: &dyn SecretStore,
) -> Result<Binding> {
    let current = store.get(&project.id)?;
    let project = &current;
    let instance = store.instance(instance_id)?;
    if !instance.ready {
        return Err(E::RecoveryRequired);
    }
    let admin_password = Zeroizing::new(
        secrets
            .get(&instance.secret_ref)
            .map_err(|_| E::SecretUnavailable)?
            .ok_or(E::SecretUnavailable)?,
    );
    let mut identity = connect(&instance.admin, &admin_password, "postgres", instance.port)?;
    let location: String = identity
        .query_one("SHOW data_directory", &[])
        .map_err(|_| E::DatabaseUnavailable)?
        .get(0);
    if fs::canonicalize(location)?
        != fs::canonicalize(store.root.join("databases").join(instance_id).join("data"))?
    {
        return Err(E::DatabaseConflict);
    }
    drop(identity);

    let _operation = store.operation_lock()?;
    let mut binding = if let Some(b) = store.binding(&project.id)? {
        if b.instance_id != instance.id {
            return Err(E::DatabaseConflict);
        }
        b
    } else {
        let b = Binding {
            project_id: project.id.clone(),
            instance_id: instance.id.clone(),
            database: format!("df_{}", project.id),
            role: format!("df_{}", project.id),
            secret_ref: format!("project/{}/database", project.id),
            ready: false,
        };
        let password = crate::password();
        secrets
            .set(&b.secret_ref, &password)
            .map_err(|_| E::SecretUnavailable)?;
        store.connection.execute(
            "INSERT INTO database_bindings VALUES(?1,?2,?3)",
            params![project.id, instance.id, serde_json::to_string(&b).unwrap()],
        )?;
        b
    };
    let password = Zeroizing::new(
        secrets
            .get(&binding.secret_ref)
            .map_err(|_| E::SecretUnavailable)?
            .ok_or(E::SecretUnavailable)?,
    );
    let marker = format!("devforge:{}:{}", instance.id, project.id);
    let mut client = connect(&instance.admin, &admin_password, "postgres", instance.port)?;
    let existing=client.query_opt("SELECT shobj_description(oid,'pg_authid'),rolsuper,rolcreatedb,rolcreaterole,rolreplication,rolbypassrls FROM pg_roles WHERE rolname=$1",&[&binding.role]).map_err(|_|E::DatabaseUnavailable)?;
    if let Some(row) = existing {
        if row.get::<_, Option<String>>(0).as_deref() != Some(marker.as_str())
            || (1..=5).any(|i| row.get::<_, bool>(i))
        {
            return Err(E::DatabaseConflict);
        }
    } else {
        let verifier = Zeroizing::new(postgres_protocol::password::scram_sha_256(
            password.as_bytes(),
        ));
        let mut tx = client.transaction().map_err(|_| E::DatabaseUnavailable)?;
        tx.batch_execute(&format!("CREATE ROLE {} LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOREPLICATION NOBYPASSRLS PASSWORD {}; COMMENT ON ROLE {} IS {};",ident(&binding.role),literal(&verifier),ident(&binding.role),literal(&marker))).map_err(|_|E::DatabaseUnavailable)?;
        tx.commit().map_err(|_| E::DatabaseUnavailable)?;
    }
    let exists=client.query_opt("SELECT r.rolname,shobj_description(d.oid,'pg_database') FROM pg_database d JOIN pg_roles r ON r.oid=d.datdba WHERE datname=$1",&[&binding.database]).map_err(|_|E::DatabaseUnavailable)?;
    if let Some(row) = exists {
        let owner: String = row.get(0);
        let comment: Option<String> = row.get(1);
        if owner != binding.role || comment.as_deref().is_some_and(|v| v != marker) {
            return Err(E::DatabaseConflict);
        }
        // A pending row plus the uniquely marked owner role admits recovery between
        // CREATE DATABASE (nontransactional) and its subsequent ownership comment.
        if comment.is_none() && binding.ready {
            return Err(E::DatabaseConflict);
        }
    } else {
        client
            .batch_execute(&format!(
                "CREATE DATABASE {} OWNER {} TEMPLATE template0 ALLOW_CONNECTIONS false",
                ident(&binding.database),
                ident(&binding.role)
            ))
            .map_err(|_| E::DatabaseUnavailable)?;
    }
    client.batch_execute(&format!("REVOKE ALL ON DATABASE {} FROM PUBLIC; GRANT CONNECT,TEMPORARY ON DATABASE {} TO {}; COMMENT ON DATABASE {} IS {}; ALTER DATABASE {} ALLOW_CONNECTIONS true;",ident(&binding.database),ident(&binding.database),ident(&binding.role),ident(&binding.database),literal(&marker),ident(&binding.database))).map_err(|_|E::DatabaseUnavailable)?;
    let credentials = Credentials {
        binding: binding.clone(),
        port: instance.port,
        password,
    };
    credentials.connection_test()?;
    binding.ready = true;
    store.connection.execute(
        "UPDATE database_bindings SET record=?1 WHERE project_id=?2",
        params![serde_json::to_string(&binding).unwrap(), project.id],
    )?;
    Ok(binding)
}
