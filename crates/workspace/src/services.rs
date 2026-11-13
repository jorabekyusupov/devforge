use super::*;
use devforge_projects::cache::{CacheInstance, RunningCache};
#[derive(Clone, Serialize)]
pub struct Cache {
    pub instance: CacheInstance,
    pub state: String,
}
impl Workspace {
    pub(crate) fn activate_generation(
        &mut self,
        id: &str,
        instance: &str,
        database: &str,
        consent: bool,
    ) -> Result<serde_json::Value> {
        self.require_stopped(id)?;
        if !consent {
            return Err(error("service_consent_required"));
        }
        let binding = self
            .projects
            .binding_history(id)?
            .into_iter()
            .find(|b| b.ready && b.instance_id == instance && b.database == database)
            .ok_or_else(|| error("database_unbound"))?;
        self.ensure_database(instance)?;
        if !self
            .projects
            .credentials_for(binding.clone(), self.secrets.as_ref())?
            .connection_test()?
        {
            return Err(error("database_unbound"));
        }
        self.projects.activate_binding(id, Some(&binding))?;
        Ok(serde_json::json!({"activated":true}))
    }

    pub(crate) fn install_redis_client(
        &mut self,
        id: &str,
        consent: bool,
        cancel: &Cancellation,
    ) -> Result<serde_json::Value> {
        self.require_stopped(id)?;
        if !consent {
            return Err(error("service_consent_required"));
        }
        let p = self.projects.get(id)?;
        let composer = self.dependency("composer")?;
        self.install_dependency(&composer, cancel)?;
        // Bootstrap deliberately omits service credentials and managed service overrides.
        let context = Context::bootstrap(
            &self.projects,
            &p.path,
            &p.php,
            &p.node,
            &self.catalog,
            &self.packages,
            self.secrets.as_ref(),
        )?;
        let result = context
            .invocation(
                Tool::Composer,
                [
                    "require",
                    "predis/predis:^3.7.1",
                    "--no-interaction",
                    "--no-scripts",
                    "--no-plugins",
                ]
                .into_iter()
                .map(Into::into)
                .collect(),
                &self.catalog,
                &self.packages,
            )?
            .run_cancellable(self.projects.root().join("client-install-logs"), cancel)?;
        if !result.success {
            return Err(error("redis_client_failed"));
        }
        Ok(serde_json::json!({"installed":true}))
    }

