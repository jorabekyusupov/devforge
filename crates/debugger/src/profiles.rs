//! Bounded Xdebug Cachegrind flat profiles and call edges. Arguments are not recorded.
use crate::{Error, Result};
use serde::Serialize;
use std::collections::BTreeMap;
#[derive(Debug, Clone, Serialize, Default)]
pub struct Function {
    pub file: String,
    pub name: String,
    pub self_ms: f64,
    pub inclusive_ms: f64,
    pub memory_bytes: i64,
    pub calls: u64,
}
#[derive(Debug, Clone, Serialize)]
pub struct Profile {
    pub total_ms: f64,
    pub memory_bytes: i64,
    pub functions: Vec<Function>,
    pub truncated: bool,
}
fn name(value: &str, map: &mut BTreeMap<String, String>) -> Result<String> {
    if value.len() > 8192 {
        return Err(Error::ProfileInvalid);
    }
    if let Some(rest) = value.strip_prefix('(') {
        let (id, tail) = rest.split_once(')').ok_or(Error::ProfileInvalid)?;
        if id.len() > 12 {
            return Err(Error::ProfileInvalid);
        }
        if !tail.trim().is_empty() {
            map.insert(id.into(), tail.trim().into());
        }
        return map.get(id).cloned().ok_or(Error::ProfileInvalid);
    }
    Ok(value.into())
}
pub fn parse(bytes: &[u8]) -> Result<Profile> {
    if bytes.len() > 32 * 1024 * 1024 {
        return Err(Error::ProfileLimit);
    }
    let text = std::str::from_utf8(bytes).map_err(|_| Error::ProfileInvalid)?;
    let mut files = BTreeMap::new();
    let mut names = BTreeMap::new();
    let mut functions: BTreeMap<(String, String), Function> = BTreeMap::new();
    let (mut file, mut function, mut target_file, mut target_fn) =
        (String::new(), String::new(), String::new(), String::new());
    let mut calls = None;
    let mut summary = None;
    let mut valid = false;
    for line in text.lines() {
        if line.len() > 65536 {
            return Err(Error::ProfileInvalid);
        }
        if let Some(v) = line.strip_prefix("events: ") {
            if v != "Time_(10ns) Memory_(bytes)" {
                return Err(Error::ProfileInvalid);
            }
            valid = true;
        } else if let Some(v) = line.strip_prefix("fl=") {
            file = name(v, &mut files)?;
        } else if let Some(v) = line.strip_prefix("fn=") {
            function = name(v, &mut names)?;
        } else if let Some(v) = line.strip_prefix("cfl=") {
            target_file = name(v, &mut files)?;
        } else if let Some(v) = line.strip_prefix("cfn=") {
            target_fn = name(v, &mut names)?;
        } else if let Some(v) = line.strip_prefix("calls=") {
            calls = Some(
                v.split_whitespace()
                    .next()
                    .ok_or(Error::ProfileInvalid)?
                    .parse::<u64>()
                    .map_err(|_| Error::ProfileInvalid)?,
            );
        } else if let Some(v) = line.strip_prefix("summary: ") {
            let v = v
                .split_whitespace()
                .map(str::parse::<i64>)
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(|_| Error::ProfileInvalid)?;
            if v.len() != 2 || v[0] < 0 {
                return Err(Error::ProfileInvalid);
            }
            summary = Some((v[0] as f64 / 100000.0, v[1]));
        } else if line
            .as_bytes()
            .first()
            .is_some_and(|b| b.is_ascii_digit() || b"+-*".contains(b))
        {
            let v = line.split_whitespace().collect::<Vec<_>>();
            if v.len() != 3 || function.is_empty() {
                return Err(Error::ProfileInvalid);
            }
            let time = v[1].parse::<u64>().map_err(|_| Error::ProfileInvalid)? as f64 / 100000.0;
            let memory = v[2].parse::<i64>().map_err(|_| Error::ProfileInvalid)?;
            let f = functions
                .entry((file.clone(), function.clone()))
                .or_insert_with(|| Function {
                    file: file.clone(),
                    name: function.clone(),
                    ..Default::default()
                });
            f.inclusive_ms += time;
            if let Some(n) = calls.take() {
                let other = functions
                    .entry((target_file.clone(), target_fn.clone()))
                    .or_insert_with(|| Function {
                        file: target_file.clone(),
                        name: target_fn.clone(),
                        ..Default::default()
                    });
                other.calls = other.calls.saturating_add(n);
            } else {
                f.self_ms += time;
                f.memory_bytes = f.memory_bytes.saturating_add(memory);
            }
            if functions.len() > 50000 {
                return Err(Error::ProfileLimit);
            }
        }
    }
    if !valid || functions.is_empty() {
        return Err(Error::ProfileInvalid);
    }
    let total = summary.unwrap_or_else(|| {
        (
            functions.values().map(|f| f.self_ms).sum(),
            functions
                .values()
                .fold(0i64, |total, f| total.saturating_add(f.memory_bytes)),
        )
    });
    let mut out = functions.into_values().collect::<Vec<_>>();
    out.sort_by(|a, b| b.self_ms.total_cmp(&a.self_ms));
    let truncated = out.len() > 500;
    out.truncate(500);
    Ok(Profile {
        total_ms: total.0,
        memory_bytes: total.1,
        functions: out,
        truncated,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn call_edges_do_not_double_count_flat_self_cost() {
        let p=parse(b"events: Time_(10ns) Memory_(bytes)\nfl=(1) /project/a.php\nfn=(1) main\n1 100000 8\ncfl=(1)\ncfn=(2) work\ncalls=2 2\n1 300000 4\nfn=(2)\n2 300000 4\nsummary: 400000 12\n").unwrap();
        assert_eq!(p.total_ms, 4.0);
        let main = p.functions.iter().find(|f| f.name == "main").unwrap();
        assert_eq!(main.self_ms, 1.0);
        assert_eq!(main.inclusive_ms, 4.0);
        assert_eq!(
            p.functions.iter().find(|f| f.name == "work").unwrap().calls,
            2
        );
        assert!(parse(b"events: unrecognised\n").is_err());
    }
}
