use devforge_projects::project::{Catalog, ProjectStore};
use devforge_runtime::{
    Cancellation,
    package::{Limits, PackageStore, Target},
};
use devforge_serving::{
    authority::Authority,
    manager::{Options, Serving},
    site::{Site, SiteStore},
};
use serde_json::json;
use std::{
    fs,
    path::{Path, PathBuf},
};
#[path = "support/secrets.rs"]
mod secrets;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let cache = PathBuf::from(args.first().ok_or("cache required")?);
    let report = PathBuf::from(args.get(1).ok_or("report required")?);
    let root = tempfile::tempdir()?;
    let result = run(root.path(), &cache, &report);
    if result.is_err() {
        show_logs(root.path());
    }
    result
}
fn run(root: &Path, cache: &Path, report: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let secrets = secrets::Secrets::open(root.join("fixture-secrets.json"));
    let projects = ProjectStore::open(root.join("projects"))?;
    let sites = SiteStore::open(root.join("serving"))?;
    let catalog = Catalog::bundled();
    let packages = PackageStore::open(root.join("packages"), Limits::default())?;
    let target = Target::current()?;
    let m0: Vec<serde_json::Value> =
        serde_json::from_str(include_str!("../../../runtime/m0-candidates.json"))?;
    for (id, p) in catalog
        .packages
        .iter()
        .filter(|(_, p)| p.target == target && ["php", "node", "caddy"].contains(&p.kind.as_str()))
    {
        eprintln!("Installing {id}");
        let source = m0
            .iter()
            .find(|e| e["url"].as_str() == Some(&p.url))
            .unwrap();
        let file = cache.join(format!(
            "{}-{}",
            source["id"].as_str().unwrap(),
            p.url.rsplit('/').next().unwrap()
        ));
        packages.install_cached(p, &file, &Cancellation::default(), |_| {})?;
    }
    let choose = |kind: &str, version: &str| {
        catalog
            .packages
            .iter()
            .find(|(_, p)| {
                p.kind == kind
                    && p.version.starts_with(version)
                    && p.target == target
                    && (kind != "php" || p.executables.contains_key("php"))
            })
            .unwrap()
            .0
            .clone()
    };
    for (name, php, node) in [("a", "8.4", "v22"), ("b", "8.5", "v24")] {
        let path = root.join(format!("{name} Ўзбек & ' quoted"));
        fs::create_dir_all(path.join("public"))?;
        fs::write(
            path.join("composer.json"),
            br#"{"require":{"php":"^8.3","laravel/framework":"^13"}}"#,
        )?;
        fs::write(
            path.join("public/index.php"),
            format!(
                "<?php header('Content-Type: application/json'); echo json_encode(['project'=>'{name}','php'=>PHP_VERSION,'secure'=>($_SERVER['HTTPS']??'off')==='on','uri'=>$_SERVER['REQUEST_URI']]);"
            ),
        )?;
        fs::write(path.join("public/static.txt"), name)?;
        let project = projects.import(
            &path,
            name,
            &choose("php", php),
            &choose("node", node),
            &catalog,
        )?;
        sites.configure(
            &projects,
            Site {
                project_id: project.id,
                domain: format!("{name}-{}.devforge.test", &sites.owner()?[..8]),
                https: true,
                vite: false,
            },
        )?;
    }
    let ca = Authority::open(sites.root().join("authority"), &sites.owner()?, &secrets)?;
    let mut serving = Serving::start(
        &sites,
        &projects,
        &catalog,
        &packages,
        &secrets,
        Options::default(),
    )?;
    let status = serving.status();
    for (index, site) in status.sites.iter().enumerate() {
        let version = if index == 0 { "8.4" } else { "8.5" };
        let host = &site.domain;
        let mut builder = reqwest::blocking::Client::builder()
            .no_proxy()
            .add_root_certificate(reqwest::Certificate::from_der(ca.der())?);
        builder = builder.resolve(host, ([127, 0, 0, 1], status.https.selected).into());
        let client = builder.build()?;
        let response: serde_json::Value = client
            .get(format!("{}/nested/path", site.https_url.as_ref().unwrap()))
            .send()?
            .error_for_status()?
            .json()?;
        assert!(response["php"].as_str().unwrap().starts_with(version));
        assert_eq!(response["secure"], true);
        assert_eq!(
            client
                .get(format!("{}/static.txt", site.https_url.as_ref().unwrap()))
                .send()?
                .text()?,
            if index == 0 { "a" } else { "b" }
        );
    }
    assert!(!secrets.values().is_empty());
    serving.stop()?;
    assert!(!serving.status().running);
    let result = json!({"host":{"os":std::env::consts::OS,"arch":std::env::consts::ARCH},"status":status,"routing_php_versions_static_and_tls":true,"system_changes":false,"passed":true});
    fs::write(report, serde_json::to_vec_pretty(&result)?)?;
    println!("M4_REPORT:{}", serde_json::to_string(&result)?);
    Ok(())
}
fn show_logs(root: &Path) {
    if let Ok(entries) = fs::read_dir(root) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                show_logs(&path);
            } else if path.extension().is_some_and(|e| e == "log")
                && let Ok(text) = fs::read_to_string(&path)
            {
                eprintln!(
                    "{}: {}",
                    path.file_name().unwrap().to_string_lossy(),
                    text.chars()
                        .rev()
                        .take(2500)
                        .collect::<String>()
                        .chars()
                        .rev()
                        .collect::<String>()
                );
            }
        }
    }
}
