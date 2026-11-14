use crate::{
    Endpoint, Error, IO_TIMEOUT, MAX_CONNECTIONS, MAX_REQUEST_BYTES, MAX_RESPONSE_BYTES, Owner,
    Result, framing,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::{sync::Semaphore, task::JoinSet};

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
        runtime.block_on(self.serve(stop, Arc::new(handler)))
    }
    async fn serve<H>(self, stop: StopSignal, handler: Arc<H>) -> Result<()>
    where
        H: Fn(PeerIdentity, Value) -> Result<Value> + Send + Sync + 'static,
    {
        self.endpoint().validate()?;
        let mut listener = os::Listener::bind(self.endpoint())?;
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
                    connections.spawn(async move {
                        let _permit = permit;
                        // Rejection never reaches the supplied handler.
                        let peer = os::peer(&stream, false)?;
                        let exchange = async {
                            let input = framing::read(&mut stream, MAX_REQUEST_BYTES).await?;
                            let response = handler(peer, input)?;
                            framing::write(&mut stream, &response, MAX_RESPONSE_BYTES).await
                        };
                        tokio::time::timeout(IO_TIMEOUT, exchange).await.map_err(|_| Error::TimedOut)?
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
    pub fn new(endpoint: Endpoint) -> Self {
        Self { endpoint }
    }
    pub fn request(&self, value: &Value) -> Result<Value> {
        self.request_with_peer(value).map(|(_, value)| value)
    }
    /// Server identity is verified before any payload/credential is written.
    pub fn request_with_peer(&self, value: &Value) -> Result<(PeerIdentity, Value)> {
        self.endpoint.validate()?;
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        runtime.block_on(async {
            tokio::time::timeout(IO_TIMEOUT, async {
                let mut stream = os::connect(&self.endpoint).await?;
                let peer = os::peer(&stream, true)?;
                framing::write(&mut stream, value, MAX_REQUEST_BYTES).await?;
                let response = framing::read(&mut stream, MAX_RESPONSE_BYTES).await?;
                Ok((peer, response))
            })
            .await
            .map_err(|_| Error::TimedOut)?
        })
    }
}
