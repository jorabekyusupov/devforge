//! Project-safe metadata and private fixed-system plans. No observer path opens
//! a CA/key store, creates files, probes system trust or launches authorization.
use super::*;
use devforge_serving::{
    authority, hosts,
    system::{self, Operation, Plan as SystemPlan},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    time::{Duration, Instant},
};

const LIFETIME: Duration = Duration::from_secs(300);
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DomainPlanKind {
    Hosts,
    Trust,
    Cleanup,
}

impl DomainPlanKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Hosts => "hosts",
            Self::Trust => "trust",
            Self::Cleanup => "cleanup",
        }
    }
}
#[derive(Clone, Serialize)]
pub struct DomainFacts {
    pub project_id: String,
    pub revision: u64,
    pub domain: String,
    pub https_enabled: bool,
    pub preferred_url: Option<String>,
    pub http_port: Option<u16>,
    pub https_port: Option<u16>,
    pub certificate_active: bool,
    pub ca_prepared: bool,
    pub ca_fingerprint: Option<String>,
    pub system_trusted: Option<bool>,
}
/// Native-owner review only: retained host names can belong to other projects.
/// This summary is never an external project-agent projection or authority.
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NativeDomainReviewSummary {
    pub steps: Vec<NativeSystemStep>,
    pub hosts: Option<NativeHostsDelta>,
    pub ca_fingerprint: String,
    pub shared_ca_preserved: bool,
    pub remaining_ms: u64,
}
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NativeSystemStep {
    pub operation: NativeSystemOperation,
    pub target: NativeSystemTarget,
}
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NativeSystemOperation {
    HostsUpdate,
    TrustInstall,
}
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NativeSystemTarget {
    SystemHosts,
    SystemTrustStore,
}
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NativeHostsDelta {
    pub added: Vec<String>,
    pub removed: Vec<String>,
    pub retained: Vec<String>,
}
/// Manager-private: never serialize raw hosts hashes, owner domains or plan DER.
#[derive(Clone)]
pub struct DomainPlan {
    kind: DomainPlanKind,
    native: SystemPlan,
    data_root: PathBuf,
    project_id: String,
    project_root: PathBuf,
    project_identity: String,
    revision: u64,
    owner: String,
    sites_hash: String,
    ca_hash: String,
    hosts_hash: String,
    original_hosts: Vec<String>,
    expires: Instant,
}
/// Mint only after the separate trusted native owner confirmation. Not a wire
/// value, not a project grant, and deliberately neither Clone nor Deserialize.
pub struct MachineConfirmation {
    fingerprint: String,
    expires: Instant,
    used: bool,
}
impl DomainPlan {
    pub fn native_review_summary(&self) -> Result<NativeDomainReviewSummary> {
        self.native.validate(&self.native.digest())?;
        let (operation, target, hosts) = match &self.native.action {
            Operation::Hosts { domains, .. } => {
                let before = self.original_hosts.iter().cloned().collect::<BTreeSet<_>>();
                let after = domains.iter().cloned().collect::<BTreeSet<_>>();
                (
                    NativeSystemOperation::HostsUpdate,
                    NativeSystemTarget::SystemHosts,
                    Some(NativeHostsDelta {
                        added: after.difference(&before).cloned().collect(),
                        removed: before.difference(&after).cloned().collect(),
                        retained: before.intersection(&after).cloned().collect(),
                    }),
                )
            }
            Operation::TrustInstall { .. } => (
                NativeSystemOperation::TrustInstall,
                NativeSystemTarget::SystemTrustStore,
                None,
            ),
            Operation::TrustRemove { .. } => {
                return Err(error("domain_review_summary_unavailable"));
            }
        };
        Ok(NativeDomainReviewSummary {
            steps: vec![NativeSystemStep { operation, target }],
            hosts,
            ca_fingerprint: self.ca_hash.clone(),
            shared_ca_preserved: true,
            remaining_ms: self
                .expires
                .saturating_duration_since(Instant::now())
                .as_millis() as u64,
        })
    }
    pub fn kind(&self) -> DomainPlanKind {
        self.kind
    }
    pub fn ca_prepared(&self) -> bool {
        true
    }
    pub fn ca_fingerprint(&self) -> &str {
        &self.ca_hash
    }
    pub fn block_reason(&self) -> Option<&'static str> {
        None
    }
    pub fn fingerprint(&self) -> Result<String> {
        let native = self.native.digest();
        self.native.validate(&native)?;
        let bytes = serde_json::to_vec(
            &serde_json::json!({"kind":self.kind.as_str(),"native":native,
            "data_root":self.data_root,"project":self.project_id,"root":self.project_root,
            "identity":self.project_identity,"revision":self.revision,"owner":self.owner,
            "sites":self.sites_hash,"ca":self.ca_hash,"hosts":self.hosts_hash}),
        )
        .map_err(|_| error("domain_state_unavailable"))?;
        Ok(devforge_serving::hash(&bytes))
    }
    /// Trusted native composition only. This constructor performs no helper/OS
    /// action; the manager must require current NativeAuthority+explicit action.
    pub fn confirm_native_owner(&self) -> Result<MachineConfirmation> {
        if Instant::now() >= self.expires {
            return Err(error("domain_review_expired"));
        }
        Ok(MachineConfirmation {
            fingerprint: self.fingerprint()?,
            expires: self.expires,
            used: false,
        })
    }
}
fn bounded(path: &Path, limit: u64) -> Result<Vec<u8>> {
    Ok(devforge_projects::manifest::read_file(path, limit)?)
}
fn current_sites(workspace: &Workspace) -> Result<(Vec<Site>, String)> {
    let mut sites = workspace.sites.sites()?;
    if sites.len() > 64 {
        return Err(error("domain_state_unavailable"));
    }
    sites.sort_by(|a, b| a.project_id.cmp(&b.project_id));
    let hash = devforge_serving::hash(
        &serde_json::to_vec(&sites).map_err(|_| error("domain_state_unavailable"))?,
    );
    Ok((sites, hash))
}
fn existing_ca(workspace: &Workspace, owner: &str) -> Result<Vec<u8>> {
    let dir = workspace.sites.root().join("authority");
    let valid = fs::symlink_metadata(&dir)
        .is_ok_and(|meta| meta.is_dir() && !meta.file_type().is_symlink())
        && fs::canonicalize(&dir).ok().as_ref() == Some(&dir);
    if !valid {
        return Err(error("native_owner_preparation_required"));
    }
    let der = bounded(&dir.join("ca.der"), 16384)
        .map_err(|_| error("native_owner_preparation_required"))?;
    authority::validate_root(&der, owner)
        .map_err(|_| error("native_owner_preparation_required"))?;
    Ok(der)
}
fn policy(cancel: &Cancellation, check: &dyn Fn() -> Result<()>) -> Result<()> {
    if cancel.is_cancelled() {
        return Err(error("cancelled"));
    }
    check()
}
fn actual_names(before: &[u8], owner: &str) -> Result<BTreeSet<String>> {
    // Existing parser establishes the exact bounded marker/line grammar first.
    hosts::patch(before, owner, &[])?;
    let text = std::str::from_utf8(before).map_err(|_| error("domain_hosts_unrepresentable"))?;
    let begin = format!("# devforge:{owner} begin");
    let end = format!("# devforge:{owner} end");
    let mut inside = false;
    let mut names = BTreeSet::new();
    for line in text.trim_start_matches('\u{feff}').lines() {
        let line = line.trim_end_matches('\r');
        if line == begin {
            inside = true;
        } else if line == end {
            inside = false;
        } else if inside {
            for name in line.split('#').next().unwrap().split_whitespace().skip(1) {
                names.insert(name.into());
            }
        }
    }
    let owned = names.iter().cloned().collect::<Vec<_>>();
    if hosts::patch(before, owner, &owned)? != before {
        // Fixed SystemPlan cannot preserve noncanonical owner comments/bytes.
        return Err(error("domain_hosts_unrepresentable"));
    }
    Ok(names)
}
impl Workspace {
    /// Manager-only registration/identity lookup for domain authorization. No
    /// global Snapshot (and therefore no unrelated backup directory effects).
    /// Project's private reference fields are not an external response DTO.
    pub fn domain_registered_project(&self, id: &str) -> Result<Project> {
        self.available_project(id)
    }
    pub fn domain_facts_guarded(
        &self,
        id: &str,
        check: &dyn Fn() -> Result<()>,
    ) -> Result<DomainFacts> {
        check()?;
        let project = self.available_project(id)?;
        let sites = self.sites.sites()?;
        let site = sites
            .iter()
            .find(|site| site.project_id == id)
            .ok_or_else(|| error("setup_site_missing"))?;
        let owner = self.sites.owner()?;
        let ca = existing_ca(self, &owner).ok();
        let status = self.sessions.get(id).map(|session| session.web.status());
        let observed = status
            .as_ref()
            .and_then(|status| status.sites.iter().find(|site| site.project_id == id));
        let facts = DomainFacts {
            project_id: id.into(),
            revision: project.revision,
            domain: site.domain.clone(),
            https_enabled: site.https,
            preferred_url: observed.map(|site| site.preferred_url.clone()),
            http_port: status.as_ref().map(|status| status.http.selected),
            https_port: status
                .as_ref()
                .filter(|_| site.https)
                .map(|status| status.https.selected),
            certificate_active: observed.is_some_and(|site| site.certificate_active),
            ca_prepared: ca.is_some(),
            ca_fingerprint: ca.as_ref().map(|der| devforge_serving::hash(der)),
            // Trust captured at an earlier serving start is not a fresh OS probe.
            system_trusted: None,
        };
        check()?;
        Ok(facts)
    }
    pub fn domain_plan_guarded(
        &self,
        id: &str,
        kind: DomainPlanKind,
        check: &dyn Fn() -> Result<()>,
    ) -> Result<DomainPlan> {
        check()?;
        self.available_project(id)?;
        existing_ca(self, &self.sites.owner()?)?;
        // Plans are memory-only. Only the fixed existing hosts target is read.
        let before = bounded(&system::hosts_path()?, 2 * 1024 * 1024)?;
        self.domain_plan_from_hosts(id, kind, &before, check)
    }
    fn domain_plan_from_hosts(
        &self,
        id: &str,
        kind: DomainPlanKind,
        before: &[u8],
        check: &dyn Fn() -> Result<()>,
    ) -> Result<DomainPlan> {
        check()?;
        let project = self.available_project(id)?;
        let (sites, sites_hash) = current_sites(self)?;
        let site = sites
            .iter()
            .find(|site| site.project_id == id)
            .ok_or_else(|| error("setup_site_missing"))?;
        let owner = self.sites.owner()?;
        let ca = existing_ca(self, &owner)?;
        let mut original_hosts = Vec::new();
        let action = if kind == DomainPlanKind::Trust {
            Operation::TrustInstall {
                certificate_der: ca.clone(),
            }
        } else {
            let mut names = actual_names(before, &owner)?;
            original_hosts = names.iter().cloned().collect();
            if kind == DomainPlanKind::Cleanup {
                names.remove(&site.domain);
                names.remove(&site.vite_domain());
            } else {
                names.insert(site.domain.clone());
                names.insert(site.vite_domain());
            }
            let domains = names.into_iter().collect::<Vec<_>>();
            hosts::patch(before, &owner, &domains)?;
            Operation::Hosts {
                before_sha256: devforge_serving::hash(before),
                domains,
            }
        };
        let native = SystemPlan {
            schema: 1,
            owner: owner.clone(),
            action,
        };
        native.validate(&native.digest())?;
        let plan = DomainPlan {
            kind,
            native,
            data_root: self.root.clone(),
            project_id: id.into(),
            project_root: project.path.clone(),
            project_identity: devforge_projects::discovery::identity(&project.path)?,
            revision: project.revision,
            owner,
            sites_hash,
            ca_hash: devforge_serving::hash(&ca),
            hosts_hash: devforge_serving::hash(before),
            original_hosts,
            expires: Instant::now() + LIFETIME,
        };
        check()?;
        Ok(plan)
    }
    #[cfg(feature = "test-support")]
    /// Owned fixture only; release builds contain no alternate hosts source.
    pub fn domain_fixture_plan(
        &self,
        id: &str,
        kind: DomainPlanKind,
        hosts: &[u8],
    ) -> Result<DomainPlan> {
        self.domain_plan_from_hosts(id, kind, hosts, &|| Ok(()))
    }
    fn validate_domain_plan(&self, id: &str, plan: &DomainPlan) -> Result<()> {
        if Instant::now() >= plan.expires {
            return Err(error("domain_review_expired"));
        }
        let project = self.available_project(id)?;
        let (_, sites_hash) = current_sites(self)?;
        let owner = self.sites.owner()?;
        let ca = existing_ca(self, &owner)?;
        if id != plan.project_id
            || self.root != plan.data_root
            || project.path != plan.project_root
            || project.revision != plan.revision
            || devforge_projects::discovery::identity(&project.path)? != plan.project_identity
            || owner != plan.owner
            || sites_hash != plan.sites_hash
            || devforge_serving::hash(&ca) != plan.ca_hash
            || devforge_serving::hash(&bounded(&system::hosts_path()?, 2 * 1024 * 1024)?)
                != plan.hosts_hash
        {
            return Err(error("setup_review_changed"));
        }
        plan.native.validate(&plan.native.digest())?;
        Ok(())
    }
    pub fn execute_domain_plan(
        &self,
        id: &str,
        plan: &DomainPlan,
        confirmation: &mut MachineConfirmation,
        cancel: &Cancellation,
        check: &dyn Fn() -> Result<()>,
    ) -> Result<serde_json::Value> {
        policy(cancel, check)?;
        if confirmation.used
            || Instant::now() >= confirmation.expires
            || confirmation.fingerprint != plan.fingerprint()?
        {
            return Err(error("native_owner_confirmation_required"));
        }
        self.validate_domain_plan(id, plan)?;
        policy(cancel, check)?;
        let helper = devforge_serving::authorize::helper_path()
            .map_err(|_| error("setup_helper_unavailable"))?;
        confirmation.used = true;
        let dir = tempfile::Builder::new()
            .prefix("domain-plan-")
            .tempdir_in(&self.root)?;
        let path = dir.path().join("plan.json");
        fs::write(
            &path,
            serde_json::to_vec(&plan.native).map_err(|_| error("domain_state_unavailable"))?,
        )?;
        self.validate_domain_plan(id, plan)?;
        policy(cancel, check)?;
        devforge_serving::authorize::apply(&helper, &path, &plan.native.digest())
            .map_err(|_| error("domain_system_effect_unconfirmed"))?;
        // No late policy check after the machine effect. Verification may still
        // report incomplete/unconfirmed; it never claims an automatic rollback.
        let verified = match &plan.native.action {
            Operation::Hosts { domains, .. } => {
                let after = bounded(&system::hosts_path()?, 2 * 1024 * 1024)?;
                hosts::patch(&after, &plan.owner, domains)? == after
            }
            Operation::TrustInstall { certificate_der } => system::trusted(certificate_der)?,
            Operation::TrustRemove { .. } => false,
        };
        if !verified {
            return Err(error("domain_system_incomplete"));
        }
        Ok(serde_json::json!({"applied":true,"kind":plan.kind.as_str(),"shared_ca_preserved":true}))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use devforge_core::{ErrorCode, SecretStore};
    use devforge_serving::authority::Authority;
    use std::sync::{Arc, Mutex};
    struct DenySecrets;
    impl SecretStore for DenySecrets {
        fn get(&self, _: &str) -> std::result::Result<Option<String>, ErrorCode> {
            panic!("domain observer must not probe keys")
        }
        fn set(&self, _: &str, _: &str) -> std::result::Result<(), ErrorCode> {
            panic!("domain observer must not create keys")
        }
        fn delete(&self, _: &str) -> std::result::Result<(), ErrorCode> {
            panic!("domain action must not delete keys")
        }
    }
    #[derive(Default)]
    struct MemorySecrets(Mutex<BTreeMap<String, String>>);
    impl SecretStore for MemorySecrets {
        fn get(&self, key: &str) -> std::result::Result<Option<String>, ErrorCode> {
            Ok(self.0.lock().unwrap().get(key).cloned())
        }
        fn set(&self, key: &str, value: &str) -> std::result::Result<(), ErrorCode> {
            self.0.lock().unwrap().insert(key.into(), value.into());
            Ok(())
        }
        fn delete(&self, key: &str) -> std::result::Result<(), ErrorCode> {
            self.0.lock().unwrap().remove(key);
            Ok(())
        }
    }
    fn register(workspace: &mut Workspace, root: &Path, name: &str) -> String {
        let path = root.join(name);
        fs::create_dir_all(path.join("public")).unwrap();
        fs::write(path.join("public/index.php"), "<?php").unwrap();
        fs::write(
            path.join("composer.json"),
            r#"{"require":{"php":"^8.3","laravel/framework":"^13"}}"#,
        )
        .unwrap();
        let target = devforge_runtime::package::Target::current().unwrap();
        let php = workspace
            .catalog
            .packages
            .iter()
            .find(|(_, p)| {
                p.kind == "php" && p.target == target && p.executables.contains_key("php")
            })
            .unwrap()
            .0
            .clone();
        let node = workspace
            .catalog
            .packages
            .iter()
            .find(|(_, p)| {
                p.kind == "node" && p.target == target && p.executables.contains_key("node")
            })
            .unwrap()
            .0
            .clone();
        workspace
            .execute(
                Command::Import {
                    path,
                    name: name.into(),
                    php,
                    node,
                    domain: format!("{name}.devforge.test"),
                    https: true,
                    vite: false,
                    queue: false,
                },
                &Cancellation::default(),
            )
            .unwrap()["project_id"]
            .as_str()
            .unwrap()
            .into()
    }
    fn fixture(ca: bool) -> (tempfile::TempDir, Workspace, String) {
        let root = tempfile::tempdir().unwrap();
        let mut workspace =
            Workspace::open(root.path().join("data"), Arc::new(DenySecrets)).unwrap();
        let id = register(&mut workspace, root.path(), "a");
        if ca {
            Authority::open(
                workspace.sites.root().join("authority"),
                &workspace.sites.owner().unwrap(),
                &MemorySecrets::default(),
            )
            .unwrap();
        }
        (root, workspace, id)
    }
    #[test]
    fn facts_and_missing_ca_plan_gate_never_create_authority_or_probe_native_keys() {
        let (_root, workspace, id) = fixture(false);
        let facts = workspace.domain_facts_guarded(&id, &|| Ok(())).unwrap();
        assert!(!facts.ca_prepared);
        assert!(facts.system_trusted.is_none());
        assert!(facts.preferred_url.is_none());
        for kind in [
            DomainPlanKind::Hosts,
            DomainPlanKind::Trust,
            DomainPlanKind::Cleanup,
        ] {
            assert_eq!(
                workspace
                    .domain_plan_guarded(&id, kind, &|| Ok(()))
                    .err()
                    .unwrap()
                    .code,
                "native_owner_preparation_required"
            );
        }
        assert!(!workspace.sites.root().join("authority").exists());
    }
    #[test]
    fn cleanup_preserves_actual_other_and_stale_entries_and_allows_only_a_hosts_removal() {
        let (root, mut workspace, id) = fixture(true);
        let owner = workspace.sites.owner().unwrap();
        let original = b"127.0.0.1 localhost\n# foreign tool unchanged\n";
        let before = hosts::patch(
            original,
            &owner,
            &[
                "a.devforge.test".into(),
                "vite-a.devforge.test".into(),
                "b.devforge.test".into(),
                "stale.devforge.test".into(),
            ],
        )
        .unwrap();
        let plan = workspace
            .domain_plan_from_hosts(&id, DomainPlanKind::Cleanup, &before, &|| Ok(()))
            .unwrap();
        let Operation::Hosts { domains, .. } = &plan.native.action else {
            panic!("cleanup attempted CA removal")
        };
        assert_eq!(
            domains,
            &vec![
                "b.devforge.test".to_string(),
                "stale.devforge.test".to_string()
            ]
        );
        let after = hosts::patch(&before, &owner, domains).unwrap();
        assert!(after.ends_with(original));
        let fresh = workspace
            .domain_plan_from_hosts(&id, DomainPlanKind::Cleanup, &before, &|| Ok(()))
            .unwrap();
        assert_eq!(plan.fingerprint().unwrap(), fresh.fingerprint().unwrap());
        register(&mut workspace, root.path(), "b");
        assert_ne!(
            plan.fingerprint().unwrap(),
            workspace
                .domain_plan_from_hosts(&id, DomainPlanKind::Cleanup, &before, &|| Ok(()))
                .unwrap()
                .fingerprint()
                .unwrap()
        );
        let only_a = hosts::patch(
            original,
            &owner,
            &["a.devforge.test".into(), "vite-a.devforge.test".into()],
        )
        .unwrap();
        let last = workspace
            .domain_plan_from_hosts(&id, DomainPlanKind::Cleanup, &only_a, &|| Ok(()))
            .unwrap();
        let Operation::Hosts { domains, .. } = &last.native.action else {
            panic!()
        };
        assert!(domains.is_empty());
        assert_eq!(hosts::patch(&only_a, &owner, domains).unwrap(), original);
        let noncanonical = String::from_utf8(before).unwrap().replace(
            &format!("# devforge:{owner} end"),
            &format!("# private comment\n# devforge:{owner} end"),
        );
        assert_eq!(
            workspace
                .domain_plan_from_hosts(
                    &id,
                    DomainPlanKind::Cleanup,
                    noncanonical.as_bytes(),
                    &|| Ok(())
                )
                .err()
                .unwrap()
                .code,
            "domain_hosts_unrepresentable"
        );
    }
    #[test]
    fn expired_or_used_confirmation_and_policy_denial_stop_before_fixed_helper_or_system_reads() {
        let (_root, workspace, id) = fixture(true);
        let mut plan = workspace
            .domain_plan_from_hosts(
                &id,
                DomainPlanKind::Trust,
                b"127.0.0.1 localhost\n",
                &|| Ok(()),
            )
            .unwrap();
        let mut confirmation = plan.confirm_native_owner().unwrap();
        assert_eq!(
            workspace
                .execute_domain_plan(
                    &id,
                    &plan,
                    &mut confirmation,
                    &Cancellation::default(),
                    &|| Err(error("cancelled"))
                )
                .unwrap_err()
                .code,
            "cancelled"
        );
        assert!(!confirmation.used);
        confirmation.used = true;
        assert_eq!(
            workspace
                .execute_domain_plan(
                    &id,
                    &plan,
                    &mut confirmation,
                    &Cancellation::default(),
                    &|| Ok(())
                )
                .unwrap_err()
                .code,
            "native_owner_confirmation_required"
        );
        plan.expires = Instant::now() - Duration::from_secs(1);
        assert_eq!(
            plan.confirm_native_owner().err().unwrap().code,
            "domain_review_expired"
        );
    }
    #[test]
    fn native_review_summary_exposes_exact_delta_and_fixed_steps_without_private_plan_fields() {
        let (_root, workspace, id) = fixture(true);
        let owner = workspace.sites.owner().unwrap();
        let before = hosts::patch(
            b"127.0.0.1 localhost\n",
            &owner,
            &["b.devforge.test".into(), "stale.devforge.test".into()],
        )
        .unwrap();
        let mut plan = workspace
            .domain_plan_from_hosts(&id, DomainPlanKind::Hosts, &before, &|| Ok(()))
            .unwrap();
        let summary = plan.native_review_summary().unwrap();
        let hosts = summary.hosts.as_ref().unwrap();
        assert_eq!(hosts.added, vec!["a.devforge.test", "vite-a.devforge.test"]);
        assert!(hosts.removed.is_empty());
        assert_eq!(
            hosts.retained,
            vec!["b.devforge.test", "stale.devforge.test"]
        );
        assert!(matches!(
            summary.steps[0].operation,
            NativeSystemOperation::HostsUpdate
        ));
        assert!(matches!(
            summary.steps[0].target,
            NativeSystemTarget::SystemHosts
        ));
        assert!(
            summary.shared_ca_preserved
                && summary.remaining_ms > 0
                && summary.remaining_ms <= 300_000
        );
        let value = serde_json::to_value(&summary).unwrap();
        let encoded = value.to_string();
        assert!(!encoded.contains(&plan.native.digest()));
        assert!(!encoded.contains(&plan.fingerprint().unwrap()));
        for name in [
            "owner",
            "project_root",
            "hosts_hash",
            "certificate_der",
            "private_key",
        ] {
            assert!(value.get(name).is_none());
        }
        let decoded: NativeDomainReviewSummary = serde_json::from_value(value).unwrap();
        assert!(decoded == summary);
        plan.expires = Instant::now() - Duration::from_millis(1);
        assert_eq!(plan.native_review_summary().unwrap().remaining_ms, 0);
        let trust = workspace
            .domain_plan_from_hosts(&id, DomainPlanKind::Trust, &before, &|| Ok(()))
            .unwrap()
            .native_review_summary()
            .unwrap();
        assert!(trust.hosts.is_none());
        assert!(matches!(
            trust.steps[0].operation,
            NativeSystemOperation::TrustInstall
        ));
        assert!(matches!(
            trust.steps[0].target,
            NativeSystemTarget::SystemTrustStore
        ));
    }
}
