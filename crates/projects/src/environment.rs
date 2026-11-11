use crate::{
    ProjectError as E, Result,
    project::{Project, ProjectStore},
};
use devforge_core::SecretStore;
use std::{collections::BTreeMap, ffi::OsString};
use zeroize::Zeroizing;
pub(crate) fn key(project: &Project, name: &str) -> String {
    format!("project/{}/env/{name}", project.id)
}
pub fn set(
    store: &ProjectStore,
    secrets: &dyn SecretStore,
    id: &str,
    revision: u64,
    name: &str,
    value: &str,
) -> Result<Project> {
    let _lock = store.operation_lock()?;
    validate(name, value)?;
    let mut p = store.get(id)?;
    if p.environment_names.len() >= 64 && !p.environment_names.iter().any(|n| n == name) {
        return Err(E::InvalidEnvironment);
    }
    if p.revision != revision {
        return Err(E::StaleRevision);
    }
    let reference = key(&p, name);
    let old = secrets
        .get(&reference)
        .map_err(|_| E::SecretUnavailable)?
        .map(Zeroizing::new);
    secrets
        .set(&reference, value)
        .map_err(|_| E::SecretUnavailable)?;
    if !p.environment_names.iter().any(|v| v == name) {
        p.environment_names.push(name.into());
        p.environment_names.sort();
    }
    if let Err(error) = store.save(&mut p, revision) {
        match old {
            Some(old) => secrets.set(&reference, &old),
            None => secrets.delete(&reference),
        }
        .map_err(|_| E::SecretUnavailable)?;
        return Err(error);
    }
    Ok(p)
}
pub(crate) fn load(
    project: &Project,
    secrets: &dyn SecretStore,
) -> Result<BTreeMap<OsString, Zeroizing<String>>> {
    let mut values = BTreeMap::new();
    for name in &project.environment_names {
        validate(name, "")?;
        let value = secrets
            .get(&key(project, name))
            .map_err(|_| E::SecretUnavailable)?
            .ok_or(E::SecretUnavailable)?;
        values.insert(name.into(), Zeroizing::new(value));
    }
    Ok(values)
}
fn validate(name: &str, value: &str) -> Result<()> {
    let reserved = [
        "PATH",
        "PATHEXT",
        "PHPRC",
        "PHP_INI_SCAN_DIR",
        "HOME",
        "USERPROFILE",
        "APPDATA",
        "LOCALAPPDATA",
        "SYSTEMROOT",
        "WINDIR",
        "TEMP",
        "TMP",
        "SHELL",
        "COMSPEC",
        "BASH_ENV",
        "ENV",
        "ZDOTDIR",
        "NODE_OPTIONS",
        "COMPOSER_HOME",
        "COMPOSER_CACHE_DIR",
        "NPM_CONFIG_CACHE",
        "NPM_CONFIG_USERCONFIG",
        "DATABASE_URL",
        "NODE_PATH",
        "NODE_EXTRA_CA_CERTS",
        "PYTHONPATH",
        "NO_DEFAULT_CURRENT_DIRECTORY_IN_EXE_PATH",
    ];
    if name.is_empty()
        || name.len() > 100
        || !name
            .bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
        || name.as_bytes()[0].is_ascii_digit()
        || reserved.contains(&name)
        || ["DB_", "PG", "LD_", "DYLD_"]
            .iter()
            .any(|p| name.starts_with(p))
        || value.len() > 4096
        || value.contains('\0')
    {
        return Err(E::InvalidEnvironment);
    }
    Ok(())
}
pub(crate) fn base() -> BTreeMap<OsString, OsString> {
    let mut env = BTreeMap::new();
    for key in [
        "SystemRoot",
        "WINDIR",
        "TEMP",
        "TMP",
        "HOME",
        "USERPROFILE",
        "APPDATA",
        "LOCALAPPDATA",
    ] {
        if let Some(value) = std::env::var_os(key) {
            env.insert(key.into(), value);
        }
    }
    env.insert("LC_ALL".into(), "C".into());
    env.insert("LANG".into(), "C".into());
    #[cfg(windows)]
    {
        if let Some(root) = std::env::var_os("SystemRoot") {
            env.insert(
                "COMSPEC".into(),
                std::path::PathBuf::from(root)
                    .join("System32")
                    .join("cmd.exe")
                    .into_os_string(),
            );
        }
        env.insert("PATHEXT".into(), ".COM;.EXE;.BAT;.CMD".into());
        env.insert("NoDefaultCurrentDirectoryInExePath".into(), "1".into());
    }
    env
}
