//! Portable team configuration. Secret values, local paths and SQL data are not
//! part of this format. Only bundled publisher-reviewed packages may be pinned.
use crate::{ProjectError as E, Result, project::Catalog};
use devforge_runtime::package::Target;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fs, io::Read, path::Path};
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub schema: u32,
    #[serde(default)]
    pub mail: Option<bool>,
    pub project: Project,
    pub runtimes: Runtimes,
    #[serde(default)]
    pub database: Option<Database>,
    #[serde(default)]
    pub redis: Option<Redis>,
    #[serde(default)]
    pub services: Drivers,
    #[serde(default)]
    pub php_settings: crate::php_settings::PhpSettings,
    #[serde(default)]
    pub environment: BTreeMap<String, String>,
    #[serde(default)]
    pub setup: Preparation,
    pub pins: BTreeMap<String, String>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Project {
    pub name: String,
    pub domain: String,
    #[serde(default)]
    pub https: bool,
    #[serde(default)]
    pub vite: bool,
    #[serde(default)]
    pub queue: bool,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Runtimes {
    pub php: String,
    pub node: String,
    pub composer: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Database {
    pub engine: crate::database::Engine,
    pub version: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Redis {
    pub version: String,
    pub dotnet: String,
}
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Drivers {
    pub cache: crate::cache::CacheDriver,
    pub queue: crate::cache::QueueDriver,
}
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Preparation {
    pub composer: bool,
    pub npm: bool,
    pub create_env: bool,
    pub generate_key: bool,
    pub predis: bool,
}
pub fn alias(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"_-".contains(&b))
}
pub fn environment_name(name: &str) -> Result<()> {
    crate::environment::validate(name, "")?;
    if ["APP_KEY", "CACHE_STORE", "CACHE_DRIVER", "QUEUE_CONNECTION"].contains(&name)
        || name.starts_with("REDIS_")
    {
        return Err(E::InvalidEnvironment);
    }
    Ok(())
}
pub fn read_file(path: &Path, limit: u64) -> Result<Vec<u8>> {
    let m = fs::symlink_metadata(path)?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if m.file_attributes() & 0x400 != 0 {
            return Err(E::InvalidManifest);
        }
    }
    if !m.is_file() || m.file_type().is_symlink() || m.len() > limit {
        return Err(E::InvalidManifest);
    }
    let mut bytes = vec![];
    fs::File::open(path)?
        .take(limit + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(E::InvalidManifest);
    }
    Ok(bytes)
}
impl Manifest {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        if bytes.len() > 65536 {
            return Err(E::InvalidManifest);
        }
        let text = std::str::from_utf8(bytes).map_err(|_| E::InvalidManifest)?;
        let mut options = serde_saphyr::Options::default();
        let mut budget = serde_saphyr::Budget::default();
        budget.max_depth = 16;
        budget.max_nodes = 4096;
        budget.max_aliases = 0;
        budget.max_anchors = 0;
        budget.max_documents = 1;
        budget.max_total_scalar_bytes = 65536;
        options.budget = Some(budget);
        options.duplicate_keys = serde_saphyr::DuplicateKeyPolicy::Error;
        options.merge_keys = serde_saphyr::MergeKeyPolicy::Error;
        options.reject_unsupported_tags = true;
        options.reject_tagged_field_names = true;
        options.with_snippet = false;
        let manifest: Self =
            serde_saphyr::from_str_with_options(text, options).map_err(|_| E::InvalidManifest)?;
        manifest.validate()?;
        Ok(manifest)
    }
    pub fn yaml(&self) -> Result<String> {
        self.validate()?;
        serde_saphyr::to_string(self).map_err(|_| E::InvalidManifest)
    }
    pub fn validate(&self) -> Result<()> {
        crate::project::validate_name(&self.project.name)?;
        self.php_settings.validate()?;
        let exact = |v: &str| {
            v.len() <= 40
                && v.split('.').count() >= 3
                && v.split('.')
                    .all(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
        };
        if self.schema != 1
            || ![
                &self.runtimes.php,
                &self.runtimes.node,
                &self.runtimes.composer,
            ]
            .into_iter()
            .all(|v| exact(v))
            || self.database.as_ref().is_some_and(|d| !exact(&d.version))
            || self
                .redis
                .as_ref()
                .is_some_and(|r| !exact(&r.version) || !exact(&r.dotnet))
            || self.environment.len() > 64
            || self.pins.len() > 64
        {
            return Err(E::InvalidManifest);
        }
        if (self.services.cache == crate::cache::CacheDriver::Redis
            || self.services.queue == crate::cache::QueueDriver::Redis)
            && self.redis.is_none()
        {
            return Err(E::InvalidManifest);
        }
        for (name, reference) in &self.environment {
            environment_name(name)?;
            if !alias(reference) {
                return Err(E::InvalidManifest);
            }
        }
        for (id, hash) in &self.pins {
            if id.len() > 120
                || !id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
                || hash.len() != 64
                || !hash.bytes().all(|b| b.is_ascii_hexdigit())
            {
                return Err(E::InvalidManifest);
            }
        }
        Ok(())
    }
    pub fn resolve(&self, catalog: &Catalog, target: Target) -> Result<BTreeMap<String, String>> {
        let select = |kind: &str, version: &str, executable: Option<&str>| {
            catalog
                .packages
                .iter()
                .find(|(id, p)| {
                    (!catalog.superseded.contains_key(*id) || self.pins.get(*id) == Some(&p.sha256))
                        && p.kind == kind
                        && (if kind == "php" {
                            p.version.split('-').next().unwrap()
                        } else {
                            p.version.trim_start_matches('v')
                        }) == version
                        && p.target == target
                        && executable.is_none_or(|k| p.executables.contains_key(k))
                })
                .map(|(id, _)| id.clone())
                .ok_or(E::RuntimeUnavailable)
        };
        let php = select("php", &self.runtimes.php, Some("php"))?;
        let server = catalog.php_server(catalog.get(&php)?)?;
        let server_id = catalog
            .packages
            .iter()
            .find(|(_, p)| *p == server)
            .map(|(id, _)| id.clone())
            .ok_or(E::RuntimeUnavailable)?;
        let proxy = select(
            "caddy",
            catalog
                .defaults
                .get("caddy")
                .ok_or(E::RuntimeUnavailable)?
                .trim_start_matches('v'),
            Some("caddy"),
        )?;
        let mut roles = BTreeMap::from([
            ("php".into(), php),
            ("php_server".into(), server_id),
            (
                "node".into(),
                select("node", &self.runtimes.node, Some("node"))?,
            ),
            (
                "composer".into(),
                select("composer", &self.runtimes.composer, None)?,
            ),
        ]);
        roles.insert("proxy".into(), proxy);
        if let Some(d) = &self.database {
            roles.insert(
                "database".into(),
                catalog
                    .packages
                    .iter()
                    .find(|(_, p)| {
                        p.kind
                            == match d.engine {
                                crate::database::Engine::Postgresql => "postgresql",
                                crate::database::Engine::Mysql => "mysql",
                            }
                            && p.version == d.version
                            && p.variant == "server"
                            && p.target == target
                    })
                    .map(|(id, _)| id.clone())
                    .ok_or(E::RuntimeUnavailable)?,
            );
        }
        if let Some(r) = &self.redis {
            roles.insert(
                "redis".into(),
                select("garnet", &r.version, Some("garnet"))?,
            );
            roles.insert(
                "dotnet".into(),
                select("dotnet", &r.dotnet, Some("dotnet"))?,
            );
        }
        Ok(roles)
    }
    pub fn verify_pins(
        &self,
        catalog: &Catalog,
        target: Target,
    ) -> Result<BTreeMap<String, String>> {
        let roles = self.resolve(catalog, target)?;
        for id in roles.values() {
            if self.pins.get(id) != Some(&catalog.get(id)?.sha256) {
                return Err(E::RuntimeUnavailable);
            }
        }
        Ok(roles)
    }
    pub fn pin_all(&mut self, catalog: &Catalog) -> Result<()> {
        self.pins.clear();
        for target in [Target::MacosArm64, Target::MacosX64, Target::WindowsX64] {
            if let Ok(roles) = self.resolve(catalog, target) {
                for id in roles.values() {
                    self.pins
                        .insert(id.clone(), catalog.get(id)?.sha256.clone());
                }
            }
        }
        if self.pins.is_empty() {
            return Err(E::RuntimeUnavailable);
        }
        Ok(())
    }
}
