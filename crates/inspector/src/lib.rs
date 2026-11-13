//! Opt-in, authenticated loopback Laravel telemetry. No Tauri or project execution.
pub mod logs;
pub mod mail;
pub mod profile;
mod sanitize;
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, State},
    http::{Request, StatusCode},
    middleware::{self, Next},
    response::Response,
    routing::post,
};
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    thread,
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::sync::{Semaphore, oneshot};
pub type Result<T> = std::result::Result<T, Error>;
#[derive(Debug)]
pub struct Error;
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("inspection_unavailable")
    }
}
impl std::error::Error for Error {}
impl From<std::io::Error> for Error {
    fn from(_: std::io::Error) -> Self {
        Self
    }
}
impl From<rusqlite::Error> for Error {
    fn from(_: rusqlite::Error) -> Self {
        Self
    }
}
pub fn valid_id(id: &str) -> bool {
    id.len() == 32 && id.bytes().all(|b| b.is_ascii_hexdigit())
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Request,
    Query,
    Job,
    Log,
    Dump,
    Exception,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Event {
    pub kind: Kind,
    pub context: String,
    pub name: String,
    pub duration_ms: f64,
    #[serde(default)]
    pub data: Value,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Batch {
    pub schema: u32,
    pub project_id: String,
    pub id: String,
    pub events: Vec<Event>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Record {
    pub sequence: i64,
    pub received_ms: u64,
    #[serde(flatten)]
    pub event: Event,
}
#[derive(Clone, Debug, Serialize)]
pub struct View {
    pub enabled: bool,
    pub events: Vec<Record>,
}
/// Contains bearer credentials: never Debug or included in UI/log DTOs.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Session {
    pub instance: String,
    pub schema: u32,
    pub project_id: String,
    pub endpoint: String,
    pub token: String,
}
pub fn session_path(project_store: &Path, id: &str) -> Result<PathBuf> {
    if !valid_id(id) {
        return Err(Error);
    }
    Ok(project_store.join("inspection").join(format!("{id}.json")))
}
pub fn read_session(project_store: &Path, id: &str) -> Result<Option<Session>> {
    let path = session_path(project_store, id)?;
    if let Ok(metadata) = fs::symlink_metadata(&path)
        && (!metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > 4096)
    {
        return Err(Error);
    }
    let bytes = match fs::read(&path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(Error),
    };
    let s: Session = serde_json::from_slice(&bytes).map_err(|_| Error)?;
    if bytes.len() > 4096
        || s.schema != 1
        || s.project_id != id
        || s.token.len() != 64
        || !s.token.bytes().all(|b| b.is_ascii_hexdigit())
    {
        return Err(Error);
    }
    let prefix = "http://127.0.0.1:";
    let suffix = "/v1/events";
    if !s.endpoint.starts_with(prefix)
        || !s.endpoint.ends_with(suffix)
        || s.endpoint[prefix.len()..s.endpoint.len() - suffix.len()]
            .parse::<u16>()
            .ok()
            .filter(|p| *p > 0)
            .is_none()
    {
        return Err(Error);
    }
    let root = project_store.parent().ok_or(Error)?.join("inspector");
    let lock = match std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(root.join("collector.lock"))
    {
        Ok(f) => f,
        Err(_) => return Ok(None),
    };
    match lock.try_lock() {
        Ok(()) => return Ok(None),
        Err(std::fs::TryLockError::WouldBlock) => {}
        Err(_) => return Err(Error),
    }
    if !valid_id(&s.instance)
        || fs::read(root.join("instance")).ok().as_deref() != Some(s.instance.as_bytes())
    {
        return Ok(None);
    }
    Ok(Some(s))
}
fn write_session(path: &Path, session: &Session) -> Result<()> {
    let parent = path.parent().ok_or(Error)?;
    fs::create_dir_all(parent)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(parent, fs::Permissions::from_mode(0o700))?;
    }
    use std::io::Write;
    let mut f = tempfile::NamedTempFile::new_in(parent)?;
    f.write_all(&serde_json::to_vec(session).map_err(|_| Error)?)?;
    f.as_file().sync_all()?;
    f.persist(path).map_err(|_| Error)?;
    Ok(())
}
struct Store {
    connection: Connection,
}
impl Store {
    fn open(root: &Path) -> Result<Self> {
        fs::create_dir_all(root)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(root, fs::Permissions::from_mode(0o700))?;
        }
        let connection = Connection::open(root.join("events.sqlite3"))?;
        connection.busy_timeout(std::time::Duration::from_secs(2))?;
        let version: u32 = connection.pragma_query_value(None, "user_version", |r| r.get(0))?;
        if version > 1 {
            return Err(Error);
        }
        connection.execute_batch("PRAGMA journal_mode=DELETE; PRAGMA secure_delete=ON; CREATE TABLE IF NOT EXISTS events(sequence INTEGER PRIMARY KEY AUTOINCREMENT,project TEXT NOT NULL,received INTEGER NOT NULL,body TEXT NOT NULL); CREATE INDEX IF NOT EXISTS events_project ON events(project,sequence); CREATE TABLE IF NOT EXISTS batches(project TEXT NOT NULL,id TEXT NOT NULL,PRIMARY KEY(project,id)); PRAGMA user_version=1;")?;
        Ok(Self { connection })
    }
    fn insert(&mut self, b: Batch, token: &str) -> Result<()> {
        if b.schema != 1
            || !valid_id(&b.id)
            || b.events.is_empty()
            || b.events.len() > 200
            || b.events.iter().any(|e| {
                !valid_id(&e.context)
                    || e.name.len() > 8192
                    || !e.duration_ms.is_finite()
                    || e.duration_ms < 0.0
                    || e.duration_ms > 86_400_000.0
            })
        {
            return Err(Error);
        }
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| Error)?
            .as_millis() as i64;
        let tx = self.connection.transaction()?;
        if tx.execute(
            "INSERT OR IGNORE INTO batches(project,id) VALUES(?1,?2)",
            params![b.project_id, b.id],
        )? == 0
        {
            return Ok(());
        }
        for mut event in b.events {
            event.name = if event.kind == Kind::Query {
                sanitize::sql(&event.name)
            } else {
                sanitize::text(&event.name, token)
            };
            event.data = sanitize::value(event.data, token, 0);
            let body = serde_json::to_string(&event).map_err(|_| Error)?;
            tx.execute(
                "INSERT INTO events(project,received,body) VALUES(?1,?2,?3)",
                params![b.project_id, now, body],
            )?;
        }
        tx.execute("DELETE FROM events WHERE sequence NOT IN (SELECT sequence FROM events ORDER BY sequence DESC LIMIT 4000) OR received<?1",[now.saturating_sub(86_400_000)])?;
        tx.execute("DELETE FROM events WHERE sequence IN (SELECT sequence FROM (SELECT sequence,SUM(length(body)) OVER (ORDER BY sequence DESC) AS bytes FROM events) WHERE bytes>16777216)",[])?;
        tx.execute("DELETE FROM batches WHERE rowid NOT IN (SELECT rowid FROM batches ORDER BY rowid DESC LIMIT 4000)",[])?;
        tx.commit()?;
        Ok(())
    }
    fn list(&self, id: &str) -> Result<Vec<Record>> {
        self.context(id, None)
    }
    fn context(&self, id: &str, context: Option<&str>) -> Result<Vec<Record>> {
        let mut s=self.connection.prepare("SELECT sequence,received,body FROM events WHERE project=?1 AND received>=?2 AND (?3 IS NULL OR json_extract(body,'$.context')=?3) ORDER BY sequence DESC LIMIT 200")?;
        let cutoff = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| Error)?
            .as_millis() as i64
            - 86_400_000;
        let rows = s.query_map(params![id, cutoff, context], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, String>(2)?,
            ))
        })?;
        let mut out = vec![];
        let mut bytes = 0;
        for r in rows {
            let (sequence, received_ms, body) = r?;
            bytes += body.len();
            if bytes > 256 * 1024 {
                break;
            }
            out.push(Record {
                sequence,
                received_ms: received_ms as u64,
                event: serde_json::from_str(&body).map_err(|_| Error)?,
            });
        }
        Ok(out)
    }
}
#[derive(Clone)]
struct ServerState {
    store: Arc<Mutex<Store>>,
    tokens: Arc<Mutex<BTreeMap<String, String>>>,
    slots: Arc<Semaphore>,
}
async fn gate(
    State(state): State<ServerState>,
    request: Request<axum::body::Body>,
    next: Next,
) -> Response {
    let Ok(_permit) = state.slots.clone().try_acquire_owned() else {
        return StatusCode::TOO_MANY_REQUESTS.into_response();
    };
    if request.headers().contains_key("origin") {
        return StatusCode::FORBIDDEN.into_response();
    }
    let token = request
        .headers()
        .get("x-devforge-token")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("");
    let authorized = state
        .tokens
        .lock()
        .is_ok_and(|tokens| tokens.values().any(|v| equal(v, token)));
    if !authorized {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    next.run(request).await
}
use axum::response::IntoResponse;
async fn ingest(
    State(state): State<ServerState>,
    headers: axum::http::HeaderMap,
    Json(batch): Json<Batch>,
) -> StatusCode {
    let token = headers
        .get("x-devforge-token")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("");
    let allowed = state
        .tokens
        .lock()
        .ok()
        .and_then(|t| t.get(&batch.project_id).map(|v| equal(v, token)))
        .unwrap_or(false);
    if !allowed {
        return StatusCode::UNAUTHORIZED;
    }
    match state
        .store
        .lock()
        .map_err(|_| Error)
        .and_then(|mut s| s.insert(batch, token))
    {
        Ok(()) => StatusCode::ACCEPTED,
        Err(_) => StatusCode::UNPROCESSABLE_ENTITY,
    }
}
fn equal(a: &str, b: &str) -> bool {
    a.len() == 64
        && b.len() == 64
        && a.bytes().zip(b.bytes()).fold(0u8, |n, (x, y)| n | (x ^ y)) == 0
}
/// Owns only its loopback listener. Dropping it closes the listener and worker.
pub struct Collector {
    instance: String,
    _lease: std::fs::File,
    state: ServerState,
    endpoint: String,
    stop: Option<oneshot::Sender<()>>,
    worker: Option<thread::JoinHandle<()>>,
    project_store: PathBuf,
    paths: Vec<PathBuf>,
}
impl Collector {
    pub fn open(root: &Path, project_store: &Path) -> Result<Self> {
        fs::create_dir_all(root)?;
        let lease = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(root.join("collector.lock"))?;
        lease.try_lock().map_err(|_| Error)?;
        let instance = uuid::Uuid::new_v4().simple().to_string();
        use std::io::Write;
        let mut receipt = tempfile::NamedTempFile::new_in(root)?;
        receipt.write_all(instance.as_bytes())?;
        receipt.persist(root.join("instance")).map_err(|_| Error)?;
        let store = Arc::new(Mutex::new(Store::open(root)?));
        let listener = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))?;
        listener.set_nonblocking(true)?;
        let port = listener.local_addr()?.port();
        let state = ServerState {
            store,
            tokens: Arc::new(Mutex::new(BTreeMap::new())),
            slots: Arc::new(Semaphore::new(8)),
        };
        let (stop, rx) = oneshot::channel();
        let server_state = state.clone();
        let worker = thread::spawn(move || {
            let Ok(rt) = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            else {
                return;
            };
            rt.block_on(async move {
                let Ok(listener) = tokio::net::TcpListener::from_std(listener) else {
                    return;
                };
                let router = Router::new()
                    .route("/v1/events", post(ingest))
                    .layer(DefaultBodyLimit::max(128 * 1024))
                    .layer(middleware::from_fn_with_state(server_state.clone(), gate))
                    .with_state(server_state);
                use std::future::IntoFuture;
                tokio::select! {_=axum::serve(listener,router).into_future()=>{},_=rx=>{}}
            });
        });
        Ok(Self {
            instance,
            _lease: lease,
            state,
            endpoint: format!("http://127.0.0.1:{port}/v1/events"),
            stop: Some(stop),
            worker: Some(worker),
            project_store: project_store.into(),
            paths: vec![],
        })
    }
    pub fn enable(&mut self, id: &str) -> Result<()> {
        let path = session_path(&self.project_store, id)?;
        let token = format!(
            "{}{}",
            uuid::Uuid::new_v4().simple(),
            uuid::Uuid::new_v4().simple()
        );
        write_session(
            &path,
            &Session {
                instance: self.instance.clone(),
                schema: 1,
                project_id: id.into(),
                endpoint: self.endpoint.clone(),
                token: token.clone(),
            },
        )?;
        self.state
            .tokens
            .lock()
            .map_err(|_| Error)?
            .insert(id.into(), token);
        self.paths.push(path);
        Ok(())
    }
    pub fn disable(&mut self, id: &str) -> Result<()> {
        self.state.tokens.lock().map_err(|_| Error)?.remove(id);
        let path = session_path(&self.project_store, id)?;
        match fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(_) => Err(Error),
        }
    }
    pub fn view(&self, id: &str) -> Result<View> {
        if !valid_id(id) {
            return Err(Error);
        }
        Ok(View {
            enabled: self
                .state
                .tokens
                .lock()
                .map_err(|_| Error)?
                .contains_key(id),
            events: self.state.store.lock().map_err(|_| Error)?.list(id)?,
        })
    }
    pub fn clear(&self, id: &str) -> Result<()> {
        if !valid_id(id) {
            return Err(Error);
        }
        self.state
            .store
            .lock()
            .map_err(|_| Error)?
            .connection
            .execute("DELETE FROM events WHERE project=?1", [id])?;
        Ok(())
    }
}
impl Drop for Collector {
    fn drop(&mut self) {
        for path in &self.paths {
            let _ = fs::remove_file(path);
        }
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

/// Standalone read path never creates a listener or rotates another owner's token.
pub fn read(root: &Path, id: &str) -> Result<View> {
    if !valid_id(id) {
        return Err(Error);
    }
    if !root.join("events.sqlite3").exists() {
        return Ok(View {
            enabled: false,
            events: vec![],
        });
    }
    let connection = Connection::open_with_flags(
        root.join("events.sqlite3"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )?;
    Ok(View {
        enabled: false,
        events: Store { connection }.list(id)?,
    })
}
pub fn read_context(root: &Path, id: &str, context: &str) -> Result<Vec<Record>> {
    if !valid_id(id) || !valid_id(context) {
        return Err(Error);
    }
    if !root.join("events.sqlite3").exists() {
        return Ok(vec![]);
    }
    let connection = Connection::open_with_flags(
        root.join("events.sqlite3"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )?;
    Store { connection }.context(id, Some(context))
}
pub fn materialize(project: &Path) -> Result<PathBuf> {
    let parent = project.join(".devforge");
    if let Ok(m) = fs::symlink_metadata(&parent) {
        if !m.is_dir() || m.file_type().is_symlink() {
            return Err(Error);
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if m.file_attributes() & 0x400 != 0 {
                return Err(Error);
            }
        }
    }
    fs::create_dir_all(&parent)?;
    let directory = parent.join("laravel-inspector");
    let files = [
        (
            "composer.json",
            include_str!("../../../packages/laravel-inspector/composer.json"),
        ),
        (
            "src/Capture.php",
            include_str!("../../../packages/laravel-inspector/src/Capture.php"),
        ),
        (
            "src/RequestCapture.php",
            include_str!("../../../packages/laravel-inspector/src/RequestCapture.php"),
        ),
        (
            "src/InspectorServiceProvider.php",
            include_str!("../../../packages/laravel-inspector/src/InspectorServiceProvider.php"),
        ),
    ];
    if directory.exists() {
        let m = fs::symlink_metadata(&directory)?;
        if m.file_type().is_symlink() || !m.is_dir() {
            return Err(Error);
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if m.file_attributes() & 0x400 != 0 {
                return Err(Error);
            }
        }
        if fs::read(directory.join(".devforge-owned")).ok().as_deref()
            != Some(b"laravel-inspector-v1")
        {
            return Err(Error);
        }
        let known: BTreeMap<String, String> = serde_json::from_str(include_str!(
            "../../../packages/laravel-inspector/legacy-hashes.json"
        ))
        .map_err(|_| Error)?;
        let mut upgrade = false;
        let src = fs::symlink_metadata(directory.join("src"))?;
        if !src.is_dir() || src.file_type().is_symlink() {
            return Err(Error);
        }
        for entry in fs::read_dir(&directory)? {
            let name = entry?.file_name();
            if !["src", "composer.json", ".devforge-owned"]
                .iter()
                .any(|s| name == *s)
            {
                return Err(Error);
            }
        }
        for entry in fs::read_dir(directory.join("src"))? {
            let name = entry?.file_name();
            if ![
                "Capture.php",
                "RequestCapture.php",
                "InspectorServiceProvider.php",
            ]
            .iter()
            .any(|s| name == *s)
            {
                return Err(Error);
            }
        }
        for (name, body) in files {
            let p = directory.join(name);
            let m = fs::symlink_metadata(&p)?;
            if !m.is_file() || m.file_type().is_symlink() {
                return Err(Error);
            }
            let bytes = fs::read(p)?;
            if bytes != body.as_bytes() {
                use sha2::{Digest, Sha256};
                let hash = Sha256::digest(&bytes)
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect::<String>();
                if known.get(name) != Some(&hash) {
                    return Err(Error);
                }
                upgrade = true;
            }
        }
        if upgrade {
            let stage = tempfile::tempdir_in(&parent)?;
            for (name, body) in files {
                let p = stage.path().join(name);
                fs::create_dir_all(p.parent().ok_or(Error)?)?;
                fs::write(p, body)?;
            }
            fs::write(
                stage.path().join(".devforge-owned"),
                b"laravel-inspector-v1",
            )?;
            let backup = parent.join(format!(
                "laravel-inspector-backup-{}",
                uuid::Uuid::new_v4().simple()
            ));
            fs::rename(&directory, &backup)?;
            if fs::rename(stage.path(), &directory).is_err() {
                let _ = fs::rename(&backup, &directory);
                return Err(Error);
            }
        }
    } else {
        let stage = tempfile::tempdir_in(&parent)?;
        for (name, body) in files {
            let p = stage.path().join(name);
            fs::create_dir_all(p.parent().ok_or(Error)?)?;
            fs::write(p, body)?;
        }
        fs::write(
            stage.path().join(".devforge-owned"),
            b"laravel-inspector-v1",
        )?;
        fs::rename(stage.path(), &directory)?;
    }
    Ok(directory)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn batch(id: &str, context: &str) -> Batch {
        Batch {
            schema: 1,
            project_id: id.into(),
            id: uuid::Uuid::new_v4().simple().to_string(),
            events: vec![Event {
                kind: Kind::Query,
                context: context.into(),
                name: "select * from users where password='private-value' and id=42".into(),
                duration_ms: 120.0,
                data: serde_json::json!({"bindings":["private-binding"],"password":"private-password","message":"token=secret-value"}),
            }],
        }
    }
    #[test]
    fn native_transport_auth_origin_scope_replay_retention_and_shutdown() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("inspector");
        let projects = temp.path().join("projects");
        let a = "a".repeat(32);
        let b = "b".repeat(32);
        let context = "c".repeat(32);
        let mut collector = Collector::open(&root, &projects).unwrap();
        collector.enable(&a).unwrap();
        collector.enable(&b).unwrap();
        let session = read_session(&projects, &a).unwrap().unwrap();
        let client = reqwest::blocking::Client::builder()
            .no_proxy()
            .timeout(std::time::Duration::from_secs(4))
            .build()
            .unwrap();
        let send = |body: &Batch, token: &str| {
            client
                .post(&session.endpoint)
                .header("x-devforge-token", token)
                .json(body)
                .send()
                .unwrap()
                .status()
                .as_u16()
        };
        let body = batch(&a, &context);
        assert_eq!(send(&body, &"0".repeat(64)), 401);
        assert_eq!(send(&batch(&b, &context), &session.token), 401);
        assert_eq!(
            client
                .post(&session.endpoint)
                .header("x-devforge-token", &session.token)
                .header("Origin", "http://foreign.test")
                .json(&body)
                .send()
                .unwrap()
                .status()
                .as_u16(),
            403
        );
        assert_eq!(send(&body, &session.token), 202);
        assert_eq!(send(&body, &session.token), 202);
        let rows = collector.view(&a).unwrap().events;
        assert_eq!(rows.len(), 1);
        let stored = serde_json::to_string(&rows).unwrap();
        assert!(!stored.contains("private-"));
        assert!(!stored.contains("secret-value"));
        assert!(collector.view(&b).unwrap().events.is_empty());
        let malformed =
            serde_json::json!({"schema":1,"project_id":a,"id":context,"events":[],"unknown":true});
        assert_eq!(
            client
                .post(&session.endpoint)
                .header("x-devforge-token", &session.token)
                .json(&malformed)
                .send()
                .unwrap()
                .status()
                .as_u16(),
            422
        );
        assert_eq!(
            client
                .post(&session.endpoint)
                .header("x-devforge-token", &session.token)
                .header("Content-Type", "application/json")
                .body("x".repeat(130 * 1024))
                .send()
                .unwrap()
                .status()
                .as_u16(),
            413
        );
        for _ in 0..3 {
            let mut many = batch(&a, &context);
            many.events = vec![many.events[0].clone(); 100];
            assert_eq!(send(&many, &session.token), 202);
        }
        assert!(collector.view(&a).unwrap().events.len() <= 200);
        collector.clear(&a).unwrap();
        assert!(collector.view(&a).unwrap().events.is_empty());
        collector.disable(&a).unwrap();
        assert_eq!(send(&body, &session.token), 401);
        assert!(read_session(&projects, &a).unwrap().is_none());
        let endpoint = session.endpoint.clone();
        drop(collector);
        assert!(client.post(endpoint).send().is_err());
    }
    #[test]
    fn restart_does_not_reuse_stale_capture_credentials_or_reset_history() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("inspector");
        let projects = temp.path().join("projects");
        let id = "a".repeat(32);
        let mut first = Collector::open(&root, &projects).unwrap();
        first.enable(&id).unwrap();
        let old = read_session(&projects, &id).unwrap().unwrap();
        first
            .state
            .store
            .lock()
            .unwrap()
            .insert(batch(&id, &"c".repeat(32)), &old.token)
            .unwrap();
        drop(first);
        // Simulate a leftover receipt after an abnormal exit.
        write_session(&session_path(&projects, &id).unwrap(), &old).unwrap();
        assert!(read_session(&projects, &id).unwrap().is_none());
        let second = Collector::open(&root, &projects).unwrap();
        assert!(read_session(&projects, &id).unwrap().is_none());
        assert_eq!(second.view(&id).unwrap().events.len(), 1);
        drop(second);
        let db = Connection::open(root.join("events.sqlite3")).unwrap();
        db.pragma_update(None, "user_version", 99).unwrap();
        drop(db);
        assert!(Collector::open(&root, &projects).is_err());
    }
    #[test]
    fn integration_copy_is_repeatable_and_preserves_conflicting_user_files() {
        let root = tempfile::tempdir().unwrap();
        let target = materialize(root.path()).unwrap();
        assert_eq!(materialize(root.path()).unwrap(), target);
        fs::write(target.join("src/Capture.php"), "user change").unwrap();
        assert!(materialize(root.path()).is_err());
        assert_eq!(
            fs::read_to_string(target.join("src/Capture.php")).unwrap(),
            "user change"
        );
        #[cfg(unix)]
        {
            let other = tempfile::tempdir().unwrap();
            let fresh = tempfile::tempdir().unwrap();
            std::os::unix::fs::symlink(other.path(), fresh.path().join(".devforge")).unwrap();
            assert!(materialize(fresh.path()).is_err());
            assert_eq!(fs::read_dir(other.path()).unwrap().count(), 0);
        }
    }
}

#[cfg(test)]
mod upgrade_tests {
    use super::*;
    #[test]
    fn recognized_legacy_bundle_is_backed_up_but_user_edits_are_not_overwritten() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join(".devforge/laravel-inspector");
        fs::create_dir_all(root.join("src")).unwrap();
        for (name, body) in [
            (
                "composer.json",
                include_str!("../tests/fixtures/legacy/composer.json"),
            ),
            (
                "src/Capture.php",
                include_str!("../tests/fixtures/legacy/src/Capture.php"),
            ),
            (
                "src/RequestCapture.php",
                include_str!("../tests/fixtures/legacy/src/RequestCapture.php"),
            ),
            (
                "src/InspectorServiceProvider.php",
                include_str!("../tests/fixtures/legacy/src/InspectorServiceProvider.php"),
            ),
        ] {
            fs::write(root.join(name), body).unwrap();
        }
        fs::write(root.join(".devforge-owned"), b"laravel-inspector-v1").unwrap();
        materialize(temp.path()).unwrap();
        assert!(
            fs::read_to_string(root.join("src/Capture.php"))
                .unwrap()
                .contains("public function log")
        );
        assert!(
            fs::read_dir(temp.path().join(".devforge"))
                .unwrap()
                .flatten()
                .any(|e| e
                    .file_name()
                    .to_string_lossy()
                    .starts_with("laravel-inspector-backup-"))
        );
        fs::write(root.join("src/Capture.php"), "USER EDIT").unwrap();
        assert!(materialize(temp.path()).is_err());
        assert_eq!(
            fs::read(root.join("src/Capture.php")).unwrap(),
            b"USER EDIT"
        );
    }
}
