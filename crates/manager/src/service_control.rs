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
const PLAN_TTL: Duration = Duration::from_secs(300);
const MAX_PLANS: usize = 128;
const MAX_PLAN_BYTES: usize = 1_048_576;
pub(crate) struct Services {
    grants: Arc<Grants>,
    plans: Mutex<VecDeque<Arc<Intent>>>,
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
}
pub(crate) struct ServiceJob {
    intent: Arc<Intent>,
    policy: Arc<Services>,
    pub(crate) deadline: Instant,
    pub(crate) cancellation: Cancellation,
}
impl ServiceJob {
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
        let backend = if initial {
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
        let Dispatch::Command(command) =
            map_mutation(&prepared, &current, &BackendContext::default()).map_err(ws_error)?
        else {
            return Err(ws_error(ErrorCode::UnsupportedCapability));
        };
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
            _ => Err(ws_error(ErrorCode::UnsupportedCapability)),
        }
    }
}
impl Services {
    pub(crate) fn new(grants: Arc<Grants>) -> Self {
        Self {
            grants,
            plans: Mutex::new(VecDeque::new()),
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
        &self,
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
        let current_digest = state_digest(&view.snapshot, &current, &intent.request, &proof)?;
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
            .find(|intent| &intent.backend == digest)
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
            if stored != fingerprint.as_str() {
                return Err(ErrorCode::IdempotencyConflict);
            }
            return mutation_receipt(receipt, revision);
        }
        let intent = self.intent(&review)?;
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
        digest: intent.backend.clone(),
        request_fingerprint: intent.fingerprint.clone(),
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
        _ => ErrorCode::StorageUnavailable,
    }
}
fn ws_error(code: ErrorCode) -> devforge_workspace::Error {
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
