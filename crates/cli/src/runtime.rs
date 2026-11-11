use devforge_runtime::{
    Cancellation, Result, RuntimeError,
    package::{Limits, Package, PackageStore},
    process::{LogPolicy, Observed, ProcessSpec, Readiness, RestartPolicy, Supervisor},
};
use std::{collections::BTreeMap, path::PathBuf, time::Duration};
pub fn run() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(2).collect();
    let catalog: serde_json::Value =
        serde_json::from_str(include_str!("../../../runtime/catalog.json"))
            .map_err(|_| RuntimeError::InvalidPackage)?;
    if args.first().map(String::as_str) == Some("catalog") {
        println!("{}", serde_json::to_string_pretty(&catalog).unwrap());
        return Ok(());
    }
    let package: Package = serde_json::from_value(
        catalog["packages"][args.get(1).ok_or(RuntimeError::InvalidPackage)?].clone(),
    )
    .map_err(|_| RuntimeError::InvalidPackage)?;
    let root = devforge_platform::app_data_dir().map_err(|_| RuntimeError::Io)?;
    let store = PackageStore::open(root.join("runtimes"), Limits::default())?;
    match args.first().map(String::as_str) {
        Some("install") => {
            let emit = |p| eprintln!("{}", serde_json::to_string(&p).unwrap());
            let installed = if let Some(archive) = args.get(2) {
                store.install_cached(
                    &package,
                    &PathBuf::from(archive),
                    &Cancellation::default(),
                    emit,
                )?
            } else {
                store.download(&package, &Cancellation::default(), emit)?
            };
            println!(
                "{}",
                serde_json::json!({"key":installed.key,"directory":installed.directory,"reused":installed.reused})
            );
        }
        Some("remove") => {
            store.remove(&package)?;
            println!("{}", serde_json::json!({"removed":true}));
        }
        Some("probe") => {
            let name = args.get(2).ok_or(RuntimeError::InvalidPackage)?;
            let executable = store.executable(&package, name)?;
            let version_flag = if package.kind == "caddy" {
                "version"
            } else if package.kind == "php" {
                "-v"
            } else {
                "--version"
            };
            let mut environment = BTreeMap::new();
            for key in ["SystemRoot", "WINDIR", "TEMP", "TMP", "HOME", "USERPROFILE"] {
                if let Some(v) = std::env::var_os(key) {
                    environment.insert(key.into(), v);
                }
            }
            environment.insert(
                "PATH".into(),
                executable.path().parent().unwrap().as_os_str().to_owned(),
            );
            let mut supervisor = Supervisor::open(root.join("process-logs"))?;
            supervisor.start(ProcessSpec {
                id: "cli-probe".into(),
                executable,
                args: vec![version_flag.into()],
                directory: std::env::current_dir()?,
                environment,
                readiness: Readiness::SuccessfulExit,
                readiness_timeout: Duration::from_secs(10),
                stop_grace: Duration::from_millis(200),
                restart: RestartPolicy::default(),
                logs: LogPolicy::default(),
            })?;
            let state = supervisor.wait_terminal("cli-probe", Duration::from_secs(15))?;
            println!(
                "{}",
                serde_json::json!({"process":state,"logs":supervisor.logs("cli-probe")?.0})
            );
            if state.observed != Observed::Succeeded {
                return Err(state.failure.unwrap_or(RuntimeError::ProcessExited));
            }
        }
        _ => return Err(RuntimeError::InvalidPackage),
    }
    Ok(())
}
