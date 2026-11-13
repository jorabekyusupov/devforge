use super::*;
use devforge_inspector::mail::{MailServer, MailStore};
impl Workspace {
    pub(crate) fn ensure_mail(&mut self, id: &str) -> Result<()> {
        self.projects.get(id)?;
        if !self.mail.contains_key(id) {
            let key = devforge_projects::mail::reference(id);
            let password = if let Some(p) = self
                .secrets
                .get(&key)
                .map_err(|_| error("mail_unavailable"))?
            {
                p
            } else {
                let p = format!("{}{}", devforge_serving::id(), devforge_serving::id());
                self.secrets
                    .set(&key, &p)
                    .map_err(|_| error("mail_unavailable"))?;
                p
            };
            let server = MailServer::start(self.projects.root(), id, password)
                .map_err(|_| error("mail_unavailable"))?;
            self.mail.insert(id.into(), server);
        }
        Ok(())
    }
    pub fn configure_mail(&mut self, id: &str, enabled: bool) -> Result<serde_json::Value> {
        self.require_stopped(id)?;
        if enabled {
            self.ensure_mail(id)?;
        } else {
            self.mail.remove(id);
        }
        devforge_projects::mail::save(&self.projects, id, enabled)?;
        Ok(serde_json::json!({"enabled":enabled}))
    }
    pub fn mail_view(&self, id: &str, query: &str, page: usize) -> Result<serde_json::Value> {
        let p = self.projects.get(id)?;
        let data = MailStore::open(self.projects.root(), id)
            .map_err(|_| error("mail_unavailable"))?
            .list(query, page)
            .map_err(|_| error("mail_unavailable"))?;
        let session = devforge_inspector::mail::read_session(self.projects.root(), id)
            .map_err(|_| error("mail_unavailable"))?;
        let port = devforge_inspector::mail::configured_port(self.projects.root(), id)
            .map_err(|_| error("mail_unavailable"))?;
        Ok(
            serde_json::json!({"enabled":p.mail,"running":session.is_some(),"port":port,"inbox":data}),
        )
    }
    pub fn mail_message(&self, id: &str, message: &str) -> Result<serde_json::Value> {
        self.projects.get(id)?;
        let detail = MailStore::open(self.projects.root(), id)
            .and_then(|s| s.detail(message))
            .map_err(|_| error("mail_unavailable"))?;
        Ok(serde_json::to_value(detail).unwrap())
    }
    pub fn mail_part(&self, id: &str, message: &str, index: usize) -> Result<serde_json::Value> {
        self.projects.get(id)?;
        let part = MailStore::open(self.projects.root(), id)
            .and_then(|s| s.part(message, index))
            .map_err(|_| error("mail_unavailable"))?;
        Ok(serde_json::to_value(part).unwrap())
    }
    pub fn logs_view(
        &self,
        id: &str,
        filter: &devforge_inspector::logs::Filter,
    ) -> Result<serde_json::Value> {
        self.projects.get(id)?;
        let view = devforge_inspector::logs::read(&self.root.join("inspector"), id, filter)
            .map_err(|_| error("inspection_unavailable"))?;
        Ok(serde_json::to_value(view).unwrap())
    }
    pub(crate) fn mail_delete(
        &self,
        id: &str,
        message: Option<&str>,
        consent: bool,
    ) -> Result<serde_json::Value> {
        self.projects.get(id)?;
        if !consent {
            return Err(error("service_consent_required"));
        }
        MailStore::open(self.projects.root(), id)
            .and_then(|s| s.delete(message))
            .map_err(|_| error("mail_unavailable"))?;
        Ok(serde_json::json!({"deleted":true}))
    }
    pub(crate) fn mail_save(
        &self,
        id: &str,
        message: &str,
        index: usize,
        path: &Path,
    ) -> Result<serde_json::Value> {
        self.projects.get(id)?;
        MailStore::open(self.projects.root(), id)
            .and_then(|s| s.save_part(message, index, path))
            .map_err(|_| error("mail_export_failed"))?;
        Ok(serde_json::json!({"saved":true}))
    }
}
