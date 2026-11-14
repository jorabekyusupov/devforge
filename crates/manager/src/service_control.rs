//! Reviewed finite service/runtime intents. Uses the existing control envelope,
//! native authority and single actor; no arbitrary command or removal surface.
use crate::{
    PeerIdentity,
    actor::{ActorHandle, Receipt},
    auth::DesktopAuthority,
    external_auth::{AuthorizedExternal, ExternalCredential},
    external_control::{Server as Grants, grant_error},
};
use devforge_control::{
    Digest, ErrorCode, Id, ProjectBinding, Request, Response, ReviewedPlan, Scope,
    limits::SafeInt,
    mutations::{
        MutationContext, MutationInput, MutationReceipt, PlanReceipt, ProjectTrust, ReviewApproval,
        fingerprint_material, prepare_mutation,
    },
};
use devforge_control_adapter::{BackendContext, Dispatch, map_mutation, sha256};
use devforge_runtime::Cancellation;
use devforge_workspace::{Snapshot, Workspace};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::VecDeque,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use zeroize::Zeroizing;
pub type ServicePlanReply = Response<PlanReceipt>;
pub type ServiceApplyReply = Response<MutationReceipt>;
pub type ServiceLookupReply = Response<Option<MutationReceipt>>;
pub type RuntimePlanReply = Response<PlanReceipt>;
pub type RuntimeApplyReply = Response<MutationReceipt>;
pub type RuntimeLookupReply = Response<Option<MutationReceipt>>;
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeSelection {
    pub project_id: Id,
    pub revision: SafeInt,
    pub input: devforge_control::mutations::ConfigureInput,
}
pub type RuntimeSelectionReply = Response<RuntimeSelection>;
pub use crate::environment_control::{EnvDocumentState, EnvPreview, MAX_EXTERNAL_YAML_BYTES};
pub type EnvPreviewReply = Response<EnvPreview>;
pub type EnvApplyReply = Response<MutationReceipt>;
pub type EnvLookupReply = Response<Option<MutationReceipt>>;
pub type DatabasePlanReply = Response<PlanReceipt>;
pub type DatabaseApplyReply = Response<MutationReceipt>;
pub type DatabaseLookupReply = Response<Option<MutationReceipt>>;
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DatabaseChoices {
    pub project_id: Id,
    pub revision: SafeInt,
    pub stopped: bool,
    pub current_instance_id: Option<Id>,
    pub instance_ids: Vec<Id>,
    pub backup_ids: Vec<Id>,
}
pub type DatabaseChoicesReply = Response<DatabaseChoices>;
pub use crate::domain_control::{DomainFacts, DomainPlanKind, DomainPlanPreview, DomainReadKind};
pub type DomainFactsReply = Response<DomainFacts>;
pub type DomainPrepareReply = Response<DomainPlanPreview>;
pub type DomainPlanReply = Response<PlanReceipt>;
pub type DomainApplyReply = Response<MutationReceipt>;
pub type DomainLookupReply = Response<Option<MutationReceipt>>;
const PLAN_TTL: Duration = Duration::from_secs(300);
const MAX_PLANS: usize = 128;
const MAX_PLAN_BYTES: usize = 1_048_576;
pub(crate) struct Services {
    grants: Arc<Grants>,
    plans: Mutex<VecDeque<Arc<Intent>>>,
    environment_versions: Mutex<crate::environment_control::Versions>,
    domain_preparations: Mutex<VecDeque<DomainPreparation>>,
}
struct DomainPreparation {
    ticket: Digest,
    client: Id,
    grant: Id,
    binding: ProjectBinding,
    kind: DomainPlanKind,
    plan: devforge_workspace::domain_control::DomainPlan,
    expires: Instant,
    bytes: usize,
}
struct Intent {
    request: Request<MutationInput>,
    binding: ProjectBinding,
    fingerprint: Digest,
    backend: Digest,
    review: ReviewedPlan,
    client: Id,
    grant: Id,
    peer: PeerIdentity,
    credential: ExternalCredential,
    expires_at: u64,
    expires: Instant,
    approved: AtomicBool,
    claimed: Mutex<Option<Id>>,
    cancellation: Mutex<Option<Cancellation>>,
    bytes: usize,
    environment: Option<crate::environment_control::EnvironmentIntent>,
    public_digest: Option<Digest>,
    public_fingerprint: Option<Digest>,
    database: Option<crate::database_control::DatabaseReadResult>,
    domain: Option<crate::domain_control::DomainIntent>,
}
pub(crate) struct ServiceJob {
    intent: Arc<Intent>,
    policy: Arc<Services>,
    pub(crate) deadline: Instant,
    pub(crate) cancellation: Cancellation,
}
impl ServiceJob {
    pub(crate) fn sensitive(&self) -> bool {
        matches!(self.intent.request.input, MutationInput::YamlSave(_))
    }
    pub(crate) fn client(&self) -> &Id {
        &self.intent.client
    }
    pub(crate) fn request(&self) -> &Request<MutationInput> {
        &self.intent.request
    }
    pub(crate) fn fingerprint(&self) -> &str {
        self.intent.fingerprint.as_str()
    }
    pub(crate) fn encoded_bytes(&self) -> usize {
        self.intent.bytes
    }
    pub(crate) fn claim(&self, id: &Id) -> Result<(), ErrorCode> {
        let mut claim = self
            .intent
            .claimed
            .lock()
            .map_err(|_| ErrorCode::StorageUnavailable)?;
        if claim.as_ref().is_some_and(|old| old != id) {
            return Err(ErrorCode::ReviewMismatch);
        }
        *claim = Some(id.clone());
        *self
            .intent
            .cancellation
            .lock()
            .map_err(|_| ErrorCode::StorageUnavailable)? = Some(self.cancellation.clone());
        Ok(())
    }
    pub(crate) fn release_claim(&self, id: &Id) {
        if let Ok(mut claim) = self.intent.claimed.lock()
            && claim.as_ref() == Some(id)
        {
            *claim = None;
        }
    }
    pub(crate) fn validate_admission(
        &self,
        snapshot: &Snapshot,
        registry: &mut crate::external_auth::ExternalRegistry,
    ) -> Result<devforge_control::mutations::PreparedMutation, ErrorCode> {
        self.check_basic()?;
        if let Some(domain) = &self.intent.domain
            && domain
                .confirmation
                .lock()
                .map_err(|_| ErrorCode::StorageUnavailable)?
                .is_none()
        {
            return Err(ErrorCode::Unauthorized);
        }
        let current = project_binding(snapshot, &self.intent.request.project_id)?;
        let proof = registry
            .authorize(
                &self.intent.peer,
                &self.intent.credential,
                &current,
                &required(&self.intent.request.input)?,
                &[],
                self.intent.request.expected_revision,
            )
            .map_err(grant_error)?;
        self.prepared(snapshot, &current, &proof, true)
    }
    fn check_basic(&self) -> Result<(), ErrorCode> {
        if self.cancellation.is_cancelled() {
            return Err(ErrorCode::Interrupted);
        }
        if Instant::now() >= self.deadline {
            return Err(ErrorCode::TimedOut);
        }
        if Instant::now() >= self.intent.expires {
            return Err(ErrorCode::ReviewExpired);
        }
        if !self.intent.approved.load(Ordering::Acquire) {
            return Err(ErrorCode::Unauthorized);
        }
        Ok(())
    }
    fn authorize(&self, binding: &ProjectBinding) -> Result<AuthorizedExternal, ErrorCode> {
        let mut lock = self
            .policy
            .grants
            .registry
            .lock()
            .map_err(|_| ErrorCode::StorageUnavailable)?;
        lock.as_mut()
            .ok_or(ErrorCode::Interrupted)?
            .authorize(
                &self.intent.peer,
                &self.intent.credential,
                binding,
                &required(&self.intent.request.input)?,
                &[],
                self.intent.request.expected_revision,
            )
            .map_err(grant_error)
    }
    fn validate(&self, snapshot: &Snapshot, initial: bool) -> Result<ProjectBinding, ErrorCode> {
        self.check_basic()?;
        let current = project_binding(snapshot, &self.intent.request.project_id)?;
        if current.root() != self.intent.binding.root() {
            return Err(ErrorCode::ProjectMismatch);
        }
        let proof = self.authorize(&current)?;
        self.prepared(snapshot, &current, &proof, initial)?;
        self.check_basic()?;
        Ok(current)
    }
    fn prepared(
        &self,
        snapshot: &Snapshot,
        current: &ProjectBinding,
        proof: &AuthorizedExternal,
        initial: bool,
    ) -> Result<devforge_control::mutations::PreparedMutation, ErrorCode> {
        if current.root() != self.intent.binding.root() {
            return Err(ErrorCode::ProjectMismatch);
        }
        if initial {
            cached_preflight(snapshot, &self.intent.request)?;
        }
        let backend = if let Some(domain) = &self.intent.domain {
            if domain_snapshot_digest(snapshot, &self.intent.request)? != domain.snapshot_digest {
                return Err(ErrorCode::ReviewMismatch);
            }
            self.intent.backend.clone()
        } else if let Some(database) = &self.intent.database {
            if database_snapshot_digest(snapshot, &self.intent.request)? != database.snapshot_digest
            {
                return Err(ErrorCode::ReviewMismatch);
            }
            self.intent.backend.clone()
        } else if let Some(environment) = &self.intent.environment {
            verify_document(current, environment.data.document_sha.as_deref())?;
            self.intent.backend.clone()
        } else if initial {
            state_digest(snapshot, current, &self.intent.request, proof)?
        } else {
            self.intent.backend.clone()
        };
        let grant = proof.control_grant()?;
        let trust = ProjectTrust::record(current);
        let bytes =
            serde_json::to_vec(&self.intent.request).map_err(|_| ErrorCode::InvalidInput)?;
        let prepared = prepare_mutation(
            &bytes,
            MutationContext {
                binding: current,
                grant: &grant,
                client: proof.client_id(),
                now: proof.observed_at(),
                trust: Some(&trust),
                review: Some(ReviewApproval {
                    plan: &self.intent.review,
                    current_backend_digest: &backend,
                    request_fingerprint: &self.intent.fingerprint,
                }),
            },
            sha256,
        )?;
        self.check_basic()?;
        Ok(prepared)
    }
    pub(crate) fn execute(self, workspace: &mut Workspace) -> devforge_workspace::Result<Value> {
        let before = workspace
            .snapshot()
            .map_err(|_| ws_error(ErrorCode::StorageUnavailable))?;
        let current = self.validate(&before, true).map_err(ws_error)?;
        if self.intent.environment.is_some() {
            let check = || {
                self.check_basic().map_err(ws_error)?;
                self.authorize(&current).map_err(ws_error)?;
                Ok(())
            };
            let (exists, sha) =
                workspace.environment_document_version(current.project_id().as_str())?;
            let (plan, plan_error) =
                if let MutationInput::EnvironmentApply(input) = &self.intent.request.input {
                    (
                        Some(Box::new(workspace.environment_plan_guarded(
                            current.project_id().as_str(),
                            input.write_env,
                            &check,
                        )?)),
                        None,
                    )
                } else {
                    (None, None)
                };
            let fresh = crate::environment_control::PrivateEnvironment {
                binding: current.clone(),
                exists,
                document_sha: sha,
                plan,
                plan_error,
            };
            let proof = self.authorize(&current).map_err(ws_error)?;
            if environment_backend(&before, &current, &self.intent.request, &proof, &fresh)
                .map_err(ws_error)?
                != self.intent.backend
            {
                return Err(ws_error(ErrorCode::ReviewMismatch));
            }
        }
        if matches!(
            self.intent.request.input,
            MutationInput::Start(_) | MutationInput::Restart(_)
        ) {
            workspace.service_preflight(current.project_id().as_str())?;
        }
        let proof = self.authorize(&current).map_err(ws_error)?;
        let grant = proof.control_grant().map_err(ws_error)?;
        let trust = ProjectTrust::record(&current);
        let prepared = prepare_mutation(
            &serde_json::to_vec(&self.intent.request)
                .map_err(|_| ws_error(ErrorCode::InvalidInput))?,
            MutationContext {
                binding: &current,
                grant: &grant,
                client: proof.client_id(),
                now: proof.observed_at(),
                trust: Some(&trust),
                review: Some(ReviewApproval {
                    plan: &self.intent.review,
                    current_backend_digest: &self.intent.backend,
                    request_fingerprint: &self.intent.fingerprint,
                }),
            },
            sha256,
        )
        .map_err(ws_error)?;
        let dispatch = map_mutation(
            &prepared,
            &current,
            &BackendContext::reviewed(&self.intent.backend),
        )
        .map_err(ws_error)?;
        let check = || {
            self.check_basic().map_err(ws_error)?;
            self.authorize(&current).map_err(ws_error)?;
            let identity = devforge_projects::discovery::identity(current.root())
                .map_err(|_| ws_error(ErrorCode::ProjectMismatch))?;
            let expected = before
                .projects
                .iter()
                .find(|r| r.project.id == current.project_id().as_str())
                .and_then(|r| r.project.filesystem_identity.as_deref());
            if expected != Some(identity.as_str()) {
                return Err(ws_error(ErrorCode::ProjectMismatch));
            }
            Ok(())
        };
        let command = match dispatch {
            Dispatch::Command(command) => command,
            Dispatch::DomainSystem {
                project_id,
                cleanup,
                plan,
            } => {
                let domain = self
                    .intent
                    .domain
                    .as_ref()
                    .ok_or_else(|| ws_error(ErrorCode::Unauthorized))?;
                if project_id != current.project_id().as_str()
                    || cleanup != (domain.kind == DomainPlanKind::Cleanup)
                    || domain_reference(&self.intent.request.input).map_err(ws_error)? != &plan
                {
                    return Err(ws_error(ErrorCode::ReviewMismatch));
                }
                let mut confirmation = domain
                    .confirmation
                    .lock()
                    .map_err(|_| ws_error(ErrorCode::StorageUnavailable))?
                    .take()
                    .ok_or_else(|| ws_error(ErrorCode::Unauthorized))?;
                let _private = workspace.execute_domain_plan(
                    &project_id,
                    &domain.plan,
                    &mut confirmation,
                    &self.cancellation,
                    &check,
                )?;
                return Ok(json!({"applied":true}));
            }
            _ => return Err(ws_error(ErrorCode::UnsupportedCapability)),
        };
        match command.as_ref() {
            devforge_workspace::Command::Start { .. } => {
                workspace.execute_service_action(current.project_id().as_str(), "start", &check)
            }
            devforge_workspace::Command::Stop { .. } => {
                workspace.execute_service_action(current.project_id().as_str(), "stop", &check)
            }
            devforge_workspace::Command::Restart { .. } => {
                workspace.execute_service_action(current.project_id().as_str(), "restart", &check)
            }
            devforge_workspace::Command::Install { .. }
            | devforge_workspace::Command::Configure { .. } => {
                workspace.execute_runtime_action(*command, &self.cancellation, &check)
            }
            devforge_workspace::Command::ConfigurationApply { .. }
            | devforge_workspace::Command::ConfigurationSave { .. } => {
                let mut command = *command;
                if let devforge_workspace::Command::ConfigurationSave { expected, .. } =
                    &mut command
                {
                    *expected = self
                        .intent
                        .environment
                        .as_ref()
                        .and_then(|env| env.expected_sha.as_ref().map(|sha| sha.to_string()));
                }
                workspace.execute_environment_action(command, &self.cancellation, &check)
            }
            devforge_workspace::Command::SwitchDatabase { .. }
            | devforge_workspace::Command::RestoreBackup { .. } => {
                let expected = self
                    .intent
                    .database
                    .as_ref()
                    .ok_or_else(|| ws_error(ErrorCode::Unauthorized))?;
                let live = workspace.database_action_state(&command, &check)?;
                if live.fingerprint()? != expected.state.fingerprint()? {
                    return Err(ws_error(ErrorCode::ReviewMismatch));
                }
                workspace.database_action_preflight(&command)?;
                let _private = workspace.execute_database_action(
                    *command,
                    &expected.state,
                    &self.cancellation,
                    &check,
                )?;
                Ok(json!({"applied":true}))
            }
            _ => Err(ws_error(ErrorCode::UnsupportedCapability)),
        }
    }
}
impl Services {
    fn read_domain(
        self: &Arc<Self>,
        actor: &ActorHandle,
        peer: &PeerIdentity,
        credential: &ExternalCredential,
        project: &Id,
        options: (Option<DomainPlanKind>, Option<u64>),
    ) -> Result<crate::domain_control::DomainReadResult, ErrorCode> {
        let (kind, revision) = options;
        let policy = self.clone();
        let peer = peer.clone();
        let credential =
            ExternalCredential::from_wire(credential.expose_secret()).map_err(grant_error)?;
        let project_id = project.clone();
        let check = Box::new(move |workspace: &Workspace| {
            let binding = domain_registration_binding(workspace, &project_id).map_err(ws_error)?;
            let required = if kind == Some(DomainPlanKind::Cleanup) {
                &[Scope::ProjectManage, Scope::SensitiveOperation][..]
            } else {
                &[Scope::Observer][..]
            };
            policy
                .grants
                .registry
                .lock()
                .map_err(|_| ws_error(ErrorCode::StorageUnavailable))?
                .as_mut()
                .ok_or_else(|| ws_error(ErrorCode::Interrupted))?
                .authorize(&peer, &credential, &binding, required, &[], revision)
                .map_err(|e| ws_error(grant_error(e)))?;
            Ok(())
        });
        let (reply, receiver) = std::sync::mpsc::sync_channel(1);
        let cancelled = Arc::new(AtomicBool::new(false));
        actor
            .queue_domain(crate::domain_control::DomainRead {
                project: project.clone(),
                kind,
                check,
                cancelled: cancelled.clone(),
                reply,
            })
            .map_err(actor_error)?;
        match receiver.recv_timeout(Duration::from_secs(5)) {
            Ok(result) => result.map_err(environment_error),
            Err(_) => {
                cancelled.store(true, Ordering::Release);
                Err(ErrorCode::TimedOut)
            }
        }
    }
    fn domain_read(
        self: &Arc<Self>,
        peer: &PeerIdentity,
        credential: &ExternalCredential,
        project: &Id,
        _operation: DomainReadKind,
        actor: &ActorHandle,
    ) -> Result<DomainFacts, ErrorCode> {
        let data = self.read_domain(actor, peer, credential, project, (None, None))?;
        domain_facts(&data.facts)
    }
    fn domain_prepare(
        self: &Arc<Self>,
        peer: &PeerIdentity,
        credential: &ExternalCredential,
        project: &Id,
        kind: DomainPlanKind,
        actor: &ActorHandle,
    ) -> Result<DomainPlanPreview, ErrorCode> {
        let data = self.read_domain(actor, peer, credential, project, (Some(kind), None))?;
        let facts = domain_facts(&data.facts)?;
        let Some(plan) = data.plan else {
            return Ok(DomainPlanPreview {
                project_id: project.clone(),
                revision: facts.revision,
                kind,
                ticket: None,
                blocked: data.blocked.map(Id::new).transpose()?,
                ca_prepared: facts.ca_prepared,
                shared_ca_preserved: true,
            });
        };
        let view = actor.external_snapshot().map_err(actor_error)?;
        let binding = project_binding(&view.snapshot, project)?;
        if facts.revision.get() != binding.revision() {
            return Err(ErrorCode::ReviewMismatch);
        }
        let scopes = if kind == DomainPlanKind::Cleanup {
            vec![Scope::ProjectManage, Scope::SensitiveOperation]
        } else {
            vec![Scope::Observer]
        };
        let proof = self
            .grants
            .registry
            .lock()
            .map_err(|_| ErrorCode::StorageUnavailable)?
            .as_mut()
            .ok_or(ErrorCode::Interrupted)?
            .authorize(
                peer,
                credential,
                &binding,
                &scopes,
                &[],
                Some(binding.revision()),
            )
            .map_err(grant_error)?;
        let mut preparations = self
            .domain_preparations
            .lock()
            .map_err(|_| ErrorCode::StorageUnavailable)?;
        preparations.retain(|p| Instant::now() < p.expires);
        let bytes = 262144;
        if preparations.len() >= 128
            || bytes > MAX_PLAN_BYTES.saturating_sub(preparations.iter().map(|p| p.bytes).sum())
        {
            return Err(ErrorCode::Busy);
        }
        let ticket = crate::environment_control::opaque()?;
        preparations.push_back(DomainPreparation {
            ticket: ticket.clone(),
            client: proof.client_id().clone(),
            grant: proof.grant_id().clone(),
            binding,
            kind,
            plan,
            expires: Instant::now() + PLAN_TTL,
            bytes,
        });
        Ok(DomainPlanPreview {
            project_id: project.clone(),
            revision: facts.revision,
            kind,
            ticket: Some(ticket),
            blocked: None,
            ca_prepared: facts.ca_prepared,
            shared_ca_preserved: true,
        })
    }
    fn domain_plan(
        self: &Arc<Self>,
        peer: PeerIdentity,
        credential: ExternalCredential,
        request: Request<MutationInput>,
        actor: &ActorHandle,
    ) -> Result<PlanReceipt, ErrorCode> {
        let reference = domain_reference(&request.input)?.clone();
        let view = actor.external_snapshot().map_err(actor_error)?;
        let binding = project_binding(&view.snapshot, &request.project_id)?;
        request.validate(&binding, true)?;
        let proof = self.authorize(
            &peer,
            &credential,
            &binding,
            &request.input,
            request.expected_revision,
        )?;
        let (kind, prepared) = {
            let preparations = self
                .domain_preparations
                .lock()
                .map_err(|_| ErrorCode::StorageUnavailable)?;
            let p = preparations
                .iter()
                .find(|p| p.ticket == reference && Instant::now() < p.expires)
                .ok_or(ErrorCode::ReviewExpired)?;
            if p.client != *proof.client_id()
                || p.grant != *proof.grant_id()
                || p.binding.root() != binding.root()
                || p.binding.project_id() != binding.project_id()
                || p.binding.revision() != binding.revision()
                || matches!(request.input, MutationInput::DomainCleanup(_))
                    != (p.kind == DomainPlanKind::Cleanup)
            {
                return Err(ErrorCode::ReviewMismatch);
            }
            (p.kind, p.plan.clone())
        };
        let data = self.read_domain(
            actor,
            &peer,
            &credential,
            &request.project_id,
            (Some(kind), request.expected_revision),
        )?;
        let current = data.plan.ok_or(ErrorCode::PreconditionFailed)?;
        if current.fingerprint().map_err(environment_error)?
            != prepared.fingerprint().map_err(environment_error)?
        {
            return Err(ErrorCode::ReviewMismatch);
        }
        let backend = domain_backend(&view.snapshot, &binding, &request, &proof, &prepared)?;
        let fingerprint = sha256(&fingerprint_material(&request, &binding)?)?;
        let mut plans = self
            .plans
            .lock()
            .map_err(|_| ErrorCode::StorageUnavailable)?;
        plans.retain(|p| Instant::now() < p.expires);
        if let Some(existing) = plans.iter().find(|p| {
            p.domain.is_some()
                && p.client == *proof.client_id()
                && p.grant == *proof.grant_id()
                && p.request.idempotency_key == request.idempotency_key
                && p.fingerprint == fingerprint
                && p.backend == backend
        }) {
            return Ok(plan_receipt(existing));
        }
        let bytes = 262144;
        if plans.len() >= MAX_PLANS
            || bytes > MAX_PLAN_BYTES.saturating_sub(plans.iter().map(|p| p.bytes).sum())
        {
            return Err(ErrorCode::Busy);
        }
        let expires_at = proof.observed_at() + PLAN_TTL.as_millis() as u64;
        SafeInt::new(expires_at)?;
        let review = ReviewedPlan::record(
            &binding,
            Id::new(request.input.tool())?,
            backend.clone(),
            expires_at,
        )?;
        let snapshot_digest = domain_snapshot_digest(&view.snapshot, &request)?;
        let intent = Arc::new(Intent {
            request,
            binding,
            fingerprint,
            backend,
            review,
            client: proof.client_id().clone(),
            grant: proof.grant_id().clone(),
            peer,
            credential,
            expires_at,
            expires: Instant::now() + PLAN_TTL,
            approved: AtomicBool::new(false),
            claimed: Mutex::new(None),
            cancellation: Mutex::new(None),
            bytes,
            environment: None,
            public_digest: Some(crate::environment_control::opaque()?),
            public_fingerprint: None,
            database: None,
            domain: Some(crate::domain_control::DomainIntent {
                plan: prepared,
                kind,
                snapshot_digest,
                confirmation: Mutex::new(None),
            }),
        });
        let receipt = plan_receipt(&intent);
        plans.push_back(intent);
        Ok(receipt)
    }
    pub(crate) fn confirm_domain(
        self: &Arc<Self>,
        authority: &DesktopAuthority<'_>,
        digest: &Digest,
        actor: &ActorHandle,
    ) -> Result<(), ErrorCode> {
        authority
            .check_current(Instant::now())
            .map_err(|_| ErrorCode::Unauthorized)?;
        if authority.root() != actor.bootstrap_ready_root().map_err(actor_error)? {
            return Err(ErrorCode::Unauthorized);
        }
        let intent = self.intent(digest)?;
        if !intent.approved.load(Ordering::Acquire) || Instant::now() >= intent.expires {
            return Err(ErrorCode::Unauthorized);
        }
        let domain = intent
            .domain
            .as_ref()
            .ok_or(ErrorCode::UnsupportedCapability)?;
        let data = self.read_domain(
            actor,
            &intent.peer,
            &intent.credential,
            &intent.request.project_id,
            (Some(domain.kind), intent.request.expected_revision),
        )?;
        let plan = data.plan.ok_or(ErrorCode::PreconditionFailed)?;
        if plan.fingerprint().map_err(environment_error)?
            != domain.plan.fingerprint().map_err(environment_error)?
        {
            return Err(ErrorCode::ReviewMismatch);
        }
        authority
            .check_current(Instant::now())
            .map_err(|_| ErrorCode::Unauthorized)?;
        *domain
            .confirmation
            .lock()
            .map_err(|_| ErrorCode::StorageUnavailable)? = Some(
            domain
                .plan
                .confirm_native_owner()
                .map_err(environment_error)?,
        );
        Ok(())
    }
    fn database_choices(
        &self,
        peer: &PeerIdentity,
        credential: &ExternalCredential,
        project: &Id,
        actor: &ActorHandle,
    ) -> Result<DatabaseChoices, ErrorCode> {
        let view = actor.external_snapshot().map_err(actor_error)?;
        let binding = project_binding(&view.snapshot, project)?;
        let mut lock = self
            .grants
            .registry
            .lock()
            .map_err(|_| ErrorCode::StorageUnavailable)?;
        let registry = lock.as_mut().ok_or(ErrorCode::Interrupted)?;
        registry
            .authorize(peer, credential, &binding, &[Scope::Observer], &[], None)
            .map_err(grant_error)?;
        let row = view
            .snapshot
            .projects
            .iter()
            .find(|row| row.project.id == project.as_str())
            .ok_or(ErrorCode::ProjectMismatch)?;
        let current_instance_id = row
            .database
            .as_ref()
            .filter(|b| b.project_id == project.as_str())
            .map(|b| Id::new(&b.instance_id))
            .transpose()?;
        let mut instance_ids = row
            .database_history
            .iter()
            .filter(|b| b.project_id == project.as_str())
            .map(|b| Id::new(&b.instance_id))
            .collect::<Result<Vec<_>, _>>()?;
        if let Some(id) = &current_instance_id {
            instance_ids.push(id.clone());
        }
        instance_ids.sort_by(|a, b| a.as_str().cmp(b.as_str()));
        instance_ids.dedup();
        let backup_ids = row
            .backups
            .iter()
            .filter(|b| b.project == project.as_str())
            .map(|b| Id::new(&b.id))
            .collect::<Result<Vec<_>, _>>()?;
        if instance_ids.len() > 128 || backup_ids.len() > 128 {
            return Err(ErrorCode::OutputLimitExceeded);
        }
        registry
            .authorize(
                peer,
                credential,
                &binding,
                &[Scope::Observer],
                &[],
                Some(binding.revision()),
            )
            .map_err(grant_error)?;
        Ok(DatabaseChoices {
            project_id: project.clone(),
            revision: SafeInt::new(binding.revision())?,
            stopped: row.state == "stopped",
            current_instance_id,
            instance_ids,
            backup_ids,
        })
    }
    fn read_database(
        self: &Arc<Self>,
        actor: &ActorHandle,
        peer: &PeerIdentity,
        credential: &ExternalCredential,
        request: &Request<MutationInput>,
    ) -> Result<crate::database_control::DatabaseReadResult, ErrorCode> {
        database_only(&request.input)?;
        let policy = self.clone();
        let peer = peer.clone();
        let credential =
            ExternalCredential::from_wire(credential.expose_secret()).map_err(grant_error)?;
        let project = request.project_id.clone();
        let revision = request.expected_revision;
        let check = Box::new(move |workspace: &Workspace| {
            let snapshot = workspace.snapshot()?;
            let binding = project_binding(&snapshot, &project).map_err(ws_error)?;
            policy
                .grants
                .registry
                .lock()
                .map_err(|_| ws_error(ErrorCode::StorageUnavailable))?
                .as_mut()
                .ok_or_else(|| ws_error(ErrorCode::Interrupted))?
                .authorize(
                    &peer,
                    &credential,
                    &binding,
                    &[Scope::ProjectManage, Scope::SensitiveOperation],
                    &[],
                    revision,
                )
                .map_err(|e| ws_error(grant_error(e)))?;
            Ok(())
        });
        let (reply, receiver) = std::sync::mpsc::sync_channel(1);
        let cancelled = Arc::new(AtomicBool::new(false));
        actor
            .queue_database(crate::database_control::DatabaseRead {
                request: request.clone(),
                command: database_command(request)?,
                check,
                cancelled: cancelled.clone(),
                reply,
            })
            .map_err(actor_error)?;
        match receiver.recv_timeout(Duration::from_secs(5)) {
            Ok(result) => {
                let data = result.map_err(environment_error)?;
                check_private_serialized_size(&data.state)?;
                Ok(data)
            }
            Err(_) => {
                cancelled.store(true, Ordering::Release);
                Err(ErrorCode::TimedOut)
            }
        }
    }
    fn database_plan(
        self: &Arc<Self>,
        peer: PeerIdentity,
        credential: ExternalCredential,
        request: Request<MutationInput>,
        actor: &ActorHandle,
    ) -> Result<PlanReceipt, ErrorCode> {
        let data = self.read_database(actor, &peer, &credential, &request)?;
        let view = actor.external_snapshot().map_err(actor_error)?;
        let binding = project_binding(&view.snapshot, &request.project_id)?;
        request.validate(&binding, true)?;
        cached_preflight(&view.snapshot, &request)?;
        let proof = self.authorize(
            &peer,
            &credential,
            &binding,
            &request.input,
            request.expected_revision,
        )?;
        let fingerprint = sha256(&fingerprint_material(&request, &binding)?)?;
        let backend = database_backend(&view.snapshot, &binding, &request, &proof, &data)?;
        let mut plans = self
            .plans
            .lock()
            .map_err(|_| ErrorCode::StorageUnavailable)?;
        plans.retain(|p| Instant::now() < p.expires);
        if let Some(existing) = plans.iter().find(|p| {
            p.database.is_some()
                && p.client == *proof.client_id()
                && p.grant == *proof.grant_id()
                && p.request.idempotency_key == request.idempotency_key
                && p.fingerprint == fingerprint
                && p.backend == backend
        }) {
            return Ok(plan_receipt(existing));
        }
        let bytes = binding
            .root()
            .as_os_str()
            .as_encoded_bytes()
            .len()
            .saturating_mul(4)
            .saturating_add(131072);
        if plans.len() >= MAX_PLANS
            || bytes > MAX_PLAN_BYTES.saturating_sub(plans.iter().map(|p| p.bytes).sum())
        {
            return Err(ErrorCode::Busy);
        }
        let expires_at = proof.observed_at() + PLAN_TTL.as_millis() as u64;
        SafeInt::new(expires_at)?;
        let review = ReviewedPlan::record(
            &binding,
            Id::new(request.input.tool())?,
            backend.clone(),
            expires_at,
        )?;
        let intent = Arc::new(Intent {
            request,
            binding,
            fingerprint,
            backend,
            review,
            client: proof.client_id().clone(),
            grant: proof.grant_id().clone(),
            peer,
            credential,
            expires_at,
            expires: Instant::now() + PLAN_TTL,
            approved: AtomicBool::new(false),
            claimed: Mutex::new(None),
            cancellation: Mutex::new(None),
            bytes,
            environment: None,
            public_digest: Some(crate::environment_control::opaque()?),
            public_fingerprint: None,
            database: Some(data),
            domain: None,
        });
        let receipt = plan_receipt(&intent);
        plans.push_back(intent);
        Ok(receipt)
    }
    fn read_environment(
        self: &Arc<Self>,
        actor: &ActorHandle,
        peer: &PeerIdentity,
        credential: &ExternalCredential,
        project: &Id,
        options: (Option<u64>, Option<bool>, &[Scope]),
    ) -> Result<crate::environment_control::PrivateEnvironment, ErrorCode> {
        let (revision, write_env, scopes) = options;
        let policy = self.clone();
        let peer = peer.clone();
        let credential =
            ExternalCredential::from_wire(credential.expose_secret()).map_err(grant_error)?;
        let project_id = project.clone();
        let scopes = scopes.to_vec();
        let check = Box::new(move |workspace: &Workspace| {
            let snapshot = workspace.snapshot()?;
            let binding = project_binding(&snapshot, &project_id).map_err(ws_error)?;
            let mut registry = policy
                .grants
                .registry
                .lock()
                .map_err(|_| ws_error(ErrorCode::StorageUnavailable))?;
            registry
                .as_mut()
                .ok_or_else(|| ws_error(ErrorCode::Interrupted))?
                .authorize(&peer, &credential, &binding, &scopes, &[], revision)
                .map_err(|e| ws_error(grant_error(e)))?;
            Ok(())
        });
        let (reply, receive) = std::sync::mpsc::sync_channel(1);
        let cancelled = Arc::new(AtomicBool::new(false));
        actor
            .queue_environment(crate::environment_control::EnvironmentRead {
                project: project.clone(),
                write_env,
                check,
                cancelled: cancelled.clone(),
                reply,
            })
            .map_err(actor_error)?;
        match receive.recv_timeout(Duration::from_secs(5)) {
            Ok(result) => {
                let data = result.map_err(environment_error)?;
                check_private_environment_size(&data)?;
                Ok(data)
            }
            Err(_) => {
                cancelled.store(true, Ordering::Release);
                Err(ErrorCode::TimedOut)
            }
        }
    }
    fn env_document(
        self: &Arc<Self>,
        peer: &PeerIdentity,
        credential: &ExternalCredential,
        project: &Id,
        actor: &ActorHandle,
    ) -> Result<EnvDocumentState, ErrorCode> {
        let data = self.read_environment(
            actor,
            peer,
            credential,
            project,
            (None, None, &[Scope::Observer]),
        )?;
        let proof = self
            .grants
            .registry
            .lock()
            .map_err(|_| ErrorCode::StorageUnavailable)?
            .as_mut()
            .ok_or(ErrorCode::Interrupted)?
            .authorize(
                peer,
                credential,
                &data.binding,
                &[Scope::Observer],
                &[],
                Some(data.binding.revision()),
            )
            .map_err(grant_error)?;
        self.environment_versions
            .lock()
            .map_err(|_| ErrorCode::StorageUnavailable)?
            .record(&data, proof.client_id(), proof.grant_id())
    }
    fn env_preview(
        self: &Arc<Self>,
        peer: PeerIdentity,
        credential: ExternalCredential,
        request: Request<MutationInput>,
        actor: &ActorHandle,
    ) -> Result<EnvPreview, ErrorCode> {
        let MutationInput::EnvironmentPlan(input) = &request.input else {
            return Err(ErrorCode::UnsupportedCapability);
        };
        let data = self.read_environment(
            actor,
            &peer,
            &credential,
            &request.project_id,
            (
                request.expected_revision,
                Some(input.write_env),
                &[Scope::Observer],
            ),
        )?;
        request.validate(&data.binding, false)?;
        let proof = self.authorize(
            &peer,
            &credential,
            &data.binding,
            &request.input,
            request.expected_revision,
        )?;
        let document = self
            .environment_versions
            .lock()
            .map_err(|_| ErrorCode::StorageUnavailable)?
            .record(&data, proof.client_id(), proof.grant_id())?;
        let (package_ids, download_ids, blockers) = environment_summary(&data)?;
        let receipt = if data.plan.is_some() {
            Some(PlanReceipt {
                project_id: request.project_id.clone(),
                revision: SafeInt::new(data.binding.revision())?,
                digest: crate::environment_control::opaque()?,
                request_fingerprint: crate::environment_control::opaque()?,
                expires_at: SafeInt::new(proof.observed_at() + PLAN_TTL.as_millis() as u64)?,
                blocked: !blockers.is_empty(),
            })
        } else {
            None
        };
        Ok(EnvPreview {
            document,
            receipt,
            write_env: input.write_env,
            yaml_bytes: None,
            package_ids,
            download_ids,
            blockers,
        })
    }
    fn env_plan(
        self: &Arc<Self>,
        peer: PeerIdentity,
        credential: ExternalCredential,
        request: Request<MutationInput>,
        actor: &ActorHandle,
    ) -> Result<EnvPreview, ErrorCode> {
        environment_only(&request.input)?;
        check_yaml_size(&request.input)?;
        let data = self.read_environment(
            actor,
            &peer,
            &credential,
            &request.project_id,
            (
                request.expected_revision,
                environment_write_env(&request.input),
                &required(&request.input)?,
            ),
        )?;
        request.validate(&data.binding, true)?;
        let proof = self.authorize(
            &peer,
            &credential,
            &data.binding,
            &request.input,
            request.expected_revision,
        )?;
        let mut versions = self
            .environment_versions
            .lock()
            .map_err(|_| ErrorCode::StorageUnavailable)?;
        let document = versions.record(&data, proof.client_id(), proof.grant_id())?;
        let expected_sha = if let MutationInput::YamlSave(input) = &request.input {
            devforge_projects::manifest::Manifest::parse(input.text.as_str().as_bytes())
                .map_err(|_| ErrorCode::InvalidInput)?;
            versions.resolve(
                input.expected.as_ref(),
                &data,
                proof.client_id(),
                proof.grant_id(),
            )?
        } else {
            None
        };
        drop(versions);
        let (package_ids, download_ids, mut blockers) = environment_summary(&data)?;
        if matches!(request.input, MutationInput::YamlSave(_)) {
            blockers.clear();
        }
        let view = actor.external_snapshot().map_err(actor_error)?;
        if view
            .snapshot
            .projects
            .iter()
            .any(|r| r.project.id == request.project_id.as_str() && r.state != "stopped")
        {
            blockers.push(Id::new("setup_stop_first")?);
        }
        if matches!(request.input, MutationInput::EnvironmentApply(_)) && data.plan.is_none() {
            return Ok(EnvPreview {
                document,
                receipt: None,
                write_env: matches!(&request.input,MutationInput::EnvironmentApply(input)if input.write_env),
                yaml_bytes: None,
                package_ids,
                download_ids,
                blockers,
            });
        }
        let backend = environment_backend(&view.snapshot, &data.binding, &request, &proof, &data)?;
        let fingerprint = sha256(&fingerprint_material(&request, &data.binding)?)?;
        let yaml_bytes = if let MutationInput::YamlSave(input) = &request.input {
            Some(SafeInt::new(input.text.as_str().len() as u64)?)
        } else {
            None
        };
        let write_env =
            matches!(&request.input,MutationInput::EnvironmentApply(input)if input.write_env);
        let bytes = data
            .binding
            .root()
            .as_os_str()
            .as_encoded_bytes()
            .len()
            .saturating_mul(4)
            .saturating_add(131072);
        let mut plans = self
            .plans
            .lock()
            .map_err(|_| ErrorCode::StorageUnavailable)?;
        plans.retain(|intent| Instant::now() < intent.expires);
        if let Some(existing) = plans.iter().find(|intent| {
            intent.environment.is_some()
                && intent.client == *proof.client_id()
                && intent.grant == *proof.grant_id()
                && intent.request.idempotency_key == request.idempotency_key
                && intent.fingerprint == fingerprint
                && intent.backend == backend
        }) {
            return Ok(EnvPreview {
                document,
                receipt: Some(plan_receipt(existing)),
                write_env,
                yaml_bytes,
                package_ids,
                download_ids,
                blockers,
            });
        }
        if plans.len() >= MAX_PLANS
            || bytes > MAX_PLAN_BYTES.saturating_sub(plans.iter().map(|p| p.bytes).sum())
        {
            return Err(ErrorCode::Busy);
        }
        let expires_at = proof.observed_at() + PLAN_TTL.as_millis() as u64;
        SafeInt::new(expires_at)?;
        let review = ReviewedPlan::record(
            &data.binding,
            Id::new(request.input.tool())?,
            backend.clone(),
            expires_at,
        )?;
        let intent = Arc::new(Intent {
            binding: data.binding.clone(),
            request,
            fingerprint,
            backend,
            review,
            client: proof.client_id().clone(),
            grant: proof.grant_id().clone(),
            peer,
            credential,
            expires_at,
            expires: Instant::now() + PLAN_TTL,
            approved: AtomicBool::new(false),
            claimed: Mutex::new(None),
            cancellation: Mutex::new(None),
            bytes,
            environment: Some(crate::environment_control::EnvironmentIntent { data, expected_sha }),
            public_digest: Some(crate::environment_control::opaque()?),
            public_fingerprint: Some(crate::environment_control::opaque()?),
            database: None,
            domain: None,
        });
        let receipt = plan_receipt(&intent);
        plans.push_back(intent);
        Ok(EnvPreview {
            document,
            receipt: Some(receipt),
            write_env,
            yaml_bytes,
            package_ids,
            download_ids,
            blockers,
        })
    }
    pub(crate) fn new(grants: Arc<Grants>) -> Self {
        Self {
            grants,
            plans: Mutex::new(VecDeque::new()),
            environment_versions: Mutex::new(Default::default()),
            domain_preparations: Mutex::new(VecDeque::new()),
        }
    }
    pub(crate) fn handles(method: &str) -> bool {
        matches!(
            method,
            "service_plan"
                | "service_apply"
                | "service_lookup"
                | "runtime_plan"
                | "runtime_apply"
                | "runtime_lookup"
                | "runtime_selection"
                | "env_preview"
                | "env_document_state"
                | "env_plan"
                | "env_apply"
                | "env_lookup"
                | "db_choices"
                | "db_plan"
                | "db_apply"
                | "db_lookup"
                | "domain_read"
                | "domain_prepare"
                | "domain_plan"
                | "domain_apply"
                | "domain_lookup"
        )
    }
    pub(crate) fn request(
        self: &Arc<Self>,
        peer: PeerIdentity,
        value: Value,
        actor: &ActorHandle,
        rejected: Option<ErrorCode>,
    ) -> Value {
        let id = value
            .get("request")
            .and_then(|r| r.get("request_id"))
            .or_else(|| value.get("request_id"))
            .and_then(Value::as_str)
            .and_then(|v| Id::new(v).ok())
            .unwrap_or_else(|| Id::new("invalid-request").expect("fixedid"));
        if let Some(code) = rejected {
            return respond::<PlanReceipt>(id, Err(code), None);
        }
        if value
            .get("request")
            .and_then(|r| r.get("api_version"))
            .and_then(Value::as_u64)
            .is_some_and(|v| v != 1)
        {
            return respond::<PlanReceipt>(id, Err(ErrorCode::UnsupportedVersion), None);
        }
        let wire: Wire = match serde_json::from_value(value) {
            Ok(wire) => wire,
            Err(_) => return respond::<PlanReceipt>(id, Err(ErrorCode::InvalidInput), None),
        };
        match wire {
            Wire::DomainRead {
                credential,
                request_id,
                project_id,
                operation,
            } => respond(
                request_id,
                credential
                    .parse()
                    .and_then(|c| self.domain_read(&peer, &c, &project_id, operation, actor)),
                None,
            ),
            Wire::DomainPrepare {
                credential,
                request_id,
                project_id,
                kind,
            } => respond(
                request_id,
                credential
                    .parse()
                    .and_then(|c| self.domain_prepare(&peer, &c, &project_id, kind, actor)),
                None,
            ),
            Wire::DomainPlan {
                credential,
                request,
            } => respond(
                id,
                domain_only(&request.input).and_then(|_| {
                    credential
                        .parse()
                        .and_then(|c| self.domain_plan(peer, c, *request, actor))
                }),
                None,
            ),
            Wire::DomainApply {
                credential,
                request,
                review,
            } => {
                let result = domain_only(&request.input).and_then(|_| {
                    credential
                        .parse()
                        .and_then(|c| self.apply(peer, c, *request, review, actor))
                });
                let op = result.as_ref().ok().map(|r| r.operation_id.clone());
                respond(id, result, op)
            }
            Wire::DomainLookup {
                credential,
                request_id,
                project_id,
                idempotency_key,
            } => {
                let result = credential
                    .parse()
                    .and_then(|c| self.lookup(&peer, &c, &project_id, &idempotency_key, actor));
                let op = result
                    .as_ref()
                    .ok()
                    .and_then(|r| r.as_ref().map(|r| r.operation_id.clone()));
                respond(request_id, result, op)
            }
            Wire::DatabaseChoices {
                credential,
                request_id,
                project_id,
            } => respond(
                request_id,
                credential
                    .parse()
                    .and_then(|c| self.database_choices(&peer, &c, &project_id, actor)),
                None,
            ),
            Wire::DatabasePlan {
                credential,
                request,
            } => respond(
                id,
                database_only(&request.input).and_then(|_| {
                    credential
                        .parse()
                        .and_then(|c| self.database_plan(peer, c, *request, actor))
                }),
                None,
            ),
            Wire::DatabaseApply {
                credential,
                request,
                review,
            } => {
                let result = database_only(&request.input).and_then(|_| {
                    credential
                        .parse()
                        .and_then(|c| self.apply(peer, c, *request, review, actor))
                });
                let op = result.as_ref().ok().map(|r| r.operation_id.clone());
                respond(id, result, op)
            }
            Wire::DatabaseLookup {
                credential,
                request_id,
                project_id,
                idempotency_key,
            } => {
                let result = credential
                    .parse()
                    .and_then(|c| self.lookup(&peer, &c, &project_id, &idempotency_key, actor));
                let op = result
                    .as_ref()
                    .ok()
                    .and_then(|r| r.as_ref().map(|r| r.operation_id.clone()));
                respond(request_id, result, op)
            }
            Wire::EnvDocument {
                credential,
                request_id,
                project_id,
            } => respond(
                request_id,
                credential
                    .parse()
                    .and_then(|c| self.env_document(&peer, &c, &project_id, actor)),
                None,
            ),
            Wire::EnvPreview {
                credential,
                request,
            } => respond(
                id,
                credential
                    .parse()
                    .and_then(|c| self.env_preview(peer, c, *request, actor)),
                None,
            ),
            Wire::EnvPlan {
                credential,
                request,
            } => respond(
                id,
                credential
                    .parse()
                    .and_then(|c| self.env_plan(peer, c, *request, actor)),
                None,
            ),
            Wire::EnvApply {
                credential,
                request,
                review,
            } => {
                let result = environment_only(&request.input)
                    .and_then(|_| check_yaml_size(&request.input))
                    .and_then(|_| {
                        credential
                            .parse()
                            .and_then(|c| self.apply(peer, c, *request, review, actor))
                    });
                let op = result.as_ref().ok().map(|r| r.operation_id.clone());
                respond(id, result, op)
            }
            Wire::EnvLookup {
                credential,
                request_id,
                project_id,
                idempotency_key,
            } => {
                let result = credential
                    .parse()
                    .and_then(|c| self.lookup(&peer, &c, &project_id, &idempotency_key, actor));
                let op = result
                    .as_ref()
                    .ok()
                    .and_then(|r| r.as_ref().map(|r| r.operation_id.clone()));
                respond(request_id, result, op)
            }
            Wire::Plan {
                credential,
                request,
            } => respond(
                id,
                service_only(&request.input).and_then(|_| {
                    credential
                        .parse()
                        .and_then(|c| self.plan(peer, c, *request, actor))
                }),
                None,
            ),
            Wire::Apply {
                credential,
                request,
                review,
            } => {
                let result = service_only(&request.input).and_then(|_| {
                    credential
                        .parse()
                        .and_then(|c| self.apply(peer, c, *request, review, actor))
                });
                let op = result.as_ref().ok().map(|r| r.operation_id.clone());
                respond(id, result, op)
            }
            Wire::Lookup {
                credential,
                request_id,
                project_id,
                idempotency_key,
            } => {
                let result = credential
                    .parse()
                    .and_then(|c| self.lookup(&peer, &c, &project_id, &idempotency_key, actor));
                let op = result
                    .as_ref()
                    .ok()
                    .and_then(|r| r.as_ref().map(|r| r.operation_id.clone()));
                respond(request_id, result, op)
            }
            Wire::RuntimePlan {
                credential,
                request,
            } => respond(
                id,
                runtime_only(&request.input).and_then(|_| {
                    credential
                        .parse()
                        .and_then(|c| self.plan(peer, c, *request, actor))
                }),
                None,
            ),
            Wire::RuntimeApply {
                credential,
                request,
                review,
            } => {
                let result = runtime_only(&request.input).and_then(|_| {
                    credential
                        .parse()
                        .and_then(|c| self.apply(peer, c, *request, review, actor))
                });
                let op = result.as_ref().ok().map(|r| r.operation_id.clone());
                respond(id, result, op)
            }
            Wire::RuntimeLookup {
                credential,
                request_id,
                project_id,
                idempotency_key,
            } => {
                let result = credential
                    .parse()
                    .and_then(|c| self.lookup(&peer, &c, &project_id, &idempotency_key, actor));
                let op = result
                    .as_ref()
                    .ok()
                    .and_then(|r| r.as_ref().map(|r| r.operation_id.clone()));
                respond(request_id, result, op)
            }
            Wire::RuntimeSelection {
                credential,
                request_id,
                project_id,
            } => respond(
                request_id,
                credential
                    .parse()
                    .and_then(|c| self.selection(&peer, &c, &project_id, actor)),
                None,
            ),
        }
    }
    pub(crate) fn clear(&self) {
        self.plans
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clear();
        self.environment_versions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clear();
        self.domain_preparations
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clear();
    }
    fn selection(
        &self,
        peer: &PeerIdentity,
        credential: &ExternalCredential,
        project: &Id,
        actor: &ActorHandle,
    ) -> Result<RuntimeSelection, ErrorCode> {
        let view = actor
            .external_snapshot()
            .map_err(|_| ErrorCode::StorageUnavailable)?;
        let binding = project_binding(&view.snapshot, project)?;
        let mut grants = self
            .grants
            .registry
            .lock()
            .map_err(|_| ErrorCode::StorageUnavailable)?;
        let registry = grants.as_mut().ok_or(ErrorCode::Interrupted)?;
        registry
            .authorize(
                peer,
                credential,
                &binding,
                &[Scope::ProjectManage, Scope::ExecutionTest],
                &[],
                None,
            )
            .map_err(grant_error)?;
        let selection = selection_projection(&view.snapshot, project)?;
        registry
            .authorize(
                peer,
                credential,
                &binding,
                &[Scope::ProjectManage, Scope::ExecutionTest],
                &[],
                Some(selection.revision.get()),
            )
            .map_err(grant_error)?;
        Ok(selection)
    }
    pub(crate) fn revoked(&self, grant: &Id) {
        if let Ok(plans) = self.plans.lock() {
            for plan in plans.iter().filter(|p| &p.grant == grant) {
                if let Ok(cancel) = plan.cancellation.lock()
                    && let Some(cancel) = cancel.as_ref()
                {
                    cancel.cancel();
                }
            }
        }
    }
    pub(crate) fn approve(
        self: &Arc<Self>,
        authority: &DesktopAuthority<'_>,
        digest: &Digest,
        actor: &ActorHandle,
    ) -> Result<PlanReceipt, ErrorCode> {
        authority
            .check_current(Instant::now())
            .map_err(|_| ErrorCode::Unauthorized)?;
        let intent = self.intent(digest)?;
        if authority.root()
            != actor
                .bootstrap_ready_root()
                .map_err(|_| ErrorCode::Interrupted)?
        {
            return Err(ErrorCode::Unauthorized);
        }
        let view = actor
            .external_snapshot()
            .map_err(|_| ErrorCode::StorageUnavailable)?;
        let current = project_binding(&view.snapshot, &intent.request.project_id)?;
        let proof = self.authorize(
            &intent.peer,
            &intent.credential,
            &current,
            &intent.request.input,
            intent.request.expected_revision,
        )?;
        let current_digest = if let Some(domain) = &intent.domain {
            let data = self.read_domain(
                actor,
                &intent.peer,
                &intent.credential,
                &intent.request.project_id,
                (Some(domain.kind), intent.request.expected_revision),
            )?;
            let plan = data.plan.as_ref().ok_or(ErrorCode::PreconditionFailed)?;
            domain_backend(&view.snapshot, &current, &intent.request, &proof, plan)?
        } else if intent.database.is_some() {
            let data =
                self.read_database(actor, &intent.peer, &intent.credential, &intent.request)?;
            database_backend(&view.snapshot, &current, &intent.request, &proof, &data)?
        } else if intent.environment.is_some() {
            let data = self.read_environment(
                actor,
                &intent.peer,
                &intent.credential,
                &intent.request.project_id,
                (
                    intent.request.expected_revision,
                    environment_write_env(&intent.request.input),
                    &required(&intent.request.input)?,
                ),
            )?;
            if data
                .plan
                .as_ref()
                .is_some_and(|plan| !plan.blockers.is_empty())
            {
                return Err(ErrorCode::PreconditionFailed);
            }
            environment_backend(&view.snapshot, &current, &intent.request, &proof, &data)?
        } else {
            state_digest(&view.snapshot, &current, &intent.request, &proof)?
        };
        if current_digest != intent.backend {
            return Err(ErrorCode::ReviewMismatch);
        }
        if Instant::now() >= intent.expires {
            return Err(ErrorCode::ReviewExpired);
        }
        authority
            .check_current(Instant::now())
            .map_err(|_| ErrorCode::Unauthorized)?;
        intent.approved.store(true, Ordering::Release);
        Ok(plan_receipt(&intent))
    }
    fn intent(&self, digest: &Digest) -> Result<Arc<Intent>, ErrorCode> {
        self.plans
            .lock()
            .map_err(|_| ErrorCode::StorageUnavailable)?
            .iter()
            .find(|intent| intent.public_digest.as_ref().unwrap_or(&intent.backend) == digest)
            .cloned()
            .ok_or(ErrorCode::ReviewExpired)
    }
    fn authorize(
        &self,
        peer: &PeerIdentity,
        credential: &ExternalCredential,
        binding: &ProjectBinding,
        input: &MutationInput,
        revision: Option<u64>,
    ) -> Result<AuthorizedExternal, ErrorCode> {
        self.grants
            .registry
            .lock()
            .map_err(|_| ErrorCode::StorageUnavailable)?
            .as_mut()
            .ok_or(ErrorCode::Interrupted)?
            .authorize(peer, credential, binding, &required(input)?, &[], revision)
            .map_err(grant_error)
    }
    pub(crate) fn plan(
        self: &Arc<Self>,
        peer: PeerIdentity,
        credential: ExternalCredential,
        request: Request<MutationInput>,
        actor: &ActorHandle,
    ) -> Result<PlanReceipt, ErrorCode> {
        required(&request.input)?;
        let view = actor
            .external_snapshot()
            .map_err(|_| ErrorCode::StorageUnavailable)?;
        let binding = project_binding(&view.snapshot, &request.project_id)?;
        request.validate(&binding, true)?;
        let proof = self.authorize(
            &peer,
            &credential,
            &binding,
            &request.input,
            request.expected_revision,
        )?;
        cached_preflight(&view.snapshot, &request)?;
        let fingerprint = sha256(&fingerprint_material(&request, &binding)?)?;
        let backend = state_digest(&view.snapshot, &binding, &request, &proof)?;
        let mut plans = self
            .plans
            .lock()
            .map_err(|_| ErrorCode::StorageUnavailable)?;
        plans.retain(|p| Instant::now() < p.expires);
        if let Some(existing) = plans.iter().find(|p| p.backend == backend) {
            return Ok(plan_receipt(existing));
        }
        let bytes = binding
            .root()
            .as_os_str()
            .as_encoded_bytes()
            .len()
            .saturating_mul(4)
            .saturating_add(4096);
        if plans.len() >= MAX_PLANS
            || bytes > MAX_PLAN_BYTES.saturating_sub(plans.iter().map(|p| p.bytes).sum())
        {
            return Err(ErrorCode::Busy);
        }
        let expires_at = proof
            .observed_at()
            .checked_add(PLAN_TTL.as_millis() as u64)
            .ok_or(ErrorCode::StorageUnavailable)?;
        SafeInt::new(binding.revision())?;
        SafeInt::new(expires_at)?;
        let review = ReviewedPlan::record(
            &binding,
            Id::new(request.input.tool())?,
            backend.clone(),
            expires_at,
        )?;
        let intent = Arc::new(Intent {
            request,
            binding,
            fingerprint,
            backend,
            review,
            client: proof.client_id().clone(),
            grant: proof.grant_id().clone(),
            peer,
            credential,
            expires_at,
            expires: Instant::now() + PLAN_TTL,
            approved: AtomicBool::new(false),
            claimed: Mutex::new(None),
            cancellation: Mutex::new(None),
            bytes,
            environment: None,
            public_digest: None,
            public_fingerprint: None,
            database: None,
            domain: None,
        });
        let receipt = plan_receipt(&intent);
        plans.push_back(intent);
        Ok(receipt)
    }
    pub(crate) fn apply(
        self: &Arc<Self>,
        peer: PeerIdentity,
        credential: ExternalCredential,
        request: Request<MutationInput>,
        review: Digest,
        actor: &ActorHandle,
    ) -> Result<MutationReceipt, ErrorCode> {
        let view = actor
            .external_snapshot()
            .map_err(|_| ErrorCode::StorageUnavailable)?;
        let current = project_binding(&view.snapshot, &request.project_id)?;
        let proof = self.authorize(&peer, &credential, &current, &request.input, None)?;
        // A retry first reconciles its original intent without executing again. Its
        // original revision participates in the same canonical contract fingerprint.
        let old = ProjectBinding::from_registered(
            request.project_id.clone(),
            current.root(),
            request.expected_revision.ok_or(ErrorCode::InvalidInput)?,
        )?;
        let fingerprint = sha256(&fingerprint_material(&request, &old)?)?;
        if let Some((receipt, stored, revision)) = actor
            .external_find(
                proof.client_id(),
                &request.project_id,
                request
                    .idempotency_key
                    .as_ref()
                    .ok_or(ErrorCode::InvalidInput)?,
            )
            .map_err(actor_error)?
        {
            if matches!(request.input, MutationInput::YamlSave(_))
                && stored.starts_with("volatile-secret-v1:")
            {
                if !actor
                    .external_fingerprint_matches(&receipt.operation_id, fingerprint.as_str())
                    .map_err(actor_error)?
                {
                    return Err(ErrorCode::IdempotencyConflict);
                }
                return mutation_receipt(receipt, revision);
            }
            if stored != fingerprint.as_str() || matches!(request.input, MutationInput::YamlSave(_))
            {
                return Err(ErrorCode::IdempotencyConflict);
            }
            return mutation_receipt(receipt, revision);
        }
        let intent = self.intent(&review)?;
        if let Some(domain) = &intent.domain {
            if domain
                .confirmation
                .lock()
                .map_err(|_| ErrorCode::StorageUnavailable)?
                .is_none()
            {
                return Err(ErrorCode::Unauthorized);
            }
            let data = self.read_domain(
                actor,
                &peer,
                &credential,
                &request.project_id,
                (Some(domain.kind), request.expected_revision),
            )?;
            let plan = data.plan.as_ref().ok_or(ErrorCode::PreconditionFailed)?;
            if domain_backend(&view.snapshot, &current, &request, &proof, plan)? != intent.backend {
                return Err(ErrorCode::ReviewMismatch);
            }
        }
        if intent.database.is_some() {
            let data = self.read_database(actor, &peer, &credential, &request)?;
            if database_backend(&view.snapshot, &current, &request, &proof, &data)?
                != intent.backend
            {
                return Err(ErrorCode::ReviewMismatch);
            }
        }
        cached_preflight(&view.snapshot, &request)?;
        if intent.client != *proof.client_id()
            || intent.grant != *proof.grant_id()
            || intent.fingerprint != fingerprint
            || intent.request.idempotency_key != request.idempotency_key
        {
            return Err(ErrorCode::ReviewMismatch);
        }
        let job = ServiceJob {
            intent,
            policy: self.clone(),
            deadline: Instant::now()
                + Duration::from_millis(request.input.limits().timeout_ms.get()),
            cancellation: Cancellation::default(),
        };
        job.check_basic()?;
        job.validate(&view.snapshot, true)?;
        // Admission takes registry -> actor-state, the same order as external
        // status/cancel. Never wait for registry while holding actor-state.
        let mut grants = self
            .grants
            .registry
            .lock()
            .map_err(|_| ErrorCode::StorageUnavailable)?;
        let receipt = actor.submit_service(job, grants.as_mut().ok_or(ErrorCode::Interrupted)?)?;
        mutation_receipt(receipt, current.revision())
    }
    pub(crate) fn lookup(
        &self,
        peer: &PeerIdentity,
        credential: &ExternalCredential,
        project: &Id,
        key: &Id,
        actor: &ActorHandle,
    ) -> Result<Option<MutationReceipt>, ErrorCode> {
        let view = actor
            .external_snapshot()
            .map_err(|_| ErrorCode::StorageUnavailable)?;
        let binding = project_binding(&view.snapshot, project)?;
        let proof = self
            .grants
            .registry
            .lock()
            .map_err(|_| ErrorCode::StorageUnavailable)?
            .as_mut()
            .ok_or(ErrorCode::Interrupted)?
            .authorize(peer, credential, &binding, &[Scope::Observer], &[], None)
            .map_err(grant_error)?;
        actor
            .external_find(proof.client_id(), project, key)
            .map_err(actor_error)?
            .map(|(r, _, revision)| mutation_receipt(r, revision))
            .transpose()
    }
}
fn required(input: &MutationInput) -> Result<Vec<Scope>, ErrorCode> {
    match input {
        MutationInput::Start(_) | MutationInput::Restart(_) => {
            Ok(vec![Scope::ProjectManage, Scope::ExecutionTest])
        }
        MutationInput::Stop(_) => Ok(vec![Scope::ProjectManage]),
        MutationInput::RuntimeInstall(_) => Ok(vec![Scope::ProjectManage]),
        MutationInput::Configure(_) => Ok(vec![Scope::ProjectManage, Scope::ExecutionTest]),
        MutationInput::EnvironmentPlan(_) => Ok(vec![Scope::Observer]),
        MutationInput::EnvironmentApply(_) => Ok(vec![
            Scope::ProjectManage,
            Scope::SensitiveOperation,
            Scope::ExecutionTest,
        ]),
        MutationInput::YamlSave(_) => Ok(vec![Scope::ProjectManage, Scope::SensitiveOperation]),
        MutationInput::DatabaseBind(_) | MutationInput::DatabaseRestore(_) => {
            Ok(vec![Scope::ProjectManage, Scope::SensitiveOperation])
        }
        MutationInput::DomainApply(_) | MutationInput::DomainCleanup(_) => {
            Ok(vec![Scope::ProjectManage, Scope::SensitiveOperation])
        }
        _ => Err(ErrorCode::UnsupportedCapability),
    }
}
fn cached_preflight(
    snapshot: &Snapshot,
    request: &Request<MutationInput>,
) -> Result<(), ErrorCode> {
    if matches!(
        request.input,
        MutationInput::Start(_) | MutationInput::Restart(_)
    ) {
        devforge_workspace::service_control::cached_preflight(
            snapshot,
            request.project_id.as_str(),
        )
        .map_err(|_| ErrorCode::PreconditionFailed)?;
    }
    if let MutationInput::RuntimeInstall(input) = &request.input {
        let catalog = devforge_projects::project::Catalog::bundled();
        let package = catalog
            .get(input.package_id.as_str())
            .map_err(|_| ErrorCode::InvalidInput)?;
        if package.target
            != devforge_runtime::package::Target::current()
                .map_err(|_| ErrorCode::PreconditionFailed)?
        {
            return Err(ErrorCode::PreconditionFailed);
        }
        package.key().map_err(|_| ErrorCode::InvalidInput)?;
    }
    if let MutationInput::Configure(input) = &request.input {
        let command = devforge_workspace::Command::Configure {
            id: request.project_id.as_str().into(),
            revision: request.expected_revision.ok_or(ErrorCode::InvalidInput)?,
            name: input.name.as_str().into(),
            php: input.php.as_str().into(),
            node: input.node.as_str().into(),
            domain: input.domain.as_str().into(),
            https: input.https,
            vite: input.vite,
            queue: input.queue,
        };
        devforge_workspace::runtime_control::validate_selection_snapshot(snapshot, &command)
            .map_err(|error| match error.code.as_str() {
                "runtime_selection_fields_changed" => ErrorCode::InvalidInput,
                "stale_revision" => ErrorCode::RevisionConflict,
                _ => ErrorCode::PreconditionFailed,
            })?;
    }
    check_yaml_size(&request.input)?;
    if matches!(
        request.input,
        MutationInput::DatabaseBind(_) | MutationInput::DatabaseRestore(_)
    ) {
        if snapshot
            .projects
            .iter()
            .any(|row| row.project.id == request.project_id.as_str() && row.state != "stopped")
        {
            return Err(ErrorCode::PreconditionFailed);
        }
        database_snapshot_digest(snapshot, request)?;
    }
    if matches!(
        request.input,
        MutationInput::EnvironmentApply(_) | MutationInput::YamlSave(_)
    ) && snapshot
        .projects
        .iter()
        .any(|r| r.project.id == request.project_id.as_str() && r.state != "stopped")
    {
        return Err(ErrorCode::PreconditionFailed);
    }
    Ok(())
}
fn service_only(input: &MutationInput) -> Result<(), ErrorCode> {
    if matches!(
        input,
        MutationInput::Start(_) | MutationInput::Stop(_) | MutationInput::Restart(_)
    ) {
        Ok(())
    } else {
        Err(ErrorCode::UnsupportedCapability)
    }
}
fn runtime_only(input: &MutationInput) -> Result<(), ErrorCode> {
    if matches!(
        input,
        MutationInput::RuntimeInstall(_) | MutationInput::Configure(_)
    ) {
        Ok(())
    } else {
        Err(ErrorCode::UnsupportedCapability)
    }
}
fn selection_projection(snapshot: &Snapshot, project: &Id) -> Result<RuntimeSelection, ErrorCode> {
    let row = snapshot
        .projects
        .iter()
        .find(|row| row.project.id == project.as_str())
        .ok_or(ErrorCode::ProjectMismatch)?;
    let site = row.site.as_ref().ok_or(ErrorCode::PreconditionFailed)?;
    Ok(RuntimeSelection {
        project_id: project.clone(),
        revision: SafeInt::new(row.project.revision)?,
        input: devforge_control::mutations::ConfigureInput {
            name: devforge_control::mutations::ProjectName::new(row.project.name.clone())?,
            php: Id::new(row.project.php.clone())?,
            node: Id::new(row.project.node.clone())?,
            domain: devforge_control::mutations::DomainName::new(site.domain.clone())?,
            https: site.https,
            vite: site.vite,
            queue: row.development.queue,
            limits: None,
        },
    })
}
fn project_binding(snapshot: &Snapshot, id: &Id) -> Result<ProjectBinding, ErrorCode> {
    let row = snapshot
        .projects
        .iter()
        .find(|r| r.project.id == id.as_str() && r.project.active)
        .ok_or(ErrorCode::ProjectMismatch)?;
    let identity = devforge_projects::discovery::identity(&row.project.path)
        .map_err(|_| ErrorCode::ProjectMismatch)?;
    if row.project.filesystem_identity.as_deref() != Some(identity.as_str()) {
        return Err(ErrorCode::ProjectMismatch);
    }
    ProjectBinding::from_registered(id.clone(), &row.project.path, row.project.revision)
}
fn state_digest(
    snapshot: &Snapshot,
    binding: &ProjectBinding,
    request: &Request<MutationInput>,
    proof: &AuthorizedExternal,
) -> Result<Digest, ErrorCode> {
    let row = snapshot
        .projects
        .iter()
        .find(|r| r.project.id == binding.project_id().as_str())
        .ok_or(ErrorCode::ProjectMismatch)?;
    let processes = row
        .processes
        .iter()
        .map(|(id, p)| json!([id, p.pid, p.observed]))
        .collect::<Vec<_>>();
    let catalog = devforge_projects::project::Catalog::bundled();
    let runtime_binding = match &request.input {
        MutationInput::RuntimeInstall(input) => {
            json!({"package":catalog.get(input.package_id.as_str()).map_err(|_|ErrorCode::InvalidInput)?})
        }
        MutationInput::Configure(input) => {
            let php = catalog
                .get(input.php.as_str())
                .map_err(|_| ErrorCode::InvalidInput)?;
            json!({"php":php,"web":catalog.php_server(php).map_err(|_|ErrorCode::PreconditionFailed)?,"node":catalog.get(input.node.as_str()).map_err(|_|ErrorCode::InvalidInput)?})
        }
        _ => Value::Null,
    };
    sha256(&serde_json::to_vec(&json!({"grant":proof.grant_id(),"client":proof.client_id(),"key":request.idempotency_key,"fingerprint":sha256(&fingerprint_material(request,binding)?)?,"identity":row.project.filesystem_identity,"php":row.project.php,"node":row.project.node,"composer":row.project.composer,"site":row.site,"queue":row.development.queue,"services":row.project.services,"database":row.database.as_ref().map(|b|(&b.instance_id,&b.database,&b.role,b.ready)),"state":row.state,"processes":processes,"queue_state":row.queue_state,"runtime_catalog":runtime_binding})).map_err(|_|ErrorCode::InvalidInput)?)
}
fn plan_receipt(intent: &Intent) -> PlanReceipt {
    PlanReceipt {
        project_id: intent.request.project_id.clone(),
        revision: SafeInt::new(intent.binding.revision()).expect("validatedrevision"),
        digest: intent
            .public_digest
            .as_ref()
            .unwrap_or(&intent.backend)
            .clone(),
        request_fingerprint: intent
            .public_fingerprint
            .as_ref()
            .unwrap_or(&intent.fingerprint)
            .clone(),
        expires_at: SafeInt::new(intent.expires_at).expect("serverclock"),
        blocked: !intent.approved.load(Ordering::Acquire),
    }
}
fn mutation_receipt(receipt: Receipt, revision: u64) -> Result<MutationReceipt, ErrorCode> {
    Ok(MutationReceipt {
        project_id: receipt.project_id,
        operation_id: receipt.operation_id,
        state: receipt.state,
        revision: SafeInt::new(revision)?,
    })
}
fn actor_error(error: crate::actor::ActorError) -> ErrorCode {
    match error {
        crate::actor::ActorError::Busy => ErrorCode::Busy,
        crate::actor::ActorError::IdempotencyConflict => ErrorCode::IdempotencyConflict,
        crate::actor::ActorError::IdempotencyUnverifiable => ErrorCode::IdempotencyUnverifiable,
        crate::actor::ActorError::ShuttingDown => ErrorCode::Interrupted,
        _ => ErrorCode::StorageUnavailable,
    }
}
fn database_only(input: &MutationInput) -> Result<(), ErrorCode> {
    if matches!(
        input,
        MutationInput::DatabaseBind(_) | MutationInput::DatabaseRestore(_)
    ) {
        Ok(())
    } else {
        Err(ErrorCode::UnsupportedCapability)
    }
}
fn domain_only(input: &MutationInput) -> Result<(), ErrorCode> {
    if matches!(
        input,
        MutationInput::DomainApply(_) | MutationInput::DomainCleanup(_)
    ) {
        Ok(())
    } else {
        Err(ErrorCode::UnsupportedCapability)
    }
}
fn domain_registration_binding(
    workspace: &Workspace,
    project: &Id,
) -> Result<ProjectBinding, ErrorCode> {
    let registered = workspace
        .domain_registered_project(project.as_str())
        .map_err(environment_error)?;
    let identity = devforge_projects::discovery::identity(&registered.path)
        .map_err(|_| ErrorCode::ProjectMismatch)?;
    if registered.id != project.as_str()
        || registered.filesystem_identity.as_deref() != Some(identity.as_str())
    {
        return Err(ErrorCode::ProjectMismatch);
    }
    ProjectBinding::from_registered(project.clone(), &registered.path, registered.revision)
}
fn domain_reference(input: &MutationInput) -> Result<&Digest, ErrorCode> {
    match input {
        MutationInput::DomainApply(input) => Ok(&input.plan),
        MutationInput::DomainCleanup(input) => Ok(&input.plan),
        _ => Err(ErrorCode::UnsupportedCapability),
    }
}
fn domain_snapshot_digest(
    snapshot: &Snapshot,
    request: &Request<MutationInput>,
) -> Result<Digest, ErrorCode> {
    if !snapshot
        .projects
        .iter()
        .any(|row| row.project.id == request.project_id.as_str() && row.site.is_some())
    {
        return Err(ErrorCode::PreconditionFailed);
    }
    if snapshot.projects.len() > 2048 {
        return Err(ErrorCode::OutputLimitExceeded);
    }
    let sites = snapshot
        .projects
        .iter()
        .filter_map(|row| row.site.as_ref())
        .collect::<Vec<_>>();
    sha256(&serde_json::to_vec(&sites).map_err(|_| ErrorCode::InvalidInput)?)
}
fn domain_backend(
    snapshot: &Snapshot,
    binding: &ProjectBinding,
    request: &Request<MutationInput>,
    proof: &AuthorizedExternal,
    plan: &devforge_workspace::domain_control::DomainPlan,
) -> Result<Digest, ErrorCode> {
    sha256(&serde_json::to_vec(&json!({"scope":binding.project_id(),"grant":proof.grant_id(),"client":proof.client_id(),"key":request.idempotency_key,"request":sha256(&fingerprint_material(request,binding)?)?,"sites":domain_snapshot_digest(snapshot,request)?,"system":plan.fingerprint().map_err(environment_error)?})).map_err(|_|ErrorCode::InvalidInput)?)
}
fn domain_facts(
    facts: &devforge_workspace::domain_control::DomainFacts,
) -> Result<DomainFacts, ErrorCode> {
    Ok(DomainFacts {
        project_id: Id::new(&facts.project_id)?,
        revision: SafeInt::new(facts.revision)?,
        domain: devforge_control::limits::Text::new(facts.domain.clone())?,
        https_enabled: facts.https_enabled,
        preferred_url: facts
            .preferred_url
            .as_ref()
            .map(|url| devforge_control::limits::Text::new(url.clone()))
            .transpose()?,
        http_port: facts.http_port,
        https_port: facts.https_port,
        certificate_active: facts.certificate_active,
        ca_prepared: facts.ca_prepared,
        ca_fingerprint: facts
            .ca_fingerprint
            .as_ref()
            .map(|hash| Digest::new(hash.clone()))
            .transpose()?,
        system_trusted: facts.system_trusted,
    })
}
fn database_command(
    request: &Request<MutationInput>,
) -> Result<devforge_workspace::Command, ErrorCode> {
    match &request.input {
        MutationInput::DatabaseBind(input) => Ok(devforge_workspace::Command::SwitchDatabase {
            id: request.project_id.as_str().into(),
            instance: input.instance_id.as_str().into(),
            consent: true,
        }),
        MutationInput::DatabaseRestore(input) => Ok(devforge_workspace::Command::RestoreBackup {
            id: request.project_id.as_str().into(),
            backup: input.backup_id.as_str().into(),
            consent: true,
        }),
        _ => Err(ErrorCode::UnsupportedCapability),
    }
}
pub(crate) fn database_snapshot_digest(
    snapshot: &Snapshot,
    request: &Request<MutationInput>,
) -> Result<Digest, ErrorCode> {
    if let MutationInput::DatabaseRestore(input) = &request.input
        && !snapshot
            .projects
            .iter()
            .find(|row| row.project.id == request.project_id.as_str())
            .is_some_and(|row| {
                row.backups.iter().any(|backup| {
                    backup.id == input.backup_id.as_str()
                        && backup.project == request.project_id.as_str()
                })
            })
    {
        return Err(ErrorCode::ProjectMismatch);
    }
    Digest::new(
        devforge_workspace::database_control::database_snapshot_fingerprint(
            snapshot,
            &database_command(request)?,
        )
        .map_err(environment_error)?,
    )
}
fn database_backend(
    snapshot: &Snapshot,
    binding: &ProjectBinding,
    request: &Request<MutationInput>,
    proof: &AuthorizedExternal,
    data: &crate::database_control::DatabaseReadResult,
) -> Result<Digest, ErrorCode> {
    if database_snapshot_digest(snapshot, request)? != data.snapshot_digest {
        return Err(ErrorCode::ReviewMismatch);
    }
    let private = data.state.fingerprint().map_err(environment_error)?;
    sha256(&serde_json::to_vec(&json!({"metadata":state_digest(snapshot,binding,request,proof)?,"database":private,"snapshot":data.snapshot_digest})).map_err(|_|ErrorCode::InvalidInput)?)
}
fn environment_only(input: &MutationInput) -> Result<(), ErrorCode> {
    if matches!(
        input,
        MutationInput::EnvironmentApply(_) | MutationInput::YamlSave(_)
    ) {
        Ok(())
    } else {
        Err(ErrorCode::UnsupportedCapability)
    }
}
fn environment_write_env(input: &MutationInput) -> Option<bool> {
    match input {
        MutationInput::EnvironmentPlan(input) => Some(input.write_env),
        MutationInput::EnvironmentApply(input) => Some(input.write_env),
        _ => None,
    }
}
fn check_yaml_size(input: &MutationInput) -> Result<(), ErrorCode> {
    if matches!(input,MutationInput::YamlSave(input)if input.text.as_str().len()>MAX_EXTERNAL_YAML_BYTES)
    {
        Err(ErrorCode::OutputLimitExceeded)
    } else {
        Ok(())
    }
}
type EnvironmentSummary = (Vec<Id>, Vec<Id>, Vec<Id>);
fn environment_summary(
    data: &crate::environment_control::PrivateEnvironment,
) -> Result<EnvironmentSummary, ErrorCode> {
    if let Some(plan) = &data.plan {
        if plan.packages.len() > 64 || plan.downloads.len() > 64 || plan.blockers.len() > 64 {
            return Err(ErrorCode::OutputLimitExceeded);
        }
        Ok((
            plan.packages
                .values()
                .map(Id::new)
                .collect::<Result<_, _>>()?,
            plan.downloads
                .iter()
                .map(Id::new)
                .collect::<Result<_, _>>()?,
            plan.blockers
                .iter()
                .map(Id::new)
                .collect::<Result<_, _>>()?,
        ))
    } else {
        Ok((
            vec![],
            vec![],
            data.plan_error
                .as_ref()
                .map(|code| Id::new(code.clone()))
                .transpose()?
                .into_iter()
                .collect(),
        ))
    }
}
fn check_private_environment_size(
    data: &crate::environment_control::PrivateEnvironment,
) -> Result<(), ErrorCode> {
    if let Some(plan) = &data.plan {
        check_private_serialized_size(plan)?;
    }
    Ok(())
}
fn check_private_serialized_size(value: &impl Serialize) -> Result<(), ErrorCode> {
    struct Count(usize);
    impl std::io::Write for Count {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if bytes.len() > 65536usize.saturating_sub(self.0) {
                return Err(std::io::Error::other("privateplanlimit"));
            }
            self.0 += bytes.len();
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    serde_json::to_writer(Count(0), value).map_err(|_| ErrorCode::OutputLimitExceeded)?;
    Ok(())
}
fn environment_backend(
    snapshot: &Snapshot,
    binding: &ProjectBinding,
    request: &Request<MutationInput>,
    proof: &AuthorizedExternal,
    data: &crate::environment_control::PrivateEnvironment,
) -> Result<Digest, ErrorCode> {
    if data.binding.project_id() != binding.project_id()
        || data.binding.root() != binding.root()
        || data.binding.revision() != binding.revision()
    {
        return Err(ErrorCode::ReviewMismatch);
    }
    match &request.input{
        MutationInput::EnvironmentApply(_)=>{let plan=data.plan.as_ref().ok_or(ErrorCode::PreconditionFailed)?;if plan.root!=binding.root()||plan.project_id.as_deref()!=Some(binding.project_id().as_str()){return Err(ErrorCode::ProjectMismatch);}Digest::new(plan.digest.clone())},
        MutationInput::YamlSave(_)=>sha256(&serde_json::to_vec(&json!({"metadata":state_digest(snapshot,binding,request,proof)?,"document":data.document_sha})).map_err(|_|ErrorCode::InvalidInput)?),
        _=>Err(ErrorCode::UnsupportedCapability),
    }
}
fn verify_document(binding: &ProjectBinding, expected: Option<&str>) -> Result<(), ErrorCode> {
    let path = binding.root().join("devforge.yaml");
    let hash = match std::fs::symlink_metadata(&path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        _ => Some(
            sha256(
                &devforge_projects::manifest::read_file(&path, 65536)
                    .map_err(|_| ErrorCode::PreconditionFailed)?,
            )?
            .as_str()
            .to_owned(),
        ),
    };
    if hash.as_deref() != expected {
        Err(ErrorCode::ReviewMismatch)
    } else {
        Ok(())
    }
}
fn environment_error(error: devforge_workspace::Error) -> ErrorCode {
    match error.code.as_str() {
        "grant_revoked" => ErrorCode::GrantRevoked,
        "grant_expired" => ErrorCode::GrantExpired,
        "project_mismatch" => ErrorCode::ProjectMismatch,
        "revision_conflict" => ErrorCode::RevisionConflict,
        "unauthorized" => ErrorCode::Unauthorized,
        "timed_out" => ErrorCode::TimedOut,
        "cancelled" | "interrupted" => ErrorCode::Interrupted,
        "busy" => ErrorCode::Busy,
        _ => ErrorCode::PreconditionFailed,
    }
}
pub(crate) fn ws_error(code: ErrorCode) -> devforge_workspace::Error {
    let code = if code == ErrorCode::Interrupted {
        "cancelled".into()
    } else {
        serde_json::to_value(code)
            .ok()
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_else(|| "storage_unavailable".into())
    };
    devforge_workspace::Error {
        scope: "workspace",
        code,
        project_id: None,
    }
}
struct Token(Zeroizing<String>);
impl Token {
    fn parse(self) -> Result<ExternalCredential, ErrorCode> {
        ExternalCredential::from_wire(&self.0).map_err(grant_error)
    }
}
impl<'de> Deserialize<'de> for Token {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = Zeroizing::new(String::deserialize(d)?);
        if v.len() != 64 {
            return Err(serde::de::Error::custom("invalidcredential"));
        }
        Ok(Self(v))
    }
}
#[derive(Deserialize)]
#[serde(tag = "method", rename_all = "snake_case", deny_unknown_fields)]
enum Wire {
    #[serde(rename = "domain_read")]
    DomainRead {
        credential: Token,
        request_id: Id,
        project_id: Id,
        operation: DomainReadKind,
    },
    #[serde(rename = "domain_prepare")]
    DomainPrepare {
        credential: Token,
        request_id: Id,
        project_id: Id,
        kind: DomainPlanKind,
    },
    #[serde(rename = "domain_plan")]
    DomainPlan {
        credential: Token,
        request: Box<Request<MutationInput>>,
    },
    #[serde(rename = "domain_apply")]
    DomainApply {
        credential: Token,
        request: Box<Request<MutationInput>>,
        review: Digest,
    },
    #[serde(rename = "domain_lookup")]
    DomainLookup {
        credential: Token,
        request_id: Id,
        project_id: Id,
        idempotency_key: Id,
    },
    #[serde(rename = "db_choices")]
    DatabaseChoices {
        credential: Token,
        request_id: Id,
        project_id: Id,
    },
    #[serde(rename = "db_plan")]
    DatabasePlan {
        credential: Token,
        request: Box<Request<MutationInput>>,
    },
    #[serde(rename = "db_apply")]
    DatabaseApply {
        credential: Token,
        request: Box<Request<MutationInput>>,
        review: Digest,
    },
    #[serde(rename = "db_lookup")]
    DatabaseLookup {
        credential: Token,
        request_id: Id,
        project_id: Id,
        idempotency_key: Id,
    },
    #[serde(rename = "env_document_state")]
    EnvDocument {
        credential: Token,
        request_id: Id,
        project_id: Id,
    },
    #[serde(rename = "env_preview")]
    EnvPreview {
        credential: Token,
        request: Box<Request<MutationInput>>,
    },
    #[serde(rename = "env_plan")]
    EnvPlan {
        credential: Token,
        request: Box<Request<MutationInput>>,
    },
    #[serde(rename = "env_apply")]
    EnvApply {
        credential: Token,
        request: Box<Request<MutationInput>>,
        review: Digest,
    },
    #[serde(rename = "env_lookup")]
    EnvLookup {
        credential: Token,
        request_id: Id,
        project_id: Id,
        idempotency_key: Id,
    },
    #[serde(rename = "service_plan")]
    Plan {
        credential: Token,
        request: Box<Request<MutationInput>>,
    },
    #[serde(rename = "service_apply")]
    Apply {
        credential: Token,
        request: Box<Request<MutationInput>>,
        review: Digest,
    },
    #[serde(rename = "service_lookup")]
    Lookup {
        credential: Token,
        request_id: Id,
        project_id: Id,
        idempotency_key: Id,
    },
    #[serde(rename = "runtime_plan")]
    RuntimePlan {
        credential: Token,
        request: Box<Request<MutationInput>>,
    },
    #[serde(rename = "runtime_apply")]
    RuntimeApply {
        credential: Token,
        request: Box<Request<MutationInput>>,
        review: Digest,
    },
    #[serde(rename = "runtime_lookup")]
    RuntimeLookup {
        credential: Token,
        request_id: Id,
        project_id: Id,
        idempotency_key: Id,
    },
    #[serde(rename = "runtime_selection")]
    RuntimeSelection {
        credential: Token,
        request_id: Id,
        project_id: Id,
    },
}
fn respond<T: Serialize>(id: Id, result: Result<T, ErrorCode>, operation: Option<Id>) -> Value {
    serde_json::to_value(match result{Ok(data)=>Response::success(id,operation,data),Err(code)=>Response::failure(id,operation,code.fault())}).unwrap_or_else(|_|json!({"api_version":1,"request_id":"invalid-request","ok":false,"error":{"code":"storage_unavailable","retryable":true},"warnings":[]}))
}

