use devforge_core::SettingsRepository;
use devforge_platform::{SqliteSettings, app_data_dir, diagnostics};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::args().nth(1).as_deref() != Some("doctor") {
        eprintln!("Usage: devforge doctor");
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
