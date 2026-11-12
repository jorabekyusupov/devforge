//! Active connections and retained generations. Physical databases are never
//! deleted when switching a connection or activating a restored generation.
use crate::{
    ProjectError as E, Result,
    database::{Binding, Credentials},
    project::ProjectStore,
};
use devforge_core::SecretStore;
use rusqlite::params;
use zeroize::Zeroizing;
impl ProjectStore {
    pub fn binding_history(&self, id: &str) -> Result<Vec<Binding>> {
        self.get(id)?;
        let mut query = self.connection.prepare(
            "SELECT record FROM database_binding_history WHERE project_id=?1 ORDER BY rowid DESC",
        )?;
        query
            .query_map([id], |r| r.get::<_, String>(0))?
            .map(|r| serde_json::from_str(&r?).map_err(|_| E::StorageUnavailable))
            .collect()
    }
    pub fn retained_binding(&self, project: &str, instance: &str) -> Result<Option<Binding>> {
        Ok(self
            .binding_history(project)?
            .into_iter()
            .find(|b| b.instance_id == instance && b.ready))
    }
    pub fn retain_binding(&self, b: &Binding) -> Result<()> {
        self.get(&b.project_id)?;
        self.instance(&b.instance_id)?;
        self.connection.execute(
            "INSERT OR REPLACE INTO database_binding_history VALUES(?1,?2,?3)",
            params![
                b.project_id,
                format!("{}/{}", b.instance_id, b.database),
                serde_json::to_string(b).map_err(|_| E::StorageUnavailable)?
            ],
        )?;
        Ok(())
    }
    pub fn activate_binding(&self, id: &str, next: Option<&Binding>) -> Result<()> {
        let _lock = self.operation_lock()?;
        self.get(id)?;
        if next.is_some_and(|b| b.project_id != id) {
            return Err(E::DatabaseConflict);
        }
        if next.is_some_and(|b| !b.ready) {
            return Err(E::RecoveryRequired);
        }
        self.connection.execute_batch("BEGIN IMMEDIATE")?;
        let result = (|| {
            if let Some(old) = self.binding(id)? {
                self.retain_binding(&old)?;
            }
            self.connection
                .execute("DELETE FROM database_bindings WHERE project_id=?1", [id])?;
            if let Some(b) = next {
                self.connection.execute(
                    "INSERT INTO database_bindings VALUES(?1,?2,?3)",
                    params![
                        id,
                        b.instance_id,
                        serde_json::to_string(b).map_err(|_| E::StorageUnavailable)?
                    ],
                )?;
            }
            Ok(())
        })();
        if result.is_ok() {
            self.connection.execute_batch("COMMIT")?;
        } else {
            let _ = self.connection.execute_batch("ROLLBACK");
        }
        result
    }
    pub fn credentials_for(
        &self,
        binding: Binding,
        secrets: &dyn SecretStore,
    ) -> Result<Credentials> {
        let instance = self.instance(&binding.instance_id)?;
        let password = Zeroizing::new(
            secrets
                .get(&binding.secret_ref)
                .map_err(|_| E::SecretUnavailable)?
                .ok_or(E::SecretUnavailable)?,
        );
        Ok(Credentials {
            engine: instance.engine,
            root: self.root.join("databases").join(&instance.id),
            binding,
            port: instance.port,
            password,
        })
    }
    pub fn staged_binding(
        &self,
        project: &str,
        instance: &str,
        secrets: &dyn SecretStore,
    ) -> Result<Binding> {
        self.get(project)?;
        let engine = self.instance(instance)?.engine;
        let nonce = crate::id();
        let suffix = &nonce[..8];
        let b = Binding {
            project_id: project.into(),
            instance_id: instance.into(),
            database: format!("df_{project}_{suffix}"),
            role: if engine == crate::database::Engine::Postgresql {
                format!("df_{project}_{suffix}")
            } else {
                format!("df_{}_{}", &project[..20.min(project.len())], suffix)
            },
            secret_ref: format!("project/{project}/database/{instance}/{nonce}"),
            ready: false,
        };
        secrets
            .set(&b.secret_ref, &crate::password())
            .map_err(|_| E::SecretUnavailable)?;
        self.retain_binding(&b)?;
        Ok(b)
    }
    pub(crate) fn save_instance(&self, instance: &crate::database::Instance) -> Result<()> {
        self.connection.execute(
            "INSERT OR REPLACE INTO instances VALUES(?1,?2)",
            params![
                instance.id,
                serde_json::to_string(instance).map_err(|_| E::StorageUnavailable)?
            ],
        )?;
        Ok(())
    }
}
