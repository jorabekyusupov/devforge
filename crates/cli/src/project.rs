use devforge_platform::{APP_IDENTIFIER, NativeSecretStore, app_data_dir};
use devforge_projects::{
    ProjectError as E, Result,
    database::{RunningPostgres, provision_on_running},
    env_file::EnvPlan,
    execution::{Context, Tool},
    project::{Catalog, ProjectStore, inspect},
    terminal,
};
use devforge_runtime::package::{Limits, PackageStore};
use std::{
    io::{self, BufRead, Read},
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
fn out(value: impl serde::Serialize) {
    println!("{}", serde_json::to_string_pretty(&value).unwrap());
}
/// Explicit interactive terminal entrypoint. No metadata migration or shared
/// Context preparation is allowed on this manager-coexisting path.
pub fn snapshot_shell(id: &str, fixture: bool) -> Result<i32> {
    let selection = crate::stores::Selection::select(fixture).map_err(|_| E::StorageUnavailable)?;
    let root = selection.root().map_err(|_| E::StorageUnavailable)?;
    let metadata = std::fs::symlink_metadata(&root)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(E::StorageUnavailable);
    }
    let _lease = devforge_platform::distribution::OperationLease::acquire(&root)?;
    let store = ProjectStore::open_existing_current(root.join("projects"))?;
    let packages = terminal::existing_packages(&root.join("runtimes"))?;
    let catalog = Catalog::bundled();
    let secrets = selection.project();
    terminal::shell(&store, id, &catalog, &packages, secrets.as_ref())
}

pub fn run() -> Result<i32> {
    let args: Vec<_> = std::env::args().skip(2).collect();
    let arg = |i| args.get(i).map(String::as_str).ok_or(E::InvalidProject);
    let root = app_data_dir().map_err(|_| E::StorageUnavailable)?;
    let store = ProjectStore::open(root.join("projects"))?;
    let packages = PackageStore::open(root.join("runtimes"), Limits::default())?;
    let catalog = Catalog::bundled();
    let secrets = NativeSecretStore::new(format!("{APP_IDENTIFIER}.projects"));
    match arg(0)? {
        "inspect" => out(inspect(Path::new(arg(1)?))?),
        "import" => out(store.import(Path::new(arg(1)?), arg(4)?, arg(2)?, arg(3)?, &catalog)?),
        "list" => out(store.list()?),
        "configure" => {
            let p = store.get(arg(1)?)?;
            out(store.configure(&p.id, p.revision, arg(4)?, arg(2)?, arg(3)?, &catalog)?);
        }
        "forget" => {
            let p = store.get(arg(1)?)?;
            store.forget(&p.id, p.revision)?;
            out(serde_json::json!({"forgotten":true,"data_preserved":true}));
        }
        "env-set" => {
            let p = store.get(arg(1)?)?;
            let mut value = zeroize::Zeroizing::new(String::new());
            io::stdin().take(4099).read_to_string(&mut value)?;
            if value.len() >= 4099 {
                return Err(E::InvalidEnvironment);
            }
            while value.ends_with(['\r', '\n']) {
                value.pop();
            }
            out(devforge_projects::environment::set(
                &store,
                &secrets,
                &p.id,
                p.revision,
                arg(2)?,
                &value,
            )?);
        }
        "db-init" => out(store.initialize_instance(
            arg(1)?,
            args.get(2)
                .map(|s| s.parse())
                .transpose()
                .map_err(|_| E::InvalidProject)?
                .unwrap_or(0),
            &catalog,
            &packages,
            &secrets,
        )?),
        "db-list" => out(store.instances()?),
        "db-serve" => {
            let mut server =
                RunningPostgres::start(&store, arg(1)?, &catalog, &packages, &secrets)?;
            let stop = Arc::new(AtomicBool::new(false));
            let flag = stop.clone();
            ctrlc::set_handler(move || flag.store(true, Ordering::Relaxed))
                .map_err(|_| E::ExecutionFailed)?;
            let stdin = stop.clone();
            std::thread::spawn(move || {
                for line in io::stdin().lock().lines() {
                    if line.ok().as_deref() == Some("quit") {
                        stdin.store(true, Ordering::Relaxed);
                        break;
                    }
                }
            });
            out(serde_json::json!({"ready":true,"instance":arg(1)?}));
            while !stop.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_millis(50));
            }
            server.stop()?;
            out(serde_json::json!({"stopped":true}));
        }
        "db-provision" => {
            let p = store.get(arg(1)?)?;
            out(provision_on_running(&store, arg(2)?, &p, &secrets)?);
        }
        "db-check" => out(
            serde_json::json!({"connected":store.credentials(arg(1)?,&secrets)?.connection_test()?}),
        ),
        "env-plan" => {
            let plan = EnvPlan::build(&store, arg(1)?, &secrets)?;
            out(
                serde_json::json!({"changed_keys":plan.changed_keys(),"values_redacted":true,"applied":false}),
            );
        }
        "env-apply" => {
            out(EnvPlan::build(&store, arg(1)?, &secrets)?.apply(&store)?);
        }
        "exec" => {
            let context = Context::resolve(&store, arg(1)?, &catalog, &packages, &secrets)?;
            let tool = Tool::parse(arg(2)?)?;
            let index = if args.get(3).is_some_and(|s| s == "--") {
                4
            } else {
                3
            };
            let command = context.invocation(
                tool,
                args[index..].iter().map(Into::into).collect(),
                &catalog,
                &packages,
            )?;
            let result = command.run(root.join("project-logs"))?;
            let code = result.exit_code.unwrap_or(1);
            out(result);
            return Ok(code);
        }
        "shell" => return terminal::shell(&store, arg(1)?, &catalog, &packages, &secrets),
        "terminal" => {
            terminal::open(&store, arg(1)?, &root)?;
            out(serde_json::json!({"opened":true}));
        }
        "doctor" => {
            let p = store.get(arg(1)?)?;
            let inspection = inspect(&p.path)?;
            devforge_projects::project::compatible(
                &inspection,
                catalog.get(&p.php)?,
                catalog.get(&p.node)?,
            )?;
            let php = packages.executable(catalog.get(&p.php)?, "php").is_ok();
            let node = packages.executable(catalog.get(&p.node)?, "node").is_ok();
            let binding = store.binding(&p.id)?;
            let connected = if binding.is_some() {
                store
                    .credentials(&p.id, &secrets)
                    .and_then(|c| c.connection_test())
                    .unwrap_or(false)
            } else {
                false
            };
            out(
                serde_json::json!({"project":p,"manifests_valid":true,"php_installed":php,"node_installed":node,"database":binding,"database_connected":connected}),
            );
        }
        _ => return Err(E::InvalidProject),
    }
    Ok(0)
}
