use devforge_core::SettingsRepository;
use devforge_platform::{SqliteSettings, app_data_dir, diagnostics};
mod api;
mod config;
mod debugger;
mod distribution;
mod e2e_guard;
mod inspector;
mod manager;
mod project;
mod runtime;
mod site;
mod stores;
mod workspaces;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if let Err(error) = stores::validate_fixture_arguments(&args) {
        eprintln!("{}", serde_json::json!({"error":error}));
        std::process::exit(1);
    }
    if args.first().is_some_and(|arg| arg == "api") {
        std::process::exit(api::run(&args[1..]));
    }
    if let Some(help) = api::legacy_help(&args) {
        println!("{help}");
        return Ok(());
    }
    if let Some(id) = manager::terminal_snapshot_id(&args) {
        match project::snapshot_shell(id, args.iter().any(|arg| arg == stores::FIXTURE_FLAG)) {
            Ok(code) => std::process::exit(code),
            Err(error) => {
                eprintln!("{}", serde_json::json!({"error":error}));
                std::process::exit(1);
            }
        }
    }
    let top = std::env::args().nth(1);
    if top.as_deref() == Some("manager") {
        if let Err(error) = manager::run() {
            eprintln!("{}", serde_json::json!({"error":error}));
            std::process::exit(1);
        }
        return Ok(());
    }
    if matches!(top.as_deref(), Some("--help" | "-h" | "help")) {
        println!(
            "{}",
            serde_json::json!({"commands":["doctor","runtime","project","site","inspect","debug","config","workspace","distribution","manager","api"]})
        );
        return Ok(());
    }
    let _legacy_owner = if matches!(
        top.as_deref(),
        Some(
            "config"
                | "workspace"
                | "debug"
                | "site"
                | "project"
                | "runtime"
                | "inspect"
                | "doctor"
        )
    ) {
        match manager::legacy_guard(top.as_deref().unwrap(), std::env::args().nth(2).as_deref()) {
            Ok(guard) => guard,
            Err(error) => {
                eprintln!("{}", serde_json::json!({"error":error}));
                std::process::exit(1);
            }
        }
    } else {
        None
    };
    if std::env::args().nth(1).as_deref() == Some("distribution") {
        if let Err(error) = distribution::run() {
            eprintln!("{}", serde_json::json!({"error":error.to_string()}));
            std::process::exit(3);
        }
        return Ok(());
    }
    if std::env::args().nth(1).as_deref() == Some("config") {
        if let Err(error) = config::run() {
            eprintln!("{}", serde_json::json!({"error":error}));
            std::process::exit(1);
        }
        return Ok(());
    }
    if std::env::args().nth(1).as_deref() == Some("workspace") {
        if let Err(error) = workspaces::run() {
            eprintln!("{}", serde_json::json!({"error":error}));
            std::process::exit(1);
        }
        return Ok(());
    }
    if std::env::args().nth(1).as_deref() == Some("debug") {
        if let Err(e) = debugger::run() {
            eprintln!("{}", serde_json::json!({"error":e}));
            std::process::exit(1);
        }
        return Ok(());
    }
    let _operation = devforge_platform::distribution::OperationLease::acquire(&app_data_dir()?)?;
    if std::env::args().nth(1).as_deref() == Some("inspect") {
        return inspector::run();
    }
    if std::env::args().nth(1).as_deref() == Some("site") {
        if let Err(error) = site::run() {
            eprintln!("{}", serde_json::json!({"error":error}));
            std::process::exit(1);
        }
        return Ok(());
    }
    if std::env::args().nth(1).as_deref() == Some("project") {
        match project::run() {
            Ok(0) => return Ok(()),
            Ok(code) => std::process::exit(code),
            Err(error) => {
                eprintln!("{}", serde_json::json!({"error":error}));
                std::process::exit(1);
            }
        }
    }
    if std::env::args().nth(1).as_deref() == Some("runtime") {
        if let Err(error) = runtime::run() {
            eprintln!("{}", serde_json::json!({"error":error}));
            std::process::exit(1);
        }
        return Ok(());
    }
    if std::env::args().nth(1).as_deref() != Some("doctor") {
        eprintln!(
            "Usage: devforge debug status|install|mode|source|profile | workspace list|plan|create|cleanup-plan|cleanup | config init|export|plan|setup|secret|status | project inspect|import|list|configure|doctor|exec|terminal | manager serve|health | doctor | inspect <project-id> | site list|configure|remove|authority|system-plan|serve | runtime catalog|install|probe|remove <id> [archive|executable]"
        );
        std::process::exit(2);
    }
    let db = SqliteSettings::open(&app_data_dir()?)?;
    println!(
        "{}",
        serde_json::to_string_pretty(
            &serde_json::json!({"diagnostics":diagnostics(&db),"settings":db.load()?})
        )?
    );
    Ok(())
}
