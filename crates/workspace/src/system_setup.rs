use super::*;
use devforge_serving::{
    authority::Authority,
    system::{self, Operation, Plan},
};
fn time() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
fn read(path: &Path) -> Result<Vec<u8>> {
    let meta = fs::symlink_metadata(path)?;
    if !meta.is_file() || meta.file_type().is_symlink() || meta.len() > 128 * 1024 {
        return Err(error("setup_review_changed"));
    }
    Ok(fs::read(path)?)
}
impl Workspace {
    pub fn https_preview(&self, id: &str) -> Result<serde_json::Value> {
        self.projects.get(id)?;
        let sites = self.sites.sites()?;
        let site = sites
            .iter()
            .find(|s| s.project_id == id)
            .ok_or_else(|| error("setup_site_missing"))?;
        let owner = self.sites.owner()?;
        let ca = Authority::open(
            self.sites.root().join("authority"),
            &owner,
            self.secrets.as_ref(),
        )?;
        let domains = sites
            .iter()
            .flat_map(|s| [s.domain.clone(), s.vite_domain()])
            .collect::<Vec<_>>();
        let hosts_path = system::hosts_path()?;
        let before = fs::read(&hosts_path)?;
        let hosts_ready = devforge_serving::hosts::patch(&before, &owner, &domains)? == before;
        Ok(
            serde_json::json!({"domain":site.domain,"https_enabled":site.https,"fingerprint":ca.fingerprint(),"system_trusted":system::trusted(ca.der())?,"hosts_path":hosts_path,"hosts_ready":hosts_ready,"domains":domains,"elevation_required":true,"helper_available":devforge_serving::authorize::helper_path().is_ok()}),
        )
    }
    pub fn https_export(&self, id: &str) -> Result<serde_json::Value> {
        let preview = self.https_preview(id)?;
        let owner = self.sites.owner()?;
        let ca = Authority::open(
            self.sites.root().join("authority"),
            &owner,
            self.secrets.as_ref(),
        )?;
        let plans = [
            system::hosts_plan(
                &owner,
                self.sites
                    .sites()?
                    .iter()
                    .flat_map(|s| [s.domain.clone(), s.vite_domain()])
                    .collect(),
            )?,
            Plan {
                schema: 1,
                owner: owner.clone(),
                action: Operation::TrustInstall {
                    certificate_der: ca.der().to_vec(),
                },
            },
        ];
        let root = self.root.join("system-setup");
        fs::create_dir_all(&root)?;
        let dir = tempfile::Builder::new().prefix("plan-").tempdir_in(root)?;
        let ticket = dir
            .path()
            .file_name()
            .unwrap()
            .to_string_lossy()
            .to_string();
        let mut output = vec![];
        for (index, plan) in plans.iter().enumerate() {
            let path = dir.path().join(format!("{index}.json"));
            let digest = plan.digest();
            plan.validate(&digest)?;
            fs::write(&path, serde_json::to_vec_pretty(plan).unwrap())?;
            output.push(serde_json::json!({"path":path,"digest":digest}));
        }
        let manifest = serde_json::json!({"schema":1,"project_id":id,"owner":owner,"created":time(),"preview":preview,"plans":output});
        let bytes = serde_json::to_vec(&manifest).unwrap();
        let digest = devforge_serving::hash(&bytes);
        fs::write(dir.path().join("review.json"), bytes)?;
        let _ = dir.keep();
        Ok(serde_json::json!({"preview":preview,"ticket":ticket,"digest":digest,"applied":false}))
    }
    pub fn https_apply(
        &self,
        id: &str,
        ticket: &str,
        digest: &str,
        consent: bool,
    ) -> Result<serde_json::Value> {
        if !consent
            || !ticket.starts_with("plan-")
            || ticket.len() > 64
            || !ticket
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'-')
            || digest.len() != 64
        {
            return Err(error("setup_system_consent"));
        }
        let dir = self.root.join("system-setup").join(ticket);
        let bytes = read(&dir.join("review.json"))?;
        if devforge_serving::hash(&bytes) != digest {
            return Err(error("setup_review_changed"));
        }
        let manifest: serde_json::Value =
            serde_json::from_slice(&bytes).map_err(|_| error("setup_review_changed"))?;
        if manifest["schema"] != 1
            || manifest["project_id"] != id
            || manifest["owner"] != self.sites.owner()?
            || manifest["created"]
                .as_u64()
                .is_none_or(|t| time() < t || time() - t > 600)
        {
            return Err(error("setup_review_expired"));
        }
        let current = self.https_preview(id)?;
        if current["https_enabled"] != true {
            return Err(error("setup_https_disabled"));
        }
        if current["fingerprint"] != manifest["preview"]["fingerprint"]
            || current["domains"] != manifest["preview"]["domains"]
        {
            return Err(error("setup_review_changed"));
        }
        let plans = manifest["plans"]
            .as_array()
            .filter(|p| p.len() == 2)
            .ok_or_else(|| error("setup_review_changed"))?;
        let helper = devforge_serving::authorize::helper_path()
            .map_err(|_| error("setup_helper_unavailable"))?;
        for (index, record) in plans.iter().enumerate() {
            let path = dir.join(format!("{index}.json"));
            if record["path"] != serde_json::to_value(&path).unwrap() {
                return Err(error("setup_review_changed"));
            }
            let plan: Plan =
                serde_json::from_slice(&read(&path)?).map_err(|_| error("setup_review_changed"))?;
            let digest = record["digest"]
                .as_str()
                .ok_or_else(|| error("setup_review_changed"))?;
            plan.validate(digest)?;
            if plan.owner != self.sites.owner()?
                || !matches!(
                    (&plan.action, index),
                    (Operation::Hosts { .. }, 0) | (Operation::TrustInstall { .. }, 1)
                )
            {
                return Err(error("setup_review_changed"));
            }
            devforge_serving::authorize::apply(&helper, &path, digest)?;
        }
        let preview = self.https_preview(id)?;
        if preview["hosts_ready"] != true || preview["system_trusted"] != true {
            return Err(error("setup_system_incomplete"));
        }
        Ok(serde_json::json!({"preview":preview,"ticket":ticket,"digest":digest,"applied":true}))
    }
}
