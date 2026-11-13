//! Standard Laravel SMTP overrides; plaintext credential never enters project JSON.
use crate::{
    ProjectError as E, Result,
    project::{Project, ProjectStore},
};
use devforge_core::SecretStore;
use std::{collections::BTreeMap, ffi::OsString};
pub const KEYS: [&str; 8] = [
    "MAIL_MAILER",
    "MAIL_SCHEME",
    "MAIL_ENCRYPTION",
    "MAIL_HOST",
    "MAIL_PORT",
    "MAIL_USERNAME",
    "MAIL_PASSWORD",
    "MAIL_URL",
];
pub fn reference(id: &str) -> String {
    format!("mail/{id}/smtp")
}
pub fn save(store: &ProjectStore, id: &str, enabled: bool) -> Result<()> {
    let _lock = store.operation_lock()?;
    let mut p = store.get(id)?;
    let revision = p.revision;
    p.mail = enabled;
    store.save(&mut p, revision)
}
pub(crate) fn apply(
    store: &ProjectStore,
    p: &Project,
    secrets: &dyn SecretStore,
    env: &mut BTreeMap<OsString, OsString>,
    sensitive: &mut Vec<String>,
) -> Result<()> {
    if !p.mail {
        return Ok(());
    }
    let session = devforge_inspector::mail::read_session(store.root(), &p.id)
        .map_err(|_| E::MailUnavailable)?
        .ok_or(E::MailUnavailable)?;
    let password = zeroize::Zeroizing::new(
        secrets
            .get(&reference(&p.id))
            .map_err(|_| E::SecretUnavailable)?
            .ok_or(E::SecretUnavailable)?,
    );
    sensitive.push(password.to_string());
    for (k, v) in [
        ("MAIL_MAILER", "smtp".into()),
        ("MAIL_SCHEME", "smtp".into()),
        ("MAIL_ENCRYPTION", "(null)".into()),
        ("MAIL_HOST", "127.0.0.1".into()),
        ("MAIL_PORT", session.port.to_string()),
        ("MAIL_USERNAME", p.id.clone()),
        ("MAIL_PASSWORD", password.to_string()),
        ("MAIL_URL", "(null)".into()),
    ] {
        env.insert(k.into(), v.into());
    }
    Ok(())
}
