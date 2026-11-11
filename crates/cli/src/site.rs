use devforge_platform::{APP_IDENTIFIER, NativeSecretStore, app_data_dir};
use devforge_projects::project::{Catalog, ProjectStore};
use devforge_runtime::package::{Limits, PackageStore};
use devforge_serving::{
    Result, ServingError as E,
    authority::Authority,
    manager::{Options, Serving},
    site::{Site, SiteStore},
    system::{self, Operation, Plan},
};
use std::{
    io::{self, BufRead},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
fn out(v: impl serde::Serialize) {
    println!("{}", serde_json::to_string_pretty(&v).unwrap());
}
pub fn run() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(2).collect();
    let arg = |i| {
        args.get(i)
            .map(String::as_str)
            .ok_or(E::InvalidConfiguration)
    };
    let root = app_data_dir().map_err(|_| E::StorageUnavailable)?;
    let sites = SiteStore::open(root.join("serving"))?;
    let projects = ProjectStore::open(root.join("projects"))?;
    let secrets = NativeSecretStore::new(format!("{APP_IDENTIFIER}.projects"));
    match arg(0)? {
        "list" => out(sites.sites()?),
        "configure" => {
            let site = Site {
                project_id: arg(1)?.into(),
                domain: arg(2)?.into(),
                https: !args.iter().any(|s| s == "--http-only"),
                vite: args.iter().any(|s| s == "--vite"),
            };
            sites.configure(&projects, site.clone())?;
            out(site);
        }
        "remove" => {
            sites.remove(arg(1)?)?;
            out(serde_json::json!({"removed":true,"system_configuration_preserved":true}));
        }
        "authority" => {
            let ca = Authority::open(sites.root().join("authority"), &sites.owner()?, &secrets)?;
            out(
                serde_json::json!({"fingerprint_sha256":ca.fingerprint(),"certificate":ca.directory().join("ca.pem"),"system_trusted":system::trusted(ca.der())?}),
            );
        }
        "system-plan" => {
            let owner = sites.owner()?;
            let plan = match arg(1)? {
                "hosts" => system::hosts_plan(
                    &owner,
                    sites
                        .sites()?
                        .iter()
                        .flat_map(|s| [s.domain.clone(), s.vite_domain()])
                        .collect(),
                )?,
                "hosts-remove" => system::hosts_plan(&owner, vec![])?,
                "trust" => {
                    let ca = Authority::open(sites.root().join("authority"), &owner, &secrets)?;
                    Plan {
                        schema: 1,
                        owner,
                        action: Operation::TrustInstall {
                            certificate_der: ca.der().to_vec(),
                        },
                    }
                }
                "trust-remove" => Plan {
                    schema: 1,
                    owner,
                    action: Operation::TrustRemove {
                        certificate_der: std::fs::read(sites.root().join("authority/ca.der"))?,
                    },
                },
                _ => return Err(E::InvalidConfiguration),
            };
            let digest = plan.digest();
            plan.validate(&digest)?;
            let path = std::path::Path::new(arg(2)?);
            let bytes = serde_json::to_vec_pretty(&plan).unwrap();
            {
                use std::io::Write;
                let mut f = std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(path)?;
                f.write_all(&bytes)?;
                f.sync_all()?;
            }
            out(
                serde_json::json!({"plan":path,"digest":digest,"applied":false,"helper":"devforge-helper apply <plan> <digest>","elevation_required":true}),
            );
        }
        "serve" => {
            let packages = PackageStore::open(root.join("runtimes"), Limits::default())?;
            let mut server = Serving::start(
                &sites,
                &projects,
                &Catalog::bundled(),
                &packages,
                &secrets,
                Options::default(),
            )?;
            let stop = Arc::new(AtomicBool::new(false));
            let signal = stop.clone();
            ctrlc::set_handler(move || signal.store(true, Ordering::Relaxed))
                .map_err(|_| E::StartFailed)?;
            let stdin = stop.clone();
            std::thread::spawn(move || {
                for line in io::stdin().lock().lines() {
                    if line.ok().as_deref() == Some("quit") {
                        stdin.store(true, Ordering::Relaxed);
                        break;
                    }
                }
            });
            out(server.status());
            while !stop.load(Ordering::Relaxed) {
                if !server.status().running {
                    return Err(E::StartFailed);
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            server.stop()?;
            out(server.status());
        }
        _ => return Err(E::InvalidConfiguration),
    };
    Ok(())
}
