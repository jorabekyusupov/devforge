use crate::{
    Endpoint, Error, IO_TIMEOUT, MAX_CONNECTIONS, MAX_REQUEST_BYTES, MAX_RESPONSE_BYTES, Owner,
    Result, framing,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    future::Future,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::{sync::Semaphore, task::JoinSet};

/// Workspace previews run several bounded Git checks. Keep the normal IPC
/// budget for all other methods and retain the same accept-time deadline.
const WORKSPACE_PREVIEW_TIMEOUT: Duration = Duration::from_secs(30);
fn request_timeout(value: &Value) -> Duration {
    match value.get("method").and_then(Value::as_str) {
        Some("workspace_prepare" | "workspace_plan" | "approve_workspace_plan") => {
            WORKSPACE_PREVIEW_TIMEOUT
        }
        _ => IO_TIMEOUT,
    }
}

#[cfg(unix)]
#[path = "unix.rs"]
pub(crate) mod os;
#[cfg(windows)]
#[path = "windows.rs"]
pub(crate) mod os;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PeerIdentity {
    /// Kernel-observed user identity. Never use an incoming JSON field instead.
    pub user: String,
    pub process_id: Option<u32>,
}

#[derive(Clone, Default)]
pub struct StopSignal(Arc<AtomicBool>);
impl StopSignal {
    pub fn request(&self) {
        self.0.store(true, Ordering::SeqCst);
    }
    pub fn is_requested(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

/// Generic authenticated transport. There is intentionally no public tool
/// handler: a real composition root must supply policy and its workspace owner.
pub struct Server {
    owner: Owner,
}
/// Minted only after the OS listener binds. Readiness of an actual actor is a
/// separate proof combined by the private bootstrap composition boundary.
pub(crate) struct ListenerReady {
    endpoint: Endpoint,
}
impl ListenerReady {
    pub(crate) fn endpoint(&self) -> &Endpoint {
        &self.endpoint
    }
}
impl Server {
    pub fn new(owner: Owner) -> Self {
        Self { owner }
    }
    pub fn endpoint(&self) -> &Endpoint {
        self.owner.endpoint()
    }
    /// The handler must only enqueue/read bounded owner work; arbitrary blocking
    /// business logic is not made cancellable by this transport. Shutdown closes
    /// listeners, drains bounded connections, then releases owner/install leases.
    pub fn run<H>(self, stop: StopSignal, handler: H) -> Result<()>
    where
        H: Fn(PeerIdentity, Value) -> Result<Value> + Send + Sync + 'static,
    {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        let handler = Arc::new(handler);
        let result = runtime.block_on(self.run_async(stop, move |peer, value| {
            let handler = handler.clone();
            async move {
                tokio::task::spawn_blocking(move || handler(peer, value))
                    .await
                    .map_err(|_| Error::HandlerFailed)?
            }
        }));
        runtime.shutdown_timeout(Duration::from_millis(100));
        result
    }
    pub async fn run_async<H, Fut>(self, stop: StopSignal, handler: H) -> Result<()>
    where
        H: Fn(PeerIdentity, Value) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<Value>> + Send + 'static,
    {
        self.run_async_ready(stop, |_| Ok(()), handler).await
    }
    pub(crate) async fn run_async_ready<H, Fut, F>(
        self,
        stop: StopSignal,
        ready: F,
        handler: H,
    ) -> Result<()>
    where
        H: Fn(PeerIdentity, Value) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<Value>> + Send + 'static,
        F: FnOnce(ListenerReady) -> Result<()>,
    {
        self.run_async_ready_with_deadline(stop, ready, move |peer, value, _, _| {
            handler(peer, value)
        })
        .await
    }
    pub(crate) async fn run_async_ready_with_deadline<H, Fut, F>(
        self,
        stop: StopSignal,
        ready: F,
        handler: H,
    ) -> Result<()>
    where
        H: Fn(PeerIdentity, Value, std::time::Instant, Arc<AtomicBool>) -> Fut
            + Send
            + Sync
            + 'static,
        Fut: Future<Output = Result<Value>> + Send + 'static,
        F: FnOnce(ListenerReady) -> Result<()>,
    {
        self.endpoint().validate()?;
        let mut listener = os::Listener::bind(self.endpoint())?;
        if let Err(error) = ready(ListenerReady {
            endpoint: self.endpoint().clone(),
        }) {
            drop(listener);
            os::cleanup(self.endpoint())?;
            return Err(error);
        }
        let handler = Arc::new(handler);
        let slots = Arc::new(Semaphore::new(MAX_CONNECTIONS));
        let mut connections = JoinSet::new();
        let result = loop {
            if stop.is_requested() {
                break Ok(());
            }
            tokio::select! {
                accepted = listener.accept() => {
                    let mut stream = match accepted { Ok(s) => s, Err(e) => break Err(e) };
                    let Ok(permit) = slots.clone().try_acquire_owned() else { continue };
                    let handler = handler.clone();
                    // Capture the deadline at listener acceptance, not when a
                    // potentially queued blocking handler eventually starts.
                    let accepted = std::time::Instant::now();
                    connections.spawn(async move {
                        let _permit = permit;
                        // Idle/partial requests retain the ordinary short budget.
                        let peer = os::peer(&stream, false)?;
                        let input = tokio::time::timeout_at(tokio::time::Instant::from_std(accepted + IO_TIMEOUT), framing::read(&mut stream, MAX_REQUEST_BYTES)).await.map_err(|_| Error::TimedOut)??;
                        let approval = input.get("method").and_then(Value::as_str) == Some("approve_workspace_plan");
                        let deadline = accepted + request_timeout(&input);
                        let cancelled = Arc::new(AtomicBool::new(false));
                        let handler_cancel = cancelled.clone();
                        if approval {
                            // A native owner dropping its connection cancels the
                            // expensive preflight. One connection carries one request.
                            use tokio::io::AsyncReadExt;
                            let (mut reader, mut writer) = tokio::io::split(stream);
                            let mut extra = [0u8; 1];
                            let exchange = async {
                                tokio::select! {
                                    response = handler(peer, input, deadline, handler_cancel) => {
                                        let response = response?;
                                        framing::write(&mut writer, &response, MAX_RESPONSE_BYTES).await
                                    },
                                    _ = reader.read(&mut extra) => { cancelled.store(true, Ordering::Release); Err(Error::HandlerFailed) },
                                }
                            };
                            match tokio::time::timeout_at(tokio::time::Instant::from_std(deadline), exchange).await {
                                Ok(result) => result,
                                Err(_) => { cancelled.store(true, Ordering::Release); Err(Error::TimedOut) },
                            }
                        } else {
                            let exchange = async {
                                let response = handler(peer, input, deadline, handler_cancel).await?;
                                framing::write(&mut stream, &response, MAX_RESPONSE_BYTES).await
                            };
                            match tokio::time::timeout_at(tokio::time::Instant::from_std(deadline), exchange).await {
                                Ok(result) => result,
                                Err(_) => { cancelled.store(true, Ordering::Release); Err(Error::TimedOut) },
                            }
                        }
                    });
                }
                _ = tokio::time::sleep(Duration::from_millis(20)) => {},
                Some(_) = connections.join_next(), if !connections.is_empty() => {},
            }
        };
        drop(listener);
        // Every connection has an IO_TIMEOUT. A user-supplied synchronous handler
        // must also be bounded; owner jobs run on their own worker, not here.
        while connections.join_next().await.is_some() {}
        os::cleanup(self.endpoint())?;
        result
    }
}

#[derive(Clone)]
pub struct Client {
    endpoint: Endpoint,
}
impl Client {
    pub(crate) fn endpoint(&self) -> &Endpoint {
        &self.endpoint
    }
    pub fn new(endpoint: Endpoint) -> Self {
        Self { endpoint }
    }
    pub fn request(&self, value: &Value) -> Result<Value> {
        self.request_with_peer(value).map(|(_, value)| value)
    }
    /// Server identity is verified before any payload/credential is written.
    pub fn request_with_peer(&self, value: &Value) -> Result<(PeerIdentity, Value)> {
        self.exchange(None, value)
    }
    /// Pin the kernel-observed owner before writing any session/key/body data.
    /// The health/bootstrap caller establishes this identity without secrets.
    pub fn request_with_expected_peer(
        &self,
        expected: &PeerIdentity,
        value: &Value,
    ) -> Result<Value> {
        self.exchange(Some(expected), value).map(|(_, value)| value)
    }
    fn exchange(
        &self,
        expected: Option<&PeerIdentity>,
        value: &Value,
    ) -> Result<(PeerIdentity, Value)> {
        self.endpoint.validate()?;
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        runtime.block_on(async {
            tokio::time::timeout(request_timeout(value), async {
                let mut stream = os::connect(&self.endpoint).await?;
                let peer = os::peer(&stream, true)?;
                if expected.is_some_and(|expected| expected != &peer) {
                    return Err(Error::UnauthorizedPeer);
                }
                framing::write(&mut stream, value, MAX_REQUEST_BYTES).await?;
                let response = framing::read(&mut stream, MAX_RESPONSE_BYTES).await?;
                Ok((peer, response))
            })
            .await
            .map_err(|_| Error::TimedOut)?
        })
    }
}

#[cfg(test)]
mod workspace_budget_tests {
    use super::*;
    #[test]
    fn only_workspace_preview_and_owner_approval_get_extended_budget() {
        for method in [
            "workspace_prepare",
            "workspace_plan",
            "approve_workspace_plan",
        ] {
            assert_eq!(
                request_timeout(&serde_json::json!({"method":method})),
                Duration::from_secs(30)
            );
        }
        for method in [
            "approve_service_plan",
            "workspace_apply",
            "site_probe",
            "health",
            "unknown",
        ] {
            assert_eq!(
                request_timeout(&serde_json::json!({"method":method})),
                IO_TIMEOUT
            );
        }
        assert_eq!(request_timeout(&serde_json::json!({})), IO_TIMEOUT);
    }
}