#[cfg(test)]
mod domain_scoped_tests {
    use super::*;
    struct NoSecrets;
    impl devforge_core::SecretStore for NoSecrets {
        fn get(&self, _: &str) -> std::result::Result<Option<String>, devforge_core::ErrorCode> {
            panic!("domain binding must not read credentials")
        }
        fn set(&self, _: &str, _: &str) -> std::result::Result<(), devforge_core::ErrorCode> {
            panic!("domain binding must not write credentials")
        }
        fn delete(&self, _: &str) -> std::result::Result<(), devforge_core::ErrorCode> {
            panic!("domain binding must not delete credentials")
        }
    }
    #[test]
    fn scoped_domain_callback_does_not_create_unrelated_backup_directories() {
        let root = crate::external_auth::test_support::root("df-domain-scope-");
        let workspace = Workspace::open(root.path().join("data"), Arc::new(NoSecrets)).unwrap();
        let catalog = devforge_projects::project::Catalog::bundled();
        let target = devforge_runtime::package::Target::current().unwrap();
        let php = catalog
            .packages
            .iter()
            .find(|(_, p)| {
                p.kind == "php" && p.target == target && p.executables.contains_key("php")
            })
            .unwrap()
            .0;
        let node = catalog
            .packages
            .iter()
            .find(|(_, p)| p.kind == "node" && p.target == target)
            .unwrap()
            .0;
        let mut own = None;
        for name in ["A", "B"] {
            let path = root.path().join(name);
            std::fs::create_dir_all(path.join("public")).unwrap();
            std::fs::write(path.join("public/index.php"), "<?php").unwrap();
            std::fs::write(
                path.join("composer.json"),
                r#"{"require":{"php":"^8.3","laravel/framework":"^13"}}"#,
            )
            .unwrap();
            let project = workspace
                .projects
                .import(&path, name, php, node, &catalog)
                .unwrap();
            if name == "A" {
                own = Some(project);
            }
        }
        let backups = workspace.projects.root().join("backups");
        assert!(!backups.exists());
        let own = own.unwrap();
        let binding = domain_registration_binding(&workspace, &Id::new(own.id).unwrap()).unwrap();
        assert_eq!(binding.root(), own.path);
        assert!(!backups.exists());
        assert!(!root.path().join("data/serving/authority").exists());
    }
}
