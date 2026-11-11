//! Disposable-VM M4 integration. Only explicit helper phases touch system state.
use devforge_projects::{
    database::RunningPostgres,
    environment,
    execution::{Context, Tool},
    project::{Catalog, ProjectStore},
};
use devforge_runtime::{
    Cancellation,
    package::{Limits, PackageStore, Target},
};
use devforge_serving::{
    authority::Authority,
    manager::{Options, Serving},
    site::{Site, SiteStore},
    system::{self, Operation, Plan},
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    ffi::OsString,
    fs,
    net::TcpListener,
    path::{Path, PathBuf},
};
#[path = "support/secrets.rs"]
mod secrets;
#[cfg(windows)]
#[path = "../../projects/examples/support/windows_unprivileged.rs"]
mod windows_unprivileged;
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
#[derive(Serialize, Deserialize)]
struct Session {
    root: PathBuf,
    owner: String,
    domains: Vec<String>,
    hosts_before: String,
    trust_was_present: bool,
}
fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    #[cfg(windows)]
    if matches!(args.first().map(String::as_str), Some("run" | "prepare"))
        && let Some(code) = windows_unprivileged::relaunch()?
    {
        std::process::exit(code);
    }
    let mode = args.first().ok_or("mode required")?;
    let path = PathBuf::from(args.get(1).ok_or("session path required")?);
    if mode == "prepare" {
        let root = path.parent().unwrap().join("m4-private-fixture");
        if root.exists() {
            return Err("fixture already exists".into());
        }
        fs::create_dir_all(&root)?;
        let root = fs::canonicalize(root)?;
        let root = dunce::simplified(&root).to_path_buf();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&root, fs::Permissions::from_mode(0o700))?;
        }
        let sites = SiteStore::open(root.join("serving"))?;
        let secrets = secrets::Secrets::open(root.join("secrets.json"));
        let owner = sites.owner()?;
        let ca = Authority::open(sites.root().join("authority"), &owner, &secrets)?;
        let domains = vec![
            format!("a-{}.devforge.test", &owner[..8]),
            format!("b-{}.devforge.test", &owner[..8]),
        ];
        let before = fs::read(system::hosts_path()?)?;
        let s = Session {
            root,
            owner: owner.clone(),
            domains: domains.clone(),
            hosts_before: devforge_serving::hash(&before),
            trust_was_present: system::trusted(ca.der())?,
        };
        assert!(!s.trust_was_present);
        save_plan(
            &s.root,
            "hosts-install",
            &system::hosts_plan(
                &owner,
                domains
                    .iter()
                    .flat_map(|d| [d.clone(), format!("vite-{d}")])
                    .collect(),
            )?,
        )?;
        save_plan(
            &s.root,
            "trust-install",
            &Plan {
                schema: 1,
                owner,
                action: Operation::TrustInstall {
                    certificate_der: ca.der().to_vec(),
                },
            },
        )?;
        fs::write(&path, serde_json::to_vec_pretty(&s)?)?;
        println!("M4_SESSION:{}", path.display());
        return Ok(());
    }
    let s: Session = serde_json::from_slice(&fs::read(&path)?)?;
    match mode.as_str() {
        "run" => {
            let result = run(&s, &args[2..]);
            if result.is_err() {
                show_logs(&s.root);
            }
            result
        }
        "cleanup-plan" => {
            save_plan(
                &s.root,
                "hosts-remove",
                &system::hosts_plan(&s.owner, vec![])?,
            )?;
            let ca = fs::read(s.root.join("serving/authority/ca.der"))?;
            let trust_effect = fs::read(s.root.join("trust-install.effect.json"))
                .ok()
                .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok());
            if !s.trust_was_present
                && trust_effect.is_some_and(|e| e["changed"] == true && e["preexisting"] == false)
            {
                save_plan(
                    &s.root,
                    "trust-remove",
                    &Plan {
                        schema: 1,
                        owner: s.owner,
                        action: Operation::TrustRemove {
                            certificate_der: ca,
                        },
                    },
                )?;
            }
            Ok(())
        }
        "verify-cleanup" => {
            assert_eq!(
                devforge_serving::hash(&fs::read(system::hosts_path()?)?),
                s.hosts_before
            );
            assert!(!system::trusted(&fs::read(
                s.root.join("serving/authority/ca.der")
            )?)?);
            println!("M4_CLEANUP:hosts_restored_certificate_removed_and_untrusted");
            Ok(())
        }
        _ => Err("unknown phase".into()),
    }
}
fn save_plan(root: &Path, name: &str, plan: &Plan) -> Result<()> {
    fs::write(
        root.join(format!("{name}.json")),
        serde_json::to_vec_pretty(plan)?,
    )?;
    fs::write(root.join(format!("{name}.digest")), plan.digest())?;
    Ok(())
}
fn run(s: &Session, args: &[String]) -> Result<()> {
    assert!(!system::elevated()?, "serving must run without admin/root");
    let root = &s.root;
    let cache = PathBuf::from(args.first().ok_or("cache required")?);
    let report = PathBuf::from(args.get(1).ok_or("report required")?);
    let secrets = secrets::Secrets::open(root.join("secrets.json"));
    let projects = ProjectStore::open(root.join("projects"))?;
    let sites = SiteStore::open(root.join("serving"))?;
    assert_eq!(sites.owner()?, s.owner);
    let ca = Authority::open(sites.root().join("authority"), &s.owner, &secrets)?;
    let trusted = system::trusted(ca.der())?;
    if args.iter().any(|a| a == "--system-integrated") {
        assert!(trusted, "CA must be trusted by native OS");
    }
    let catalog = Catalog::bundled();
    let packages = PackageStore::open(root.join("packages"), Limits::default())?;
    let target = Target::current()?;
    let m0: Vec<serde_json::Value> =
        serde_json::from_str(include_str!("../../../runtime/m0-candidates.json"))?;
    for (id, p) in catalog.packages.iter().filter(|(_, p)| {
        p.target == target
            && (["php", "node", "composer", "caddy"].contains(&p.kind.as_str())
                || (p.kind == "postgresql" && p.variant == "server"))
    }) {
        eprintln!("Installing {id}");
        let source = m0
            .iter()
            .find(|e| e["url"].as_str() == Some(&p.url))
            .ok_or("missing pinned source")?;
        let file = cache.join(format!(
            "{}-{}",
            source["id"].as_str().unwrap(),
            p.url.rsplit('/').next().unwrap()
        ));
        if file.is_file() {
            packages.install_cached(p, &file, &Cancellation::default(), |_| {})?;
        } else if args.iter().any(|a| a == "--download-missing") {
            packages.download(p, &Cancellation::default(), |_| {})?;
        } else {
            return Err(format!("missing archive {id}").into());
        }
    }
    let select = |kind: &str, v: &str| {
        catalog
            .packages
            .iter()
            .find(|(_, p)| {
                p.target == target
                    && p.kind == kind
                    && p.version.starts_with(v)
                    && (kind != "php" || p.executables.contains_key("php"))
            })
            .unwrap()
            .0
            .clone()
    };
    let seed = root.join("bootstrap");
    fs::create_dir_all(seed.join("public"))?;
    fs::write(
        seed.join("composer.json"),
        br#"{"require":{"php":"^8.3","laravel/framework":"^13"}}"#,
    )?;
    fs::write(seed.join("public/index.php"), "<?php")?;
    let bootstrap = projects.import(
        &seed,
        "bootstrap",
        &select("php", "8.4"),
        &select("node", "v22"),
        &catalog,
    )?;
    let exec = |id: &str, tool: Tool, args: Vec<OsString>| -> Result<String> {
        let result = Context::resolve(&projects, id, &catalog, &packages, &secrets)?
            .invocation(tool, args, &catalog, &packages)?
            .run(root.join("command-logs"))?;
        let output = result
            .logs
            .iter()
            .map(|l| l.text.clone())
            .collect::<String>();
        if !result.success {
            return Err(format!("{} failed: {output}", tool.name()).into());
        }
        Ok(output)
    };
    let ap = root.join("A Ўзбек & ' Laravel");
    let bp = root.join("B Русский Laravel");
    exec(
        &bootstrap.id,
        Tool::Composer,
        vec![
            "create-project".into(),
            "laravel/laravel".into(),
            ap.as_os_str().into(),
            "v13.11.0".into(),
            "--no-dev".into(),
            "--no-scripts".into(),
            "--no-plugins".into(),
            "--no-interaction".into(),
            "--prefer-dist".into(),
        ],
    )?;
    copy_tree(&ap, &bp)?;
    let mut a = projects.import(
        &ap,
        "A",
        &select("php", "8.4"),
        &select("node", "v22"),
        &catalog,
    )?;
    let mut b = projects.import(
        &bp,
        "B",
        &select("php", "8.5"),
        &select("node", "v24"),
        &catalog,
    )?;
    let pg = catalog
        .packages
        .iter()
        .find(|(_, p)| p.target == target && p.kind == "postgresql" && p.variant == "server")
        .unwrap()
        .0;
    let instance = projects.initialize_instance(pg, 0, &catalog, &packages, &secrets)?;
    let mut database =
        RunningPostgres::start(&projects, &instance.id, &catalog, &packages, &secrets)?;
    for project in [&mut a, &mut b] {
        database.provision(&projects, project, &secrets)?;
        for (key, value) in [
            ("APP_ENV", "testing"),
            ("APP_KEY", "0123456789abcdef0123456789abcdef"),
            ("SESSION_DRIVER", "file"),
            ("SESSION_COOKIE", "devforge_m4_session"),
            ("CACHE_STORE", "array"),
            ("LOG_CHANNEL", "stderr"),
        ] {
            *project = environment::set(
                &projects,
                &secrets,
                &project.id,
                project.revision,
                key,
                value,
            )?;
        }
        projects
            .credentials(&project.id, &secrets)?
            .connect()?
            .batch_execute(&format!(
                "CREATE TABLE m4_sentinel(value text); INSERT INTO m4_sentinel VALUES('{}')",
                project.name
            ))?;
        fs::write(
            project.path.join("routes/web.php"),
            include_str!("support/routes.php"),
        )?;
        fs::write(
            project.path.join("public/static.txt"),
            project.name.as_bytes(),
        )?;
    }
    fs::write(ap.join("package.json"),br#"{"private":true,"type":"module","devDependencies":{"vite":"8.3.8","laravel-vite-plugin":"3.2.1"}}"#)?;
    fs::write(
        ap.join("vite.config.js"),
        "import {defineConfig} from 'vite'; import laravel from 'laravel-vite-plugin'; export default defineConfig({plugins:[laravel({input:['resources/js/m4.js'],refresh:false})]});",
    )?;
    fs::create_dir_all(ap.join("resources/js"))?;
    fs::write(
        ap.join("resources/js/m4-message.js"),
        "export const message='BEFORE';\n",
    )?;
    fs::write(ap.join("resources/js/m4.js"), include_str!("support/m4.js"))?;
    fs::write(
        ap.join("resources/views/m4.blade.php"),
        include_str!("support/m4.blade.php"),
    )?;
    fs::write(
        bp.join("resources/views/m4.blade.php"),
        "<!doctype html><html><body>B Laravel</body></html>",
    )?;
    exec(
        &a.id,
        Tool::Npm,
        ["install", "--ignore-scripts", "--no-audit", "--no-fund"]
            .into_iter()
            .map(Into::into)
            .collect(),
    )?;
    sites.configure(
        &projects,
        Site {
            project_id: a.id.clone(),
            domain: s.domains[0].clone(),
            https: true,
            vite: true,
        },
    )?;
    sites.configure(
        &projects,
        Site {
            project_id: b.id.clone(),
            domain: s.domains[1].clone(),
            https: true,
            vite: false,
        },
    )?;
    let laravel_version = exec(&a.id, Tool::Artisan, vec!["--version".into()])?;
    let mut serving = Serving::start(
        &sites,
        &projects,
        &catalog,
        &packages,
        &secrets,
        Options::default(),
    )?;
    let status = serving.status();
    assert!(status.running);
    assert!(status.sites.iter().all(|s| s.php_ready));
    if trusted {
        assert!(status.sites.iter().all(|s| s.domain_resolves
            && s.system_trusted
            && s.preferred_url.starts_with("https://")));
    }
    for (index, site) in status.sites.iter().enumerate() {
        let client = reqwest::blocking::Client::builder()
            .no_proxy()
            .add_root_certificate(reqwest::Certificate::from_der(ca.der())?)
            .resolve(&site.domain, ([127, 0, 0, 1], status.https.selected).into())
            .build()?;
        let url = site.https_url.as_ref().unwrap();
        let value: serde_json::Value = client
            .get(format!("{url}/__m4/nested/path"))
            .send()?
            .error_for_status()?
            .json()?;
        assert_eq!(value["database"], if index == 0 { "A" } else { "B" });
        assert_eq!(value["secure"], true);
        assert!(
            value["php"]
                .as_str()
                .unwrap()
                .starts_with(if index == 0 { "8.4" } else { "8.5" })
        );
        assert_eq!(
            client
                .get(format!("{url}/static.txt"))
                .send()?
                .error_for_status()?
                .text()?,
            if index == 0 { "A" } else { "B" }
        );
        assert_eq!(client.get(format!("{url}/.env")).send()?.status(), 404);
    }
    let browser = if args.iter().any(|a| a == "--browser") {
        let config = root.join("browser-input.json");
        fs::write(
            &config,
            serde_json::to_vec(
                &json!({"url":status.sites[0].preferred_url,"message_file":ap.join("resources/js/m4-message.js"),"upload_file":root.join("upload.txt"),"root":root}),
            )?,
        )?;
        fs::write(root.join("upload.txt"), "M4_UPLOAD")?;
        let script = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../apps/desktop/scripts/m4-browser.mjs");
        let output = exec(
            &a.id,
            Tool::Node,
            vec![script.into_os_string(), config.into_os_string()],
        )?;
        let encoded = output
            .lines()
            .find_map(|l| l.strip_prefix("M4_BROWSER_REPORT:"))
            .ok_or_else(|| format!("browser report missing: {output}"))?;
        serde_json::from_str::<serde_json::Value>(encoded)?
    } else {
        json!({"executed":false})
    };
    serving.stop()?;
    assert!(!ap.join("public/hot").exists());
    // Foreign listeners remain open throughout a full restart with explicit alternate ports.
    let foreign_http = TcpListener::bind(("127.0.0.1", 0))?;
    let foreign_https = TcpListener::bind(("127.0.0.1", 0))?;
    let options = Options {
        http_port: foreign_http.local_addr()?.port(),
        https_port: foreign_https.local_addr()?.port(),
    };
    let mut restarted = Serving::start(&sites, &projects, &catalog, &packages, &secrets, options)?;
    let conflict = restarted.status();
    assert!(conflict.http.fallback && conflict.https.fallback);
    assert_ne!(conflict.http.selected, options.http_port);
    assert!(std::net::TcpStream::connect(foreign_http.local_addr()?).is_ok());
    assert!(std::net::TcpStream::connect(foreign_https.local_addr()?).is_ok());
    restarted.stop()?;
    let denied = system::hosts_plan(&s.owner, vec![])?;
    assert_eq!(
        denied.apply(&denied.digest()).unwrap_err(),
        devforge_serving::ServingError::PermissionRequired
    );
    // A fresh, untrusted authority still offers a working HTTP *.localhost URL.
    let fallback_sites = SiteStore::open(root.join("untrusted-serving"))?;
    fallback_sites.configure(
        &projects,
        Site {
            project_id: b.id.clone(),
            domain: format!("fallback-{}.devforge.test", &s.owner[..8]),
            https: true,
            vite: false,
        },
    )?;
    let mut fallback = Serving::start(
        &fallback_sites,
        &projects,
        &catalog,
        &packages,
        &secrets,
        Options {
            http_port: 0,
            https_port: 0,
        },
    )?;
    let fs_status = fallback.status();
    assert!(!fs_status.sites[0].system_trusted);
    assert!(
        fs_status.sites[0]
            .preferred_url
            .starts_with("http://fallback-")
    );
    let host = fallback_sites.sites()?[0].alias();
    let client = reqwest::blocking::Client::builder()
        .no_proxy()
        .resolve(&host, ([127, 0, 0, 1], fs_status.http.selected).into())
        .build()?;
    assert_eq!(
        client
            .get(format!("{}/static.txt", fs_status.sites[0].fallback_url))
            .send()?
            .error_for_status()?
            .text()?,
        "B"
    );
    let fallback_browser = if args.iter().any(|a| a == "--browser") {
        let config = root.join("fallback-browser.json");
        fs::write(
            &config,
            serde_json::to_vec(
                &json!({"mode":"fallback","url":fs_status.sites[0].fallback_url,"root":root}),
            )?,
        )?;
        let script = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../apps/desktop/scripts/m4-browser.mjs");
        let output = exec(
            &b.id,
            Tool::Node,
            vec![script.into_os_string(), config.into_os_string()],
        )?;
        let encoded = output
            .lines()
            .find_map(|l| l.strip_prefix("M4_BROWSER_REPORT:"))
            .ok_or_else(|| format!("fallback browser report missing: {output}"))?;
        serde_json::from_str::<serde_json::Value>(encoded)?
    } else {
        json!({"executed":false})
    };
    fallback.stop()?;
    database.stop()?;
    for path in files(root)
        .into_iter()
        .filter(|p| p.extension().is_some_and(|e| e == "log"))
    {
        let text = fs::read_to_string(path)?;
        for secret in secrets.values().iter().filter(|s| s.len() >= 16) {
            assert!(!text.contains(secret), "secret in owned log");
        }
    }
    let result = json!({"host":{"os":std::env::consts::OS,"arch":std::env::consts::ARCH},"laravel_version":laravel_version.trim(),"status":status,"conflict_ports":conflict,"browser":browser,"fallback_browser":fallback_browser,"laravel_routing_php_versions_database_static_tls":true,"denied_elevation_recovery":true,"untrusted_http_localhost_fallback":true,"untrusted_fallback_status":fs_status,"foreign_listeners_preserved":true,"restart_and_hot_cleanup":true,"secret_logs_clean":true,"system_trusted":trusted,"passed":true});
    fs::write(report, serde_json::to_vec_pretty(&result)?)?;
    println!("M4_REPORT:{}", serde_json::to_string(&result)?);
    Ok(())
}
fn copy_tree(from: &Path, to: &Path) -> Result<()> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let p = entry.path();
        let dest = to.join(entry.file_name());
        if p.is_dir() {
            copy_tree(&p, &dest)?;
        } else {
            fs::copy(p, dest)?;
        }
    }
    Ok(())
}
fn files(root: &Path) -> Vec<PathBuf> {
    let mut out = vec![];
    if let Ok(entries) = fs::read_dir(root) {
        for e in entries.flatten() {
            if e.path().is_dir() {
                out.extend(files(&e.path()));
            } else {
                out.push(e.path());
            }
        }
    }
    out
}
fn show_logs(root: &Path) {
    for p in files(root)
        .iter()
        .filter(|p| p.extension().is_some_and(|e| e == "log"))
    {
        if let Ok(text) = fs::read_to_string(p) {
            eprintln!(
                "LOG {}: {}",
                p.file_name().unwrap().to_string_lossy(),
                text.chars()
                    .rev()
                    .take(3000)
                    .collect::<String>()
                    .chars()
                    .rev()
                    .collect::<String>()
            );
        }
    }
}
