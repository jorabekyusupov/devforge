//! Bounded read-only registry projection for storage inventory. No secrets fetched.
use crate::{
    cache::CacheInstance,
    database::{Binding, Instance},
    project::{Project, ProjectStore},
};
use serde::de::DeserializeOwned;
#[derive(Clone, Copy)]
pub struct Limits {
    pub max_rows: usize,
    pub max_record_bytes: usize,
    pub max_total_bytes: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            max_rows: 2048,
            max_record_bytes: 262144,
            max_total_bytes: 4 * 1024 * 1024,
        }
    }
}
/// Deliberately not Serialize: callers must explicitly select nonsecret fields.
#[derive(Default)]
pub struct Metadata {
    pub projects: Vec<Project>,
    pub instances: Vec<Instance>,
    pub caches: Vec<CacheInstance>,
    pub active_bindings: Vec<Binding>,
    pub retained_bindings: Vec<Binding>,
    pub rows_examined: usize,
    pub bytes_read: usize,
    pub complete: bool,
    pub problems: Vec<&'static str>,
}
impl Metadata {
    fn problem(&mut self, code: &'static str) {
        self.complete = false;
        if !self.problems.contains(&code) {
            self.problems.push(code);
        }
    }
}
struct Table {
    name: &'static str,
    filter: &'static str,
    order: &'static str,
}
fn query<T: DeserializeOwned>(
    store: &ProjectStore,
    table: Table,
    limits: Limits,
    out: &mut Metadata,
) -> Vec<T> {
    let remaining = limits.max_rows.saturating_sub(out.rows_examined);
    if remaining == 0 {
        out.problem("metadata_row_limit");
        return vec![];
    }
    // All identifiers/fragments come from the fixed private table inventory below.
    // octet_length's direct column argument lets SQLite use stored field length
    // without materializing oversized TEXT/BLOB overflow payload pages.
    let sql = format!(
        "SELECT rowid,octet_length(record) FROM {} {} ORDER BY {} LIMIT ?1",
        table.name, table.filter, table.order
    );
    let Ok(mut statement) = store.connection.prepare(&sql) else {
        out.problem("metadata_query_unavailable");
        return vec![];
    };
    let Ok(mut rows) = statement.query([(remaining + 1) as i64]) else {
        out.problem("metadata_query_unavailable");
        return vec![];
    };
    let mut result = vec![];
    loop {
        let row = match rows.next() {
            Ok(Some(row)) => row,
            Ok(None) => break,
            Err(_) => {
                out.problem("metadata_query_unavailable");
                break;
            }
        };
        if out.rows_examined == limits.max_rows {
            out.problem("metadata_row_limit");
            break;
        }
        out.rows_examined += 1;
        let (rowid, expected_bytes) = match (
            row.get::<_, i64>(0),
            row.get::<_, i64>(1)
                .ok()
                .and_then(|n| usize::try_from(n).ok()),
        ) {
            (Ok(rowid), Some(bytes)) => (rowid, bytes),
            _ => {
                out.problem("metadata_record_invalid");
                continue;
            }
        };
        if expected_bytes > limits.max_record_bytes {
            out.problem("metadata_record_byte_limit");
            continue;
        }
        let capacity = limits
            .max_record_bytes
            .min(limits.max_total_bytes.saturating_sub(out.bytes_read));
        if expected_bytes > capacity {
            out.problem("metadata_total_byte_limit");
            break;
        }
        // A separate, bounded lookup happens only AFTER both byte budgets pass.
        // Recheck in SQL too: another connection must not grow a record beyond
        // remaining capacity between metadata observation and retrieval.
        let record_sql = format!(
            "SELECT record,octet_length(record) FROM {} WHERE rowid=?1 AND octet_length(record)<=?2",
            table.name
        );
        let fetched = store.connection.query_row(
            &record_sql,
            rusqlite::params![rowid, capacity as i64],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
        );
        let (record, actual_bytes) = match fetched {
            Ok((record, bytes)) => match usize::try_from(bytes) {
                Ok(bytes) => (record, bytes),
                Err(_) => {
                    out.problem("metadata_record_invalid");
                    continue;
                }
            },
            Err(rusqlite::Error::QueryReturnedNoRows) => {
                out.problem("metadata_changed_during_scan");
                continue;
            }
            Err(_) => {
                out.problem("metadata_record_invalid");
                continue;
            }
        };
        if record.len() != actual_bytes || actual_bytes > capacity {
            out.problem("metadata_changed_during_scan");
            continue;
        }
        out.bytes_read += actual_bytes;
        if actual_bytes != expected_bytes {
            out.problem("metadata_changed_during_scan");
        }
        match serde_json::from_str(&record) {
            Ok(value) => result.push(value),
            Err(_) => out.problem("metadata_record_invalid"),
        }
    }
    result
}
impl ProjectStore {
    pub fn storage_metadata(&self, limits: Limits) -> Metadata {
        let mut out = Metadata {
            complete: true,
            ..Default::default()
        };
        if limits.max_rows == 0
            || limits.max_rows > 4096
            || limits.max_record_bytes == 0
            || limits.max_record_bytes > 262144
            || limits.max_total_bytes == 0
            || limits.max_total_bytes > 16 * 1024 * 1024
        {
            out.problem("invalid_metadata_limits");
            return out;
        }
        out.projects = query(
            self,
            Table {
                name: "projects",
                filter: "WHERE active=1",
                order: "id",
            },
            limits,
            &mut out,
        );
        out.instances = query(
            self,
            Table {
                name: "instances",
                filter: "",
                order: "id",
            },
            limits,
            &mut out,
        );
        out.caches = query(
            self,
            Table {
                name: "cache_instances",
                filter: "",
                order: "id",
            },
            limits,
            &mut out,
        );
        out.active_bindings = query(
            self,
            Table {
                name: "database_bindings",
                filter: "WHERE project_id IN (SELECT id FROM projects WHERE active=1)",
                order: "project_id",
            },
            limits,
            &mut out,
        );
        out.retained_bindings = query(
            self,
            Table {
                name: "database_binding_history",
                filter: "WHERE project_id IN (SELECT id FROM projects WHERE active=1)",
                order: "project_id,binding_key",
            },
            limits,
            &mut out,
        );
        out
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn oversized_corrupt_and_budgeted_records_are_explicit_without_large_reads() {
        let temp = tempfile::tempdir().unwrap();
        let store = ProjectStore::open(temp.path().join("metadata")).unwrap();
        for (id, record) in [("one", "x".repeat(4096)), ("two", "malformed".into())] {
            store
                .connection
                .execute(
                    "INSERT INTO projects VALUES(?1,?1,1,1,?2)",
                    rusqlite::params![id, record],
                )
                .unwrap();
        }
        let snapshot = store.storage_metadata(Limits {
            max_record_bytes: 32,
            ..Default::default()
        });
        assert!(!snapshot.complete);
        assert!(snapshot.problems.contains(&"metadata_record_byte_limit"));
        assert!(snapshot.problems.contains(&"metadata_record_invalid"));
        assert_eq!(snapshot.bytes_read, 9);
        let limited = store.storage_metadata(Limits {
            max_rows: 1,
            ..Default::default()
        });
        assert_eq!(limited.rows_examined, 1);
        assert!(limited.problems.contains(&"metadata_row_limit"));
        let limited = store.storage_metadata(Limits {
            max_total_bytes: 1,
            ..Default::default()
        });
        assert_eq!(limited.bytes_read, 0);
        assert!(limited.problems.contains(&"metadata_total_byte_limit"));
    }
    fn page_misses(store: &ProjectStore) -> i32 {
        let mut current = 0;
        let mut highwater = 0;
        // The handle belongs exclusively to this test's Connection; SQLite's
        // per-connection page-cache counter does not inspect any payload bytes.
        let result = unsafe {
            rusqlite::ffi::sqlite3_db_status(
                store.connection.handle(),
                rusqlite::ffi::SQLITE_DBSTATUS_CACHE_MISS,
                &mut current,
                &mut highwater,
                0,
            )
        };
        assert_eq!(result, rusqlite::ffi::SQLITE_OK);
        current
    }
    #[test]
    fn oversized_registry_payload_does_not_load_overflow_pages_for_byte_length() {
        let temp = tempfile::tempdir().unwrap();
        let store = ProjectStore::open(temp.path().join("metadata")).unwrap();
        // SQLite creates this 8MiB corrupt TEXT fixture without a Rust payload
        // allocation. The tiny cache makes accidental payload loading observable.
        store
            .connection
            .execute(
                "INSERT INTO projects VALUES('oversized','oversized',1,1,CAST(zeroblob(8388608) AS TEXT))",
                [],
            )
            .unwrap();
        store
            .connection
            .execute_batch("PRAGMA cache_size=-64; PRAGMA shrink_memory;")
            .unwrap();
        let before = page_misses(&store);
        let snapshot = store.storage_metadata(Limits {
            max_record_bytes: 32,
            ..Default::default()
        });
        let pages = page_misses(&store) - before;
        assert!(snapshot.problems.contains(&"metadata_record_byte_limit"));
        assert_eq!(snapshot.bytes_read, 0);
        assert!(
            pages < 128,
            "size-only scan loaded {pages} pages of oversized payload"
        );
    }
    #[test]
    fn registry_limits_count_unicode_bytes_not_characters() {
        let temp = tempfile::tempdir().unwrap();
        let store = ProjectStore::open(temp.path().join("metadata")).unwrap();
        store
            .connection
            .execute(
                "INSERT INTO projects VALUES('unicode','unicode',1,1,?1)",
                ["я".repeat(8)],
            )
            .unwrap();
        let snapshot = store.storage_metadata(Limits {
            max_record_bytes: 8,
            ..Default::default()
        });
        assert!(snapshot.problems.contains(&"metadata_record_byte_limit"));
        assert_eq!(snapshot.bytes_read, 0);
    }
}