    pub(crate) fn require_stopped(&self, id: &str) -> Result<()> {
        self.projects.get(id)?;
        if self.sessions.contains_key(id) {
            return Err(error("stop_before_database_change"));
        }
        Ok(())
    }
    pub(crate) fn switch_database(
        &mut self,
        id: &str,
        instance: &str,
        consent: bool,
    ) -> Result<serde_json::Value> {
        self.require_stopped(id)?;
        if !consent {
            return Err(error("service_consent_required"));
        }
        if self
            .projects
            .binding(id)?
            .is_some_and(|b| b.instance_id == instance)
        {
            return Ok(serde_json::json!({"switched":false}));
        }
        self.ensure_database(instance)?;
        let binding = if let Some(b) = self.projects.retained_binding(id, instance)? {
            self.projects
                .credentials_for(b.clone(), self.secrets.as_ref())?
                .connection_test()?;
            b
        } else {
            self.databases.get_mut(instance).unwrap().provision_staged(
                &self.projects,
                id,
                self.secrets.as_ref(),
            )?
        };
        self.projects.activate_binding(id, Some(&binding))?;
        Ok(serde_json::json!({"switched":true}))
    }
    pub(crate) fn dependency(&self, kind: &str) -> Result<String> {
        self.catalog
            .packages
            .iter()
            .find(|(_, p)| p.kind == kind && Some(p.target) == Target::current().ok())
            .map(|(id, _)| id.clone())
            .ok_or_else(|| error("service_unavailable"))
    }
    pub(crate) fn install_dependency(&self, id: &str, cancel: &Cancellation) -> Result<()> {
        let p = self.catalog.get(id)?;
        let installed = if let Some(name) = p.executables.keys().next() {
            self.packages.executable(p, name).is_ok()
        } else if let Some(name) = p.resources.keys().next() {
            self.packages.resource(p, name).is_ok()
        } else {
            false
        };
        if !installed {
            self.packages.download(p, cancel, |_| {})?;
        }
        Ok(())
    }
    pub(crate) fn create_cache(
        &mut self,
        id: &str,
        package: &str,
        cancel: &Cancellation,
    ) -> Result<serde_json::Value> {
        self.require_stopped(id)?;
        let runtime = self.dependency("dotnet")?;
        self.install_dependency(&runtime, cancel)?;
        let i = self.projects.initialize_cache(
            id,
            package,
            &runtime,
            &self.catalog,
            &self.packages,
            self.secrets.as_ref(),
        )?;
        Ok(serde_json::to_value(i).unwrap())
    }
    pub(crate) fn ensure_cache(&mut self, id: &str) -> Result<()> {
        if !self.caches.contains_key(id) {
            let s = RunningCache::start(
                &self.projects,
                id,
                &self.catalog,
                &self.packages,
                self.secrets.as_ref(),
            )?;
            self.caches.insert(id.into(), s);
        }
        Ok(())
    }
    pub(crate) fn stop_cache(&mut self, id: &str) -> Result<serde_json::Value> {
        let instance = self.projects.cache_instance(id)?;
        if self.sessions.contains_key(&instance.project_id) {
            return Err(error("database_in_use"));
        }
        if let Some(mut s) = self.caches.remove(id) {
            s.stop()?;
        }
        Ok(serde_json::json!({"stopped":true}))
    }
    pub(crate) fn cache_snapshot(&self) -> Result<Vec<Cache>> {
        Ok(self
            .projects
            .cache_instances()?
            .into_iter()
            .map(|instance| {
                let state = self
                    .caches
                    .get(&instance.id)
                    .and_then(|s| s.status())
                    .map(|s| format!("{:?}", s.observed).to_lowercase())
                    .unwrap_or_else(|| "stopped".into());
                Cache { instance, state }
            })
            .collect())
    }
    pub(crate) fn create_backup(
        &mut self,
        id: &str,
        cancel: &Cancellation,
    ) -> Result<serde_json::Value> {
        self.require_stopped(id)?;
        let b = self
            .projects
            .binding(id)?
            .ok_or_else(|| error("database_unbound"))?;
        self.ensure_database(&b.instance_id)?;
        if self.projects.instance(&b.instance_id)?.engine
            == devforge_projects::database::Engine::Postgresql
        {
            let tools = self.dependency("postgresql-tools")?;
            self.install_dependency(&tools, cancel)?;
        }
        let m = self.projects.create_backup(
            id,
            &self.catalog,
            &self.packages,
            self.secrets.as_ref(),
            cancel,
        )?;
        Ok(serde_json::to_value(m).unwrap())
    }
    pub(crate) fn restore_backup(
        &mut self,
        id: &str,
        backup: &str,
        consent: bool,
        cancel: &Cancellation,
    ) -> Result<serde_json::Value> {
        self.require_stopped(id)?;
        if !consent {
            return Err(error("service_consent_required"));
        }
        let b = self
            .projects
            .binding(id)?
            .ok_or_else(|| error("database_unbound"))?;
        self.ensure_database(&b.instance_id)?;
        if self.projects.instance(&b.instance_id)?.engine
            == devforge_projects::database::Engine::Postgresql
        {
            let tools = self.dependency("postgresql-tools")?;
            self.install_dependency(&tools, cancel)?;
        }
        let server = self.databases.get_mut(&b.instance_id).unwrap();
        let b = self.projects.restore_backup(
            id,
            backup,
            server,
            &self.catalog,
            &self.packages,
            self.secrets.as_ref(),
            cancel,
        )?;
        Ok(serde_json::json!({"restored":true,"database":b.database}))
    }
}
