//! Read-only external dumps. Passwords enter only private option files, never argv or logs.
use super::*;
#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct Source {
    pub host: String,
    pub port: u16,
    pub database: String,
    pub user: String,
    pub password_ref: String,
    pub tls_ca: Option<PathBuf>,
    #[serde(default)]
    pub loopback_unencrypted: bool,
}
impl Source {
    pub fn validate(&self) -> Result<()> {
        if self.host.is_empty()
            || self.host.len() > 253
            || self.port == 0
            || self.host.starts_with(['-', '.'])
            || !self
                .host
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b".:-".contains(&b))
            || [&self.database, &self.user].iter().any(|s| {
                s.is_empty()
                    || s.len() > 128
                    || s.starts_with('-')
                    || s.contains(['\r', '\n', '\0'])
            })
            || !crate::manifest::alias(&self.password_ref)
        {
            return Err(E::InvalidRemoteSource);
        }
        if !self
            .database
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
            || self.database.len() > 63
            || (self.host.contains(':') && self.host.parse::<std::net::IpAddr>().is_err())
        {
            return Err(E::InvalidRemoteSource);
        }
        if self.loopback_unencrypted {
            // Names may resolve elsewhere; insecure mode accepts literal loopback addresses only.
            if !self
                .host
                .parse::<std::net::IpAddr>()
                .is_ok_and(|ip| ip.is_loopback())
                || self.tls_ca.is_some()
            {
                return Err(E::InvalidRemoteSource);
            }
        } else {
            let ca = self.tls_ca.as_deref().ok_or(E::InvalidRemoteSource)?;
            if !ca.is_absolute() {
                return Err(E::InvalidRemoteSource);
            }
            crate::manifest::read_file(ca, 1024 * 1024).map_err(|_| E::InvalidRemoteSource)?;
        }
        Ok(())
    }
}
fn mysql_quote(s: &str) -> String {
    format!(
        "\"{}\"",
        s.replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\r', "\\r")
            .replace('\n', "\\n")
            .replace('\t', "\\t")
    )
}
fn pg_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace(':', "\\:")
}
pub(super) fn options(s: &Source, c: &Credentials, dir: &Path) -> Result<tempfile::NamedTempFile> {
    s.validate()?;
    if c.password.contains(['\0', '\n', '\r']) {
        return Err(E::InvalidRemoteSource);
    }
    let mut f = tempfile::NamedTempFile::new_in(dir)?;
    let text = Zeroizing::new(if c.engine == Engine::Postgresql {
        format!(
            "{}:{}:{}:{}:{}\n",
            pg_escape(&s.host),
            s.port,
            pg_escape(&s.database),
            pg_escape(&s.user),
            pg_escape(&c.password)
        )
    } else {
        format!(
            "[client]\nhost={}\nport={}\nuser={}\npassword={}\nprotocol=TCP\nssl-mode={}\n{}",
            mysql_quote(&s.host),
            s.port,
            mysql_quote(&s.user),
            mysql_quote(&c.password),
            if s.loopback_unencrypted {
                "DISABLED"
            } else {
                "VERIFY_IDENTITY"
            },
            s.tls_ca
                .as_ref()
                .map(|p| format!("ssl-ca={}\n", mysql_quote(&p.to_string_lossy())))
                .unwrap_or_else(|| "get-server-public-key=TRUE\n".into())
        )
    });
    f.write_all(text.as_bytes())?;
    f.as_file().sync_all()?;
    Ok(f)
}
impl ProjectStore {
    #[allow(
        clippy::too_many_arguments,
        reason = "External dump uses the same verified runtime and cancellation boundary as local backup"
    )]
    pub fn create_remote_backup(
        &self,
        project: &str,
        source: &Source,
        password: Zeroizing<String>,
        catalog: &Catalog,
        packages: &PackageStore,
        secrets: &dyn SecretStore,
        cancel: &Cancellation,
    ) -> Result<Manifest> {
        source.validate()?;
        let mut c = self.credentials(project, secrets)?;
        let instance = self.instance(&c.binding.instance_id)?;
        let package = catalog.get(&instance.package)?;
        c.port = source.port;
        c.binding.database = source.database.clone();
        c.binding.role = source.user.clone();
        c.password = password;
        let temp = tempfile::tempdir_in(self.backup_root(project)?)?;
        let payload = temp.path().join("payload");
        let (tool, args) = if c.engine == Engine::Postgresql {
            (
                "pg_dump",
                vec![
                    "--format=custom".into(),
                    "--no-owner".into(),
                    "--no-privileges".into(),
                    "--lock-wait-timeout=5s".into(),
                    format!("--file={}", dunce::simplified(&payload).display()).into(),
                    format!("--dbname={}", source.database).into(),
                ],
            )
        } else {
            (
                "mysqldump",
                vec![
                    "--single-transaction".into(),
                    "--skip-lock-tables".into(),
                    "--no-tablespaces".into(),
                    "--set-gtid-purged=OFF".into(),
                    "--skip-triggers".into(),
                    "--routines=FALSE".into(),
                    "--events=FALSE".into(),
                    format!("--result-file={}", dunce::simplified(&payload).display()).into(),
                    "--".into(),
                    source.database.clone().into(),
                ],
            )
        };
        run_connection(
            &c,
            &instance.package,
            tool,
            args,
            None,
            catalog,
            packages,
            temp.path(),
            cancel,
            Some(source),
        )?;
        self.publish_payload(project, &payload, c.engine, &package.version)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn external_transport_and_option_escaping() {
        let mut s = Source {
            host: "127.0.0.1".into(),
            port: 5432,
            database: "app_db".into(),
            user: "reader".into(),
            password_ref: "remote-password".into(),
            tls_ca: None,
            loopback_unencrypted: true,
        };
        assert!(s.validate().is_ok());
        s.host = "localhost".into();
        assert!(s.validate().is_err());
        s.host = "db.example.test".into();
        s.loopback_unencrypted = false;
        assert!(s.validate().is_err());
        assert_eq!(pg_escape("a:b\\c"), "a\\:b\\\\c");
        assert_eq!(mysql_quote("a\"b\\c\n"), "\"a\\\"b\\\\c\\n\"");
    }
}
