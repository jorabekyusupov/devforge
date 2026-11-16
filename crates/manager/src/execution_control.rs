//! Native-reviewed finite execution. Arguments and context fingerprints remain
//! private RAM; external projections contain only bounded redacted process output.
use crate::{
    actor::ActorHandle,
    external_auth::ExternalCredential,
    service_control::{Services, ws_error},
};
use devforge_control::{
    Digest, ErrorCode, Id, ProjectBinding, Request, Scope,
    mutations::{ExecutionTool, MutationInput},
};
pub use devforge_workspace::execution_control::{
    ExecutionContextSummary, ExecutionLog, ExecutionOutput,
};
use devforge_workspace::{
    Workspace,
    execution_control::{ExecutionContextFacts, ExecutionRequest},
};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc::SyncSender,
};
pub(crate) type Check = Box<dyn Fn(&Workspace) -> devforge_workspace::Result<()> + Send>;
pub(crate) struct ExecutionRead {
    pub project: Id,
    pub request: ExecutionRequest,
    pub check: Check,
    pub cancelled: Arc<AtomicBool>,
    pub reply: SyncSender<devforge_workspace::Result<ExecutionReadResult>>,
}
pub(crate) struct ExecutionReadResult {
    pub binding: ProjectBinding,
    pub facts: ExecutionContextFacts,
}
impl ExecutionRead {
    pub(crate) fn execute(self, workspace: &Workspace) {
        let run = || {
            let check = || {
                if self.cancelled.load(Ordering::Acquire) {
                    return Err(ws_error(ErrorCode::Interrupted));
                }
                (self.check)(workspace)
            };
            check()?;
            let binding =
                crate::service_control::domain_registration_binding(workspace, &self.project)
                    .map_err(ws_error)?;
            let facts = workspace.execution_preflight(self.project.as_str(), &self.request)?;
            check()?;
            Ok(ExecutionReadResult { binding, facts })
        };
        let _ = self.reply.send(run());
    }
}
pub(crate) fn request(input: &MutationInput) -> Result<ExecutionRequest, ErrorCode> {
    use devforge_control::mutations::ExecutionTool as C;
    use devforge_workspace::execution_control::ExecutionTool as W;
    let (tool, args, test) = match input {
        MutationInput::ExecutePhp(p) => (C::Php, &p.args, false),
        MutationInput::ExecuteArtisan(p) => (C::Artisan, &p.args, false),
        MutationInput::ExecuteComposer(p) => (C::Composer, &p.args, false),
        MutationInput::ExecuteNode(p) => (C::Node, &p.args, false),
        MutationInput::ExecuteNpm(p) => (C::Npm, &p.args, false),
        MutationInput::ExecuteGit(p) => (C::Git, &p.args, false),
        MutationInput::ManagedTest(p) => (p.tool, &p.args, true),
        _ => return Err(ErrorCode::UnsupportedCapability),
    };
    if input.limits().timeout_ms.get() > 300_000 {
        return Err(ErrorCode::InvalidInput);
    }
    Ok(ExecutionRequest {
        tool: match tool {
            C::Php => W::Php,
            C::Artisan => W::Artisan,
            C::Composer => W::Composer,
            C::Node => W::Node,
            C::Npm => W::Npm,
            C::Git => W::Git,
        },
        args: args.as_slice().to_vec(),
        timeout_ms: input.limits().timeout_ms.get(),
        managed_test: test,
    })
}
pub(crate) fn is_execution(input: &MutationInput) -> bool {
    matches!(
        input,
        MutationInput::ExecutePhp(_)
            | MutationInput::ExecuteArtisan(_)
            | MutationInput::ExecuteComposer(_)
            | MutationInput::ExecuteNode(_)
            | MutationInput::ExecuteNpm(_)
            | MutationInput::ExecuteGit(_)
            | MutationInput::ManagedTest(_)
    )
}
pub(crate) fn required(facts: &ExecutionContextFacts) -> Vec<Scope> {
    let mut scopes = vec![Scope::ExecutionTest];
    if facts.sensitive_context() {
        scopes.extend([Scope::ProjectManage, Scope::SensitiveOperation]);
    }
    scopes
}
pub(crate) fn tool(input: &MutationInput) -> Result<ExecutionTool, ErrorCode> {
    Ok(match input {
        MutationInput::ExecutePhp(_) => ExecutionTool::Php,
        MutationInput::ExecuteArtisan(_) => ExecutionTool::Artisan,
        MutationInput::ExecuteComposer(_) => ExecutionTool::Composer,
        MutationInput::ExecuteNode(_) => ExecutionTool::Node,
        MutationInput::ExecuteNpm(_) => ExecutionTool::Npm,
        MutationInput::ExecuteGit(_) => ExecutionTool::Git,
        MutationInput::ManagedTest(p) => p.tool,
        _ => return Err(ErrorCode::UnsupportedCapability),
    })
}
pub(crate) fn backend(
    snapshot: &devforge_workspace::Snapshot,
    binding: &ProjectBinding,
    request: &Request<MutationInput>,
    proof: &crate::external_auth::AuthorizedExternal,
    facts: &ExecutionContextFacts,
) -> Result<Digest, ErrorCode> {
    let base = crate::service_control::state_digest(snapshot, binding, request, proof)?;
    let state = facts
        .fingerprint()
        .map_err(crate::service_control::environment_error)?;
    devforge_control_adapter::sha256(
        &serde_json::to_vec(&(base, state)).map_err(|_| ErrorCode::InvalidInput)?,
    )
}
impl Services {
    pub(crate) fn execution_output(
        &self,
        peer: &crate::PeerIdentity,
        credential: &ExternalCredential,
        project: &Id,
        operation: &Id,
        actor: &ActorHandle,
    ) -> Result<ExecutionOutput, ErrorCode> {
        let view = actor
            .external_snapshot()
            .map_err(crate::service_control::actor_error)?;
        let binding = crate::service_control::project_binding(&view.snapshot, project)?;
        let mut lock = self
            .grants
            .registry
            .lock()
            .map_err(|_| ErrorCode::StorageUnavailable)?;
        let registry = lock.as_mut().ok_or(ErrorCode::Interrupted)?;
        let proof = registry
            .authorize(
                peer,
                credential,
                &binding,
                &[Scope::Observer, Scope::ExecutionTest],
                &[],
                None,
            )
            .map_err(crate::external_control::grant_error)?;
        let value = actor
            .external_result(proof.client_id(), project, operation)
            .map_err(|error| match error {
                crate::actor::ActorError::NotReady => ErrorCode::Busy,
                crate::actor::ActorError::ResultUnavailable => ErrorCode::ResultUnavailable,
                crate::actor::ActorError::OperationNotFound => ErrorCode::Unauthorized,
                _ => crate::service_control::actor_error(error),
            })?;
        let output: ExecutionOutput =
            serde_json::from_value(value).map_err(|_| ErrorCode::UnsupportedCapability)?;
        registry
            .authorize(
                peer,
                credential,
                &binding,
                &[Scope::Observer, Scope::ExecutionTest],
                &[],
                None,
            )
            .map_err(crate::external_control::grant_error)?;
        Ok(output)
    }

