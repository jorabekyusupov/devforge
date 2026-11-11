use devforge_core::SettingsRepository;
use devforge_platform::{SqliteSettings, app_data_dir, diagnostics};
mod project;
mod runtime;
mod site;

fn main() -> Result<(), Box<dyn std::error::Error>> {
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
            "Usage: devforge doctor | site list|configure|remove|authority|system-plan|serve | runtime catalog|install|probe|remove <id> [archive|executable]"
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
