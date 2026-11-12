use devforge_core::SecretStore;
use devforge_platform::{
    APP_IDENTIFIER, NativeSecretStore, app_data_dir, distribution::OperationLease,
};
use devforge_runtime::Cancellation;
use devforge_workspace::{Error, Workspace};
use std::{io::Read, path::Path, sync::Arc};
fn error(code: &str) -> Error {
    Error {
        scope: "workspace",
        code: code.into(),
        project_id: None,
    }
}
pub fn run() -> Result<(), Error> {
    let args = std::env::args().skip(2).collect::<Vec<_>>();
    let arg = |n| {
        args.get(n)
            .map(String::as_str)
            .ok_or_else(|| error("manifest_usage"))
    };
    let action = arg(0)?;
    let root = app_data_dir().map_err(|_| error("storage_unavailable"))?;
    let _lease = if ["setup", "secret"].contains(&action) {
        OperationLease::exclusive(&root)
    } else {
        OperationLease::acquire(&root)
    }
    .map_err(|_| error("manifest_busy"))?;
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
    let mut w = Workspace::open(&root, secrets)?;
    let cancel = Cancellation::default();
    let signal = cancel.clone();
    ctrlc::set_handler(move || signal.cancel()).map_err(|_| error("manifest_busy"))?;
    let output = match action {
        "init" => {
            if args.len() != 5 {
                return Err(error("manifest_usage"));
            }
            w.manifest_init(Path::new(arg(1)?), arg(2)?, arg(3)?, arg(4)?)?;
            serde_json::json!({"schema":1,"created":true})
        }
        "export" => {
            if !(2..=3).contains(&args.len()) {
                return Err(error("manifest_usage"));
            }
            let m = w.manifest_export(arg(1)?, args.get(2).map(Path::new))?;
            serde_json::json!({"schema":1,"exported":true,"secret_values_exported":false,"manifest":m})
        }
        "plan" => {
            if args.len() > 3 || args.get(2).is_some_and(|v| v != "--write-env") {
                return Err(error("manifest_usage"));
            }
            serde_json::to_value(
                w.manifest_plan(Path::new(arg(1)?), args.iter().any(|s| s == "--write-env"))?,
            )
            .unwrap()
        }
        "status" => {
            if args.len() != 2 {
                return Err(error("manifest_usage"));
            }
            serde_json::to_value(w.manifest_status(Path::new(arg(1)?))?).unwrap()
        }
        "secret" => {
            if args.len() != 3 {
                return Err(error("manifest_usage"));
            }
            let mut value = zeroize::Zeroizing::new(String::new());
            std::io::stdin()
                .take(4099)
                .read_to_string(&mut value)
                .map_err(|_| error("manifest_secret_invalid"))?;
            if value.len() >= 4099 {
                return Err(error("manifest_secret_invalid"));
            }
            while value.ends_with(['\r', '\n']) {
                value.pop();
            }
            w.manifest_secret(Path::new(arg(1)?), arg(2)?, &value)?;
            serde_json::json!({"schema":1,"stored":true,"reference":arg(2)?})
        }
        "setup" => {
            let mut review = None;
            let mut write = false;
            let mut trusted = false;
            let mut i = 2;
            while i < args.len() {
                match args[i].as_str() {
                    "--review" => {
                        i += 1;
                        if review.is_some() {
                            return Err(error("manifest_usage"));
                        }
                        review = Some(arg(i)?);
                    }
                    "--write-env" => write = true,
                    "--trust" => trusted = true,
                    _ => return Err(error("manifest_usage")),
                }
                i += 1;
            }
            serde_json::to_value(w.manifest_setup(
                Path::new(arg(1)?),
                review.ok_or_else(|| error("manifest_usage"))?,
                write,
                trusted,
                &cancel,
            )?)
            .unwrap()
        }
        _ => return Err(error("manifest_usage")),
    };
    println!("{}", serde_json::to_string_pretty(&output).unwrap());
    Ok(())
}
