//! Explicit native DB acceptance through the public reviewed client. All data,
//! credentials and discovery roots are disposable; source cache is read-only.
#![cfg(target_os = "macos")]
mod common;
use devforge_control::{
    Id, OperationState, Request, Scope,
    mutations::{DatabaseBindInput, DatabaseRestoreInput, MutationInput, MutationReceipt},
};
use devforge_core::{ErrorCode as CoreError, SecretStore, Settings, SettingsRepository};
use devforge_manager::{
    Endpoint,
    auth::{DesktopAuth, Policy, TrustedDesktopAction},
    controller::{Controller, StartIntent, Stores, local_owner_user},
    external_auth::{ExternalCredential, IssuedGrant},
    external_client::{DatabaseClient, ExternalClient},
    external_control::{ExternalInput, ExternalOutput},
    native_client::NativeClient,
};
use devforge_projects::{
    database::{Binding, Engine},
    project::{Catalog, Project, ProjectStore},
};
use devforge_runtime::{
    Cancellation,
    package::{Target, hash_file},
};
use mysql::prelude::Queryable;
use std::{
    collections::BTreeMap,
    fs,
    path::Path,
    sync::{Arc, Condvar, Mutex},
    thread,
    time::{Duration, Instant},
};
fn id(value: &str) -> Id {
    Id::new(value).unwrap()
}
#[derive(Default)]
struct Memory {
    values: Mutex<BTreeMap<String, String>>,
    gate: Mutex<(bool, bool)>,
    signal: Condvar,
}
impl SecretStore for Memory {
    fn get(&self, key: &str) -> Result<Option<String>, CoreError> {
        Ok(self.values.lock().unwrap().get(key).cloned())
    }
    fn set(&self, key: &str, value: &str) -> Result<(), CoreError> {
        let mut gate = self.gate.lock().unwrap();
        if gate.0 {
            gate.1 = true;
            self.signal.notify_all();
            while gate.0 {
                gate = self.signal.wait(gate).unwrap();
            }
        }
        drop(gate);
        self.values.lock().unwrap().insert(key.into(), value.into());
        Ok(())
    }
    fn delete(&self, key: &str) -> Result<(), CoreError> {
        self.values.lock().unwrap().remove(key);
        Ok(())
    }
}
struct Release(Arc<Memory>);
impl Drop for Release {
    fn drop(&mut self) {
        self.0.gate.lock().unwrap().0 = false;
        self.0.signal.notify_all();
    }
}
fn copy_cache(source: &Path, destination: &Path, boundary: &Path) {
    let real = fs::canonicalize(source).unwrap();
    assert!(real.starts_with(boundary));
    if real.is_dir() {
        fs::create_dir_all(destination).unwrap();
        for item in fs::read_dir(real).unwrap() {
            let item = item.unwrap();
            copy_cache(&item.path(), &destination.join(item.file_name()), boundary);
        }
    } else {
        fs::copy(real, destination).unwrap();
    }
}
fn project(store: &ProjectStore, catalog: &Catalog, path: &Path, name: &str) -> Project {
    fs::create_dir_all(path.join("public")).unwrap();
    fs::write(path.join("public/index.php"), b"<?php").unwrap();
    fs::write(
        path.join("composer.json"),
        br#"{"require":{"php":"^8.3","laravel/framework":"^13"}}"#,
    )
    .unwrap();
    let select = |kind: &str, tool: &str| {
        catalog
            .packages
            .iter()
            .find(|(_, p)| {
                p.kind == kind
                    && p.target == Target::current().unwrap()
                    && p.executables.contains_key(tool)
            })
            .unwrap()
            .0
            .clone()
    };
    store
        .import(
            path,
            name,
            &select("php", "php"),
            &select("node", "node"),
            catalog,
        )
        .unwrap()
}
struct Rig {
    root: tempfile::TempDir,
    native: NativeClient,
    endpoint: Endpoint,
    worker: Option<thread::JoinHandle<devforge_manager::controller::Result<()>>>,
    secrets: Arc<Memory>,
    a: Project,
    b: Project,
    first: String,
    second: String,
    backup: String,
    engine: Engine,
    version: String,
    original: Binding,
}
impl Drop for Rig {
    fn drop(&mut self) {
        self.secrets.gate.lock().unwrap().0 = false;
        self.secrets.signal.notify_all();
        let _ = self.native.quit();
        if let Some(worker) = self.worker.take() {
            let joined = worker.join();
            if !thread::panicking() {
                joined.unwrap().unwrap();
            }
        }
    }
}
impl Rig {
    fn new(engine: Engine) -> Self {
        let root = common::root("df-db-native-");
        let cache = fs::canonicalize(
            std::env::var_os("DEVFORGE_DB_CONTROL_RUNTIME_STORE")
                .expect("explicit approved immutable cache required"),
        )
        .unwrap();
        let catalog = Catalog::bundled();
        let target = Target::current().unwrap();
        let select = |kind: &str| {
            catalog
                .packages
                .iter()
                .find(|(_, p)| {
                    p.kind == kind
                        && p.target == target
                        && cache
                            .join("packages")
                            .join(p.key().unwrap())
                            .join(".devforge-receipt.json")
                            .is_file()
                })
                .map(|(id, p)| (id.clone(), p.clone()))
                .expect("reviewed current-catalog runtime absent")
        };
        let (package_id, package) = select(if engine == Engine::Postgresql {
            "postgresql"
        } else {
            "mysql"
        });
        let secrets = Arc::new(Memory::default());
        let mut workspace =
            devforge_workspace::Workspace::open(root.path(), secrets.clone()).unwrap();
        let mut selected = vec![package.clone()];
        if engine == Engine::Postgresql {
            selected.push(select("postgresql-tools").1);
        }
        for p in selected {
            copy_cache(
                &cache.join("packages").join(p.key().unwrap()),
                &root.path().join("runtimes/packages").join(p.key().unwrap()),
                &cache,
            );
            for tool in p.executables.keys() {
                workspace.packages.executable(&p, tool).unwrap();
            }
        }
        let a = project(
            &workspace.projects,
            &catalog,
            &root.path().join("Alpha Ўзбек"),
            "Alpha",
        );
        let b = project(
            &workspace.projects,
            &catalog,
            &root.path().join("Bravo"),
            "Bravo",
        );
        let cancel = Cancellation::default();
        let create = |w: &mut devforge_workspace::Workspace| {
            w.execute(
                devforge_workspace::Command::CreateDatabase {
                    package: package_id.clone(),
                },
                &cancel,
            )
            .unwrap()["id"]
                .as_str()
                .unwrap()
                .to_owned()
        };
        let first = create(&mut workspace);
        let second = create(&mut workspace);
        for project in [&a, &b] {
            workspace
                .execute(
                    devforge_workspace::Command::BindDatabase {
                        id: project.id.clone(),
                        instance: first.clone(),
                    },
                    &cancel,
                )
                .unwrap();
        }
        let original = workspace.projects.binding(&a.id).unwrap().unwrap();
        for (p, value) in [(&a, 701), (&b, 333)] {
            let credentials = workspace
                .projects
                .credentials(&p.id, secrets.as_ref())
                .unwrap();
            if engine == Engine::Postgresql {
                credentials.connect().unwrap().batch_execute(&format!("CREATE TABLE db_control_canary(value BIGINT); INSERT INTO db_control_canary VALUES({value})")).unwrap();
            } else {
                let mut conn = credentials.mysql().unwrap();
                conn.query_drop("CREATE TABLE db_control_canary(value BIGINT)")
                    .unwrap();
                conn.query_drop(format!("INSERT INTO db_control_canary VALUES({value})"))
                    .unwrap();
            }
        }
        let backup = workspace
            .projects
            .create_backup(
                &a.id,
                &catalog,
                &workspace.packages,
                secrets.as_ref(),
                &cancel,
            )
            .unwrap();
        if engine == Engine::Postgresql {
            workspace
                .projects
                .credentials(&a.id, secrets.as_ref())
                .unwrap()
                .connect()
                .unwrap()
                .batch_execute("UPDATE db_control_canary SET value=902")
                .unwrap();
        } else {
            workspace
                .projects
                .credentials(&a.id, secrets.as_ref())
                .unwrap()
                .mysql()
                .unwrap()
                .query_drop("UPDATE db_control_canary SET value=902")
                .unwrap();
        }
        drop(workspace);
        let folder = root.path().join("discovery-empty");
        fs::create_dir(&folder).unwrap();
        devforge_platform::SqliteSettings::open(root.path())
            .unwrap()
            .save(&Settings {
                project_folder: Some(folder.to_str().unwrap().into()),
                ..Default::default()
            })
            .unwrap();
        let control = Arc::new(Memory::default());
        let owner = common::acquire(root.path()).unwrap();
        let mut auth = DesktopAuth::new(
            owner.endpoint().clone(),
            local_owner_user().unwrap(),
            Policy::default(),
        )
        .unwrap();
        let key = auth
            .rotate_desktop_key(
                &TrustedDesktopAction::from_explicit_desktop_action(),
                control.as_ref(),
                Instant::now(),
            )
            .unwrap();
        drop(owner);
        let controller = Controller::start(
            root.path(),
            StartIntent::ExplicitUserStart,
            Stores {
                workspace: secrets.clone(),
                desktop: control,
            },
        )
        .unwrap();
        let endpoint = controller.endpoint().clone();
        let worker = thread::spawn(move || controller.serve(None));
        let until = Instant::now() + Duration::from_secs(10);
        while ExternalClient::observe_owner(&endpoint).is_err() {
            assert!(Instant::now() < until);
            thread::sleep(Duration::from_millis(10));
        }
        let native = NativeClient::authenticate_explicit(endpoint.clone(), &key).unwrap();
        Self {
            root,
            native,
            endpoint,
            worker: Some(worker),
            secrets,
            a,
            b,
            first,
            second,
            backup: backup.id,
            engine,
            version: package.version,
            original,
        }
    }
    fn store(&self) -> ProjectStore {
        ProjectStore::open_existing_current(self.root.path().join("projects")).unwrap()
    }
    fn grant(&mut self) -> IssuedGrant {
        self.native
            .issue_external_grant(
                &id(&self.a.id),
                &[
                    Scope::Observer,
                    Scope::ProjectManage,
                    Scope::SensitiveOperation,
                ],
                Duration::from_secs(600),
            )
            .unwrap()
    }
    fn client(&self, grant: &IssuedGrant) -> DatabaseClient {
        DatabaseClient::connect(
            self.endpoint.clone(),
            ExternalCredential::from_wire(grant.credential.expose_secret()).unwrap(),
        )
        .unwrap()
    }
    fn external(&self, grant: &IssuedGrant) -> ExternalClient {
        ExternalClient::connect(
            self.endpoint.clone(),
            ExternalCredential::from_wire(grant.credential.expose_secret()).unwrap(),
        )
        .unwrap()
    }
    fn request(&self, tool: &str, selected: &str, key: &str) -> Request<MutationInput> {
        Request {
            api_version: Default::default(),
            request_id: id(key),
            project_id: id(&self.a.id),
            expected_revision: Some(self.a.revision),
            idempotency_key: Some(id(key)),
            input: if tool == "bind" {
                MutationInput::DatabaseBind(DatabaseBindInput {
                    instance_id: id(selected),
                    limits: None,
                })
            } else {
                MutationInput::DatabaseRestore(DatabaseRestoreInput {
                    backup_id: id(selected),
                    limits: None,
                })
            },
        }
    }
    fn terminal(&self, grant: &IssuedGrant, receipt: &MutationReceipt) -> OperationState {
        let mut client = self.external(grant);
        let until = Instant::now() + Duration::from_secs(90);
        loop {
            let response = client
                .request(
                    id("db-observe"),
                    ExternalInput::OperationStatus {
                        project_id: receipt.project_id.clone(),
                        operation_id: receipt.operation_id.clone(),
                    },
                    None,
                )
                .unwrap();
            if response
                .error
                .as_ref()
                .is_some_and(|e| e.code == devforge_control::ErrorCode::Busy)
            {
                assert!(Instant::now() < until);
                thread::sleep(Duration::from_millis(20));
                continue;
            }
            let Some(ExternalOutput::OperationStatus(data)) = response.data else {
                panic!("DB status denied: {:?}", response.error)
            };
            let state = data.items[0].state;
            if state.terminal() {
                return state;
            }
            assert!(Instant::now() < until);
            thread::sleep(Duration::from_millis(20));
        }
    }
    fn apply(
        &mut self,
        client: &mut DatabaseClient,
        request: &Request<MutationInput>,
    ) -> (devforge_control::Digest, MutationReceipt) {
        let plan = client.plan(request).unwrap();
        assert!(plan.is_ok(), "DB plan denied: {:?}", plan.error());
        let plan = plan.data().unwrap();
        self.native.approve_service_plan(&plan.digest).unwrap();
        let reply = client.apply(request, &plan.digest).unwrap();
        assert!(reply.is_ok(), "DB apply denied: {:?}", reply.error());
        (plan.digest.clone(), reply.data().unwrap().clone())
    }
}
#[cfg(target_os = "macos")]
#[test]
#[ignore = "explicit isolated cached PostgreSQL/MySQL native reviewed client flow; no downloads"]
fn public_reviewed_database_bind_restore_replay_cancel_preserves_other_project() {
    for engine in [Engine::Postgresql, Engine::Mysql] {
        let mut rig = Rig::new(engine);
        let grant = rig.grant();
        let mut client = rig.client(&grant);
        let warm = rig.request("bind", &rig.first, "warm-owned-db");
        let (_, receipt) = rig.apply(&mut client, &warm);
        assert_eq!(rig.terminal(&grant, &receipt), OperationState::Succeeded);
        let credentials = rig
            .store()
            .credentials(&rig.b.id, rig.secrets.as_ref())
            .unwrap();
        let mut live = if engine == Engine::Postgresql {
            Some(credentials.connect().unwrap())
        } else {
            None
        };
        let mut mysql_live = if engine == Engine::Mysql {
            Some(credentials.mysql().unwrap())
        } else {
            None
        };
        let mysql_connection_id = mysql_live.as_mut().map(|conn| {
            conn.query_first::<u64, _>("SELECT CONNECTION_ID()")
                .unwrap()
                .unwrap()
        });
        let second = rig.request("bind", &rig.second, "bind-second");
        let (_, receipt) = rig.apply(&mut client, &second);
        assert_eq!(rig.terminal(&grant, &receipt), OperationState::Succeeded);
        assert_eq!(
            rig.store().binding(&rig.a.id).unwrap().unwrap().instance_id,
            rig.second
        );
        let back = rig.request("bind", &rig.first, "reuse-retained");
        let (digest, receipt) = rig.apply(&mut client, &back);
        assert_eq!(rig.terminal(&grant, &receipt), OperationState::Succeeded);
        let restored_binding = rig.store().binding(&rig.a.id).unwrap().unwrap();
        assert_eq!(restored_binding.database, rig.original.database);
        let replay = client.apply(&back, &digest).unwrap();
        assert_eq!(replay.data().unwrap().operation_id, receipt.operation_id);
        assert_eq!(
            client
                .lookup(
                    id("lookup-bind"),
                    &id(&rig.a.id),
                    back.idempotency_key.as_ref().unwrap()
                )
                .unwrap()
                .data()
                .unwrap()
                .as_ref()
                .unwrap()
                .operation_id,
            receipt.operation_id
        );
        assert!(
            rig.store()
                .credentials(&rig.b.id, rig.secrets.as_ref())
                .unwrap()
                .connection_test()
                .unwrap()
        );
        if let Some(live) = &mut live {
            assert_eq!(
                live.query_one("SELECT value FROM db_control_canary", &[])
                    .unwrap()
                    .get::<_, i64>(0),
                333
            );
        }
        let restore = rig.request("restore", &rig.backup, "restore-cancelled");
        *rig.secrets.gate.lock().unwrap() = (true, false);
        let release = Release(rig.secrets.clone());
        let (_, receipt) = rig.apply(&mut client, &restore);
        let gate = rig.secrets.gate.lock().unwrap();
        let (held, wait) = rig
            .secrets
            .signal
            .wait_timeout_while(gate, Duration::from_secs(10), |g| !g.1)
            .unwrap();
        assert!(!wait.timed_out());
        drop(held);
        let cancel = rig
            .external(&grant)
            .request(
                id("cancel-owned-restore"),
                ExternalInput::OperationCancel {
                    project_id: receipt.project_id.clone(),
                    operation_id: receipt.operation_id.clone(),
                },
                None,
            )
            .unwrap();
        assert!(cancel.ok);
        drop(release);
        assert_eq!(rig.terminal(&grant, &receipt), OperationState::Cancelled);
        assert_eq!(
            rig.store().binding(&rig.a.id).unwrap().unwrap().database,
            rig.original.database
        );
        let restore = rig.request("restore", &rig.backup, "restore-approved");
        let path = rig.store().backup_path(&rig.a.id, &rig.backup).unwrap();
        let backup_hash = hash_file(&path).unwrap();
        let (digest, receipt) = rig.apply(&mut client, &restore);
        assert_eq!(rig.terminal(&grant, &receipt), OperationState::Succeeded);
        let new_binding = rig.store().binding(&rig.a.id).unwrap().unwrap();
        assert_ne!(new_binding.database, rig.original.database);
        assert!(
            rig.store()
                .credentials(&rig.a.id, rig.secrets.as_ref())
                .unwrap()
                .connection_test()
                .unwrap()
        );
        assert_eq!(hash_file(&path).unwrap(), backup_hash);
        if let Some(live) = &mut live {
            assert_eq!(
                live.query_one("SELECT value FROM db_control_canary", &[])
                    .unwrap()
                    .get::<_, i64>(0),
                333
            );
            assert_eq!(
                rig.store()
                    .credentials(&rig.a.id, rig.secrets.as_ref())
                    .unwrap()
                    .connect()
                    .unwrap()
                    .query_one("SELECT value FROM db_control_canary", &[])
                    .unwrap()
                    .get::<_, i64>(0),
                701
            );
            assert_eq!(
                rig.store()
                    .credentials_for(rig.original.clone(), rig.secrets.as_ref())
                    .unwrap()
                    .connect()
                    .unwrap()
                    .query_one("SELECT value FROM db_control_canary", &[])
                    .unwrap()
                    .get::<_, i64>(0),
                902
            );
        }
        if let Some(live) = &mut mysql_live {
            assert_eq!(
                live.query_first::<i64, _>("SELECT value FROM db_control_canary")
                    .unwrap(),
                Some(333)
            );
            assert_eq!(
                live.query_first::<u64, _>("SELECT CONNECTION_ID()")
                    .unwrap(),
                mysql_connection_id
            );
            assert_eq!(
                rig.store()
                    .credentials(&rig.a.id, rig.secrets.as_ref())
                    .unwrap()
                    .mysql()
                    .unwrap()
                    .query_first::<i64, _>("SELECT value FROM db_control_canary")
                    .unwrap(),
                Some(701)
            );
            assert_eq!(
                rig.store()
                    .credentials_for(rig.original.clone(), rig.secrets.as_ref())
                    .unwrap()
                    .mysql()
                    .unwrap()
                    .query_first::<i64, _>("SELECT value FROM db_control_canary")
                    .unwrap(),
                Some(902)
            );
        }
        let replay = client.apply(&restore, &digest).unwrap();
        assert_eq!(replay.data().unwrap().operation_id, receipt.operation_id);
        assert_eq!(
            rig.store().binding(&rig.a.id).unwrap().unwrap().database,
            new_binding.database
        );
        let journal = fs::read(rig.root.path().join("control/operations.sqlite3")).unwrap();
        let text = String::from_utf8_lossy(&journal);
        for value in rig.secrets.values.lock().unwrap().values() {
            assert!(!text.contains(value));
        }
        println!(
            "{}",
            serde_json::json!({"suite":"public_database_control","engine":rig.engine,"version":rig.version,"target":Target::current().unwrap(),"bind_retained_reuse":true,"restore_fresh_generation":true,"replay_lookup_one_effect":true,"cancel_before_activation_preserves_binding":true,"backup_hash_preserved":true,"other_project_connection_preserved":true,"live_canary_and_old_generation_preserved":true,"native_credentials":false,"downloads":false})
        );
    }
}
