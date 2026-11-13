//! Server-side filtering over the complete bounded M7 event store.
use super::*;
#[derive(Clone, Serialize, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct Filter {
    pub query: String,
    pub level: String,
    pub kind: String,
    pub since: u64,
    pub page: usize,
}
#[derive(Serialize)]
pub struct Page {
    pub entries: Vec<Record>,
    pub total: usize,
    pub page: usize,
}
pub fn read(root: &Path, id: &str, filter: &Filter) -> Result<Page> {
    if !valid_id(id)
        || filter.query.len() > 512
        || filter.page > 10000
        || !["", "all", "log", "exception"].contains(&filter.kind.as_str())
        || ![
            "",
            "all",
            "debug",
            "info",
            "notice",
            "warning",
            "error",
            "critical",
            "alert",
            "emergency",
        ]
        .contains(&filter.level.as_str())
    {
        return Err(Error);
    }
    let store = Store::open(root)?;
    let mut q=store.connection.prepare("SELECT sequence,received,body FROM events WHERE project=?1 ORDER BY sequence DESC LIMIT 4000")?;
    let rows = q.query_map([id], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, i64>(1)?,
            r.get::<_, String>(2)?,
        ))
    })?;
    let query = filter.query.to_lowercase();
    let mut entries = vec![];
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| Error)?
        .as_millis() as u64;
    for row in rows {
        let (sequence, received, body) = row?;
        if received < 0
            || (received as u64) < now.saturating_sub(86_400_000)
            || (received as u64) < filter.since
        {
            continue;
        }
        let event: Event = serde_json::from_str(&body).map_err(|_| Error)?;
        if !matches!(event.kind, Kind::Log | Kind::Exception) {
            continue;
        }
        let level = if event.kind == Kind::Exception {
            "error"
        } else {
            event.name.as_str()
        };
        if !["", "all"].contains(&filter.kind.as_str())
            && ((filter.kind == "log") != (event.kind == Kind::Log))
        {
            continue;
        }
        if !["", "all"].contains(&filter.level.as_str()) && filter.level != level {
            continue;
        }
        if !format!("{} {} {}", event.name, event.context, body)
            .to_lowercase()
            .contains(&query)
        {
            continue;
        }
        entries.push(Record {
            sequence,
            received_ms: received as u64,
            event,
        });
    }
    let total = entries.len();
    let page = filter.page.min(total.saturating_sub(1) / 50);
    let entries = entries.into_iter().skip(page * 50).take(50).collect();
    Ok(Page {
        entries,
        total,
        page,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn full_retained_log_search_filters_and_redaction() {
        let temp = tempfile::tempdir().unwrap();
        let mut s = Store::open(temp.path()).unwrap();
        let a = "a".repeat(32);
        let b = "b".repeat(32);
        let events=(0..300).map(|n|Event{kind:Kind::Log,context:if n==0{"e".repeat(32)}else{"c".repeat(32)},name:if n%2==0{"error"}else{"info"}.into(),duration_ms:0.0,data:serde_json::json!({"message":format!("Салом item {n}"),"context":{"api_token":"do-not-show"}})}).collect::<Vec<_>>();
        for chunk in events.chunks(100) {
            s.insert(
                Batch {
                    schema: 1,
                    project_id: a.clone(),
                    id: uuid::Uuid::new_v4().simple().to_string(),
                    events: chunk.to_vec(),
                },
                &"d".repeat(64),
            )
            .unwrap();
        }
        drop(s);
        let view = read(
            temp.path(),
            &a,
            &Filter {
                level: "error".into(),
                query: "салом".into(),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(view.total, 150);
        assert_eq!(
            super::super::read_context(temp.path(), &a, &"e".repeat(32))
                .unwrap()
                .len(),
            1
        );
        assert_eq!(view.entries.len(), 50);
        assert!(
            !serde_json::to_string(&view)
                .unwrap()
                .contains("do-not-show")
        );
        assert_eq!(
            read(
                temp.path(),
                &a,
                &Filter {
                    query: "item 0\"".into(),
                    ..Default::default()
                }
            )
            .unwrap()
            .total,
            1
        );
        assert_eq!(read(temp.path(), &b, &Filter::default()).unwrap().total, 0);
    }
}
