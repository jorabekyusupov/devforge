use devforge_core::SecretStore;
use devforge_platform::{
    APP_IDENTIFIER, NativeSecretStore, app_data_dir, distribution::OperationLease,
};
use devforge_workspace::{Command, Error, Workspace};
use std::sync::Arc;
fn error(code: &str) -> Error {
    Error {
        scope: "debugger",
        code: code.into(),
        project_id: None,
    }
}
pub fn run() -> Result<(), Error> {
    let args = std::env::args().skip(2).collect::<Vec<_>>();
    let arg = |i| {
        args.get(i)
            .map(String::as_str)
            .ok_or_else(|| error("unavailable"))
    };
    let action = arg(0)?;
    let root = app_data_dir().map_err(|_| error("unavailable"))?;
    let _lease = if ["install", "mode"].contains(&action) {
        OperationLease::exclusive(&root)
    } else {
        OperationLease::acquire(&root)
    }
    .map_err(|_| error("busy"))?;
    let secrets: Arc<dyn SecretStore + Send + Sync> = {
        #[cfg(feature = "test-support")]
        {
            if std::env::var_os("DEVFORGE_TEST_SECRETS").is_some() {
                Arc::new(devforge_workspace::testing::FixtureSecrets::open(
                    root.join("e2e-secrets.json"),
                ))
            } else {
                Arc::new(NativeSecretStore::new(format!("{APP_IDENTIFIER}.projects")))
            }
        }
        #[cfg(not(feature = "test-support"))]
        {
            Arc::new(NativeSecretStore::new(format!("{APP_IDENTIFIER}.projects")))
        }
    };
    let mut w = Workspace::open(root, secrets)?;
    let cancel = devforge_runtime::Cancellation::default();
    let signal = cancel.clone();
    ctrlc::set_handler(move || signal.cancel()).map_err(|_| error("busy"))?;
    let id = arg(1)?;
    let value = match action {
        "status" if args.len() == 2 => w.debug_view(id)?,
        "profile" if args.len() == 3 => w.debug_profile(id, arg(2)?)?,
        "source" if args.len() == 3 => w.debug_source(id, arg(2)?)?,
        "install" if args.len() == 3 && arg(2)? == "--trust-source" => w.execute(
            Command::DebugInstall {
                id: id.into(),
                trusted: true,
            },
            &cancel,
        )?,
        "mode" if args.len() == 4 => {
            let p = w.projects.get(id)?;
            let mode = match arg(2)? {
                "off" => devforge_debugger::Mode::Off,
                "debug" => devforge_debugger::Mode::Debug,
                "profile" => devforge_debugger::Mode::Profile,
                _ => return Err(error("unavailable")),
            };
            let mut settings = p.debugger;
            settings.mode = mode;
            settings.runtime = if arg(3)? == "none" {
                None
            } else {
                Some(arg(3)?.into())
            };
            w.execute(
                Command::DebugConfigure {
                    id: id.into(),
                    revision: p.revision,
                    settings,
                    consent: true,
                },
                &cancel,
            )?
        }
        _ => return Err(error("unavailable")),
    };
    w.shutdown()?;
    println!("{}", serde_json::json!({"schema":1,"result":value}));
    Ok(())
}
