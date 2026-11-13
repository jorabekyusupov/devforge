//! Authenticated local SMTP sink. No forwarding, DNS lookup or external delivery.
use base64::{Engine, engine::general_purpose::STANDARD};
use mail_parser::MessageParser;
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::TcpStream,
    sync::{Semaphore, oneshot},
    task::JoinSet,
};
use zeroize::Zeroizing;
pub const MAX_MESSAGE: usize = 2 * 1024 * 1024;
#[derive(Debug, Clone, Copy)]
pub struct Error;
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("mail_unavailable")
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
pub type Result<T> = std::result::Result<T, Error>;
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Session {
    pub schema: u32,
    pub project_id: String,
    pub port: u16,
    pub instance: String,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Summary {
    pub id: String,
    pub received_ms: u64,
    pub subject: String,
    pub from: String,
    pub to: String,
    pub bytes: usize,
    pub attachments: usize,
}
#[derive(Serialize)]
pub struct Page {
    pub messages: Vec<Summary>,
    pub total: usize,
    pub page: usize,
}
#[derive(Serialize)]
pub struct Attachment {
    pub index: usize,
    pub name: String,
    pub mime: String,
    pub bytes: usize,
    pub cid: Option<String>,
}
#[derive(Serialize)]
pub struct Detail {
    pub summary: Summary,
    pub text: String,
    pub html: String,
    pub attachments: Vec<Attachment>,
    pub truncated: bool,
}
#[derive(Serialize)]
pub struct Part {
    pub attachment: Attachment,
    pub base64: String,
}
fn private(root: &Path) -> Result<()> {
    fs::create_dir_all(root)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(root, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}
fn write(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::io::Write;
    let mut file = tempfile::NamedTempFile::new_in(path.parent().ok_or(Error)?)?;
    file.write_all(bytes)?;
    file.as_file().sync_all()?;
    file.persist(path).map_err(|_| Error)?;
    Ok(())
}
fn read(path: &Path) -> Result<Vec<u8>> {
    use std::io::Read;
    let m = fs::symlink_metadata(path)?;
    if !m.is_file() || m.file_type().is_symlink() || m.len() > 4096 {
        return Err(Error);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if m.file_attributes() & 0x400 != 0 {
            return Err(Error);
        }
    }
    let mut out = vec![];
    File::open(path)?.take(4097).read_to_end(&mut out)?;
    if out.len() > 4096 {
        return Err(Error);
    }
    Ok(out)
}
fn folder(project_store: &Path, id: &str) -> Result<PathBuf> {
    if !super::valid_id(id) {
        return Err(Error);
    }
    Ok(project_store.parent().ok_or(Error)?.join("mail").join(id))
}
pub fn read_session(project_store: &Path, id: &str) -> Result<Option<Session>> {
    let root = folder(project_store, id)?;
    let file = match OpenOptions::new()
        .read(true)
        .write(true)
        .open(root.join("server.lock"))
    {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(Error),
    };
    match file.try_lock() {
        Ok(()) => return Ok(None),
        Err(std::fs::TryLockError::WouldBlock) => {}
        Err(_) => return Err(Error),
    }
    let session: Session =
        serde_json::from_slice(&read(&root.join("session.json"))?).map_err(|_| Error)?;
    if session.schema != 1
        || session.project_id != id
        || session.port == 0
        || !super::valid_id(&session.instance)
    {
        return Err(Error);
    }
    Ok(Some(session))
}
pub fn configured_port(project_store: &Path, id: &str) -> Result<Option<u16>> {
    let path = folder(project_store, id)?.join("port.json");
    if !path.exists() {
        return Ok(None);
    }
    let port: u16 = serde_json::from_slice(&read(&path)?).map_err(|_| Error)?;
    if port == 0 {
        return Err(Error);
    }
    Ok(Some(port))
}
pub struct MailStore {
    root: PathBuf,
    connection: Connection,
}
impl MailStore {
    pub fn open(project_store: &Path, id: &str) -> Result<Self> {
        let root = folder(project_store, id)?;
        if let Ok(m) = fs::symlink_metadata(&root)
            && (!m.is_dir() || m.file_type().is_symlink())
        {
            return Err(Error);
        }
        let marker = root.join(".devforge-inbox");
        if root.exists() && !marker.exists() && fs::read_dir(&root)?.next().is_some() {
            return Err(Error);
        }
        private(&root)?;
        let parent = fs::canonicalize(project_store.parent().ok_or(Error)?)?;
        if !fs::canonicalize(&root)?.starts_with(&parent) {
            return Err(Error);
        }
        if !marker.exists() {
            write(&marker, b"devforge-inbox-v1\n")?;
        } else if read(&marker)? != b"devforge-inbox-v1\n" {
            return Err(Error);
        }
        if let Ok(m) = fs::symlink_metadata(root.join("inbox.sqlite3"))
            && (!m.is_file() || m.file_type().is_symlink())
        {
            return Err(Error);
        }
        let connection = Connection::open(root.join("inbox.sqlite3"))?;
        connection.busy_timeout(Duration::from_secs(2))?;
        let version: u32 = connection.pragma_query_value(None, "user_version", |r| r.get(0))?;
        if version > 1 {
            return Err(Error);
        }
        connection.execute_batch("PRAGMA journal_mode=DELETE; PRAGMA secure_delete=ON; PRAGMA auto_vacuum=FULL; CREATE TABLE IF NOT EXISTS messages(sequence INTEGER PRIMARY KEY AUTOINCREMENT,id TEXT UNIQUE NOT NULL,received INTEGER NOT NULL,summary TEXT NOT NULL,search TEXT NOT NULL,raw BLOB NOT NULL); PRAGMA user_version=1;")?;
        Ok(Self { root, connection })
    }
    fn save(&mut self, raw: &[u8], sender: &str, recipients: &[String]) -> Result<()> {
        if raw.len() > MAX_MESSAGE {
            return Err(Error);
        }
        let message = MessageParser::default().parse(raw).ok_or(Error)?;
        if message.attachments().len() > 50 {
            return Err(Error);
        }
        let id = uuid::Uuid::new_v4().simple().to_string();
        let received_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| Error)?
            .as_millis() as u64;
        let from = message
            .from()
            .map(|v| {
                v.mailboxes()
                    .take(20)
                    .map(|a| format!("{} <{}>", a.name().unwrap_or(""), a.address().unwrap_or("")))
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| sender.into());
        let subject = message
            .subject()
            .unwrap_or("")
            .chars()
            .take(1024)
            .collect::<String>();
        let to = recipients.join(", ");
        let summary = Summary {
            id: id.clone(),
            received_ms,
            subject,
            from: from.chars().take(2048).collect(),
            to: to.chars().take(4096).collect(),
            bytes: raw.len(),
            attachments: message.attachments().len(),
        };
        let search = format!("{} {} {}", summary.subject, summary.from, summary.to).to_lowercase();
        let tx = self.connection.transaction()?;
        tx.execute(
            "INSERT INTO messages(id,received,summary,search,raw)VALUES(?1,?2,?3,?4,?5)",
            params![
                id,
                received_ms as i64,
                serde_json::to_string(&summary).map_err(|_| Error)?,
                search,
                raw
            ],
        )?;
        tx.execute("DELETE FROM messages WHERE sequence NOT IN(SELECT sequence FROM messages ORDER BY sequence DESC LIMIT 200)",[])?;
        tx.execute("DELETE FROM messages WHERE sequence IN(SELECT sequence FROM(SELECT sequence,SUM(length(raw))OVER(ORDER BY sequence DESC)AS bytes FROM messages)WHERE bytes>67108864)",[])?;
        tx.commit()?;
        Ok(())
    }
    pub fn list(&self, query: &str, page: usize) -> Result<Page> {
        if query.len() > 512 || page > 10000 {
            return Err(Error);
        }
        let pattern = format!(
            "%{}%",
            query
                .to_lowercase()
                .replace('\\', "\\\\")
                .replace('%', "\\%")
                .replace('_', "\\_")
        );
        let total: i64 = self.connection.query_row(
            "SELECT COUNT(*) FROM messages WHERE search LIKE ?1 ESCAPE '\\'",
            [&pattern],
            |r| r.get(0),
        )?;
        let page = page.min((total as usize).saturating_sub(1) / 25);
        let mut q=self.connection.prepare("SELECT summary FROM messages WHERE search LIKE ?1 ESCAPE '\\' ORDER BY sequence DESC LIMIT 25 OFFSET ?2")?;
        let messages = q
            .query_map(params![pattern, (page * 25) as i64], |r| {
                r.get::<_, String>(0)
            })?
            .map(|r| serde_json::from_str(&r?).map_err(|_| Error))
            .collect::<Result<_>>()?;
        Ok(Page {
            messages,
            total: total as usize,
            page,
        })
    }
    fn record(&self, id: &str) -> Result<(Summary, Vec<u8>)> {
        if !super::valid_id(id) {
            return Err(Error);
        }
        let (s, raw): (String, Vec<u8>) = self.connection.query_row(
            "SELECT summary,raw FROM messages WHERE id=?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        if raw.len() > MAX_MESSAGE {
            return Err(Error);
        }
        Ok((serde_json::from_str(&s).map_err(|_| Error)?, raw))
    }
    pub fn detail(&self, id: &str) -> Result<Detail> {
        let (summary, raw) = self.record(id)?;
        let m = MessageParser::default().parse(&raw).ok_or(Error)?;
        let attachments = m
            .attachments()
            .enumerate()
            .map(|(n, p)| attachment(n, p))
            .collect();
        let text = m
            .body_text(0)
            .unwrap_or_default()
            .chars()
            .take(65536)
            .collect();
        let html = m
            .body_html(0)
            .unwrap_or_default()
            .chars()
            .take(65536)
            .collect();
        Ok(Detail {
            summary,
            text,
            html,
            attachments,
            truncated: m.body_text(0).is_some_and(|v| v.chars().count() > 65536)
                || m.body_html(0).is_some_and(|v| v.chars().count() > 65536),
        })
    }
    pub fn part(&self, id: &str, index: usize) -> Result<Part> {
        let (_, raw) = self.record(id)?;
        let m = MessageParser::default().parse(&raw).ok_or(Error)?;
        let part = m.attachments().nth(index).ok_or(Error)?;
        if part.decoded_len() > MAX_MESSAGE {
            return Err(Error);
        }
        Ok(Part {
            attachment: attachment(index, part),
            base64: STANDARD.encode(part.decoded().as_ref()),
        })
    }
    pub fn save_part(&self, id: &str, index: usize, destination: &Path) -> Result<()> {
        let part = self.part(id, index)?;
        let bytes = STANDARD.decode(&part.base64).map_err(|_| Error)?;
        use std::io::Write;
        let mut f = tempfile::NamedTempFile::new_in(destination.parent().ok_or(Error)?)?;
        f.write_all(&bytes)?;
        f.as_file().sync_all()?;
        f.persist_noclobber(destination).map_err(|_| Error)?;
        Ok(())
    }
    pub fn delete(&self, id: Option<&str>) -> Result<()> {
        if let Some(id) = id {
            if !super::valid_id(id) {
                return Err(Error);
            }
            self.connection
                .execute("DELETE FROM messages WHERE id=?1", [id])?;
        } else {
            self.connection.execute("DELETE FROM messages", [])?;
        }
        Ok(())
    }
    pub fn root(&self) -> &Path {
        &self.root
    }
}
fn attachment(index: usize, p: mail_parser::MessagePart<'_>) -> Attachment {
    let mime = p
        .content_type()
        .map(|c| format!("{}/{}", c.ctype(), c.subtype().unwrap_or("octet-stream")))
        .unwrap_or_else(|| "application/octet-stream".into());
    Attachment {
        index,
        name: p
            .attachment_name()
            .unwrap_or("attachment")
            .chars()
            .take(200)
            .collect(),
        mime,
        bytes: p.decoded_len(),
        cid: p.content_id().map(|s| s.chars().take(256).collect()),
    }
}
pub struct MailServer {
    pub session: Session,
    _lease: File,
    stop: Option<oneshot::Sender<()>>,
    worker: Option<thread::JoinHandle<()>>,
}
impl MailServer {
    pub fn start(project_store: &Path, id: &str, password: String) -> Result<Self> {
        if password.len() != 64 || !super::valid_id(id) {
            return Err(Error);
        }
        let store = MailStore::open(project_store, id)?;
        let root = store.root().to_owned();
        let lease = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(root.join("server.lock"))?;
        lease.try_lock().map_err(|_| Error)?;
        let port = configured_port(project_store, id)?.unwrap_or(0);
        let listener = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port))?;
        listener.set_nonblocking(true)?;
        let port = listener.local_addr()?.port();
        write(
            &root.join("port.json"),
            &serde_json::to_vec(&port).map_err(|_| Error)?,
        )?;
        let session = Session {
            schema: 1,
            project_id: id.into(),
            port,
            instance: uuid::Uuid::new_v4().simple().to_string(),
        };
        write(
            &root.join("session.json"),
            &serde_json::to_vec(&session).map_err(|_| Error)?,
        )?;
        let store = Arc::new(Mutex::new(store));
        let password = Arc::new(Zeroizing::new(password));
        let project = id.to_string();
        let (stop, rx) = oneshot::channel();
        let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel(1);
        let worker = thread::spawn(move || {
            if let Ok(rt) = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                rt.block_on(async move {let Ok(listener)=tokio::net::TcpListener::from_std(listener)else{return;};let _=ready_tx.send(());let slots=Arc::new(Semaphore::new(8));let mut tasks=JoinSet::new();tokio::pin!(rx);
   loop{tokio::select!{_=&mut rx=>break,result=listener.accept()=>{let Ok((socket,peer))=result else{break;};if !peer.ip().is_loopback(){continue;}let Ok(permit)=slots.clone().try_acquire_owned()else{continue;};let store=store.clone();let password=password.clone();let project=project.clone();tasks.spawn(async move{let _permit=permit;let _=tokio::time::timeout(Duration::from_secs(120),smtp(socket,store,&project,&password)).await;});},Some(_)=tasks.join_next(),if !tasks.is_empty()=>{}}}
   tasks.abort_all();while tasks.join_next().await.is_some(){}
  });
            }
        });
        if ready_rx.recv_timeout(Duration::from_secs(5)).is_err() {
            let _ = stop.send(());
            let _ = worker.join();
            return Err(Error);
        }
        Ok(Self {
            session,
            _lease: lease,
            stop: Some(stop),
            worker: Some(worker),
        })
    }
}
impl Drop for MailServer {
    fn drop(&mut self) {
        if let Some(tx) = self.stop.take() {
            let _ = tx.send(());
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
async fn line(
    reader: &mut BufReader<tokio::net::tcp::OwnedReadHalf>,
    max: usize,
) -> Result<Vec<u8>> {
    let mut bytes = vec![];
    let count = tokio::time::timeout(
        Duration::from_secs(15),
        reader.take((max + 1) as u64).read_until(b'\n', &mut bytes),
    )
    .await
    .map_err(|_| Error)??;
    if count == 0 || count > max || !bytes.ends_with(b"\r\n") {
        return Err(Error);
    }
    Ok(bytes)
}
async fn reply(write: &mut tokio::net::tcp::OwnedWriteHalf, message: &str) -> Result<()> {
    write.write_all(message.as_bytes()).await?;
    Ok(())
}
fn equal(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |n, (a, b)| n | (a ^ b)) == 0
}
async fn smtp(
    socket: TcpStream,
    store: Arc<Mutex<MailStore>>,
    user: &str,
    password: &str,
) -> Result<()> {
    let (read, mut write) = socket.into_split();
    let mut reader = BufReader::new(read);
    reply(&mut write, "220 DevForge local sink ESMTP\r\n").await?;
    let (mut greeted, mut authenticated) = (false, false);
    let mut sender = None::<String>;
    let mut recipients = vec![];
    let mut attempts = 0;
    for _ in 0..100 {
        let bytes = line(&mut reader, 4096).await?;
        let raw = std::str::from_utf8(&bytes)
            .map_err(|_| Error)?
            .trim_end_matches(['\r', '\n']);
        let (cmd, arg) = raw.split_once(' ').unwrap_or((raw, ""));
        let cmd = cmd.to_ascii_uppercase();
        match cmd.as_str() {
            "EHLO" | "HELO" => {
                greeted = true;
                sender = None;
                recipients.clear();
                reply(&mut write,"250-DevForge\r\n250-SIZE 2097152\r\n250-8BITMIME\r\n250-SMTPUTF8\r\n250 AUTH PLAIN LOGIN\r\n").await?;
            }
            "AUTH" if greeted && !authenticated => {
                attempts += 1;
                let (method, initial) = arg.split_once(' ').unwrap_or((arg, ""));
                let valid = match method.to_ascii_uppercase().as_str() {
                    "PLAIN" => {
                        let value = if initial.is_empty() {
                            reply(&mut write, "334 \r\n").await?;
                            line(&mut reader, 4096).await?
                        } else {
                            initial.as_bytes().to_vec()
                        };
                        let decoded =
                            Zeroizing::new(STANDARD.decode(value.trim_ascii()).map_err(|_| Error)?);
                        let fields = decoded.split(|b| *b == 0).collect::<Vec<_>>();
                        fields.len() == 3
                            && fields[1] == user.as_bytes()
                            && equal(std::str::from_utf8(fields[2]).unwrap_or(""), password)
                    }
                    "LOGIN" => {
                        let username = if initial.is_empty() {
                            reply(&mut write, "334 VXNlcm5hbWU6\r\n").await?;
                            line(&mut reader, 4096).await?
                        } else {
                            initial.as_bytes().to_vec()
                        };
                        reply(&mut write, "334 UGFzc3dvcmQ6\r\n").await?;
                        let value = line(&mut reader, 4096).await?;
                        let decoded =
                            Zeroizing::new(STANDARD.decode(value.trim_ascii()).map_err(|_| Error)?);
                        STANDARD.decode(username.trim_ascii()).map_err(|_| Error)?
                            == user.as_bytes()
                            && equal(std::str::from_utf8(&decoded).unwrap_or(""), password)
                    }
                    _ => false,
                };
                if valid {
                    authenticated = true;
                    reply(&mut write, "235 2.7.0 Authenticated\r\n").await?;
                } else {
                    reply(&mut write, "535 5.7.8 Authentication failed\r\n").await?;
                    if attempts >= 3 {
                        break;
                    }
                }
            }
            "MAIL" | "RCPT" | "DATA" if !authenticated => {
                reply(&mut write, "530 5.7.0 Authentication required\r\n").await?
            }
            "MAIL" => {
                if let Some(value) = arg
                    .get(..5)
                    .filter(|v| v.eq_ignore_ascii_case("FROM:"))
                    .map(|_| &arg[5..])
                {
                    sender = Some(value.chars().take(1024).collect());
                    recipients.clear();
                    reply(&mut write, "250 2.1.0 Sender accepted\r\n").await?;
                } else {
                    reply(&mut write, "501 5.5.2 Invalid sender\r\n").await?;
                }
            }
            "RCPT" if sender.is_some() => {
                if recipients.len() < 100 && arg.to_ascii_uppercase().starts_with("TO:") {
                    recipients.push(arg[3..].chars().take(1024).collect());
                    reply(&mut write, "250 2.1.5 Recipient accepted\r\n").await?;
                } else {
                    reply(&mut write, "452 4.5.3 Recipient limit\r\n").await?;
                }
            }
            "DATA" if sender.is_some() && !recipients.is_empty() => {
                reply(&mut write, "354 End with CRLF.CRLF\r\n").await?;
                let mut data = vec![];
                loop {
                    let bytes =
                        match line(&mut reader, MAX_MESSAGE.saturating_sub(data.len()) + 3).await {
                            Ok(v) => v,
                            Err(_) => {
                                reply(&mut write, "552 5.3.4 Message limit\r\n").await?;
                                return Ok(());
                            }
                        };
                    if bytes == b".\r\n" {
                        break;
                    }
                    data.extend_from_slice(if bytes.starts_with(b"..") {
                        &bytes[1..]
                    } else {
                        &bytes
                    });
                    if data.len() > MAX_MESSAGE {
                        reply(&mut write, "552 5.3.4 Message limit\r\n").await?;
                        return Ok(());
                    }
                }
                let result = store.lock().map_err(|_| Error)?.save(
                    &data,
                    sender.as_deref().unwrap(),
                    &recipients,
                );
                sender = None;
                recipients.clear();
                reply(
                    &mut write,
                    if result.is_ok() {
                        "250 2.0.0 Captured locally\r\n"
                    } else {
                        "554 5.6.0 Message rejected\r\n"
                    },
                )
                .await?;
            }
            "RSET" => {
                sender = None;
                recipients.clear();
                reply(&mut write, "250 2.0.0 Reset\r\n").await?;
            }
            "NOOP" => reply(&mut write, "250 2.0.0 OK\r\n").await?,
            "QUIT" => {
                reply(&mut write, "221 2.0.0 Bye\r\n").await?;
                break;
            }
            _ => reply(&mut write, "503 5.5.1 Unsupported command or sequence\r\n").await?,
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{BufRead, BufReader, Write},
        net::TcpStream,
    };
    fn response(r: &mut BufReader<TcpStream>) -> String {
        let mut out = String::new();
        loop {
            let mut l = String::new();
            r.read_line(&mut l).unwrap();
            assert!(!l.is_empty());
            let last = l.as_bytes().get(3) != Some(&b'-');
            out.push_str(&l);
            if last {
                return out;
            }
        }
    }
    fn send(r: &mut BufReader<TcpStream>, s: &str) -> String {
        r.get_mut().write_all(s.as_bytes()).unwrap();
        response(r)
    }
    fn client(port: u16) -> BufReader<TcpStream> {
        let s = TcpStream::connect(("127.0.0.1", port)).unwrap();
        s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let mut r = BufReader::new(s);
        assert!(response(&mut r).starts_with("220"));
        assert!(send(&mut r, "EHLO example.test\r\n").contains("AUTH PLAIN LOGIN"));
        r
    }
    fn auth(r: &mut BufReader<TcpStream>, id: &str, pw: &str) -> String {
        send(
            r,
            &format!(
                "AUTH PLAIN {}\r\n",
                STANDARD.encode(format!("\0{id}\0{pw}"))
            ),
        )
    }
    const MIME: &str = "From: Developer <hello@example.test>\r\nTo: tester@example.test\r\nSubject: =?UTF-8?B?0KHQsNC70L7QvA==?=\r\nMIME-Version: 1.0\r\nContent-Type: multipart/mixed; boundary=X\r\n\r\n--X\r\nContent-Type: multipart/alternative; boundary=Y\r\n\r\n--Y\r\nContent-Type: text/plain; charset=utf-8\r\n\r\nPlain Uzbek Русский\r\n--Y\r\nContent-Type: text/html; charset=utf-8\r\n\r\n<h1>MAIL_HTML</h1><script>window.evil=1</script>\r\n--Y--\r\n--X\r\nContent-Type: text/plain\r\nContent-Disposition: attachment; filename=note.txt\r\nContent-Transfer-Encoding: base64\r\n\r\nYXR0YWNobWVudA==\r\n--X--\r\n";
    #[test]
    fn authenticated_smtp_mime_isolation_persistence_and_shutdown() {
        let temp = tempfile::tempdir().unwrap();
        let projects = temp.path().join("projects");
        fs::create_dir(&projects).unwrap();
        let a = "a".repeat(32);
        let b = "b".repeat(32);
        let password = "c".repeat(64);
        let server = MailServer::start(&projects, &a, password.clone()).unwrap();
        let port = server.session.port;
        let mut c = client(port);
        assert!(send(&mut c, "MAIL FROM:<a@example.test>\r\n").starts_with("530"));
        assert!(auth(&mut c, &b, &password).starts_with("535"));
        assert!(auth(&mut c, &a, &password).starts_with("235"));
        assert!(send(&mut c, "MAIL FROM:<a@example.test>\r\n").starts_with("250"));
        assert!(send(&mut c, "RCPT TO:<b@example.test>\r\n").starts_with("250"));
        assert!(send(&mut c, "DATA\r\n").starts_with("354"));
        assert!(send(&mut c, &format!("{MIME}.\r\n")).starts_with("250"));
        assert!(send(&mut c, "QUIT\r\n").starts_with("221"));
        let store = MailStore::open(&projects, &a).unwrap();
        let page = store.list("Салом", 0).unwrap();
        assert_eq!(page.total, 1);
        let id = &page.messages[0].id;
        let detail = store.detail(id).unwrap();
        assert!(detail.text.contains("Русский") && detail.html.contains("MAIL_HTML"));
        assert_eq!(detail.attachments.len(), 1);
        assert_eq!(
            STANDARD.decode(store.part(id, 0).unwrap().base64).unwrap(),
            b"attachment"
        );
        assert!(MailStore::open(&projects, &b).unwrap().detail(id).is_err());
        drop(store);
        drop(c);
        drop(server);
        assert!(read_session(&projects, &a).unwrap().is_none());
        assert!(TcpStream::connect(("127.0.0.1", port)).is_err());
        let restarted = MailServer::start(&projects, &a, password).unwrap();
        assert_eq!(restarted.session.port, port);
        let store = MailStore::open(&projects, &a).unwrap();
        assert_eq!(store.list("", 0).unwrap().total, 1);
        store.delete(None).unwrap();
        assert_eq!(store.list("", 0).unwrap().total, 0);
    }
    #[test]
    fn login_and_message_limit_are_enforced() {
        let temp = tempfile::tempdir().unwrap();
        let projects = temp.path().join("projects");
        fs::create_dir(&projects).unwrap();
        let id = "a".repeat(32);
        let pw = "b".repeat(64);
        let server = MailServer::start(&projects, &id, pw.clone()).unwrap();
        let mut c = client(server.session.port);
        assert!(send(&mut c, "AUTH LOGIN\r\n").starts_with("334"));
        assert!(send(&mut c, &format!("{}\r\n", STANDARD.encode(&id))).starts_with("334"));
        assert!(send(&mut c, &format!("{}\r\n", STANDARD.encode(&pw))).starts_with("235"));
        send(&mut c, "MAIL FROM:<a@b.test>\r\n");
        send(&mut c, "RCPT TO:<b@b.test>\r\n");
        assert!(send(&mut c, "DATA\r\n").starts_with("354"));
        let data = "x".repeat(MAX_MESSAGE + 20) + "\r\n.\r\n";
        let _ = c.get_mut().write_all(data.as_bytes());
        assert!(response(&mut c).starts_with("552"));
        assert_eq!(
            MailStore::open(&projects, &id)
                .unwrap()
                .list("", 0)
                .unwrap()
                .total,
            0
        );
    }
    #[test]
    fn mailbox_rejects_foreign_directory_and_invalid_part() {
        let temp = tempfile::tempdir().unwrap();
        let projects = temp.path().join("projects");
        fs::create_dir(&projects).unwrap();
        let root = temp.path().join("mail").join("a".repeat(32));
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("foreign"), "keep").unwrap();
        assert!(MailStore::open(&projects, &"a".repeat(32)).is_err());
        assert_eq!(fs::read(root.join("foreign")).unwrap(), b"keep");
    }
}
