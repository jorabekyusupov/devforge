//! Re-check M0 cached candidates through the production M2 store/supervisor.
//! Usage: cargo run -p devforge-runtime --example catalog_smoke -- <cache> <report> [--download-missing]
use devforge_runtime::{
    Cancellation,
    package::{Limits, Package, PackageStore, Target},
    process::{LogPolicy, Observed, ProcessSpec, Readiness, RestartPolicy, Supervisor},
};
use serde::Deserialize;
use serde_json::json;
use std::{
    collections::BTreeMap,
    fs,
    io::{Read, Write},
    net::TcpStream,
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant},
};
#[derive(Deserialize)]
struct Catalog {
    packages: BTreeMap<String, Package>,
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let cache = PathBuf::from(args.first().ok_or("cache path required")?);
    let report = PathBuf::from(args.get(1).ok_or("report path required")?);
    let catalog: Catalog = serde_json::from_str(include_str!("../../../runtime/m2-catalog.json"))?;
    let work = tempfile::tempdir()?;
    let store = PackageStore::open(work.path().join("runtime-store"), Limits::default())?;
    let mut supervisor = Supervisor::open(work.path().join("logs"))?;
    let mut results = vec![];
    let mut nodes = vec![];
    for (id, package) in catalog
        .packages
        .iter()
        .filter(|(_, p)| p.target == Target::current().unwrap())
    {
        eprintln!("Installing {id}");
        let filename = package.url.rsplit('/').next().unwrap();
        let archive = cache.join(format!("{id}-{filename}"));
        let installed = if archive.is_file() {
            store.install_cached(package, &archive, &Cancellation::default(), |_| {})?
        } else if args.iter().any(|arg| arg == "--download-missing") {
            store.download(package, &Cancellation::default(), |_| {})?
        } else {
            return Err(format!(
                "Missing cached archive for {id}; use --download-missing explicitly"
            )
            .into());
        };
        let mut probes = vec![];
        for name in package.executables.keys() {
            let process_id = format!("probe-{}-{}", results.len(), probes.len());
            let flag = if package.kind == "caddy" {
                "version"
            } else if package.kind == "php" {
                "-v"
            } else {
                "--version"
            };
            let spec = spec(
                &store,
                package,
                name,
                &process_id,
                work.path(),
                vec![flag.into()],
                Readiness::SuccessfulExit,
            )?;
            supervisor.start(spec)?;
            let state = supervisor.wait_terminal(&process_id, Duration::from_secs(20))?;
            let logs = supervisor.logs(&process_id)?.0;
            if state.observed != Observed::Succeeded {
                return Err(format!("{id}/{name}: {state:?} {logs:?}").into());
            }
            probes.push(json!({"executable":name,"state":state,"logs":logs}));
            supervisor.stop(&process_id)?;
        }
        if package.kind == "node" {
            let node = store.executable(package, "node")?;
            let npm = if package.target == Target::WindowsX64 {
                node.path()
                    .parent()
                    .unwrap()
                    .join("node_modules/npm/bin/npm-cli.js")
            } else {
                node.path()
                    .parent()
                    .unwrap()
                    .parent()
                    .unwrap()
                    .join("lib/node_modules/npm/bin/npm-cli.js")
            };
            let name = format!("npm-{}", nodes.len());
            supervisor.start(spec(
                &store,
                package,
                "node",
                &name,
                work.path(),
                vec![npm.clone().into_os_string(), "--version".into()],
                Readiness::SuccessfulExit,
            )?)?;
            let state = supervisor.wait_terminal(&name, Duration::from_secs(20))?;
            if state.observed != Observed::Succeeded {
                return Err(format!(
                    "npm: {state:?}; executable={npm:?}; logs={:?}",
                    supervisor.logs(&name)?.0
                )
                .into());
            }
            probes.push(json!({"executable":"npm via installed node","state":state,"logs":supervisor.logs(&name)?.0}));
            supervisor.stop(&name)?;
            nodes.push(package.clone());
        }
        results.push(json!({"catalog_id":id,"key":installed.key,"installed":true,"probes":probes}));
    }
    let mut services = vec![];
    for (index, package) in nodes.iter().enumerate() {
        let id = format!("node-service-{index}");
        let body = format!("devforge-m2-{}", package.version);
        let code = format!(
            "const http=require('http');const server=http.createServer((req,res)=>res.end({}));server.listen(0,'127.0.0.1',()=>console.log('M2_READY:'+server.address().port));process.stdin.on('data',()=>server.close(()=>process.exit(0)));",
            serde_json::to_string(&body)?
        );
        supervisor.start(spec(
            &store,
            package,
            "node",
            &id,
            work.path(),
            vec!["-e".into(), code.into()],
            Readiness::Output {
                stream: "stdout",
                needle: "M2_READY:".into(),
            },
        )?)?;
        let until = Instant::now() + Duration::from_secs(10);
        while supervisor.snapshot(&id)?.observed != Observed::Ready {
            if Instant::now() > until {
                return Err("node readiness timeout".into());
            }
            thread::sleep(Duration::from_millis(10));
        }
        let logs = supervisor
            .logs(&id)?
            .0
            .iter()
            .map(|l| l.text.clone())
            .collect::<String>();
        let port = logs
            .lines()
            .find_map(|l| l.strip_prefix("M2_READY:"))
            .ok_or("port missing")?
            .parse::<u16>()?;
        let mut stream = TcpStream::connect(("127.0.0.1", port))?;
        stream.set_read_timeout(Some(Duration::from_secs(3)))?;
        stream.write_all(b"GET / HTTP/1.0\r\nHost: localhost\r\n\r\n")?;
        let mut response = String::new();
        stream.read_to_string(&mut response)?;
        if !response.contains("200 OK") || !response.ends_with(&body) {
            return Err("wrong HTTP result".into());
        }
        services.push(json!({"version":package.version,"ready":true,"http_status":200,"body":body,"removal_while_running":store.remove(package).err()}));
    }
    for index in 0..nodes.len() {
        let state = supervisor.stop(&format!("node-service-{index}"))?;
        if state.observed != Observed::Stopped {
            return Err("stop failed".into());
        }
    }
    drop(supervisor);
    for package in &nodes {
        store.remove(package)?;
    }
    let result = json!({"host":{"os":std::env::consts::OS,"arch":std::env::consts::ARCH},"source":if args.iter().any(|a|a=="--download-missing"){"cached or explicit HTTPS download"}else{"cached"},"scope":"M2 installation plus actual managed version probes/npm/two parallel Node HTTP services; task-local storage; no system/project changes","packages":results,"node_services":services,"node_removal_after_stop":true,"passed":true});
    fs::write(report, serde_json::to_vec_pretty(&result)?)?;
    println!("M2_REPORT:{}", serde_json::to_string(&result)?);
    Ok(())
}
fn spec(
    store: &PackageStore,
    package: &Package,
    name: &str,
    id: &str,
    cwd: &Path,
    args: Vec<std::ffi::OsString>,
    readiness: Readiness,
) -> Result<ProcessSpec, Box<dyn std::error::Error>> {
    let executable = store.executable(package, name)?;
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
    Ok(ProcessSpec {
        id: id.into(),
        executable,
        args,
        directory: fs::canonicalize(cwd)?,
        environment,
        readiness,
        readiness_timeout: Duration::from_secs(10),
        stop_grace: Duration::from_millis(500),
        restart: RestartPolicy::default(),
        logs: LogPolicy::default(),
    })
}
