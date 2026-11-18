//! Bounded project metadata shared by import and watched-folder discovery.
use crate::{
    Result,
    manifest::read_file,
    project::{Inspection, inspect_manifest},
};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, fs, path::Path};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseHint {
    pub engine: String,
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Metadata {
    pub inspection: Inspection,
    pub extensions: BTreeSet<String>,
    pub database: Option<DatabaseHint>,
}

pub fn read(path: &Path) -> Result<Metadata> {
    let (inspection, composer) = inspect_manifest(path)?;
    let extensions: BTreeSet<_> = ["require", "require-dev"]
        .into_iter()
        .filter_map(|section| composer[section].as_object())
        .flat_map(|requirements| requirements.keys())
        .filter_map(|name| name.strip_prefix("ext-"))
        .filter(|name| {
            !name.is_empty()
                && name.len() <= 64
                && name
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
        })
        .map(str::to_ascii_lowercase)
        .take(128)
        .collect();
    let database = database_hint(&inspection.path, &extensions);
    Ok(Metadata {
        inspection,
        extensions,
        database,
    })
}

fn database_hint(root: &Path, extensions: &BTreeSet<String>) -> Option<DatabaseHint> {
    for source in [".env", ".env.example"] {
        let path = root.join(source);
        if fs::symlink_metadata(&path).is_err_and(|err| err.kind() == std::io::ErrorKind::NotFound)
        {
            continue;
        }
        let bytes = zeroize::Zeroizing::new(read_file(&path, 1024 * 1024).ok()?);
        let text = std::str::from_utf8(&bytes).ok()?;
        let mut values = text
            .trim_start_matches('\u{feff}')
            .lines()
            .filter_map(|line| {
                let line = line.trim().strip_prefix("export ").unwrap_or(line.trim());
                let (key, value) = line.split_once('=')?;
                (key.trim() == "DB_CONNECTION").then_some(value.trim())
            });
        let Some(value) = values.next() else {
            continue;
        };
        if values.next().is_some() {
            return None;
        }
        let value = value
            .split_once(" #")
            .map_or(value, |(value, _)| value)
            .trim()
            .trim_matches(['\'', '"']);
        let engine = match value {
            "mysql" | "mariadb" => "mysql",
            "pgsql" => "postgresql",
            "sqlite" => "sqlite",
            "sqlsrv" => "sqlsrv",
            _ => return None,
        };
        return Some(DatabaseHint {
            engine: engine.into(),
            source: source.into(),
        });
    }
    let mut engines = [
        ("pdo_mysql", "mysql"),
        ("pdo_pgsql", "postgresql"),
        ("pdo_sqlite", "sqlite"),
        ("pdo_sqlsrv", "sqlsrv"),
    ]
    .into_iter()
    .filter(|(extension, _)| extensions.contains(*extension));
    let (_, engine) = engines.next()?;
    engines.next().is_none().then_some(DatabaseHint {
        engine: engine.into(),
        source: "composer.json".into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn database_detection_prefers_local_env_and_refuses_ambiguous_hints() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join(".env.example"), "DB_CONNECTION=mysql\n").unwrap();
        let extensions = BTreeSet::from(["pdo_pgsql".into(), "pdo_mysql".into()]);
        assert_eq!(
            database_hint(root.path(), &extensions).unwrap().engine,
            "mysql"
        );
        fs::write(
            root.path().join(".env"),
            "PASSWORD=secret\nDB_CONNECTION='pgsql' # local\n",
        )
        .unwrap();
        let hint = database_hint(root.path(), &extensions).unwrap();
        assert_eq!(
            (hint.engine.as_str(), hint.source.as_str()),
            ("postgresql", ".env")
        );
        fs::write(
            root.path().join(".env"),
            "DB_CONNECTION=mysql\nDB_CONNECTION=sqlite\n",
        )
        .unwrap();
        assert!(database_hint(root.path(), &extensions).is_none());
    }
}
