//! Bounded stdio transport for the pure read-only MCP contract. There is no
//! Workspace, NativeClient, manager launch or shutdown path in this adapter.
use devforge_control::ErrorCode;
use devforge_manager::{
    Endpoint,
    credential_file::{CredentialFile, LoadedCredential},
    external_client::BoundReadOnlyClient,
    mcp_contract::{self, Action, MAX_REQUEST_BYTES, MAX_RESPONSE_BYTES, Protocol, ToolCall},
};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    io::{self, BufRead, Write},
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender},
    },
    thread,
    time::Duration,
};
use zeroize::Zeroizing;

const QUEUE: usize = 8;
enum Frame {
    Json(Zeroizing<Vec<u8>>),
    TooLarge,
    Partial,
}
enum Input {
    Frame(Frame),
    End,
    Failed,
}
struct Job {
    call: ToolCall,
    key: String,
    cancelled: Arc<AtomicBool>,
}
struct Completed {
    key: String,
    response: Option<Value>,
}

/// Drain oversized lines without accumulating them. A partial final line is not
/// a framed request; EOF remains adapter shutdown, never manager shutdown.
fn frame(reader: &mut impl BufRead) -> io::Result<Option<Frame>> {
    let mut bytes = Zeroizing::new(Vec::new());
    let mut oversized = false;
    loop {
        let chunk = reader.fill_buf()?;
        if chunk.is_empty() {
            return Ok(if oversized {
                Some(Frame::TooLarge)
            } else if bytes.is_empty() {
                None
            } else {
                Some(Frame::Partial)
            });
        }
        let newline = chunk.iter().position(|byte| *byte == b'\n');
        let length = newline.unwrap_or(chunk.len());
        if !oversized {
            if bytes.len().saturating_add(length) > MAX_REQUEST_BYTES {
                oversized = true;
                bytes.clear();
            } else {
                bytes.extend_from_slice(&chunk[..length]);
            }
        }
        reader.consume(length + usize::from(newline.is_some()));
        if newline.is_some() {
            if oversized {
                return Ok(Some(Frame::TooLarge));
            }
            if bytes.last() == Some(&b'\r') {
                bytes.pop();
            }
            return Ok(Some(Frame::Json(bytes)));
        }
    }
}
fn rpc_error(id: Option<Value>, code: i32, message: &str, data: Option<Value>) -> Value {
    let mut reply = json!({"jsonrpc":"2.0","error":{"code":code,"message":message}});
    if let Some(id) = id {
        reply["id"] = id;
    }
    if let Some(data) = data {
        reply["error"]["data"] = data;
    }
    reply
}
fn write_reply(writer: &mut impl Write, reply: Value) -> io::Result<()> {
    let mut encoded = Zeroizing::new(serde_json::to_vec(&reply)?);
    if encoded.len() > MAX_RESPONSE_BYTES {
        encoded = Zeroizing::new(serde_json::to_vec(&mcp_contract::output_limit(
            reply.get("id").cloned(),
        ))?);
    }
    writer.write_all(&encoded)?;
    writer.write_all(b"\n")?;
    writer.flush()
}
fn key(id: &Value) -> String {
    id.to_string()
}

