//! App-owned launch receipt and cancellable local HTTP trigger. Tokens never enter snapshots.
use crate::{
    Breakpoint, Error, Mode, Result,
    server::{Server, Snapshot},
};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File},
    io::{Read, Write},
    net::{Ipv4Addr, TcpStream},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Launch {
    pub schema: u32,
    pub project: String,
    pub instance: String,
    pub runtime: String,
    pub mode: Mode,
    pub key: String,
    pub port: Option<u16>,
    pub output: PathBuf,
}
#[derive(Debug, Clone, Serialize)]
pub struct Request {
    pub state: String,
    pub status: Option<u16>,
    pub error: Option<Error>,
}
pub struct Activation {
    launch: Launch,
    root: PathBuf,
    _lease: File,
    server: Option<Server>,
    request: Arc<Mutex<Request>>,
    cancel: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}
fn ordinary(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|m| m.is_dir() && !m.file_type().is_symlink())
}
fn root(path: &Path, id: &str) -> Result<PathBuf> {
    if id.len() != 32 || !id.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(Error::Unauthorized);
    }
    fs::create_dir_all(path)?;
    let path = fs::canonicalize(path)?;
    let marker = path.join(".devforge-debug");
    if !marker.exists() {
        if fs::read_dir(&path)?.next().is_some() {
            return Err(Error::Unauthorized);
        }
        fs::write(&marker, id)?;
    }
    if fs::symlink_metadata(&marker)?.file_type().is_symlink() || fs::read_to_string(&marker)? != id
    {
        return Err(Error::Unauthorized);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700))?;
    }
    Ok(path)
}
pub fn read(path: &Path, id: &str) -> Result<Option<Launch>> {
    if !path.exists() {
        return Ok(None);
    }
    let path = root(path, id)?;
    let lock = path.join("owner.lock");
    if fs::symlink_metadata(&lock).is_ok_and(|m| !m.is_file() || m.file_type().is_symlink()) {
        return Err(Error::Unauthorized);
    }
    let lease = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(lock)?;
    if lease.try_lock().is_ok() {
        return Ok(None);
    }
    let file = path.join("launch.json");
    let metadata = fs::symlink_metadata(&file)?;
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > 8192 {
        return Err(Error::Unauthorized);
    }
    let value: Launch = serde_json::from_slice(&fs::read(file)?).map_err(|_| Error::Protocol)?;
    if value.schema != 1
        || value.project != id
        || value.key.len() != 64
        || !value.key.bytes().all(|b| b.is_ascii_hexdigit())
        || value.output != path.join("profiles")
        || !ordinary(&value.output)
        || value.port == Some(0)
    {
        return Err(Error::Unauthorized);
    }
    Ok(Some(value))
}
impl Activation {
    pub fn start(
        path: &Path,
        project: &str,
        runtime: String,
        mode: Mode,
        source: &Path,
        bps: Vec<Breakpoint>,
        mut secrets: Vec<String>,
    ) -> Result<Self> {
        if mode == Mode::Off {
            return Err(Error::Unavailable);
        }
        let path = root(path, project)?;
        let lock = path.join("owner.lock");
        if fs::symlink_metadata(&lock).is_ok_and(|m| m.file_type().is_symlink()) {
            return Err(Error::Unauthorized);
        }
        let lease = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(lock)?;
        lease.try_lock().map_err(|_| Error::Busy)?;
        let output = path.join("profiles");
        if !output.exists() {
            fs::create_dir(&output)?;
        }
        if !ordinary(&output) || fs::canonicalize(&output)? != output {
            return Err(Error::Unauthorized);
        }
        crate::store::trim(&output)?;
        let key = format!(
            "{}{}",
            uuid::Uuid::new_v4().simple(),
            uuid::Uuid::new_v4().simple()
        );
        secrets.push(key.clone());
        let server = if mode == Mode::Debug {
            Some(Server::start(source, key.clone(), bps, secrets)?)
        } else {
            None
        };
        let launch = Launch {
            schema: 1,
            project: project.into(),
            instance: uuid::Uuid::new_v4().simple().to_string(),
            runtime,
            mode,
            key,
            port: server.as_ref().map(|s| s.snapshot().port),
            output,
        };
        let file = path.join("launch.json");
        if fs::symlink_metadata(&file).is_ok_and(|m| m.file_type().is_symlink()) {
            return Err(Error::Unauthorized);
        }
        let mut temp = tempfile::NamedTempFile::new_in(&path)?;
        temp.write_all(&serde_json::to_vec(&launch).unwrap())?;
        temp.as_file().sync_all()?;
        temp.persist(file).map_err(|_| Error::Unavailable)?;
        Ok(Self {
            launch,
            root: path,
            _lease: lease,
            server,
            request: Arc::new(Mutex::new(Request {
                state: "idle".into(),
                status: None,
                error: None,
            })),
            cancel: Arc::new(AtomicBool::new(false)),
            worker: None,
        })
    }
    pub fn launch(&self) -> &Launch {
        &self.launch
    }
    pub fn snapshot(&self) -> Option<Snapshot> {
        self.server.as_ref().map(Server::snapshot)
    }
    pub fn request(&self) -> Request {
        self.request.lock().unwrap().clone()
    }
    pub fn control(&self, command: crate::server::Control) -> Result<()> {
        self.server
            .as_ref()
            .ok_or(Error::Unavailable)?
            .control(command)
    }
    pub fn get(&mut self, base: &str, path: &str) -> Result<()> {
        if self.request.lock().unwrap().state == "running" {
            return Err(Error::Busy);
        }
        if !path.starts_with('/')
            || path.starts_with("//")
            || path.contains(['\\', '\0', '\r', '\n'])
            || path.len() > 4096
        {
            return Err(Error::Unauthorized);
        }
        let base = url::Url::parse(base).map_err(|_| Error::Unauthorized)?;
        let url = base.join(path).map_err(|_| Error::Unauthorized)?;
        let hostname = base.host_str().ok_or(Error::Unauthorized)?.to_string();
        if base.scheme() != "http"
            || url.host_str() != base.host_str()
            || url.port_or_known_default() != base.port_or_known_default()
            || !(hostname.ends_with(".localhost") || hostname == "127.0.0.1")
        {
            return Err(Error::Unauthorized);
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        let port = base.port_or_known_default().ok_or(Error::Unauthorized)?;
        let target = format!(
            "{}{}",
            url.path(),
            url.query().map(|s| format!("?{s}")).unwrap_or_default()
        );
        let key = self.launch.key.clone();
        let cancel = self.cancel.clone();
        let state = self.request.clone();
        *state.lock().unwrap() = Request {
            state: "running".into(),
            status: None,
            error: None,
        };
        self.worker = Some(thread::spawn(move || {
            let key = zeroize::Zeroizing::new(key);
            let result = (|| -> Result<u16> {
                let mut stream = TcpStream::connect_timeout(
                    &(Ipv4Addr::LOCALHOST, port).into(),
                    Duration::from_secs(2),
                )?;
                stream.set_read_timeout(Some(Duration::from_millis(100)))?;
                stream.set_write_timeout(Some(Duration::from_secs(2)))?;
                stream.write_all(format!("GET {target} HTTP/1.1\r\nHost: {hostname}:{port}\r\nCookie: XDEBUG_TRIGGER={}\r\nConnection: close\r\n\r\n",key.as_str()).as_bytes())?;
                let until = Instant::now() + Duration::from_secs(1800);
                let mut bytes = Vec::new();
                let mut buffer = [0u8; 1024];
                while !cancel.load(Ordering::Relaxed) && Instant::now() < until {
                    match stream.read(&mut buffer) {
                        Ok(0) => return Err(Error::Unavailable),
                        Ok(n) => {
                            bytes.extend_from_slice(&buffer[..n]);
                            if bytes.len() > 8192 {
                                return Err(Error::Protocol);
                            }
                            if bytes.windows(2).any(|b| b == b"\r\n") {
                                let line = std::str::from_utf8(&bytes)
                                    .map_err(|_| Error::Protocol)?
                                    .lines()
                                    .next()
                                    .ok_or(Error::Protocol)?;
                                return line
                                    .split_whitespace()
                                    .nth(1)
                                    .ok_or(Error::Protocol)?
                                    .parse()
                                    .map_err(|_| Error::Protocol);
                            }
                        }
                        Err(e)
                            if matches!(
                                e.kind(),
                                std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                            ) => {}
                        Err(_) => return Err(Error::Unavailable),
                    }
                }
                Err(Error::Cancelled)
            })();
            let mut s = state.lock().unwrap();
            match result {
                Ok(status) => {
                    s.state = "completed".into();
                    s.status = Some(status);
                }
                Err(e) => {
                    s.state = "failed".into();
                    s.error = Some(e);
                }
            }
        }));
        Ok(())
    }
}
impl Drop for Activation {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
        self.server.take();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        let path = self.root.join("launch.json");
        let ours = fs::read(&path)
            .ok()
            .and_then(|b| serde_json::from_slice::<Launch>(&b).ok())
            .is_some_and(|v| v.instance == self.launch.instance);
        if ours {
            let _ = fs::remove_file(path);
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn launch_is_owned_stale_and_http_trigger_is_project_local() {
        let temp = tempfile::tempdir().unwrap();
        let id = "a".repeat(32);
        let p = temp.path().join("debug");
        assert!(read(&p, &id).unwrap().is_none());
        let mut live = Activation::start(
            &p,
            &id,
            "test".into(),
            Mode::Profile,
            temp.path(),
            vec![],
            vec![],
        )
        .unwrap();
        assert!(read(&p, &id).unwrap().is_some());
        assert!(
            Activation::start(
                &p,
                &id,
                "test".into(),
                Mode::Profile,
                temp.path(),
                vec![],
                vec![]
            )
            .is_err()
        );
        assert!(live.get("http://example.test:80", "/").is_err());
        assert!(
            live.get("http://owned.localhost:8080", "//outside.test")
                .is_err()
        );
        drop(live);
        assert!(read(&p, &id).unwrap().is_none());
    }
}
