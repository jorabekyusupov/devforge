use devforge_platform::{
    app_data_dir,
    distribution::{AppLease, prerequisites},
};
use devforge_serving::{
    site::SiteStore,
    system::{self, Operation, Plan},
};
use std::{fs, io::Write, path::Path};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
fn plans(root: &Path) -> Result<Vec<Plan>> {
    let sites_path = root.join("serving/sites.json");
    if !sites_path.exists() {
        return Ok(vec![]);
    }
    let sites = SiteStore::open(root.join("serving"))?;
    let owner = sites.owner()?;
    let hosts = system::hosts_plan(&owner, vec![])?;
    let before = fs::read(system::hosts_path()?)?;
    let mut result = vec![];
    if devforge_serving::hosts::patch(&before, &owner, &[])? != before {
        result.push(hosts);
    }
    let ca = sites.root().join("authority/ca.der");
    if ca.exists() {
        // Removal validates ownership even for an expired root and never reads its key.
        let plan = Plan {
            schema: 1,
            owner,
            action: Operation::TrustRemove {
                certificate_der: fs::read(ca)?,
            },
        };
        plan.validate(&plan.digest())?;
        if let Operation::TrustRemove { certificate_der } = &plan.action
            && system::trust_present(certificate_der)?
        {
            result.push(plan);
        }
    }
    Ok(result)
}
pub fn run() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(2).collect();
    let command = args.first().map(String::as_str).unwrap_or("");
    if command == "prerequisites" {
        println!("{}", serde_json::to_string_pretty(&prerequisites()?)?);
        return Ok(());
    }
    let root = app_data_dir().map_err(|e| e.to_string())?;
    if !AppLease::stopped(&root)? {
        return Err("desktop_running_quit_from_tray".into());
    }
    if command == "check-stopped" {
        return Ok(());
    }
    let plans = plans(&root)?;
    match command {
        "check-uninstall" if !plans.is_empty() => {
            Err("owned_system_resources_require_cleanup_plan".into())
        }
        "check-uninstall" => Ok(()),
        "cleanup-plan" => {
            let output = Path::new(args.get(1).ok_or("output_directory_required")?);
            fs::create_dir(output)?;
            let mut report = vec![];
            for (i, plan) in plans.iter().enumerate() {
                let path = output.join(format!("cleanup-{i}.json"));
                let mut f = fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&path)?;
                f.write_all(&serde_json::to_vec_pretty(plan)?)?;
                f.sync_all()?;
                report.push(serde_json::json!({"plan":path,"digest":plan.digest()}));
            }
            println!(
                "{}",
                serde_json::json!({"plans":report,"applied":false,"elevation_required":!plans.is_empty(),"data_preserved":true,"macos_nontrusting_metadata_may_remain":cfg!(target_os="macos")})
            );
            Ok(())
        }
        _ => Err("invalid_distribution_command".into()),
    }
}
