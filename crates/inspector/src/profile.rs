use crate::{Kind, Record};
use serde::Serialize;
use std::collections::BTreeMap;
#[derive(Default, Serialize)]
pub struct Pattern {
    pub sql: String,
    pub count: usize,
    pub total_ms: f64,
    pub slow: bool,
    pub repeated: bool,
}
#[derive(Default, Serialize)]
pub struct Profile {
    pub captured_queries: usize,
    pub sql_ms: f64,
    pub duration_ms: Option<f64>,
    pub peak_memory_bytes: Option<u64>,
    pub truncated: bool,
    pub patterns: Vec<Pattern>,
}
pub fn analyze(records: &[Record]) -> BTreeMap<String, Profile> {
    let mut profiles: BTreeMap<String, Profile> = BTreeMap::new();
    let mut groups: BTreeMap<(String, String), Pattern> = BTreeMap::new();
    for row in records {
        let event = &row.event;
        let profile = profiles.entry(event.context.clone()).or_default();
        if event.kind == Kind::Query {
            profile.captured_queries += 1;
            profile.sql_ms += event.duration_ms;
            let pattern = groups
                .entry((event.context.clone(), event.name.clone()))
                .or_insert_with(|| Pattern {
                    sql: event.name.clone(),
                    ..Default::default()
                });
            pattern.count += 1;
            pattern.total_ms += event.duration_ms;
            pattern.slow |= event.duration_ms >= 100.0;
            pattern.repeated = pattern.count >= 5;
        }
        if matches!(event.kind, Kind::Request | Kind::Job) {
            profile.duration_ms = Some(event.duration_ms);
            profile.peak_memory_bytes = event.data["peak_memory_bytes"].as_u64();
            profile.truncated = event.data["truncated"].as_bool().unwrap_or(false);
        }
    }
    for ((context, _), pattern) in groups {
        profiles.get_mut(&context).unwrap().patterns.push(pattern);
    }
    for profile in profiles.values_mut() {
        profile
            .patterns
            .sort_by(|a, b| b.total_ms.total_cmp(&a.total_ms));
    }
    profiles
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn repetition_is_scoped_to_context() {
        let events = (0..6)
            .map(|i| Record {
                sequence: i,
                received_ms: 0,
                event: crate::Event {
                    kind: Kind::Query,
                    context: if i < 5 { "a" } else { "b" }.into(),
                    name: "select * from users where id=?".into(),
                    duration_ms: 25.0,
                    data: serde_json::Value::Null,
                },
            })
            .collect::<Vec<_>>();
        let p = analyze(&events);
        assert!(p["a"].patterns[0].repeated);
        assert!(!p["b"].patterns[0].repeated);
        assert_eq!(p["a"].sql_ms, 125.0);
    }
}
