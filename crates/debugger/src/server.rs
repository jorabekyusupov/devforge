use crate::{
    Breakpoint, Error, Result,
    protocol::{self, Element, Frames},
};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde::Serialize;
use std::{
    collections::BTreeMap,
    io::Write,
    net::{Shutdown, TcpListener, TcpStream},
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
#[derive(Debug, Clone, Serialize)]
pub struct Frame {
    pub depth: u32,
    pub function: String,
    pub file: Option<String>,
    pub line: u32,
}
#[derive(Debug, Clone, Serialize)]
pub struct Variable {
    pub assign: Option<String>,
    pub name: String,
    pub kind: String,
    pub value: String,
    pub children: Vec<Variable>,
}
#[derive(Debug, Clone, Serialize)]
pub struct Snapshot {
    pub state: String,
    pub port: u16,
    pub frames: Vec<Frame>,
    pub variables: Vec<Variable>,
    pub depth: u32,
    pub error: Option<Error>,
}
#[derive(Clone)]
pub enum Control {
    Run,
    StepInto,
    StepOver,
    StepOut,
    Stop,
    Detach,
    Frame(u32),
    Breakpoints(Vec<Breakpoint>),
    Set(crate::Assignment),
}
pub struct Server {
    state: Arc<Mutex<Snapshot>>,
    tx: mpsc::SyncSender<Control>,
    stop: Arc<AtomicBool>,
    socket: Arc<Mutex<Option<TcpStream>>>,
    worker: Option<JoinHandle<()>>,
}
fn scrub(text: &str, secrets: &[String]) -> String {
    let mut value = text.to_owned();
    for secret in secrets.iter().filter(|s| !s.is_empty()) {
        value = value.replace(secret, "[REDACTED]");
        // DBGp can truncate a long value at max_data. Mask a known secret's visible prefix too.
        if secret.len() > 32 {
            let prefix = secret.chars().take(32).collect::<String>();
            if let Some(start) = value.find(&prefix)
                && secret.starts_with(&value[start..])
            {
                value.replace_range(start.., "[REDACTED]");
            }
        }
    }
    value.chars().take(4096).collect()
}
fn sensitive(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    [
        "password",
        "passwd",
        "secret",
        "token",
        "authorization",
        "cookie",
        "credential",
        "app_key",
        "xdebug_trigger",
        "idekey",
    ]
    .iter()
    .any(|key| name.contains(key))
}
fn variables(e: &Element, secrets: &[String], depth: usize) -> Vec<Variable> {
    if depth > 2 {
        return vec![];
    }
    e.children
        .iter()
        .filter(|c| c.name == "property")
        .take(32)
        .map(|p| {
            let name = scrub(p.attr("name"), secrets);
            let masked = sensitive(&name);
            let text = if p.attr("encoding") == "base64" {
                STANDARD
                    .decode(p.text.trim())
                    .ok()
                    .and_then(|b| String::from_utf8(b).ok())
                    .unwrap_or_default()
            } else {
                p.text.clone()
            };
            let expression = format!("${}", name.trim_start_matches('$'));
            let assign = crate::Assignment {
                name: expression.clone(),
                kind: p.attr("type").into(),
                value: match p.attr("type") {
                    "int" => "0",
                    "float" => "0",
                    "bool" => "0",
                    _ => "",
                }
                .into(),
            };
            Variable {
                assign: if depth == 0
                    && !masked
                    && text.chars().count() <= 512
                    && !secrets.iter().any(|s| !s.is_empty() && text.contains(s))
                    && assign.validate().is_ok()
                {
                    Some(expression)
                } else {
                    None
                },
                name,
                kind: p.attr("type").chars().take(40).collect(),
                value: if masked {
                    "[REDACTED]".into()
                } else {
                    scrub(&text, secrets).chars().take(512).collect()
                },
                children: if masked {
                    vec![]
                } else {
                    variables(p, secrets, depth + 1)
                },
            }
        })
        .collect()
}
fn relative(root: &Path, uri: &str) -> Option<String> {
    let path = url::Url::parse(uri).ok()?.to_file_path().ok()?;
    let path = std::fs::canonicalize(path).ok()?;
    path.strip_prefix(root)
        .ok()?
        .to_str()
        .map(|s| s.replace('\\', "/"))
}
struct Connection {
    stream: TcpStream,
    frames: Frames,
    transaction: u32,
}
impl Connection {
    fn send(&mut self, command: &str) -> Result<u32> {
        self.transaction = self.transaction.checked_add(1).ok_or(Error::Protocol)?;
        let i = self.transaction;
        let wire = if let Some((prefix, data)) = command.split_once(" -- ") {
            format!("{prefix} -i {i} -- {data}\0")
        } else {
            format!("{command} -i {i}\0")
        };
        self.stream.write_all(wire.as_bytes())?;
        Ok(i)
    }
    fn receive(&mut self, stop: &AtomicBool, timeout: Duration) -> Result<Element> {
        let until = Instant::now() + timeout;
        loop {
            if stop.load(Ordering::Relaxed) {
                return Err(Error::Cancelled);
            }
            if Instant::now() > until {
                return Err(Error::Protocol);
            }
            if let Some(bytes) = self.frames.receive(&mut self.stream)? {
                return protocol::xml(&bytes);
            }
        }
    }
    fn call(&mut self, command: &str, stop: &AtomicBool) -> Result<Element> {
        let id = self.send(command)?;
        loop {
            let e = self.receive(stop, Duration::from_secs(5))?;
            if e.name == "response" {
                if e.attr("transaction_id") != id.to_string() || e.child("error").is_some() {
                    eprintln!(
                        "DBGP command {} failed with code {}",
                        command.split_whitespace().next().unwrap_or("unknown"),
                        e.child("error")
                            .map(|v| v.attr("code"))
                            .unwrap_or("transaction")
                    );
                    return Err(Error::Protocol);
                }
                return Ok(e);
            }
        }
    }
}
fn breakpoints(
    conn: &mut Connection,
    root: &Path,
    values: &[Breakpoint],
    ids: &mut BTreeMap<String, String>,
    stop: &AtomicBool,
) -> Result<()> {
    for id in ids.values() {
        conn.call(&format!("breakpoint_remove -d {id}"), stop)?;
    }
    ids.clear();
    for b in values {
        if b.line == 0 || b.line > 1_000_000 {
            return Err(Error::InvalidBreakpoint);
        }
        let path = crate::source(root, &b.file)?;
        let uri = url::Url::from_file_path(&path).map_err(|_| Error::SourceOutside)?;
        let e = conn.call(
            &format!("breakpoint_set -t line -f {uri} -n {}", b.line),
            stop,
        )?;
        let id = e.attr("id");
        if id.is_empty() || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'.') {
            return Err(Error::Protocol);
        }
        ids.insert(format!("{}:{}", b.file, b.line), id.into());
    }
    Ok(())
}
fn refresh(
    conn: &mut Connection,
    root: &Path,
    secrets: &[String],
    state: &Arc<Mutex<Snapshot>>,
    depth: u32,
    stop: &AtomicBool,
) -> Result<()> {
    let stack = conn.call("stack_get", stop)?;
    let mut frames = vec![];
    for e in stack.children.iter().filter(|e| e.name == "stack").take(64) {
        frames.push(Frame {
            depth: e.attr("level").parse().map_err(|_| Error::Protocol)?,
            function: scrub(e.attr("where"), secrets),
            file: relative(root, e.attr("filename")),
            line: e.attr("lineno").parse().unwrap_or(0),
        });
    }
    let selected = if frames.iter().any(|f| f.depth == depth) {
        depth
    } else {
        0
    };
    let values = if frames.is_empty() {
        vec![]
    } else {
        let context = conn.call(&format!("context_get -d {selected} -c 0"), stop)?;
        variables(&context, secrets, 0)
    };
    let mut s = state.lock().unwrap();
    s.frames = frames;
    s.variables = values;
    s.depth = selected;
    s.state = "paused".into();
    s.error = None;
    Ok(())
}
struct Peer<'a> {
    root: &'a Path,
    key: &'a str,
    secrets: &'a [String],
}
fn conversation(
    stream: TcpStream,
    peer: Peer<'_>,
    bps: &mut Vec<Breakpoint>,
    rx: &mpsc::Receiver<Control>,
    state: &Arc<Mutex<Snapshot>>,
    stop: &AtomicBool,
) -> Result<()> {
    let Peer { root, key, secrets } = peer;
    stream.set_read_timeout(Some(Duration::from_millis(100)))?;
    stream.set_write_timeout(Some(Duration::from_secs(2)))?;
    let mut conn = Connection {
        stream,
        frames: Frames::default(),
        transaction: 0,
    };
    let init = conn.receive(stop, Duration::from_secs(3))?;
    if init.name != "init"
        || init.attr("language") != "PHP"
        || !protocol::constant_time(init.attr("idekey"), key)
    {
        return Err(Error::Unauthorized);
    }
    for (name, value) in [
        ("max_children", 32),
        ("max_data", 512),
        ("max_depth", 2),
        ("show_hidden", 0),
    ] {
        conn.call(&format!("feature_set -n {name} -v {value}"), stop)?;
    }
    let mut ids = BTreeMap::new();
    breakpoints(&mut conn, root, bps, &mut ids, stop)?;
    refresh(&mut conn, root, secrets, state, 0, stop)?;
    let mut running = None;
    while !stop.load(Ordering::Relaxed) {
        if let Ok(command) = rx.try_recv() {
            if running.is_some() {
                if matches!(command, Control::Stop | Control::Detach) {
                    let _ = conn.stream.shutdown(Shutdown::Both);
                    return Ok(());
                }
                continue;
            }
            match command {
                Control::Run | Control::StepInto | Control::StepOver | Control::StepOut => {
                    let command = match command {
                        Control::Run => "run",
                        Control::StepInto => "step_into",
                        Control::StepOver => "step_over",
                        _ => "step_out",
                    };
                    running = Some(conn.send(command)?);
                    let mut s = state.lock().unwrap();
                    s.state = "running".into();
                    s.variables.clear();
                }
                Control::Stop => {
                    let _ = conn.call("stop", stop);
                    return Ok(());
                }
                Control::Detach => {
                    let _ = conn.call("detach", stop);
                    return Ok(());
                }
                Control::Set(value) => {
                    value.validate()?;
                    let depth = state.lock().unwrap().depth;
                    let encoded = STANDARD.encode(value.value.as_bytes());
                    let response = conn.call(
                        &format!(
                            "property_set -n {} -d {depth} -c 0 -t {} -l {} -- {encoded}",
                            value.name,
                            value.kind,
                            value.value.len()
                        ),
                        stop,
                    )?;
                    if response.attr("success") != "1" {
                        return Err(Error::InvalidVariable);
                    }
                    refresh(&mut conn, root, secrets, state, depth, stop)?;
                }
                Control::Frame(depth) => refresh(&mut conn, root, secrets, state, depth, stop)?,
                Control::Breakpoints(values) => {
                    breakpoints(&mut conn, root, &values, &mut ids, stop)?;
                    *bps = values;
                }
            }
        }
        if let Some(id) = running {
            if let Some(bytes) = conn.frames.receive(&mut conn.stream)? {
                let e = protocol::xml(&bytes)?;
                if e.name != "response" {
                    continue;
                }
                if e.attr("transaction_id") != id.to_string() || e.child("error").is_some() {
                    return Err(Error::Protocol);
                }
                match e.attr("status") {
                    "break" => {
                        running = None;
                        refresh(&mut conn, root, secrets, state, 0, stop)?;
                    }
                    "stopping" | "stopped" => {
                        let _ = conn.call("stop", stop);
                        return Ok(());
                    }
                    _ => return Err(Error::Protocol),
                }
            }
        } else {
            thread::sleep(Duration::from_millis(20));
        }
    }
    Ok(())
}
impl Server {
    pub fn start(
        root: &Path,
        key: String,
        mut bps: Vec<Breakpoint>,
        secrets: Vec<String>,
    ) -> Result<Self> {
        if key.len() != 64 || !key.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(Error::Unauthorized);
        }
        let root = std::fs::canonicalize(root)?;
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))?;
        listener.set_nonblocking(true)?;
        let port = listener.local_addr()?.port();
        let state = Arc::new(Mutex::new(Snapshot {
            state: "listening".into(),
            port,
            frames: vec![],
            variables: vec![],
            depth: 0,
            error: None,
        }));
        let stop = Arc::new(AtomicBool::new(false));
        let socket = Arc::new(Mutex::new(None));
        let (tx, rx) = mpsc::sync_channel(16);
        let (view, closing, active) = (state.clone(), stop.clone(), socket.clone());
        let worker = thread::spawn(move || {
            let key = zeroize::Zeroizing::new(key);
            while !closing.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((stream, address)) => {
                        if !address.ip().is_loopback() {
                            continue;
                        }
                        *active.lock().unwrap() = stream.try_clone().ok();
                        {
                            let mut s = view.lock().unwrap();
                            s.state = "connecting".into();
                            s.error = None;
                        }
                        let result = conversation(
                            stream,
                            Peer {
                                root: &root,
                                key: &key,
                                secrets: &secrets,
                            },
                            &mut bps,
                            &rx,
                            &view,
                            &closing,
                        );
                        active.lock().unwrap().take();
                        let mut s = view.lock().unwrap();
                        s.state = "listening".into();
                        s.frames.clear();
                        s.variables.clear();
                        if let Err(e) = result
                            && e != Error::Cancelled
                        {
                            s.error = Some(e);
                        }
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(25))
                    }
                    Err(_) => break,
                }
            }
        });
        Ok(Self {
            state,
            tx,
            stop,
            socket,
            worker: Some(worker),
        })
    }
    pub fn snapshot(&self) -> Snapshot {
        self.state.lock().unwrap().clone()
    }
    pub fn control(&self, command: Control) -> Result<()> {
        let mut state = self.state.lock().unwrap();
        if let Control::Set(value) = &command {
            value.validate()?;
            if !state
                .variables
                .iter()
                .any(|v| v.assign.as_deref() == Some(value.name.as_str()))
            {
                return Err(Error::InvalidVariable);
            }
        }
        let detach = matches!(command, Control::Detach);
        if state.state != "paused" && !(detach && state.state == "running") {
            return Err(Error::Busy);
        }
        let resumes = matches!(
            command,
            Control::Run
                | Control::StepInto
                | Control::StepOver
                | Control::StepOut
                | Control::Stop
                | Control::Detach
        );
        self.tx.try_send(command).map_err(|e| match e {
            mpsc::TrySendError::Full(_) => Error::Busy,
            mpsc::TrySendError::Disconnected(_) => Error::Unavailable,
        })?;
        // Disable further resume/step actions immediately, before the engine ACK.
        if resumes {
            state.state = "running".into();
            state.variables.clear();
        }
        Ok(())
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(s) = self.socket.lock().unwrap().take() {
            let _ = s.shutdown(Shutdown::Both);
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn control_backlog_is_bounded_and_resume_disables_duplicate_steps() {
        let root = tempfile::tempdir().unwrap();
        let server = Server::start(root.path(), "a".repeat(64), vec![], vec![]).unwrap();
        server.state.lock().unwrap().state = "paused".into();
        for _ in 0..16 {
            server.control(Control::Frame(0)).unwrap();
        }
        assert_eq!(server.control(Control::Frame(0)).err(), Some(Error::Busy));
        let next = Server::start(root.path(), "b".repeat(64), vec![], vec![]).unwrap();
        next.state.lock().unwrap().state = "paused".into();
        next.control(Control::StepInto).unwrap();
        assert_eq!(next.snapshot().state, "running");
        assert_eq!(next.control(Control::StepOver).err(), Some(Error::Busy));
    }
    #[test]
    fn unknown_clients_are_refused_and_values_redacted() {
        let dir = tempfile::tempdir().unwrap();
        let server = Server::start(
            dir.path(),
            "a".repeat(64),
            vec![],
            vec!["known-value".into()],
        )
        .unwrap();
        let mut c =
            TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, server.snapshot().port)).unwrap();
        let body = "<init language='PHP' idekey='wrong'/>";
        c.write_all(format!("{}\0{}\0", body.len(), body).as_bytes())
            .unwrap();
        let until = Instant::now() + Duration::from_secs(2);
        while server.snapshot().error.is_none() && Instant::now() < until {
            thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(server.snapshot().error, Some(Error::Unauthorized));
        let e=protocol::xml(b"<response><property name='password' type='string'>secret</property><property name='order' type='string'>known-value</property></response>").unwrap();
        let v = variables(&e, &["known-value".into()], 0);
        assert_eq!(v[0].value, "[REDACTED]");
        assert_eq!(v[1].value, "[REDACTED]");
        let secret = "s".repeat(1000);
        assert_eq!(
            scrub(&secret[..512], std::slice::from_ref(&secret)),
            "[REDACTED]"
        );
        assert_eq!(
            scrub(&format!("prefix {}", &secret[..512]), &[secret]),
            "prefix [REDACTED]"
        );
    }
}
