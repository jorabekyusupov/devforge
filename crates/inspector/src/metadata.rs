//! Noncreating metadata-only retained windows. Never deserialize Event, message
//! Summary, MIME, raw/search columns, SQL/name/data/stack or content filters.
use rusqlite::{Connection, OpenFlags, params};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Debug)]
pub enum ActivityKind {
    Inspector { context: Option<String> },
    Logs { level: Option<String> },
    Mail,
}
impl ActivityKind {
    fn tag(&self) -> &'static str {
        match self {
            Self::Inspector { .. } => "inspector",
            Self::Logs { .. } => "logs",
            Self::Mail => "mail",
        }
    }
    pub fn validate(&self) -> Result<(), Error> {
        match self {
            Self::Inspector {
                context: Some(context),
            } if !super::valid_id(context) => Err(Error::InvalidInput),
            Self::Logs { level: Some(level) } if !LEVELS.contains(&level.as_str()) => {
                Err(Error::InvalidInput)
            }
            _ => Ok(()),
        }
    }
}
#[derive(Clone, Serialize)]
pub struct MetadataRow {
    pub sequence: u64,
    pub item: MetadataItem,
}
#[derive(Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MetadataItem {
    Inspector {
        context_id: String,
        method: String,
        status: Option<u16>,
        queries: u64,
        elapsed_ms: u64,
    },
    Log {
        event_id: String,
        level: String,
        code: String,
        observed_at: u64,
    },
    Mail {
        mail_id: String,
        received_at: u64,
        attachments: u64,
    },
}
#[derive(Clone, Serialize)]
pub struct MetadataWindow {
    pub rows: Vec<MetadataRow>,
    pub epoch: String,
    pub high_water: u64,
    pub low_water: u64,
    pub observed_at: u64,
    pub row_truncated: bool,
    pub byte_truncated: bool,
    pub missing: bool,
    pub origin: &'static str,
    pub source_rows: u64,
    pub source_bytes: u64,
}
#[derive(Debug, Clone, Copy)]
pub enum Error {
    InvalidInput,
    Unavailable,
    UnsafeStore,
    Corrupt,
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::InvalidInput => "metadata_invalid_input",
            _ => "metadata_unavailable",
        })
    }
}
impl std::error::Error for Error {}
impl From<std::io::Error> for Error {
    fn from(_: std::io::Error) -> Self {
        Self::Unavailable
    }
}
impl From<rusqlite::Error> for Error {
    fn from(_: rusqlite::Error) -> Self {
        Self::Unavailable
    }
}
const LEVELS: [&str; 8] = [
    "debug",
    "info",
    "notice",
    "warning",
    "error",
    "critical",
    "alert",
    "emergency",
];
const SAFE_MAX: u64 = 9_007_199_254_740_990;
fn hash(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
fn now() -> Result<u64, Error> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| Error::Unavailable)?
        .as_millis() as u64)
}
fn ordinary(path: &Path, directory: bool) -> Result<bool, Error> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(_) => return Err(Error::Unavailable),
    };
    if metadata.file_type().is_symlink()
        || metadata.is_dir() != directory
        || (!directory && !metadata.is_file())
        || fs::canonicalize(path)? != path
    {
        return Err(Error::UnsafeStore);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return Err(Error::UnsafeStore);
        }
    }
    Ok(true)
}
#[cfg(unix)]
fn file_identity(file: &File) -> Result<String, Error> {
    use std::os::unix::fs::MetadataExt;
    let m = file.metadata()?;
    if m.nlink() != 1 {
        return Err(Error::UnsafeStore);
    }
    let created = m
        .created()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|t| t.as_nanos())
        .unwrap_or(0);
    Ok(format!("{}:{}:{created}", m.dev(), m.ino()))
}
#[cfg(windows)]
fn file_identity(file: &File) -> Result<String, Error> {
    use std::{ffi::c_void, mem::MaybeUninit, os::windows::io::AsRawHandle};
    // Exact BY_HANDLE_FILE_INFORMATION ABI (fileapi.h); no new platform dependency.
    #[repr(C)]
    struct Info {
        attributes: u32,
        created: [u32; 2],
        accessed: [u32; 2],
        written: [u32; 2],
        volume: u32,
        size_high: u32,
        size_low: u32,
        links: u32,
        index_high: u32,
        index_low: u32,
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        #[link_name = "GetFileInformationByHandle"]
        fn information(handle: *mut c_void, out: *mut Info) -> i32;
    }
    let mut out = MaybeUninit::<Info>::uninit();
    // SAFETY: file owns a live ordinary-file handle; Info matches the Win32 ABI.
    if unsafe { information(file.as_raw_handle(), out.as_mut_ptr()) } == 0 {
        return Err(Error::Unavailable);
    }
    // SAFETY: success initializes the complete documented output structure.
    let out = unsafe { out.assume_init() };
    if out.links != 1 || out.attributes & 0x400 != 0 {
        return Err(Error::UnsafeStore);
    }
    Ok(format!(
        "{}:{}:{}:{}:{}",
        out.volume, out.index_high, out.index_low, out.created[1], out.created[0]
    ))
}
struct Source {
    path: PathBuf,
    epoch: String,
}
fn source(root: &Path, id: &str, kind: &ActivityKind) -> Result<Option<Source>, Error> {
    if !super::valid_id(id) {
        return Err(Error::InvalidInput);
    }
    kind.validate()?;
    if !ordinary(root, true)? {
        return Err(Error::UnsafeStore);
    }
    let directories = if matches!(kind, ActivityKind::Mail) {
        vec!["mail", id]
    } else {
        vec!["inspector"]
    };
    let mut directory = root.to_path_buf();
    for component in directories {
        directory.push(component);
        if !ordinary(&directory, true)? {
            return Ok(None);
        }
    }
    let path = directory.join(if matches!(kind, ActivityKind::Mail) {
        "inbox.sqlite3"
    } else {
        "events.sqlite3"
    });
    if !ordinary(&path, false)? {
        return Ok(None);
    }
    if matches!(kind, ActivityKind::Mail) {
        let marker = directory.join(".devforge-inbox");
        if !ordinary(&marker, false)?
            || fs::metadata(&marker)?.len() > 64
            || fs::read(marker)? != b"devforge-inbox-v1\n"
        {
            return Err(Error::UnsafeStore);
        }
    }
    let mut file = File::open(&path)?;
    let identity = file_identity(&file)?;
    let mut header = [0u8; 100];
    file.read_exact(&mut header).map_err(|_| Error::Corrupt)?;
    if &header[..16] != b"SQLite format 3\0"
        || header[18] != 1
        || header[19] != 1
        || u32::from_be_bytes(header[60..64].try_into().unwrap()) != 1
    {
        return Err(Error::Corrupt);
    }
    // Exclude change-counter/page-count/write-time: ordinary inserts and clears
    // must not fabricate a new source epoch. Manager windows remain frozen.
    let epoch = hash(
        format!(
            "metadata-v1:{}:{id}:{identity}:{:?}:{:?}:{:?}:{:?}",
            kind.tag(),
            &header[40..44],
            &header[44..48],
            &header[60..64],
            &header[68..72]
        )
        .as_bytes(),
    );
    Ok(Some(Source { path, epoch }))
}
pub fn epoch(root: &Path, id: &str, kind: &ActivityKind) -> Result<String, Error> {
    Ok(source(root, id, kind)?
        .map(|s| s.epoch)
        .unwrap_or_else(|| hash(format!("metadata-v1:{}:{id}:absent", kind.tag()).as_bytes())))
}
pub fn read(root: &Path, id: &str, kind: &ActivityKind) -> Result<MetadataWindow, Error> {
    let observed_at = now()?;
    let expected_epoch = epoch(root, id, kind)?;
    let mut window = MetadataWindow {
        rows: Vec::new(),
        epoch: expected_epoch.clone(),
        high_water: 0,
        low_water: 0,
        observed_at,
        row_truncated: false,
        byte_truncated: false,
        missing: false,
        origin: "retained_native_metadata",
        source_rows: 0,
        source_bytes: 0,
    };
    let Some(source) = source(root, id, kind)? else {
        window.missing = true;
        window.origin = "native_store_absent";
        return Ok(window);
    };
    if source.epoch != expected_epoch {
        return Err(Error::Unavailable);
    }
    let connection = Connection::open_with_flags(
        &source.path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    connection.busy_timeout(Duration::from_millis(100))?;
    let transaction = connection.unchecked_transaction()?;
    let mail = matches!(kind, ActivityKind::Mail);
    let row_limit = if matches!(kind, ActivityKind::Logs { .. }) {
        4000
    } else {
        200
    };
    let byte_limit = if matches!(kind, ActivityKind::Inspector { .. }) {
        256 * 1024
    } else {
        16 * 1024 * 1024
    };
    let cutoff = if mail {
        0
    } else {
        observed_at.saturating_sub(86_400_000)
    };
    let sql = if mail {
        "SELECT sequence,received,octet_length(summary) FROM messages ORDER BY sequence DESC LIMIT ?1"
    } else {
        "SELECT sequence,received,octet_length(body) FROM events WHERE project=?2 AND received>=?3 ORDER BY sequence DESC LIMIT ?1"
    };
    let mut statement = transaction.prepare(sql)?;
    let mut rows = if mail {
        statement.query(params![row_limit + 1])?
    } else {
        statement.query(params![row_limit + 1, id, cutoff as i64])?
    };
    while let Some(row) = rows.next()? {
        let sequence = row.get::<_, i64>(0)?;
        let received = row.get::<_, i64>(1)?;
        let bytes = row.get::<_, i64>(2)?;
        if sequence <= 0 || received < 0 || received as u64 > SAFE_MAX || bytes < 0 {
            return Err(Error::Corrupt);
        }
        if window.high_water == 0 {
            window.high_water = sequence as u64;
        }
        if window.source_rows >= row_limit as u64 {
            window.row_truncated = true;
            break;
        }
        if bytes as u64 > byte_limit - window.source_bytes {
            window.byte_truncated = true;
            break;
        }
        window.source_rows += 1;
        window.source_bytes += bytes as u64;
        window.low_water = sequence as u64;
    }
    drop(rows);
    drop(statement);
    if window.source_rows > 0 {
        if mail {
            read_mail(&transaction, &mut window)?;
        } else {
            read_events(&transaction, id, kind, cutoff, &mut window)?;
        }
    }
    transaction.commit()?;
    if epoch(root, id, kind)? != expected_epoch {
        return Err(Error::Unavailable);
    }
    Ok(window)
}
fn read_mail(connection: &Connection, window: &mut MetadataWindow) -> Result<(), Error> {
    let mut statement=connection.prepare("SELECT sequence,received,CASE WHEN length(id)=32 THEN id END,CASE WHEN json_valid(summary) THEN CASE WHEN json_type(summary,'$.attachments')='integer' THEN json_extract(summary,'$.attachments') END END FROM messages WHERE sequence>=?1 AND sequence<=?2 ORDER BY sequence DESC")?;
    let mut rows = statement.query(params![window.low_water as i64, window.high_water as i64])?;
    while let Some(row) = rows.next()? {
        let sequence = row.get::<_, i64>(0)?;
        let received = row.get::<_, i64>(1)?;
        let id = row.get::<_, Option<String>>(2)?.ok_or(Error::Corrupt)?;
        let count = row.get::<_, Option<i64>>(3)?.ok_or(Error::Corrupt)?;
        if !super::valid_id(&id) || !(0..=SAFE_MAX as i64).contains(&count) {
            return Err(Error::Corrupt);
        }
        window.rows.push(MetadataRow {
            sequence: sequence as u64,
            item: MetadataItem::Mail {
                mail_id: id,
                received_at: received as u64,
                attachments: count as u64,
            },
        });
    }
    Ok(())
}
fn read_events(
    connection: &Connection,
    id: &str,
    kind: &ActivityKind,
    cutoff: u64,
    window: &mut MetadataWindow,
) -> Result<(), Error> {
    // Only safe scalar projections leave SQLite. Raw name/data/body never enter Rust.
    let sql = r#"SELECT sequence,received,json_valid(body),
    CASE WHEN json_valid(body) THEN json_extract(body,'$.kind') END,
    CASE WHEN json_valid(body) THEN CASE WHEN length(json_extract(body,'$.context'))=32 THEN json_extract(body,'$.context') END END,
    CASE WHEN json_valid(body) THEN CASE WHEN json_extract(body,'$.kind')='exception' THEN 'error'
      WHEN json_extract(body,'$.name') IN ('debug','info','notice','warning','error','critical','alert','emergency') THEN json_extract(body,'$.name') ELSE 'unknown' END END,
    CASE WHEN json_valid(body) THEN CASE WHEN json_extract(body,'$.kind')='request' THEN
      CASE upper(CASE WHEN instr(json_extract(body,'$.name'),' ')>0 THEN substr(json_extract(body,'$.name'),1,instr(json_extract(body,'$.name'),' ')-1) ELSE json_extract(body,'$.name') END)
      WHEN 'GET' THEN 'GET' WHEN 'HEAD' THEN 'HEAD' WHEN 'POST' THEN 'POST' WHEN 'PUT' THEN 'PUT' WHEN 'PATCH' THEN 'PATCH' WHEN 'DELETE' THEN 'DELETE' WHEN 'OPTIONS' THEN 'OPTIONS' WHEN 'CONNECT' THEN 'CONNECT' WHEN 'TRACE' THEN 'TRACE' ELSE 'OTHER' END END END,
    CASE WHEN json_valid(body) THEN CASE WHEN json_type(body,'$.data.status')='integer' AND json_extract(body,'$.data.status') BETWEEN 100 AND 599 THEN json_extract(body,'$.data.status') END END,
    CASE WHEN json_valid(body) THEN CASE WHEN json_type(body,'$.data.query_count')='integer' THEN json_extract(body,'$.data.query_count') ELSE 0 END END,
    CASE WHEN json_valid(body) THEN CASE WHEN json_type(body,'$.duration_ms') IN ('integer','real') THEN json_extract(body,'$.duration_ms') END END
    FROM events WHERE project=?1 AND sequence>=?2 AND sequence<=?3 AND received>=?4 ORDER BY sequence DESC"#;
    let mut statement = connection.prepare(sql)?;
    let mut rows = statement.query(params![
        id,
        window.low_water as i64,
        window.high_water as i64,
        cutoff as i64
    ])?;
    while let Some(row) = rows.next()? {
        let sequence = row.get::<_, i64>(0)? as u64;
        let received = row.get::<_, i64>(1)? as u64;
        if row.get::<_, i64>(2)? != 1 {
            return Err(Error::Corrupt);
        }
        let event = row.get::<_, Option<String>>(3)?.ok_or(Error::Corrupt)?;
        if !["request", "query", "job", "log", "dump", "exception"].contains(&event.as_str()) {
            return Err(Error::Corrupt);
        }
        let context = row.get::<_, Option<String>>(4)?.ok_or(Error::Corrupt)?;
        if !super::valid_id(&context) {
            return Err(Error::Corrupt);
        }
        match kind {
            ActivityKind::Inspector { context: filter } if event == "request" => {
                if filter.as_ref().is_some_and(|filter| filter != &context) {
                    continue;
                }
                let queries = row.get::<_, Option<i64>>(8)?.ok_or(Error::Corrupt)?;
                let elapsed = row.get::<_, Option<f64>>(9)?.ok_or(Error::Corrupt)?;
                if queries < 0
                    || queries as u64 > SAFE_MAX
                    || !elapsed.is_finite()
                    || !(0.0..=86_400_000.0).contains(&elapsed)
                {
                    return Err(Error::Corrupt);
                }
                window.rows.push(MetadataRow {
                    sequence,
                    item: MetadataItem::Inspector {
                        context_id: context,
                        method: row.get::<_, Option<String>>(6)?.ok_or(Error::Corrupt)?,
                        status: row.get::<_, Option<u16>>(7)?,
                        queries: queries as u64,
                        elapsed_ms: elapsed.ceil() as u64,
                    },
                });
            }
            ActivityKind::Logs { level } if event == "log" || event == "exception" => {
                let actual = row.get::<_, Option<String>>(5)?.ok_or(Error::Corrupt)?;
                if level.as_ref().is_some_and(|filter| filter != &actual) {
                    continue;
                }
                window.rows.push(MetadataRow {
                    sequence,
                    item: MetadataItem::Log {
                        event_id: format!("event-{sequence}"),
                        level: actual,
                        code: if event == "exception" {
                            "captured_exception"
                        } else {
                            "captured_log"
                        }
                        .into(),
                        observed_at: received,
                    },
                });
            }
            _ => {}
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Batch, Event, Kind, Store};
    fn fixture() -> (tempfile::TempDir, PathBuf, Store) {
        let temp = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(temp.path()).unwrap();
        let store = Store::open(&root.join("inspector")).unwrap();
        (temp, root, store)
    }
    fn insert(store: &mut Store, project: &str, events: Vec<Event>) {
        store
            .insert(
                Batch {
                    schema: 1,
                    project_id: project.into(),
                    id: uuid::Uuid::new_v4().simple().to_string(),
                    events,
                },
                &"f".repeat(64),
            )
            .unwrap();
    }
    fn event(kind: Kind, name: &str, data: serde_json::Value) -> Event {
        Event {
            kind,
            context: "c".repeat(32),
            name: name.into(),
            duration_ms: 1.25,
            data,
        }
    }
    #[test]
    fn actual_sequences_safe_fields_scope_filters_and_epoch_survive_append_and_clear() {
        let (_temp, root, mut store) = fixture();
        let a = "a".repeat(32);
        let b = "b".repeat(32);
        insert(
            &mut store,
            &a,
            vec![
                event(
                    Kind::Query,
                    "SELECT PRIVATE_CANARY",
                    serde_json::json!({"bindings":["PRIVATE_CANARY"]}),
                ),
                event(
                    Kind::Request,
                    "POST /PRIVATE_CANARY",
                    serde_json::json!({"status":201,"query_count":3,"headers":"PRIVATE_CANARY"}),
                ),
                event(
                    Kind::Log,
                    "warning",
                    serde_json::json!({"message":"PRIVATE_CANARY","context":{"ordinary":"PRIVATE_CANARY"}}),
                ),
                event(
                    Kind::Exception,
                    "PrivateCanaryException",
                    serde_json::json!({"stack":"PRIVATE_CANARY"}),
                ),
            ],
        );
        insert(
            &mut store,
            &b,
            vec![event(
                Kind::Request,
                "GET /other",
                serde_json::json!({"status":200,"query_count":999}),
            )],
        );
        let inspector = read(
            &root,
            &a,
            &ActivityKind::Inspector {
                context: Some("c".repeat(32)),
            },
        )
        .unwrap();
        assert_eq!(inspector.rows.len(), 1);
        assert_eq!(inspector.rows[0].sequence, 2);
        let MetadataItem::Inspector {
            method,
            status,
            queries,
            elapsed_ms,
            ..
        } = &inspector.rows[0].item
        else {
            panic!()
        };
        assert_eq!(
            (method.as_str(), *status, *queries, *elapsed_ms),
            ("POST", Some(201), 3, 2)
        );
        let logs = read(&root, &a, &ActivityKind::Logs { level: None }).unwrap();
        assert_eq!(
            logs.rows.iter().map(|r| r.sequence).collect::<Vec<_>>(),
            vec![4, 3]
        );
        assert!(
            !serde_json::to_string(&logs)
                .unwrap()
                .contains("PRIVATE_CANARY")
        );
        assert!(
            !serde_json::to_string(&inspector)
                .unwrap()
                .contains("PRIVATE_CANARY")
        );
        assert!(
            !serde_json::to_string(&logs)
                .unwrap()
                .contains("PrivateCanaryException")
        );
        assert_eq!(
            read(
                &root,
                &a,
                &ActivityKind::Logs {
                    level: Some("warning".into())
                }
            )
            .unwrap()
            .rows
            .len(),
            1
        );
        let old = inspector.epoch.clone();
        insert(
            &mut store,
            &a,
            vec![event(
                Kind::Request,
                "GET /later",
                serde_json::json!({"status":200,"query_count":0}),
            )],
        );
        assert_eq!(
            epoch(&root, &a, &ActivityKind::Inspector { context: None }).unwrap(),
            old
        );
        store
            .connection
            .execute("DELETE FROM events WHERE project=?1", [&a])
            .unwrap();
        assert_eq!(
            epoch(&root, &a, &ActivityKind::Inspector { context: None }).unwrap(),
            old
        );
        assert_eq!(inspector.rows[0].sequence, 2); // already observed immutable window
        assert!(
            read(&root, &a, &ActivityKind::Inspector { context: None })
                .unwrap()
                .rows
                .is_empty()
        );
    }
    #[test]
    fn missing_is_noncreating_and_existing_mail_projects_only_id_time_attachment_count() {
        let temp = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(temp.path()).unwrap();
        let id = "a".repeat(32);
        assert!(read(&root, &id, &ActivityKind::Mail).unwrap().missing);
        assert!(!root.join("mail").exists());
        fs::create_dir(root.join("projects")).unwrap();
        let inbox = crate::mail::MailStore::open(&root.join("projects"), &id).unwrap();
        let connection = Connection::open(inbox.root().join("inbox.sqlite3")).unwrap();
        connection.execute("INSERT INTO messages(id,received,summary,search,raw)VALUES(?1,1234,?2,?3,?4)",params!["b".repeat(32),serde_json::json!({"id":"b".repeat(32),"attachments":2,"subject":"PRIVATE_MAIL_CANARY","from":"PRIVATE_MAIL_CANARY","to":"PRIVATE_MAIL_CANARY"}).to_string(),"PRIVATE_MAIL_CANARY",b"PRIVATE_MAIL_CANARY".as_slice()]).unwrap();
        let window = read(&root, &id, &ActivityKind::Mail).unwrap();
        assert_eq!(window.rows.len(), 1);
        assert_eq!(window.rows[0].sequence, 1);
        let MetadataItem::Mail {
            received_at,
            attachments,
            ..
        } = &window.rows[0].item
        else {
            panic!()
        };
        assert_eq!((*received_at, *attachments), (1234, 2));
        assert!(
            !serde_json::to_string(&window)
                .unwrap()
                .contains("PRIVATE_MAIL_CANARY")
        );
    }
    #[test]
    fn mail_metadata_accepts_fifty_one_attachments_and_rejects_unsafe_counts() {
        let temp = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(temp.path()).unwrap();
        let id = "a".repeat(32);
        fs::create_dir(root.join("projects")).unwrap();
        let inbox = crate::mail::MailStore::open(&root.join("projects"), &id).unwrap();
        let connection = Connection::open(inbox.root().join("inbox.sqlite3")).unwrap();
        connection
            .execute(
                "INSERT INTO messages(id,received,summary,search,raw)VALUES(?1,1234,'{}','',X'')",
                ["b".repeat(32)],
            )
            .unwrap();
        for count in [51, SAFE_MAX as i64] {
            connection
                .execute(
                    "UPDATE messages SET summary=?1",
                    [serde_json::json!({"attachments":count}).to_string()],
                )
                .unwrap();
            let window = read(&root, &id, &ActivityKind::Mail).unwrap();
            let MetadataItem::Mail { attachments, .. } = &window.rows[0].item else {
                panic!()
            };
            assert_eq!(*attachments, count as u64);
        }
        for count in [-1, SAFE_MAX as i64 + 1] {
            connection
                .execute(
                    "UPDATE messages SET summary=?1",
                    [serde_json::json!({"attachments":count}).to_string()],
                )
                .unwrap();
            assert!(matches!(
                read(&root, &id, &ActivityKind::Mail),
                Err(Error::Corrupt)
            ));
        }
    }
    #[test]
    fn native_prefix_row_byte_limits_and_corruption_are_not_silently_empty() {
        let (_temp, root, mut store) = fixture();
        let id = "a".repeat(32);
        for n in 0..201 {
            insert(
                &mut store,
                &id,
                vec![event(
                    Kind::Request,
                    "GET /row",
                    serde_json::json!({"status":200,"query_count":n}),
                )],
            );
        }
        let window = read(&root, &id, &ActivityKind::Inspector { context: None }).unwrap();
        assert_eq!(window.rows.len(), 200);
        assert!(window.row_truncated);
        assert_eq!((window.high_water, window.low_water), (201, 2));
        let large=serde_json::json!({"kind":"request","context":"c".repeat(32),"name":"GET /hidden","duration_ms":1,"data":{"padding":"z".repeat(300_000)}}).to_string();
        store
            .connection
            .execute(
                "INSERT INTO events(project,received,body)VALUES(?1,?2,?3)",
                params![id, now().unwrap() as i64, large],
            )
            .unwrap();
        let window = read(&root, &id, &ActivityKind::Inspector { context: None }).unwrap();
        assert!(window.byte_truncated && !window.missing && window.rows.is_empty());
        store.connection.execute("DELETE FROM events", []).unwrap();
        store
            .connection
            .execute(
                "INSERT INTO events(project,received,body)VALUES(?1,?2,'not-json')",
                params![id, now().unwrap() as i64],
            )
            .unwrap();
        assert!(read(&root, &id, &ActivityKind::Logs { level: None }).is_err());
    }
    #[test]
    fn invalid_metadata_filters_and_unsafe_links_fail_without_bootstrap() {
        let temp = tempfile::tempdir().unwrap();
        let absent = temp.path().join("absent");
        let id = "a".repeat(32);
        assert!(matches!(
            read(
                &absent,
                &id,
                &ActivityKind::Inspector {
                    context: Some("invalid".into())
                }
            ),
            Err(Error::InvalidInput)
        ));
        assert!(matches!(
            read(
                &absent,
                &id,
                &ActivityKind::Logs {
                    level: Some("PRIVATE_QUERY".into())
                }
            ),
            Err(Error::InvalidInput)
        ));
        assert!(!absent.exists());
        #[cfg(unix)]
        {
            let root = fs::canonicalize(temp.path()).unwrap();
            let outside = tempfile::tempdir().unwrap();
            std::os::unix::fs::symlink(outside.path(), root.join("inspector")).unwrap();
            assert!(matches!(
                read(&root, &id, &ActivityKind::Inspector { context: None }),
                Err(Error::UnsafeStore)
            ));
            assert_eq!(fs::read_dir(outside.path()).unwrap().count(), 0);
        }
    }
    #[cfg(unix)]
    #[test]
    fn readonly_source_reader_never_chmods_or_rewrites_native_store() {
        use std::os::unix::fs::PermissionsExt;
        let (_temp, root, mut store) = fixture();
        let id = "a".repeat(32);
        insert(
            &mut store,
            &id,
            vec![event(
                Kind::Log,
                "info",
                serde_json::json!({"message":"private"}),
            )],
        );
        drop(store);
        let directory = root.join("inspector");
        let database = directory.join("events.sqlite3");
        let bytes = fs::read(&database).unwrap();
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o500)).unwrap();
        assert_eq!(
            read(&root, &id, &ActivityKind::Logs { level: None })
                .unwrap()
                .rows
                .len(),
            1
        );
        assert_eq!(
            fs::metadata(&directory).unwrap().permissions().mode() & 0o777,
            0o500
        );
        assert_eq!(fs::read(&database).unwrap(), bytes);
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o700)).unwrap();
    }
}
