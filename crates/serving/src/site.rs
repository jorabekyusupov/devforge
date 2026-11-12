use crate::{Result, ServingError as E, atomic, private_dir};
use devforge_projects::project::ProjectStore;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    fs::{self, File, OpenOptions},
    net::TcpListener,
    path::{Path, PathBuf},
};
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Site {
    pub project_id: String,
    pub domain: String,
    pub https: bool,
    pub vite: bool,
}
impl Site {
    pub fn alias(&self) -> String {
        self.domain
            .strip_suffix(".devforge.test")
            .map(|p| format!("{p}.localhost"))
            .unwrap_or_else(|| self.domain.clone())
    }
    pub fn vite_domain(&self) -> String {
        format!("vite-{}", self.domain)
    }
    pub fn vite_alias(&self) -> String {
        format!("vite-{}", self.alias())
    }
    pub fn names(&self) -> Vec<String> {
        vec![
            self.domain.clone(),
            self.alias(),
            self.vite_domain(),
            self.vite_alias(),
        ]
    }
}
pub fn domain(value: &str) -> Result<()> {
    if value.len() > 230
        || !value.ends_with(".devforge.test")
        || value != value.to_ascii_lowercase()
    {
        return Err(E::InvalidDomain);
    }
    for label in value.split('.') {
        if label.is_empty()
            || label.len() > 63
            || label.starts_with('-')
            || label.ends_with('-')
            || !label
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        {
            return Err(E::InvalidDomain);
        }
    }
    Ok(())
}
pub(crate) fn certificate_name(value: &str) -> bool {
    domain(value).is_ok()
        || (value.ends_with(".localhost")
            && domain(&value.replace(".localhost", ".devforge.test")).is_ok())
}
#[derive(Serialize, Deserialize)]
struct Model {
    schema: u32,
    owner: String,
    sites: Vec<Site>,
}
pub struct SiteStore {
    root: PathBuf,
}
impl SiteStore {
    pub fn open(root: impl AsRef<Path>) -> Result<Self> {
        fs::create_dir_all(root.as_ref())?;
        let root = fs::canonicalize(root)?;
        let path = root.join("sites.json");
        if !path.exists() {
            if fs::read_dir(&root)?.next().is_some() {
                return Err(E::StorageUnavailable);
            }
            private_dir(&root)?;
            atomic(
                &path,
                &serde_json::to_vec(&Model {
                    schema: 1,
                    owner: crate::id(),
                    sites: Vec::new(),
                })
                .unwrap(),
            )?;
        }
        let store = Self { root };
        store.model()?;
        Ok(store)
    }
    pub fn root(&self) -> &Path {
        &self.root
    }
    fn model(&self) -> Result<Model> {
        let bytes = fs::read(self.root.join("sites.json"))?;
        if bytes.len() > 1024 * 1024 {
            return Err(E::StorageUnavailable);
        }
        let m: Model = serde_json::from_slice(&bytes).map_err(|_| E::StorageUnavailable)?;
        if m.schema != 1 || !owner(&m.owner) {
            return Err(E::StorageUnavailable);
        }
        let mut names = HashSet::new();
        for site in &m.sites {
            domain(&site.domain)?;
            for name in site.names() {
                if !names.insert(name) {
                    return Err(E::DomainConflict);
                }
            }
        }
        Ok(m)
    }
    pub fn owner(&self) -> Result<String> {
        Ok(self.model()?.owner)
    }
    pub fn sites(&self) -> Result<Vec<Site>> {
        Ok(self.model()?.sites)
    }
    pub fn configure(&self, projects: &ProjectStore, site: Site) -> Result<()> {
        projects.get(&site.project_id)?;
        domain(&site.domain)?;
        domain(&site.vite_domain())?;
        let _lock = self.lock("store.lock")?;
        let mut model = self.model()?;
        let wanted: HashSet<_> = site.names().into_iter().collect();
        if model
            .sites
            .iter()
            .filter(|s| s.project_id != site.project_id)
            .any(|s| s.names().iter().any(|name| wanted.contains(name)))
        {
            return Err(E::DomainConflict);
        }
        model.sites.retain(|s| s.project_id != site.project_id);
        model.sites.push(site);
        atomic(
            &self.root.join("sites.json"),
            &serde_json::to_vec_pretty(&model).unwrap(),
        )
    }
    pub fn remove(&self, project_id: &str) -> Result<()> {
        let _lock = self.lock("store.lock")?;
        let mut m = self.model()?;
        m.sites.retain(|s| s.project_id != project_id);
        atomic(
            &self.root.join("sites.json"),
            &serde_json::to_vec_pretty(&m).unwrap(),
        )
    }
    /// Holds the same project-session lease as serving while offline setup mutates configuration.
    pub fn project_mutation_guard(&self, id: &str) -> Result<File> {
        if id.len() != 32 || !id.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(E::InvalidDomain);
        }
        self.lock(&format!("session-{id}.lock"))
    }
    pub(crate) fn lock(&self, name: &str) -> Result<File> {
        let f = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(self.root.join(name))?;
        f.try_lock().map_err(|_| E::PortUnavailable)?;
        Ok(f)
    }
}
pub(crate) fn owner(value: &str) -> bool {
    value.len() == 32 && value.bytes().all(|b| b.is_ascii_hexdigit())
}
#[derive(Debug, Clone, Serialize)]
pub struct PortChoice {
    pub requested: u16,
    pub selected: u16,
    pub fallback: bool,
    pub permission_denied: bool,
}
pub(crate) struct PortReservation {
    pub choice: PortChoice,
    _socket: TcpListener,
}
impl PortReservation {
    pub fn choose(preferred: u16, fallback: u16) -> Result<Self> {
        match TcpListener::bind(("127.0.0.1", preferred)) {
            Ok(socket) => Ok(Self {
                choice: PortChoice {
                    requested: preferred,
                    selected: socket.local_addr()?.port(),
                    fallback: false,
                    permission_denied: false,
                },
                _socket: socket,
            }),
            Err(error) => {
                let denied = error.kind() == std::io::ErrorKind::PermissionDenied;
                let socket = TcpListener::bind(("127.0.0.1", fallback))
                    .or_else(|_| TcpListener::bind(("127.0.0.1", 0)))
                    .map_err(|_| E::PortUnavailable)?;
                Ok(Self {
                    choice: PortChoice {
                        requested: preferred,
                        selected: socket.local_addr()?.port(),
                        fallback: true,
                        permission_denied: denied,
                    },
                    _socket: socket,
                })
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn domains_are_owned_exact_hostnames_not_configuration_text() {
        for bad in [
            "example.com",
            "foo.test",
            "Foo.devforge.test",
            "*.devforge.test",
            "a.devforge.test\nadmin on",
            "-a.devforge.test",
            "a..devforge.test",
        ] {
            assert!(domain(bad).is_err(), "{bad}");
        }
        assert!(domain("project-a.devforge.test").is_ok());
    }
    #[test]
    fn conflict_uses_a_different_loopback_port_without_stopping_listener() {
        let old = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = old.local_addr().unwrap().port();
        let chosen = PortReservation::choose(port, 0).unwrap();
        assert!(chosen.choice.fallback);
        assert_ne!(chosen.choice.selected, port);
        assert!(std::net::TcpStream::connect(("127.0.0.1", port)).is_ok());
    }
}