pub fn run(args: &[String]) -> i32 {
    if args.is_empty()
        || matches!(args,[arg] if matches!(arg.as_str(),"--help"|"-h"|"help"))
        || matches!(args,[serve,help] if serve=="serve"&&matches!(help.as_str(),"--help"|"-h"))
    {
        println!(
            "{}",
            json!({"usage":"devforge mcp serve --credential-file ABSOLUTE_PATH","protocol_versions":["2026-07-28","2025-11-25"],"transport":"stdio","read_only":true,"credential":"explicit protected file; never token argv/environment or protocol stdin","owner_autostart":false})
        );
        return 0;
    }
    let [serve, flag, path] = args else {
        eprintln!("mcp_invalid_arguments");
        return 2;
    };
    if serve != "serve" || flag != "--credential-file" {
        eprintln!("mcp_invalid_arguments");
        return 2;
    }
    let loaded = match CredentialFile::load(Path::new(path)) {
        Ok(loaded) => loaded,
        Err(_) => {
            eprintln!("mcp_credential_unavailable");
            return 3;
        }
    };
    match serve_stdio(loaded) {
        Ok(()) => 0,
        Err(_) => {
            eprintln!("mcp_stdio_unavailable");
            4
        }
    }
}
fn serve_stdio(loaded: LoadedCredential) -> io::Result<()> {
    let mut protocol = Protocol::new(loaded.project_id().clone());
    let (input_tx, input_rx) = mpsc::sync_channel(QUEUE);
    let reader = thread::spawn(move || {
        let stdin = io::stdin();
        let mut reader = stdin.lock();
        loop {
            let message = match frame(&mut reader) {
                Ok(Some(frame)) => Input::Frame(frame),
                Ok(None) => Input::End,
                Err(_) => Input::Failed,
            };
            let done = matches!(message, Input::End | Input::Failed);
            if input_tx.send(message).is_err() || done {
                break;
            }
        }
    });
    let (job_tx, job_rx) = mpsc::sync_channel::<Job>(QUEUE);
    let (done_tx, done_rx) = mpsc::sync_channel::<Completed>(QUEUE);
    let worker = thread::spawn(move || read_worker(loaded, job_rx, done_tx));
    let mut pending: HashMap<String, Arc<AtomicBool>> = HashMap::new();
    let stdout = io::stdout();
    let mut writer = stdout.lock();
    let outcome = transport_loop(
        &mut protocol,
        input_rx,
        &job_tx,
        done_rx,
        &mut pending,
        &mut writer,
    );
    for cancelled in pending.values() {
        cancelled.store(true, Ordering::Release);
    }
    drop(job_tx);
    // IPC reads are bounded by the existing manager timeout. Queued cancelled
    // work is skipped; joining never sends a manager operation cancellation.
    let _ = worker.join();
    // On EOF the reader is already complete. On output failure it may still be
    // blocked in stdin; dropping its handle allows the CLI process to exit.
    if reader.is_finished() {
        let _ = reader.join();
    }
    outcome
}
fn transport_loop(
    protocol: &mut Protocol,
    input: Receiver<Input>,
    jobs: &SyncSender<Job>,
    done: Receiver<Completed>,
    pending: &mut HashMap<String, Arc<AtomicBool>>,
    writer: &mut impl Write,
) -> io::Result<()> {
    loop {
        while let Ok(completed) = done.try_recv() {
            let cancelled = pending
                .remove(&completed.key)
                .is_some_and(|flag| flag.load(Ordering::Acquire));
            if !cancelled && let Some(reply) = completed.response {
                write_reply(writer, reply)?;
            }
        }
        let message = match input.recv_timeout(Duration::from_millis(10)) {
            Ok(message) => message,
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => return Ok(()),
        };
        let value = match message {
            Input::End => return Ok(()),
            Input::Failed => return Err(io::Error::other("stdio read failed")),
            Input::Frame(Frame::TooLarge) => {
                write_reply(
                    writer,
                    rpc_error(None, -32600, "Input limit exceeded", None),
                )?;
                continue;
            }
            Input::Frame(Frame::Partial) => {
                write_reply(writer, mcp_contract::parse_error())?;
                continue;
            }
            Input::Frame(Frame::Json(bytes)) => match serde_json::from_slice::<Value>(&bytes) {
                Ok(value) => value,
                Err(_) => {
                    write_reply(writer, mcp_contract::parse_error())?;
                    continue;
                }
            },
        };
        if let Some(id) = value.get("id")
            && pending.contains_key(&key(id))
        {
            write_reply(
                writer,
                rpc_error(
                    Some(id.clone()),
                    -32600,
                    "Duplicate pending request ID",
                    None,
                ),
            )?;
            continue;
        }
        match protocol.receive(value) {
            Action::Ignore => {}
            Action::Reply(reply) => write_reply(writer, reply)?,
            Action::Cancel(id) => {
                if let Some(flag) = pending.get(&key(&id)) {
                    flag.store(true, Ordering::Release);
                }
            }
            Action::Call(call) => {
                let id = key(call.jsonrpc_id());
                let cancelled = Arc::new(AtomicBool::new(false));
                let job = Job {
                    call,
                    key: id.clone(),
                    cancelled: cancelled.clone(),
                };
                match jobs.try_send(job) {
                    Ok(()) => {
                        pending.insert(id, cancelled);
                    }
                    Err(mpsc::TrySendError::Full(job)) => {
                        write_reply(writer, Protocol::complete(job.call, Err(ErrorCode::Busy)))?
                    }
                    Err(mpsc::TrySendError::Disconnected(job)) => write_reply(
                        writer,
                        Protocol::complete(job.call, Err(ErrorCode::StorageUnavailable)),
                    )?,
                }
            }
        }
    }
}
fn read_worker(loaded: LoadedCredential, jobs: Receiver<Job>, done: SyncSender<Completed>) {
    let mut loaded = Some(loaded);
    let mut client = None;
    let mut failure = None;
    while let Ok(job) = jobs.recv() {
        if job.cancelled.load(Ordering::Acquire) {
            if done
                .send(Completed {
                    key: job.key,
                    response: None,
                })
                .is_err()
            {
                break;
            }
            continue;
        }
        if client.is_none() && failure.is_none() {
            let connect = devforge_platform::app_data_dir()
                .map_err(|_| ErrorCode::StorageUnavailable)
                .and_then(|root| {
                    Endpoint::existing(&root).map_err(|_| ErrorCode::StorageUnavailable)
                })
                .and_then(|endpoint| {
                    BoundReadOnlyClient::connect(
                        endpoint,
                        loaded.take().expect("single credential consumption"),
                    )
                });
            match connect {
                Ok(connected) => client = Some(connected),
                Err(error) => failure = Some(error),
            }
        }
        let result = if let Some(client) = client.as_mut() {
            client.request(job.call.request_id.clone(), job.call.input.clone())
        } else {
            Err(failure.unwrap_or(ErrorCode::StorageUnavailable))
        };
        let reply = if job.cancelled.load(Ordering::Acquire) {
            None
        } else {
            Some(Protocol::complete(job.call, result))
        };
        if done
            .send(Completed {
                key: job.key,
                response: reply,
            })
            .is_err()
        {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cancellation_drops_a_pending_read_reply_without_blocking_protocol_ping() {
        struct Replies(Vec<u8>, SyncSender<Value>);
        impl Write for Replies {
            fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
                self.0.extend_from_slice(bytes);
                Ok(bytes.len())
            }
            fn flush(&mut self) -> io::Result<()> {
                self.1
                    .send(serde_json::from_slice(&std::mem::take(&mut self.0))?)
                    .map_err(|_| io::Error::other("test reader closed"))
            }
        }
        let (input_tx, input_rx) = mpsc::sync_channel(QUEUE);
        let (job_tx, _job_rx) = mpsc::sync_channel(QUEUE);
        let (done_tx, done_rx) = mpsc::sync_channel(QUEUE);
        let (reply_tx, reply_rx) = mpsc::sync_channel(QUEUE);
        let cancelled = Arc::new(AtomicBool::new(false));
        let flag = cancelled.clone();
        let key = key(&json!("read-1"));
        let read_key = key.clone();
        let worker = thread::spawn(move || {
            let mut protocol = Protocol::new(devforge_control::Id::new("project").unwrap());
            let mut pending = HashMap::from([(key, flag)]);
            transport_loop(
                &mut protocol,
                input_rx,
                &job_tx,
                done_rx,
                &mut pending,
                &mut Replies(vec![], reply_tx),
            )
            .unwrap();
            assert!(pending.is_empty());
        });
        let send = |value: Value| {
            input_tx
                .send(Input::Frame(Frame::Json(Zeroizing::new(
                    serde_json::to_vec(&value).unwrap(),
                ))))
                .unwrap();
        };
        let ping = |id| json!({"jsonrpc":"2.0","id":id,"method":"ping","params":{"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{}}}});
        send(
            json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":"read-1"}}),
        );
        send(ping(1));
        assert_eq!(
            reply_rx.recv_timeout(Duration::from_secs(2)).unwrap()["id"],
            1
        );
        assert!(cancelled.load(Ordering::Acquire));
        done_tx
            .send(Completed {
                key: read_key,
                response: Some(json!({"jsonrpc":"2.0","id":"read-1","result":{}})),
            })
            .unwrap();
        send(ping(2));
        assert_eq!(
            reply_rx.recv_timeout(Duration::from_secs(2)).unwrap()["id"],
            2
        );
        input_tx.send(Input::End).unwrap();
        worker.join().unwrap();
        assert!(reply_rx.try_recv().is_err());
    }
    #[test]
    fn bounded_framing_drains_oversize_preserves_next_line_and_detects_partial_eof() {
        let mut bytes = vec![b'x'; MAX_REQUEST_BYTES + 1];
        bytes.extend_from_slice(b"\n{}\r\nlast");
        let mut reader = io::Cursor::new(bytes);
        assert!(matches!(frame(&mut reader).unwrap(), Some(Frame::TooLarge)));
        assert!(matches!(frame(&mut reader).unwrap(),Some(Frame::Json(value)) if &*value==b"{}"));
        assert!(matches!(frame(&mut reader).unwrap(), Some(Frame::Partial)));
        assert!(frame(&mut reader).unwrap().is_none());
        let mut output = vec![];
        write_reply(
            &mut output,
            json!({"id":1,"result":{"large":"x".repeat(MAX_RESPONSE_BYTES)}}),
        )
        .unwrap();
        assert!(output.len() < MAX_RESPONSE_BYTES);
        let reply: Value = serde_json::from_slice(&output).unwrap();
        assert_eq!(reply["error"]["data"]["code"], "output_limit_exceeded");
        assert_eq!(reply["id"], 1);
    }
}
