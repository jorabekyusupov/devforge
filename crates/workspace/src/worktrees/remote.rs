//! External read-only database copy into an owned workspace's staged local generation.
use super::*;
use devforge_projects::backup::remote::Source;
use zeroize::Zeroizing;
impl Workspace {
    fn remote_password(&self, project: &Project, source: &Source) -> Result<Zeroizing<String>> {
        source.validate()?;
        self.manifest_secret_value(&project.path, &source.password_ref)
    }
    pub fn worktree_remote_plan(&self, id: &str, source: &Source) -> Result<serde_json::Value> {
        let record = self.worktree_record(id)?;
        if record.stage != "ready" {
            return Err(error("worktree_archive_unavailable"));
        }
        let project = self.projects.get(
            record
                .project_id
                .as_deref()
                .ok_or_else(|| error("worktree_ownership"))?,
        )?;
        self.worktree_identity(&record, &project.path)?;
        self.require_stopped(&project.id)?;
        let password = self.remote_password(&project, source)?;
        let binding = self
            .projects
            .binding(&project.id)?
            .ok_or_else(|| error("database_unbound"))?;
        let instance = self.projects.instance(&binding.instance_id)?;
        let ca = source
            .tls_ca
            .as_ref()
            .map(|p| devforge_projects::manifest::read_file(p, 1024 * 1024))
            .transpose()?;
        let digest = digest(&(
            &record,
            &project,
            &binding,
            &instance,
            source,
            digest(&password.as_str()),
            ca,
        ));
        Ok(
            serde_json::json!({"digest":digest,"project":project.id,"engine":instance.engine,"source":source,"retained_database":binding.database,"tables_only":instance.engine==devforge_projects::database::Engine::Mysql}),
        )
    }
    pub fn worktree_remote_copy(
        &mut self,
        id: &str,
        source: &Source,
        review: &str,
        consent: bool,
        cancel: &Cancellation,
    ) -> Result<serde_json::Value> {
        let _lock = self.worktree_lock()?;
        if !consent {
            return Err(error("service_consent_required"));
        }
        let plan = self.worktree_remote_plan(id, source)?;
        if plan["digest"] != review {
            return Err(error("worktree_review_changed"));
        }
        let project = self.projects.get(plan["project"].as_str().unwrap())?;
        let password = self.remote_password(&project, source)?;
        let b = self
            .projects
            .binding(&project.id)?
            .ok_or_else(|| error("database_unbound"))?;
        self.ensure_database(&b.instance_id)?;
        if self.projects.instance(&b.instance_id)?.engine
            == devforge_projects::database::Engine::Postgresql
        {
            let tools = self.dependency("postgresql-tools")?;
            self.install_dependency(&tools, cancel)?;
        }
        let backup = self.projects.create_remote_backup(
            &project.id,
            source,
            password,
            &self.catalog,
            &self.packages,
            self.secrets.as_ref(),
            cancel,
        )?;
        // Recheck local ownership, secret and CA after network work, before activating data.
        if self.worktree_remote_plan(id, source)?["digest"] != review {
            return Err(error("worktree_review_changed"));
        }
        self.restore_backup(&project.id, &backup.id, true, cancel)
    }
}
