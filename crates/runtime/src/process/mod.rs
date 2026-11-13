mod budget;
pub mod host;
mod log;
pub use budget::FileBudget;
mod redact;
mod tree;
use crate::{Cancellation, Result, RuntimeError, package::VerifiedExecutable, relative_path};
pub use log::{LogChunk, LogPolicy};
use serde::Serialize;
use std::{
    collections::{BTreeMap, HashMap, VecDeque},
    ffi::OsString,
    fs::{self, File, OpenOptions},
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

#[derive(Clone)]
pub enum Readiness {
    Output {
        stream: &'static str,
        needle: String,
    },
    SuccessfulExit,
    Tcp {
        address: std::net::SocketAddr,
    },
}
#[derive(Clone, Copy)]
pub struct RestartPolicy {
    pub max_restarts: u32,
    pub backoff: Duration,
}
impl Default for RestartPolicy {
    fn default() -> Self {
        Self {
            max_restarts: 0,
            backoff: Duration::from_millis(100),
        }
    }
}
#[derive(Clone)]
pub struct ProcessSpec {
    pub input: Option<PathBuf>,
    pub id: String,
    pub executable: VerifiedExecutable,
    pub args: Vec<OsString>,
    pub directory: PathBuf,
    pub environment: BTreeMap<OsString, OsString>,
    pub readiness: Readiness,
    pub readiness_timeout: Duration,
    pub stop_grace: Duration,
    pub restart: RestartPolicy,
    pub logs: LogPolicy,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Observed {
    Starting,
    Ready,
    Backoff,
    Stopping,
    Stopped,
    Succeeded,
    Failed,
}
#[derive(Debug, Clone, Serialize)]
pub struct Transition {
    pub sequence: u64,
    pub state: Observed,
}
#[derive(Debug, Clone, Serialize)]
pub struct Snapshot {
    pub desired_running: bool,
    pub observed: Observed,
    pub pid: Option<u32>,
    pub restarts: u32,
    pub failure: Option<RuntimeError>,
    pub exit_code: Option<i32>,
    pub history: VecDeque<Transition>,
}
impl Snapshot {
    fn set(&mut self, state: Observed, pid: Option<u32>, failure: Option<RuntimeError>) {
        if state == Observed::Starting && pid.is_none() {
            self.exit_code = None;
        }
        self.observed = state;
        self.pid = pid;
        self.failure = failure;
        let sequence = self.history.back().map_or(1, |e| e.sequence + 1);
        if self.history.len() == 32 {
            self.history.pop_front();
        }
        self.history.push_back(Transition { sequence, state });
    }
}
struct Managed {
    state: Arc<Mutex<Snapshot>>,
    logs: Arc<Mutex<log::Logs>>,
    stop: Cancellation,
    worker: Option<JoinHandle<()>>,
    _ownership: File,
    budget: Arc<Mutex<Option<FileBudget>>>,
}
impl Drop for Managed {
    fn drop(&mut self) {
        self.stop.cancel();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
/// Owner-scoped background workers. Window lifetime is deliberately unrelated:
/// a future app/supervisor composition root owns this, never a React window.
pub struct Supervisor {
    root: PathBuf,
    processes: HashMap<String, Managed>,
}
impl Supervisor {
    pub fn open(root: impl AsRef<Path>) -> Result<Self> {
        fs::create_dir_all(root.as_ref())?;
        let root = fs::canonicalize(root)?;
        let marker = root.join(".devforge-process-logs");
        if !marker.exists() {
            if fs::read_dir(&root)?.next().is_some() {
                return Err(RuntimeError::PackageConflict);
            }
            fs::write(&marker, b"devforge-process-logs-v1\n")?;
        }
        if fs::read(&marker)? != b"devforge-process-logs-v1\n" {
            return Err(RuntimeError::PackageConflict);
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&root, fs::Permissions::from_mode(0o700))?;
        }
        Ok(Self {
            root,
            processes: HashMap::new(),
        })
    }
    pub fn start(&mut self, spec: ProcessSpec) -> Result<()> {
        self.start_redacted(spec, Vec::new())
    }
    pub fn start_redacted(&mut self, spec: ProcessSpec, secrets: Vec<String>) -> Result<()> {
        if secrets.len() > 128 || secrets.iter().any(|s| s.is_empty() || s.len() > 16384) {
            return Err(RuntimeError::InvalidProcess);
        }
        let secrets = Arc::new(
            secrets
                .into_iter()
                .map(zeroize::Zeroizing::new)
                .collect::<Vec<_>>(),
        );
        let id = relative_path(&spec.id)?;
        if id.components().count() != 1
            || spec.id.len() > 80
            || self.processes.contains_key(&spec.id)
            || self.processes.len() >= 64
            || spec.restart.max_restarts > 10
            || spec.restart.backoff > Duration::from_secs(60)
            || spec.readiness_timeout.is_zero()
            || spec.readiness_timeout > Duration::from_secs(300)
            || spec.stop_grace > Duration::from_secs(30)
            || !spec.directory.is_absolute()
            || !spec.directory.is_dir()
            || matches!(&spec.readiness,Readiness::Output {stream,needle} if !["stdout","stderr"].contains(stream) || needle.is_empty() || needle.len()>1024)
            || matches!(&spec.readiness,Readiness::Tcp{address} if !address.ip().is_loopback()||address.port()==0)
        {
            return Err(RuntimeError::InvalidProcess);
        }
        spec.executable.check()?;
        let ownership = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(self.root.join(format!("{}.lock", spec.id)))?;
        ownership.try_lock().map_err(|_| RuntimeError::Busy)?;
        let logs = Arc::new(Mutex::new(log::Logs::open(
            self.root.join(format!("{}.log", spec.id)),
            spec.logs,
        )?));
        let mut snapshot = Snapshot {
            desired_running: true,
            observed: Observed::Starting,
            pid: None,
            restarts: 0,
            failure: None,
            exit_code: None,
            history: VecDeque::new(),
        };
        snapshot.set(Observed::Starting, None, None);
        let state = Arc::new(Mutex::new(snapshot));
        let stop = Cancellation::default();
        let worker_state = state.clone();
        let worker_logs = logs.clone();
        let worker_stop = stop.clone();
        let budget = Arc::new(Mutex::new(None));
        let worker_budget = budget.clone();
        let key = spec.id.clone();
        let worker = thread::Builder::new()
            .name(format!("devforge-{}", spec.id))
            .spawn(move || {
                run(
                    spec,
                    worker_state,
                    worker_logs,
                    worker_stop,
                    secrets,
                    worker_budget,
                )
            })?;
        self.processes.insert(
            key,
            Managed {
                state,
                logs,
                stop,
                worker: Some(worker),
                _ownership: ownership,
                budget,
            },
        );
        Ok(())
    }
    pub fn attach_file_budget(&self, id: &str, budget: FileBudget) -> Result<()> {
        budget.validate()?;
        let process = self.processes.get(id).ok_or(RuntimeError::InvalidProcess)?;
        *process.budget.lock().unwrap() = Some(budget);
        Ok(())
    }
    pub fn snapshot(&self, id: &str) -> Result<Snapshot> {
        Ok(self
            .processes
            .get(id)
            .ok_or(RuntimeError::InvalidProcess)?
            .state
            .lock()
            .unwrap()
            .clone())
    }
    pub fn logs(&self, id: &str) -> Result<(Vec<LogChunk>, u64)> {
        let logs = self
            .processes
            .get(id)
            .ok_or(RuntimeError::InvalidProcess)?
            .logs
            .lock()
            .unwrap();
        Ok((logs.chunks.iter().cloned().collect(), logs.dropped_bytes))
    }
    /// Nonblocking cancellation includes startup/readiness and backoff.
    pub fn request_stop(&self, id: &str) -> Result<()> {
        self.processes
            .get(id)
            .ok_or(RuntimeError::InvalidProcess)?
            .stop
            .cancel();
        Ok(())
    }
    /// A service-specific adapter will perform its own authenticated/owned stop.
    /// Mark the following exit expected without prematurely killing the process.
    pub fn expect_exit(&self, id: &str) -> Result<()> {
        let mut s = self
            .processes
            .get(id)
            .ok_or(RuntimeError::InvalidProcess)?
            .state
            .lock()
            .unwrap();
        s.desired_running = false;
        let pid = s.pid;
        s.set(Observed::Stopping, pid, None);
        Ok(())
    }
    pub fn stop(&mut self, id: &str) -> Result<Snapshot> {
        let mut managed = self
            .processes
            .remove(id)
            .ok_or(RuntimeError::InvalidProcess)?;
        managed.stop.cancel();
        if let Some(worker) = managed.worker.take() {
            worker.join().map_err(|_| RuntimeError::Io)?;
        }
        let snapshot = managed.state.lock().unwrap().clone();
        Ok(snapshot)
    }
    pub fn wait_terminal(&self, id: &str, timeout: Duration) -> Result<Snapshot> {
        let deadline = Instant::now() + timeout;
        loop {
            let s = self.snapshot(id)?;
            if matches!(
                s.observed,
                Observed::Succeeded | Observed::Failed | Observed::Stopped
            ) {
                return Ok(s);
            }
            if Instant::now() >= deadline {
                return Err(RuntimeError::ReadinessTimeout);
            }
            thread::sleep(Duration::from_millis(10));
        }
    }
}
fn set(
    state: &Mutex<Snapshot>,
    observed: Observed,
    pid: Option<u32>,
    failure: Option<RuntimeError>,
) {
    state.lock().unwrap().set(observed, pid, failure);
}
fn run(
    spec: ProcessSpec,
    state: Arc<Mutex<Snapshot>>,
    logs: Arc<Mutex<log::Logs>>,
    stop: Cancellation,
    secrets: Arc<Vec<zeroize::Zeroizing<String>>>,
    budget: Arc<Mutex<Option<FileBudget>>>,
) {
    loop {
        if stop.is_cancelled() {
            break;
        }
        set(&state, Observed::Starting, None, None);
        let failure = attempt(&spec, &state, &logs, &stop, &secrets, &budget);
        if stop.is_cancelled() {
            break;
        }
        if !state.lock().unwrap().desired_running {
            set(&state, Observed::Stopped, None, None);
            return;
        }
        match failure {
            Ok(()) => {
                state.lock().unwrap().desired_running = false;
                set(&state, Observed::Succeeded, None, None);
                return;
            }
            Err(error) => {
                if error == RuntimeError::FileBudget {
                    state.lock().unwrap().desired_running = false;
                    set(&state, Observed::Failed, None, Some(error));
                    return;
                }
                let restarts = state.lock().unwrap().restarts;
                if restarts >= spec.restart.max_restarts {
                    state.lock().unwrap().desired_running = false;
                    set(&state, Observed::Failed, None, Some(error));
                    return;
                }
                state.lock().unwrap().restarts += 1;
                set(&state, Observed::Backoff, None, Some(error));
                let deadline = Instant::now() + spec.restart.backoff;
                while Instant::now() < deadline && !stop.is_cancelled() {
                    thread::sleep(Duration::from_millis(10));
                }
            }
        }
    }
    state.lock().unwrap().desired_running = false;
    set(&state, Observed::Stopped, None, None);
}
fn reader(
    mut pipe: impl Read + Send + 'static,
    stream: &'static str,
    logs: Arc<Mutex<log::Logs>>,
    ready: Arc<AtomicBool>,
    failed: Arc<AtomicBool>,
    readiness: Readiness,
    secrets: Arc<Vec<zeroize::Zeroizing<String>>>,
) -> JoinHandle<()> {
    thread::spawn(move || {
        let mut redactor = redact::Redactor::new(&secrets);
        let mut bytes = [0; 4096];
        let mut tail = Vec::new();
        loop {
            let n = match pipe.read(&mut bytes) {
                Ok(0) => {
                    let tail = redactor.push(&[], true);
                    if !tail.is_empty() && logs.lock().unwrap().push(stream, &tail).is_err() {
                        failed.store(true, Ordering::Relaxed);
                    }
                    break;
                }
                Ok(n) => n,
                Err(_) => {
                    failed.store(true, Ordering::Relaxed);
                    break;
                }
            };
            let safe = redactor.push(&bytes[..n], false);
            if let Readiness::Output {
                stream: wanted,
                needle,
            } = &readiness
                && *wanted == stream
            {
                tail.extend_from_slice(&bytes[..n]);
                let matched = tail.windows(needle.len()).any(|w| w == needle.as_bytes());
                let keep = needle.len().saturating_sub(1);
                if tail.len() > keep {
                    tail.drain(..tail.len() - keep);
                }
                if !safe.is_empty() && logs.lock().unwrap().push(stream, &safe).is_err() {
                    failed.store(true, Ordering::Relaxed);
                }
                if matched {
                    ready.store(true, Ordering::Relaxed);
                }
                continue;
            }
            if !safe.is_empty() && logs.lock().unwrap().push(stream, &safe).is_err() {
                failed.store(true, Ordering::Relaxed);
            }
        }
    })
}
fn attempt(
    spec: &ProcessSpec,
    state: &Mutex<Snapshot>,
    logs: &Arc<Mutex<log::Logs>>,
    stop: &Cancellation,
    secrets: &Arc<Vec<zeroize::Zeroizing<String>>>,
    budget: &Arc<Mutex<Option<FileBudget>>>,
) -> Result<()> {
    spec.executable.check()?;
    let mut command = Command::new(spec.executable.path());
    command
        .args(&spec.args)
        .current_dir(dunce::simplified(&spec.directory))
        .env_clear()
        .envs(&spec.environment)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(path) = &spec.input {
        let mut options = OpenOptions::new();
        options.read(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            options.custom_flags(0x00200000);
        }
        let file = options
            .open(path)
            .map_err(|_| RuntimeError::InvalidProcess)?;
        let metadata = file.metadata().map_err(|_| RuntimeError::InvalidProcess)?;
        if !metadata.is_file()
            || metadata.file_type().is_symlink()
            || metadata.len() > 4 * 1024 * 1024 * 1024
        {
            return Err(RuntimeError::InvalidProcess);
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if metadata.file_attributes() & 0x400 != 0 {
                return Err(RuntimeError::InvalidProcess);
            }
        }
        command.stdin(Stdio::from(file));
    }
    let mut tree = tree::Tree::spawn(&mut command)?;
    let pid = tree.child.id();
    set(state, Observed::Starting, Some(pid), None);
    let ready = Arc::new(AtomicBool::new(false));
    let failed = Arc::new(AtomicBool::new(false));
    let stdout = reader(
        tree.child.stdout.take().unwrap(),
        "stdout",
        logs.clone(),
        ready.clone(),
        failed.clone(),
        spec.readiness.clone(),
        secrets.clone(),
    );
    let stderr = reader(
        tree.child.stderr.take().unwrap(),
        "stderr",
        logs.clone(),
        ready.clone(),
        failed.clone(),
        spec.readiness.clone(),
        secrets.clone(),
    );
    let deadline = Instant::now() + spec.readiness_timeout;
    let mut is_ready = false;
    let mut checked = Instant::now() - Duration::from_secs(1);
    let outcome = (|| loop {
        if stop.is_cancelled() {
            set(state, Observed::Stopping, Some(pid), None);
            break Err(RuntimeError::Cancelled);
        }
        if failed.load(Ordering::Relaxed) {
            break Err(RuntimeError::LogFailed);
        }
        if checked.elapsed() >= Duration::from_millis(100) {
            checked = Instant::now();
            if let Some(limit) = budget.lock().unwrap().as_ref()
                && limit.exceeded()?
            {
                break Err(RuntimeError::FileBudget);
            }
        }
        if tree.exited()? {
            break Ok(());
        }
        if !is_ready && ready.load(Ordering::Relaxed) {
            is_ready = true;
            set(state, Observed::Ready, Some(pid), None);
        }
        if !is_ready
            && let Readiness::Tcp { address } = spec.readiness
            && std::net::TcpStream::connect_timeout(&address, Duration::from_millis(30)).is_ok()
        {
            is_ready = true;
            set(state, Observed::Ready, Some(pid), None);
        }
        if !is_ready && Instant::now() >= deadline {
            break Err(RuntimeError::ReadinessTimeout);
        }
        thread::sleep(Duration::from_millis(10));
    })();
    // Cleanup also runs after root crash: its descendants must not outlive the
    // managed attempt or retain stdout pipes and block the reader joins.
    let code = tree.finish(
        if outcome.is_ok() || outcome == Err(RuntimeError::FileBudget) {
            Duration::ZERO
        } else {
            spec.stop_grace
        },
    );
    state.lock().unwrap().exit_code = code.as_ref().ok().copied().flatten();
    drop(tree);
    let _ = stdout.join();
    let _ = stderr.join();
    outcome?;
    if failed.load(Ordering::Relaxed) {
        return Err(RuntimeError::LogFailed);
    }
    if matches!(spec.readiness, Readiness::SuccessfulExit) && code? == Some(0) {
        Ok(())
    } else {
        Err(RuntimeError::ProcessExited)
    }
}
