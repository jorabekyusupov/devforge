//! Scoped public domain facts and private existing-SystemPlan worker capture.
use devforge_control::{
    Digest, Id,
    limits::{SafeInt, Text},
};
use serde::{Deserialize, Serialize};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc::SyncSender,
};
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DomainReadKind {
    Url,
    Ports,
    CertificateStatus,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DomainPlanKind {
    Hosts,
    Trust,
    Cleanup,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DomainFacts {
    pub project_id: Id,
    pub revision: SafeInt,
    pub domain: Text<230>,
    pub https_enabled: bool,
    pub preferred_url: Option<Text<2048>>,
    pub http_port: Option<u16>,
    pub https_port: Option<u16>,
    pub certificate_active: bool,
    pub ca_prepared: bool,
    pub ca_fingerprint: Option<Digest>,
    pub system_trusted: Option<bool>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DomainPlanPreview {
    pub project_id: Id,
    pub revision: SafeInt,
    pub kind: DomainPlanKind,
    pub ticket: Option<Digest>,
    pub blocked: Option<Id>,
    pub ca_prepared: bool,
    pub shared_ca_preserved: bool,
}
pub(crate) struct DomainIntent {
    pub plan: devforge_workspace::domain_control::DomainPlan,
    pub kind: DomainPlanKind,
    pub snapshot_digest: Digest,
    pub confirmation:
        std::sync::Mutex<Option<devforge_workspace::domain_control::MachineConfirmation>>,
}
type Check = Box<dyn Fn(&devforge_workspace::Workspace) -> devforge_workspace::Result<()> + Send>;
pub(crate) struct DomainReadResult {
    pub facts: devforge_workspace::domain_control::DomainFacts,
    pub plan: Option<devforge_workspace::domain_control::DomainPlan>,
    pub blocked: Option<String>,
}
pub(crate) struct DomainRead {
    pub project: Id,
    pub kind: Option<DomainPlanKind>,
    pub check: Check,
    pub cancelled: Arc<AtomicBool>,
    pub reply: SyncSender<devforge_workspace::Result<DomainReadResult>>,
}
impl DomainRead {
    pub(crate) fn execute(self, workspace: &devforge_workspace::Workspace) {
        let run = || {
            let check = || {
                if self.cancelled.load(Ordering::Acquire) {
                    return Err(devforge_workspace::Error {
                        scope: "workspace",
                        code: "cancelled".into(),
                        project_id: None,
                    });
                }
                (self.check)(workspace)
            };
            check()?;
            let facts = workspace.domain_facts_guarded(self.project.as_str(), &check)?;
            let (plan, blocked) = if let Some(kind) = self.kind {
                let kind = match kind {
                    DomainPlanKind::Hosts => {
                        devforge_workspace::domain_control::DomainPlanKind::Hosts
                    }
                    DomainPlanKind::Trust => {
                        devforge_workspace::domain_control::DomainPlanKind::Trust
                    }
                    DomainPlanKind::Cleanup => {
                        devforge_workspace::domain_control::DomainPlanKind::Cleanup
                    }
                };
                match workspace.domain_plan_guarded(self.project.as_str(), kind, &check) {
                    Ok(plan) => (Some(plan), None),
                    Err(error) => {
                        check()?;
                        (None, Some(error.code))
                    }
                }
            } else {
                (None, None)
            };
            check()?;
            Ok(DomainReadResult {
                facts,
                plan,
                blocked,
            })
        };
        let _ = self.reply.send(run());
    }
}