    pub(crate) fn authorize_execution(
        &self,
        peer: &crate::PeerIdentity,
        credential: &ExternalCredential,
        binding: &ProjectBinding,
        facts: &ExecutionContextFacts,
        revision: Option<u64>,
    ) -> Result<crate::external_auth::AuthorizedExternal, ErrorCode> {
        self.grants
            .registry
            .lock()
            .map_err(|_| ErrorCode::StorageUnavailable)?
            .as_mut()
            .ok_or(ErrorCode::Interrupted)?
            .authorize(peer, credential, binding, &required(facts), &[], revision)
            .map_err(crate::external_control::grant_error)
    }
    pub(crate) fn read_execution(
        self: &Arc<Self>,
        actor: &ActorHandle,
        peer: &crate::PeerIdentity,
        credential: &ExternalCredential,
        request: &Request<MutationInput>,
    ) -> Result<ExecutionReadResult, ErrorCode> {
        let view = actor
            .external_snapshot()
            .map_err(crate::service_control::actor_error)?;
        let binding = crate::service_control::project_binding(&view.snapshot, &request.project_id)?;
        self.authorize(
            peer,
            credential,
            &binding,
            &request.input,
            request.expected_revision,
        )?;
        let policy = self.clone();
        let owned_peer = peer.clone();
        let owned_credential = ExternalCredential::from_wire(credential.expose_secret())
            .map_err(|_| ErrorCode::Unauthorized)?;
        let project = request.project_id.clone();
        let revision = request.expected_revision;
        let check = Box::new(move |workspace: &Workspace| {
            let current = crate::service_control::domain_registration_binding(workspace, &project)
                .map_err(ws_error)?;
            let mut lock = policy
                .grants
                .registry
                .lock()
                .map_err(|_| ws_error(ErrorCode::StorageUnavailable))?;
            lock.as_mut()
                .ok_or_else(|| ws_error(ErrorCode::Interrupted))?
                .authorize(
                    &owned_peer,
                    &owned_credential,
                    &current,
                    &[Scope::ExecutionTest],
                    &[],
                    revision,
                )
                .map_err(crate::external_control::grant_error)
                .map_err(ws_error)?;
            Ok(())
        });
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        let cancelled = Arc::new(AtomicBool::new(false));
        actor
            .queue_execution(ExecutionRead {
                project: request.project_id.clone(),
                request: self::request(&request.input)?,
                check,
                cancelled: cancelled.clone(),
                reply: tx,
            })
            .map_err(crate::service_control::actor_error)?;
        let data = rx
            .recv_timeout(crate::IO_TIMEOUT)
            .map_err(|_| {
                cancelled.store(true, Ordering::Release);
                ErrorCode::TimedOut
            })?
            .map_err(crate::service_control::environment_error)?;
        self.authorize_execution(
            peer,
            credential,
            &data.binding,
            &data.facts,
            request.expected_revision,
        )?;
        Ok(data)
    }
}
