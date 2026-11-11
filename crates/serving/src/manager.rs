use crate::{
    Result, ServingError as E,
    authority::Authority,
    config::{self, Route},
    site::{PortChoice, PortReservation, SiteStore},
};
use devforge_core::SecretStore;
use devforge_projects::{
    execution::{Context, Invocation, Tool},
    project::{Catalog, ProjectStore},
};
use devforge_runtime::{
    package::{PackageStore, Target},
    process::{LogPolicy, Observed, ProcessSpec, Readiness, RestartPolicy, Supervisor},
};
use serde::Serialize;
use std::{
    fs::{self, File},
    net::{SocketAddr, ToSocketAddrs},
    path::PathBuf,
    thread,
    time::{Duration, Instant},
};

#[derive(Clone, Copy)]
pub struct Options {
    pub http_port: u16,
    pub https_port: u16,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            http_port: 80,
            https_port: 443,
        }
    }
}
#[derive(Debug, Clone, Serialize)]
pub struct SiteStatus {
    pub project_id: String,
    pub domain: String,
    pub domain_resolves: bool,
    pub http_url: String,
    pub https_url: Option<String>,
    pub fallback_url: String,
    pub preferred_url: String,
    pub certificate_active: bool,
    pub system_trusted: bool,
    pub php_ready: bool,
    pub vite_ready: bool,
}
#[derive(Debug, Clone, Serialize)]
pub struct Status {
    pub running: bool,
    pub http: PortChoice,
    pub https: PortChoice,
    pub sites: Vec<SiteStatus>,
    pub nonce: String,
}
pub struct Serving {
    supervisor: Supervisor,
    _invocations: Vec<Invocation>,
    _lock: Option<File>,
    status: Status,
    status_file: PathBuf,
    root: PathBuf,
    hot_files: Vec<(PathBuf, String)>,
    stopped: bool,
}
impl Serving {
    pub fn start(
        store: &SiteStore,
        projects: &ProjectStore,
        catalog: &Catalog,
        packages: &PackageStore,
        secrets: &dyn SecretStore,
        options: Options,
    ) -> Result<Self> {
        if crate::system::elevated()? {
            return Err(E::ElevatedProcessUnsupported);
        }
        let lock = store.lock("session.lock")?;
        let sites = store.sites()?;
        if sites.is_empty() || sites.len() > 16 {
            return Err(E::InvalidConfiguration);
        }
        let http = PortReservation::choose(options.http_port, 8080)?;
        let https = PortReservation::choose(options.https_port, 8443)?;
        let h = http.choice.clone();
        let s = https.choice.clone();
        if h.selected == s.selected {
            return Err(E::InvalidConfiguration);
        }
        let nonce = crate::id();
        let root = store.root().join("sessions").join(&nonce);
        crate::private_dir(&root)?;
        let ca = if sites.iter().any(|site| site.https) {
            Some(Authority::open(
                store.root().join("authority"),
                &store.owner()?,
                secrets,
            )?)
        } else {
            None
        };
        let trusted = ca
            .as_ref()
            .is_some_and(|ca| crate::system::trusted(ca.der()).unwrap_or(false));
        let status = Status {
            running: false,
            http: h,
            https: s,
            sites: Vec::new(),
            nonce: nonce.clone(),
        };
        let mut manager = Self {
            supervisor: Supervisor::open(store.root().join("logs"))?,
            _invocations: Vec::new(),
            _lock: Some(lock),
            status,
            status_file: store.root().join("status.json"),
            root,
            hot_files: Vec::new(),
            stopped: false,
        };
        let mut routes = Vec::new();
        for site in sites {
            let project = projects.get(&site.project_id)?;
            let public =
                fs::canonicalize(project.path.join("public")).map_err(|_| E::InvalidPublicRoot)?;
            if !public.starts_with(&project.path) || !public.join("index.php").is_file() {
                return Err(E::InvalidPublicRoot);
            }
            let resolves = (site.domain.as_str(), 80)
                .to_socket_addrs()
                .is_ok_and(|all| {
                    let addresses: Vec<_> = all.collect();
                    addresses
                        .iter()
                        .any(|a| a.ip() == std::net::Ipv4Addr::LOCALHOST)
                        && addresses.iter().all(|a| a.ip().is_loopback())
                });
            let host = if resolves {
                site.domain.clone()
            } else {
                site.alias()
            };
            let secure = site.https && trusted;
            let origin = url(
                if secure { "https" } else { "http" },
                &host,
                if secure {
                    manager.status.https.selected
                } else {
                    manager.status.http.selected
                },
            );
            let php_port = PortReservation::choose(0, 0)?;
            let port = php_port.choice.selected;
            let config = manager.root.join(format!("fpm-{}.conf", project.id));
            let args = if cfg!(windows) {
                vec!["-b".into(), format!("127.0.0.1:{port}").into()]
            } else {
                crate::atomic(&config, config::fpm(port).as_bytes())?;
                vec![
                    "--nodaemonize".into(),
                    "--fpm-config".into(),
                    dunce::simplified(&config).as_os_str().to_owned(),
                ]
            };
            let invocation = Context::resolve(projects, &project.id, catalog, packages, secrets)?
                .web_origin(&origin)?
                .invocation(Tool::PhpWeb, args, catalog, packages)?;
            let id = invocation.id().to_owned();
            drop(php_port);
            invocation.start_service(
                &mut manager.supervisor,
                Readiness::Tcp {
                    address: ([127, 0, 0, 1], port).into(),
                },
            )?;
            manager._invocations.push(invocation);
            manager.wait_ready(&id)?;
            let probe = manager.root.join(format!("probe-{}.php", project.id));
            crate::atomic(&probe,format!("<?php header('Content-Type: application/json'); echo json_encode(['token'=>'{nonce}','php'=>PHP_VERSION,'ini'=>php_ini_loaded_file()]);").as_bytes())?;
            crate::fastcgi::probe(port, &probe, &nonce, &catalog.get(&project.php)?.version)?;
            let leaf = if site.https {
                Some(
                    ca.as_ref()
                        .ok_or(E::AuthorityInvalid)?
                        .issue(&manager.root.join("tls").join(&project.id), &site.names())?,
                )
            } else {
                None
            };
            let mut vite_port = None;
            if site.vite {
                let vite_entry = project.path.join("node_modules/vite/dist/node/index.js");
                if !vite_entry.is_file() {
                    return Err(E::ViteUnavailable);
                }
                let hot = project.path.join("public/hot");
                if hot.exists() {
                    return Err(E::HotFileConflict);
                }
                let reserve = PortReservation::choose(0, 0)?;
                let vport = reserve.choice.selected;
                let vhost = if resolves {
                    site.vite_domain()
                } else {
                    site.vite_alias()
                };
                let vorigin = url(
                    if secure { "https" } else { "http" },
                    &vhost,
                    if secure {
                        manager.status.https.selected
                    } else {
                        manager.status.http.selected
                    },
                );
                let data = manager.root.join(format!("vite-{}.json", project.id));
                let launcher = manager.root.join("vite-launcher.mjs");
                crate::atomic(&launcher, VITE_LAUNCHER.as_bytes())?;
                let cors = [
                    url("http", &site.domain, manager.status.http.selected),
                    url("http", &site.alias(), manager.status.http.selected),
                    url("https", &site.domain, manager.status.https.selected),
                    url("https", &site.alias(), manager.status.https.selected),
                ];
                crate::atomic(&data,&serde_json::to_vec(&serde_json::json!({"entry":dunce::simplified(&vite_entry),"root":dunce::simplified(&project.path),"port":vport,"origin":vorigin,"host":vhost,"publicPort":if secure{manager.status.https.selected}else{manager.status.http.selected},"protocol":if secure{"wss"}else{"ws"},"cors":cors,"nonce":nonce})).unwrap())?;
                let invocation =
                    Context::resolve(projects, &project.id, catalog, packages, secrets)?
                        .web_origin(&origin)?
                        .frontend()
                        .invocation(
                            Tool::Node,
                            vec![
                                dunce::simplified(&launcher).as_os_str().to_owned(),
                                dunce::simplified(&data).as_os_str().to_owned(),
                            ],
                            catalog,
                            packages,
                        )?;
                let id = invocation.id().to_owned();
                drop(reserve);
                {
                    use std::io::Write;
                    let mut reservation = std::fs::OpenOptions::new()
                        .create_new(true)
                        .write(true)
                        .open(&hot)
                        .map_err(|_| E::HotFileConflict)?;
                    reservation.write_all(vorigin.as_bytes())?;
                }
                manager.hot_files.push((hot, vorigin));
                invocation.start_service(
                    &mut manager.supervisor,
                    Readiness::Output {
                        stream: "stdout",
                        needle: format!("DEVFORGE_VITE_READY:{nonce}"),
                    },
                )?;
                manager._invocations.push(invocation);
                manager.wait_ready(&id)?;
                vite_port = Some(vport);
            }
            manager.status.sites.push(SiteStatus {
                project_id: project.id.clone(),
                domain: site.domain.clone(),
                domain_resolves: resolves,
                http_url: url("http", &site.domain, manager.status.http.selected),
                https_url: site
                    .https
                    .then(|| url("https", &site.domain, manager.status.https.selected)),
                fallback_url: url("http", &site.alias(), manager.status.http.selected),
                preferred_url: origin,
                certificate_active: site.https,
                system_trusted: trusted,
                php_ready: true,
                vite_ready: site.vite,
            });
            routes.push(Route {
                site,
                public,
                php: port,
                vite: vite_port,
                leaf,
            });
        }
        let text = config::caddy(
            &routes,
            manager.status.http.selected,
            manager.status.https.selected,
            &nonce,
            &manager.root.join("caddy-data"),
        )?;
        let path = manager.root.join("Caddyfile");
        crate::atomic(&path, text.as_bytes())?;
        let version = catalog.defaults.get("caddy").ok_or(E::RuntimeUnavailable)?;
        let package = catalog
            .packages
            .values()
            .find(|p| {
                p.kind == "caddy" && p.version == *version && p.target == Target::current().unwrap()
            })
            .ok_or(E::RuntimeUnavailable)?;
        let executable = packages.executable(package, "caddy")?;
        let mut environment = std::collections::BTreeMap::new();
        for key in [
            "SystemRoot",
            "WINDIR",
            "TEMP",
            "TMP",
            "HOME",
            "USERPROFILE",
            "APPDATA",
            "LOCALAPPDATA",
        ] {
            if let Some(value) = std::env::var_os(key) {
                environment.insert(key.into(), value);
            }
        }
        manager.supervisor.start(ProcessSpec {
            id: "caddy-validate".into(),
            executable: executable.clone(),
            args: vec![
                "validate".into(),
                "--config".into(),
                dunce::simplified(&path).as_os_str().to_owned(),
                "--adapter".into(),
                "caddyfile".into(),
            ],
            directory: manager.root.clone(),
            environment: environment.clone(),
            readiness: Readiness::SuccessfulExit,
            readiness_timeout: Duration::from_secs(10),
            stop_grace: Duration::from_secs(2),
            restart: RestartPolicy::default(),
            logs: LogPolicy::default(),
        })?;
        if manager
            .supervisor
            .wait_terminal("caddy-validate", Duration::from_secs(15))?
            .observed
            != Observed::Succeeded
        {
            return Err(E::InvalidConfiguration);
        }
        manager.supervisor.stop("caddy-validate")?;
        drop(http);
        drop(https);
        manager.supervisor.start(ProcessSpec {
            id: "caddy".into(),
            executable,
            args: vec![
                "run".into(),
                "--config".into(),
                dunce::simplified(&path).as_os_str().to_owned(),
                "--adapter".into(),
                "caddyfile".into(),
            ],
            directory: manager.root.clone(),
            environment,
            readiness: Readiness::Tcp {
                address: ([127, 0, 0, 1], manager.status.http.selected).into(),
            },
            readiness_timeout: Duration::from_secs(15),
            stop_grace: Duration::from_secs(5),
            restart: RestartPolicy {
                max_restarts: 1,
                backoff: Duration::from_millis(200),
            },
            logs: LogPolicy::default(),
        })?;
        manager.wait_ready("caddy")?;
        for route in &routes {
            probe(
                &route.site.domain,
                manager.status.http.selected,
                false,
                None,
                &nonce,
            )?;
            if route.site.https {
                probe(
                    &route.site.domain,
                    manager.status.https.selected,
                    true,
                    ca.as_ref().map(Authority::der),
                    &nonce,
                )?;
            }
        }
        manager.status.running = true;
        crate::atomic(
            &store.root().join("status.json"),
            &serde_json::to_vec_pretty(&manager.status).unwrap(),
        )?;
        Ok(manager)
    }
    fn wait_ready(&self, id: &str) -> Result<()> {
        let until = Instant::now() + Duration::from_secs(30);
        loop {
            let s = self.supervisor.snapshot(id)?;
            if s.observed == Observed::Ready {
                return Ok(());
            }
            if matches!(s.observed, Observed::Failed | Observed::Stopped) || Instant::now() >= until
            {
                return Err(E::StartFailed);
            }
            thread::sleep(Duration::from_millis(20));
        }
    }
    pub fn status(&self) -> Status {
        let mut status = self.status.clone();
        status.running = !self.stopped
            && self
                .supervisor
                .snapshot("caddy")
                .is_ok_and(|s| s.observed == Observed::Ready);
        for site in &mut status.sites {
            site.php_ready = self
                .supervisor
                .snapshot(&format!("exec-{}-php-web", site.project_id))
                .is_ok_and(|s| s.observed == Observed::Ready);
            if site.vite_ready {
                site.vite_ready = self
                    .supervisor
                    .snapshot(&format!("exec-{}-node", site.project_id))
                    .is_ok_and(|s| s.observed == Observed::Ready);
            }
        }
        status
    }
    pub fn stop(&mut self) -> Result<()> {
        if self.stopped {
            return Ok(());
        }
        let _ = self.supervisor.stop("caddy");
        for invocation in self._invocations.iter().rev() {
            let _ = self.supervisor.stop(invocation.id());
        }
        for (path, origin) in &self.hot_files {
            if fs::read_to_string(path).is_ok_and(|value| {
                reqwest::Url::parse(value.trim()).ok() == reqwest::Url::parse(origin).ok()
            }) {
                fs::remove_file(path)?;
            }
        }
        self._invocations.clear();
        self._lock.take();
        self.stopped = true;
        self.status.running = false;
        crate::atomic(
            &self.status_file,
            &serde_json::to_vec_pretty(&self.status).unwrap(),
        )?;
        Ok(())
    }
    pub fn directory(&self) -> &std::path::Path {
        &self.root
    }
}
impl Drop for Serving {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}
pub fn url(scheme: &str, host: &str, port: u16) -> String {
    if (scheme == "http" && port == 80) || (scheme == "https" && port == 443) {
        format!("{scheme}://{host}")
    } else {
        format!("{scheme}://{host}:{port}")
    }
}
fn probe(host: &str, port: u16, tls: bool, root: Option<&[u8]>, nonce: &str) -> Result<()> {
    let mut builder = reqwest::blocking::Client::builder()
        .no_proxy()
        .connect_timeout(Duration::from_secs(2))
        .timeout(Duration::from_secs(5))
        .resolve(host, SocketAddr::from(([127, 0, 0, 1], port)));
    if let Some(root) = root {
        builder = builder.add_root_certificate(
            reqwest::Certificate::from_der(root).map_err(|_| E::AuthorityInvalid)?,
        );
    }
    let client = builder.build().map_err(|_| E::ProbeFailed)?;
    let response = client
        .get(format!(
            "{}/__devforge_health_{nonce}",
            url(if tls { "https" } else { "http" }, host, port)
        ))
        .send()
        .map_err(|_| E::ProbeFailed)?;
    if response.status() != 200 || response.text().map_err(|_| E::ProbeFailed)? != nonce {
        return Err(E::ProbeFailed);
    }
    Ok(())
}
const VITE_LAUNCHER: &str = r#"import {readFile} from 'node:fs/promises';
import {pathToFileURL} from 'node:url';
const data=JSON.parse(await readFile(process.argv[2],'utf8'));
const vite=await import(pathToFileURL(data.entry).href);
const loaded=await vite.loadConfigFromFile({command:'serve',mode:'development'},undefined,data.root);
const override={configFile:false,root:data.root,clearScreen:false,server:{host:'127.0.0.1',port:data.port,strictPort:true,https:false,origin:data.origin,allowedHosts:[data.host],cors:{origin:data.cors},hmr:{protocol:data.protocol,host:data.host,port:data.port,clientPort:data.publicPort}}};
const merged=vite.mergeConfig(loaded?.config??{},override);merged.server={...merged.server,...override.server};
const server=await vite.createServer(merged);
if(server.config.server.host!=='127.0.0.1'||server.config.server.port!==data.port||server.config.server.https)throw new Error('VITE_BIND_POLICY');
await server.listen();console.log('DEVFORGE_VITE_READY:'+data.nonce);
let closing=false;const close=async()=>{if(closing)return;closing=true;await server.close();process.exit(0);};
process.on('SIGTERM',close);process.on('SIGINT',close);process.stdin.on('data',close);
"#;
