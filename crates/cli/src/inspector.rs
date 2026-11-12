pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let id = std::env::args()
        .nth(2)
        .ok_or("Usage: devforge inspect <project-id>")?;
    let root = devforge_platform::app_data_dir()?;
    devforge_projects::project::ProjectStore::open(root.join("projects"))?.get(&id)?;
    let capture = devforge_inspector::read(&root.join("inspector"), &id)?;
    println!(
        "{}",
        serde_json::to_string_pretty(
            &serde_json::json!({"project_id":id,"profiles":devforge_inspector::profile::analyze(&capture.events),"events":capture.events})
        )?
    );
    Ok(())
}
