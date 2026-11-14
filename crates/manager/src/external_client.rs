//! Restricted external-grant SDK. No NativeClient, native credential provider,
//! manager launch, grant issuer, project path or generic RPC escape hatch.
use crate::{
    Client, Endpoint, PeerIdentity,
    external_auth::ExternalCredential,
    external_control::{ExternalInput, ExternalReply, ExternalRequest},
};
use devforge_control::{
    Digest, ErrorCode, Id, Request, Response,
    mutations::{MutationInput, MutationReceipt, PlanReceipt},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use zeroize::Zeroize;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OwnerState {
    Ready,
    Quitting,
    Unavailable,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Health {
    pub api_version: u16,
    pub owner_pid: u32,
    pub state: OwnerState,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Error(ErrorCode);
impl Error {
    pub fn control_code(self) -> ErrorCode {
        self.0
    }
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self.0)
    }
}
impl std::error::Error for Error {}
pub type Result<T> = std::result::Result<T, Error>;
pub struct ExternalClient {
    transport: Client,
    peer: PeerIdentity,
    credential: ExternalCredential,
}
impl std::fmt::Debug for ExternalClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExternalClient")
            .field("owner_pid", &self.peer.process_id)
            .finish_non_exhaustive()
    }
}
impl ExternalClient {
    pub fn observe_owner(endpoint: &Endpoint) -> Result<Health> {
        observe(endpoint).map(|(_, health)| health)
    }
    pub fn connect(endpoint: Endpoint, credential: ExternalCredential) -> Result<Self> {
        let (peer, health) = observe(&endpoint)?;
        if health.state != OwnerState::Ready {
            return Err(Error(ErrorCode::Interrupted));
        }
        Ok(Self {
            transport: Client::new(endpoint),
            peer,
            credential,
        })
    }
    pub fn request(
        &mut self,
        request_id: Id,
        input: ExternalInput,
        expected_revision: Option<u64>,
    ) -> Result<ExternalReply> {
        let tool = input.tool();
        let request = ExternalRequest {
            api_version: 1,
            request_id: request_id.clone(),
            expected_revision,
            input,
        };
        let wire = SecretRequest(
            json!({"method":"external","credential":self.credential.expose_secret(),"request":request}),
        );
        let raw = self
            .transport
            .request_with_expected_peer(&self.peer, &wire.0)
            .map_err(transport_error);
        let response: ExternalReply =
            serde_json::from_value(raw?).map_err(|_| Error(ErrorCode::InvalidInput))?;
        if response.api_version != 1
            || response.request_id != request_id
            || response.ok != response.data.is_some()
            || response.ok == response.error.is_some()
            || response.warnings.len() > 16
            || response
                .data
                .as_ref()
                .is_some_and(|data| data.tool() != tool)
        {
            return Err(Error(ErrorCode::InvalidInput));
        }
        Ok(response)
    }
}
/// Fixed configuration is an additional constraint, never authority. This
/// read-only adapter verifies the requested project against the manager's grant
/// before delivering any data, and permits only the five M13.3 projections.
pub struct BoundReadOnlyClient {
    client: ExternalClient,
    project: Id,
}
impl BoundReadOnlyClient {
    pub fn connect(
        endpoint: Endpoint,
        loaded: crate::credential_file::LoadedCredential,
    ) -> std::result::Result<Self, ErrorCode> {
        let (project, credential) = loaded.into_parts();
        let client = ExternalClient::connect(endpoint, credential).map_err(Error::control_code)?;
        let mut result = Self { client, project };
        let response = result.request(
            Id::new("mcp-project-binding")?,
            ExternalInput::ProjectGet {
                project_id: result.project.clone(),
            },
        )?;
        if !response.ok {
            return Err(response
                .error
                .map(|e| e.code)
                .unwrap_or(ErrorCode::StorageUnavailable));
        }
        Ok(result)
    }
    pub fn request(
        &mut self,
        request_id: Id,
        input: ExternalInput,
    ) -> std::result::Result<ExternalReply, ErrorCode> {
        match &input {
            ExternalInput::OperationCancel { .. } => return Err(ErrorCode::UnsupportedCapability),
            ExternalInput::ProjectGet { project_id }
            | ExternalInput::OperationStatus { project_id, .. }
                if project_id != &self.project =>
            {
                return Err(ErrorCode::ProjectMismatch);
            }
            _ => {}
        }
        let reply = self
            .client
            .request(request_id, input, None)
            .map_err(Error::control_code)?;
        if let Some(
            crate::external_control::ExternalOutput::ProjectGet(output)
            | crate::external_control::ExternalOutput::ProjectList(output),
        ) = &reply.data
            && (output.items.len() != 1 || output.items[0].project_id != self.project)
        {
            return Err(ErrorCode::ProjectMismatch);
        }
        Ok(reply)
    }
}
/// Finite external service controller. No native desktop authority, arbitrary
/// Command, execution payload or automatic retry is accepted by this client.
pub struct ServiceClient {
    client: ExternalClient,
}
impl std::fmt::Debug for ServiceClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ServiceClient")
            .field("owner_pid", &self.client.peer.process_id)
            .finish_non_exhaustive()
    }
}
impl ServiceClient {
    pub fn connect(
        endpoint: Endpoint,
        credential: ExternalCredential,
    ) -> std::result::Result<Self, ErrorCode> {
        Ok(Self {
            client: ExternalClient::connect(endpoint, credential).map_err(Error::control_code)?,
        })
    }
    pub fn plan(
        &mut self,
        request: &Request<MutationInput>,
    ) -> std::result::Result<Response<PlanReceipt>, ErrorCode> {
        service_request(request)?;
        let reply: Response<PlanReceipt> = self.exchange(
            &request.request_id,
            json!({"method":"service_plan","request":request}),
            false,
        )?;
        if reply.data().is_some_and(|plan| {
            plan.project_id != request.project_id
                || Some(plan.revision.get()) != request.expected_revision
        }) {
            return Err(ErrorCode::InvalidInput);
        }
        Ok(reply)
    }
    /// A transport/decoding failure after possible admission is deliberately
    /// unconfirmed. Keep this original key and use lookup/status; never invent a
    /// replacement key or infer that an effect did not happen.
    pub fn apply(
        &mut self,
        request: &Request<MutationInput>,
        review: &Digest,
    ) -> std::result::Result<Response<MutationReceipt>, ErrorCode> {
        service_request(request)?;
        let reply: Response<MutationReceipt> = self.exchange(
            &request.request_id,
            json!({"method":"service_apply","request":request,"review":review}),
            true,
        )?;
        if reply.data().is_some_and(|receipt| {
            receipt.project_id != request.project_id
                || Some(receipt.revision.get()) != request.expected_revision
                || reply
                    .operation_id()
                    .is_some_and(|id| id != &receipt.operation_id)
        }) {
            return Err(ErrorCode::AdmissionUnconfirmed);
        }
        Ok(reply)
    }
    /// None is a point-in-time observation, not proof a timed-out handler can
    /// never commit. It also makes no statement about secret/body equivalence.
    pub fn lookup(
        &mut self,
        request_id: Id,
        project_id: &Id,
        idempotency_key: &Id,
    ) -> std::result::Result<Response<Option<MutationReceipt>>, ErrorCode> {
        let reply:Response<Option<MutationReceipt>>=self.exchange(&request_id,json!({"method":"service_lookup","request_id":request_id,"project_id":project_id,"idempotency_key":idempotency_key}),false)?;
        if reply
            .data()
            .and_then(Option::as_ref)
            .is_some_and(|receipt| {
                &receipt.project_id != project_id
                    || reply
                        .operation_id()
                        .is_some_and(|id| id != &receipt.operation_id)
            })
        {
            return Err(ErrorCode::InvalidInput);
        }
        Ok(reply)
    }
    fn exchange<T: serde::de::DeserializeOwned>(
        &mut self,
        request_id: &Id,
        mut wire: Value,
        admission: bool,
    ) -> std::result::Result<Response<T>, ErrorCode> {
        wire["credential"] = json!(self.client.credential.expose_secret());
        let wire = SecretRequest(wire);
        let raw = self
            .client
            .transport
            .request_with_expected_peer(&self.client.peer, &wire.0)
            .map_err(|error| {
                if admission && !matches!(error, crate::Error::UnauthorizedPeer) {
                    ErrorCode::AdmissionUnconfirmed
                } else {
                    transport_error(error).control_code()
                }
            })?;
        let invalid = if admission {
            ErrorCode::AdmissionUnconfirmed
        } else {
            ErrorCode::InvalidInput
        };
        let reply: Response<T> = serde_json::from_value(raw).map_err(|_| invalid)?;
        if reply.request_id() != request_id
            || reply.is_ok() != reply.data().is_some()
            || reply.is_ok() == reply.error().is_some()
        {
            return Err(invalid);
        }
        Ok(reply)
    }
}
fn service_request(request: &Request<MutationInput>) -> std::result::Result<(), ErrorCode> {
    if !matches!(
        request.input,
        MutationInput::Start(_) | MutationInput::Stop(_) | MutationInput::Restart(_)
    ) {
        return Err(ErrorCode::UnsupportedCapability);
    }
    if !request
        .expected_revision
        .is_some_and(|revision| revision <= devforge_control::limits::SafeInt::MAX)
        || request.idempotency_key.is_none()
    {
        return Err(ErrorCode::InvalidInput);
    }
    Ok(())
}
/// Explicitly selected service surface, still constrained by the configured
/// project and the server's native approval/grant checks. The read-only public
/// wrapper remains separate and unchanged.
pub struct BoundServiceClient {
    service: ServiceClient,
    project: Id,
}
impl BoundServiceClient {
    pub fn connect(
        endpoint: Endpoint,
        loaded: crate::credential_file::LoadedCredential,
    ) -> std::result::Result<Self, ErrorCode> {
        let (project, credential) = loaded.into_parts();
        let mut service = ServiceClient::connect(endpoint, credential)?;
        let reply = service
            .client
            .request(
                Id::new("service-project-binding")?,
                ExternalInput::ProjectGet {
                    project_id: project.clone(),
                },
                None,
            )
            .map_err(Error::control_code)?;
        if !reply.ok {
            return Err(reply
                .error
                .map(|fault| fault.code)
                .unwrap_or(ErrorCode::InvalidInput));
        }
        if !matches!(reply.data,Some(crate::external_control::ExternalOutput::ProjectGet(ref output)) if output.items.len()==1&&output.items[0].project_id==project)
        {
            return Err(ErrorCode::ProjectMismatch);
        }
        Ok(Self { service, project })
    }
    pub fn read(
        &mut self,
        request_id: Id,
        input: ExternalInput,
    ) -> std::result::Result<ExternalReply, ErrorCode> {
        match &input {
            ExternalInput::OperationCancel { .. } => return Err(ErrorCode::UnsupportedCapability),
            ExternalInput::ProjectGet { project_id }
            | ExternalInput::OperationStatus { project_id, .. }
                if project_id != &self.project =>
            {
                return Err(ErrorCode::ProjectMismatch);
            }
            _ => {}
        }
        let reply = self
            .service
            .client
            .request(request_id, input, None)
            .map_err(Error::control_code)?;
        if let Some(
            crate::external_control::ExternalOutput::ProjectGet(output)
            | crate::external_control::ExternalOutput::ProjectList(output),
        ) = &reply.data
            && (output.items.len() != 1 || output.items[0].project_id != self.project)
        {
            return Err(ErrorCode::ProjectMismatch);
        }
        Ok(reply)
    }
    pub fn plan(
        &mut self,
        request: &Request<MutationInput>,
    ) -> std::result::Result<Response<PlanReceipt>, ErrorCode> {
        if request.project_id != self.project {
            return Err(ErrorCode::ProjectMismatch);
        }
        self.service.plan(request)
    }
    pub fn apply(
        &mut self,
        request: &Request<MutationInput>,
        review: &Digest,
    ) -> std::result::Result<Response<MutationReceipt>, ErrorCode> {
        if request.project_id != self.project {
            return Err(ErrorCode::ProjectMismatch);
        }
        self.service.apply(request, review)
    }
    pub fn lookup(
        &mut self,
        request_id: Id,
        idempotency_key: &Id,
    ) -> std::result::Result<Response<Option<MutationReceipt>>, ErrorCode> {
        self.service
            .lookup(request_id, &self.project, idempotency_key)
    }
}

/// Finite runtime installation/selection client. Installation changes the shared
/// managed cache, never project pins. Selection uses the canonical ConfigureInput
/// projection; the server guards every preserved non-runtime field.
pub struct RuntimeClient {
    inner: ServiceClient,
}
impl std::fmt::Debug for RuntimeClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RuntimeClient")
            .field("owner_pid", &self.inner.client.peer.process_id)
            .finish_non_exhaustive()
    }
}
impl RuntimeClient {
    pub fn connect(
        endpoint: Endpoint,
        credential: ExternalCredential,
    ) -> std::result::Result<Self, ErrorCode> {
        Ok(Self {
            inner: ServiceClient::connect(endpoint, credential)?,
        })
    }
    pub fn selection(
        &mut self,
        request_id: Id,
        project_id: &Id,
    ) -> std::result::Result<Response<crate::service_control::RuntimeSelection>, ErrorCode> {
        let reply: Response<crate::service_control::RuntimeSelection> = self.inner.exchange(
            &request_id,
            json!({"method":"runtime_selection","request_id":request_id,"project_id":project_id}),
            false,
        )?;
        if reply
            .data()
            .is_some_and(|selection| &selection.project_id != project_id)
        {
            return Err(ErrorCode::ProjectMismatch);
        }
        Ok(reply)
    }
    /// Only PHP/Node and limits are supplied by this caller. Other ConfigureInput
    /// fields come from the manager. The expected revision remains the caller's
    /// original revision so apply can reconcile a prior exact admitted intent.
    pub fn select_request(
        &mut self,
        request_id: Id,
        project_id: Id,
        expected_revision: u64,
        idempotency_key: Id,
        pins: (Id, Id),
        limits: Option<devforge_control::mutations::MutationLimits>,
    ) -> std::result::Result<Request<MutationInput>, ErrorCode> {
        let reply = self.selection(request_id.clone(), &project_id)?;
        let mut input = reply
            .data()
            .ok_or_else(|| {
                reply
                    .error()
                    .map(|fault| fault.code)
                    .unwrap_or(ErrorCode::InvalidInput)
            })?
            .input
            .clone();
        input.php = pins.0;
        input.node = pins.1;
        input.limits = limits;
        let request = Request {
            api_version: Default::default(),
            request_id,
            project_id,
            expected_revision: Some(expected_revision),
            idempotency_key: Some(idempotency_key),
            input: MutationInput::Configure(input),
        };
        runtime_request(&request)?;
        Ok(request)
    }
    pub fn plan(
        &mut self,
        request: &Request<MutationInput>,
    ) -> std::result::Result<Response<PlanReceipt>, ErrorCode> {
        runtime_request(request)?;
        let reply: Response<PlanReceipt> = self.inner.exchange(
            &request.request_id,
            json!({"method":"runtime_plan","request":request}),
            false,
        )?;
        if reply.data().is_some_and(|plan| {
            plan.project_id != request.project_id
                || Some(plan.revision.get()) != request.expected_revision
        }) {
            return Err(ErrorCode::InvalidInput);
        }
        Ok(reply)
    }
    pub fn apply(
        &mut self,
        request: &Request<MutationInput>,
        review: &Digest,
    ) -> std::result::Result<Response<MutationReceipt>, ErrorCode> {
        runtime_request(request)?;
        let reply: Response<MutationReceipt> = self.inner.exchange(
            &request.request_id,
            json!({"method":"runtime_apply","request":request,"review":review}),
            true,
        )?;
        if reply.data().is_some_and(|receipt| {
            receipt.project_id != request.project_id
                || Some(receipt.revision.get()) != request.expected_revision
                || reply
                    .operation_id()
                    .is_some_and(|id| id != &receipt.operation_id)
        }) {
            return Err(ErrorCode::AdmissionUnconfirmed);
        }
        Ok(reply)
    }
    pub fn lookup(
        &mut self,
        request_id: Id,
        project_id: &Id,
        idempotency_key: &Id,
    ) -> std::result::Result<Response<Option<MutationReceipt>>, ErrorCode> {
        let reply:Response<Option<MutationReceipt>>=self.inner.exchange(&request_id,json!({"method":"runtime_lookup","request_id":request_id,"project_id":project_id,"idempotency_key":idempotency_key}),false)?;
        if reply
            .data()
            .and_then(Option::as_ref)
            .is_some_and(|receipt| {
                &receipt.project_id != project_id
                    || reply
                        .operation_id()
                        .is_some_and(|id| id != &receipt.operation_id)
            })
        {
            return Err(ErrorCode::InvalidInput);
        }
        Ok(reply)
    }
}
fn runtime_request(request: &Request<MutationInput>) -> std::result::Result<(), ErrorCode> {
    if !matches!(
        request.input,
        MutationInput::RuntimeInstall(_) | MutationInput::Configure(_)
    ) {
        return Err(ErrorCode::UnsupportedCapability);
    }
    if !request
        .expected_revision
        .is_some_and(|revision| revision <= devforge_control::limits::SafeInt::MAX)
        || request.idempotency_key.is_none()
    {
        return Err(ErrorCode::InvalidInput);
    }
    Ok(())
}
pub struct BoundRuntimeClient {
    runtime: RuntimeClient,
    project: Id,
}
impl BoundRuntimeClient {
    pub fn connect(
        endpoint: Endpoint,
        loaded: crate::credential_file::LoadedCredential,
    ) -> std::result::Result<Self, ErrorCode> {
        let bound = BoundServiceClient::connect(endpoint, loaded)?;
        Ok(Self {
            runtime: RuntimeClient {
                inner: bound.service,
            },
            project: bound.project,
        })
    }
    pub fn read(
        &mut self,
        request_id: Id,
        input: ExternalInput,
    ) -> std::result::Result<ExternalReply, ErrorCode> {
        bound_read(
            &mut self.runtime.inner.client,
            &self.project,
            request_id,
            input,
        )
    }
    pub fn select_request(
        &mut self,
        request_id: Id,
        expected_revision: u64,
        idempotency_key: Id,
        pins: (Id, Id),
        limits: Option<devforge_control::mutations::MutationLimits>,
    ) -> std::result::Result<Request<MutationInput>, ErrorCode> {
        self.runtime.select_request(
            request_id,
            self.project.clone(),
            expected_revision,
            idempotency_key,
            pins,
            limits,
        )
    }
    pub fn plan(
        &mut self,
        request: &Request<MutationInput>,
    ) -> std::result::Result<Response<PlanReceipt>, ErrorCode> {
        if request.project_id != self.project {
            return Err(ErrorCode::ProjectMismatch);
        }
        self.runtime.plan(request)
    }
    pub fn apply(
        &mut self,
        request: &Request<MutationInput>,
        review: &Digest,
    ) -> std::result::Result<Response<MutationReceipt>, ErrorCode> {
        if request.project_id != self.project {
            return Err(ErrorCode::ProjectMismatch);
        }
        self.runtime.apply(request, review)
    }
    pub fn lookup(
        &mut self,
        request_id: Id,
        key: &Id,
    ) -> std::result::Result<Response<Option<MutationReceipt>>, ErrorCode> {
        self.runtime.lookup(request_id, &self.project, key)
    }
}
/// Explicit combined adapter surface. Each method still delegates to a finite
/// service/runtime validator; this is not an arbitrary mutation or command gate.
pub struct BoundActionsClient {
    bound: BoundRuntimeClient,
}
impl BoundActionsClient {
    pub fn connect(
        endpoint: Endpoint,
        loaded: crate::credential_file::LoadedCredential,
    ) -> std::result::Result<Self, ErrorCode> {
        Ok(Self {
            bound: BoundRuntimeClient::connect(endpoint, loaded)?,
        })
    }
    pub fn read(
        &mut self,
        id: Id,
        input: ExternalInput,
    ) -> std::result::Result<ExternalReply, ErrorCode> {
        self.bound.read(id, input)
    }
    pub fn service_plan(
        &mut self,
        request: &Request<MutationInput>,
    ) -> std::result::Result<Response<PlanReceipt>, ErrorCode> {
        if request.project_id != self.bound.project {
            return Err(ErrorCode::ProjectMismatch);
        }
        self.bound.runtime.inner.plan(request)
    }
    pub fn service_apply(
        &mut self,
        request: &Request<MutationInput>,
        review: &Digest,
    ) -> std::result::Result<Response<MutationReceipt>, ErrorCode> {
        if request.project_id != self.bound.project {
            return Err(ErrorCode::ProjectMismatch);
        }
        self.bound.runtime.inner.apply(request, review)
    }
    pub fn runtime(&mut self) -> &mut BoundRuntimeClient {
        &mut self.bound
    }
}
fn bound_read(
    client: &mut ExternalClient,
    project: &Id,
    request_id: Id,
    input: ExternalInput,
) -> std::result::Result<ExternalReply, ErrorCode> {
    match &input {
        ExternalInput::OperationCancel { .. } => return Err(ErrorCode::UnsupportedCapability),
        ExternalInput::ProjectGet { project_id }
        | ExternalInput::OperationStatus { project_id, .. }
            if project_id != project =>
        {
            return Err(ErrorCode::ProjectMismatch);
        }
        _ => {}
    }
    let reply = client
        .request(request_id, input, None)
        .map_err(Error::control_code)?;
    if let Some(
        crate::external_control::ExternalOutput::ProjectGet(output)
        | crate::external_control::ExternalOutput::ProjectList(output),
    ) = &reply.data
        && (output.items.len() != 1 || &output.items[0].project_id != project)
    {
        return Err(ErrorCode::ProjectMismatch);
    }
    Ok(reply)
}

/// Finite masked environment/configuration adapter. It exposes no raw document,
/// .env value, native credential provider or generic mutation escape hatch.
pub struct EnvClient {
    inner: ServiceClient,
}
impl std::fmt::Debug for EnvClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EnvClient")
            .field("owner_pid", &self.inner.client.peer.process_id)
            .finish_non_exhaustive()
    }
}
impl EnvClient {
    pub fn connect(
        endpoint: Endpoint,
        credential: ExternalCredential,
    ) -> std::result::Result<Self, ErrorCode> {
        Ok(Self {
            inner: ServiceClient::connect(endpoint, credential)?,
        })
    }
    pub fn document_state(
        &mut self,
        id: Id,
        project: &Id,
    ) -> std::result::Result<Response<crate::service_control::EnvDocumentState>, ErrorCode> {
        env_document_state(&mut self.inner, id, project)
    }
    pub fn plan(
        &mut self,
        request: &Request<MutationInput>,
    ) -> std::result::Result<Response<crate::service_control::EnvPreview>, ErrorCode> {
        env_plan(&mut self.inner, request)
    }
    pub fn apply(
        &mut self,
        request: &Request<MutationInput>,
        review: &Digest,
    ) -> std::result::Result<Response<MutationReceipt>, ErrorCode> {
        env_apply(&mut self.inner, request, review)
    }
    pub fn lookup(
        &mut self,
        id: Id,
        project: &Id,
        key: &Id,
    ) -> std::result::Result<Response<Option<MutationReceipt>>, ErrorCode> {
        env_lookup(&mut self.inner, id, project, key)
    }
}
fn env_request(
    request: &Request<MutationInput>,
    apply: bool,
) -> std::result::Result<(), ErrorCode> {
    match &request.input {
        MutationInput::EnvironmentPlan(_) if !apply => {}
        MutationInput::EnvironmentApply(_) => {}
        MutationInput::YamlSave(input) => {
            if input.text.as_str().len() > crate::service_control::MAX_EXTERNAL_YAML_BYTES {
                return Err(ErrorCode::OutputLimitExceeded);
            }
        }
        _ => return Err(ErrorCode::UnsupportedCapability),
    }
    if request
        .expected_revision
        .is_some_and(|revision| revision > devforge_control::limits::SafeInt::MAX)
    {
        return Err(ErrorCode::InvalidInput);
    }
    if request.input.mutates()
        && (request.expected_revision.is_none() || request.idempotency_key.is_none())
    {
        return Err(ErrorCode::InvalidInput);
    }
    Ok(())
}
fn env_document_state(
    client: &mut ServiceClient,
    id: Id,
    project: &Id,
) -> std::result::Result<Response<crate::service_control::EnvDocumentState>, ErrorCode> {
    let reply: Response<crate::service_control::EnvDocumentState> = client.exchange(
        &id,
        json!({"method":"env_document_state","request_id":id,"project_id":project}),
        false,
    )?;
    if reply.data().is_some_and(|document| {
        &document.project_id != project || !document.yaml_exists && document.version.is_some()
    }) {
        return Err(ErrorCode::InvalidInput);
    }
    Ok(reply)
}
fn env_plan(
    client: &mut ServiceClient,
    request: &Request<MutationInput>,
) -> std::result::Result<Response<crate::service_control::EnvPreview>, ErrorCode> {
    env_request(request, false)?;
    let method = if matches!(request.input, MutationInput::EnvironmentPlan(_)) {
        "env_preview"
    } else {
        "env_plan"
    };
    let reply: Response<crate::service_control::EnvPreview> = client.exchange(
        &request.request_id,
        json!({"method":method,"request":request}),
        false,
    )?;
    if reply.data().is_some_and(|preview| {
        preview.document.project_id != request.project_id
            || preview
                .receipt
                .as_ref()
                .is_some_and(|receipt| receipt.project_id != request.project_id)
    }) {
        return Err(ErrorCode::ProjectMismatch);
    }
    Ok(reply)
}
fn env_apply(
    client: &mut ServiceClient,
    request: &Request<MutationInput>,
    review: &Digest,
) -> std::result::Result<Response<MutationReceipt>, ErrorCode> {
    env_request(request, true)?;
    let reply: Response<MutationReceipt> = client.exchange(
        &request.request_id,
        json!({"method":"env_apply","request":request,"review":review}),
        true,
    )?;
    if reply.data().is_some_and(|receipt| {
        receipt.project_id != request.project_id
            || Some(receipt.revision.get()) != request.expected_revision
            || reply
                .operation_id()
                .is_some_and(|id| id != &receipt.operation_id)
    }) {
        return Err(ErrorCode::AdmissionUnconfirmed);
    }
    Ok(reply)
}
fn env_lookup(
    client: &mut ServiceClient,
    id: Id,
    project: &Id,
    key: &Id,
) -> std::result::Result<Response<Option<MutationReceipt>>, ErrorCode> {
    let reply: Response<Option<MutationReceipt>> = client.exchange(
        &id,
        json!({"method":"env_lookup","request_id":id,"project_id":project,"idempotency_key":key}),
        false,
    )?;
    if reply
        .data()
        .and_then(Option::as_ref)
        .is_some_and(|receipt| {
            &receipt.project_id != project
                || reply
                    .operation_id()
                    .is_some_and(|id| id != &receipt.operation_id)
        })
    {
        return Err(ErrorCode::InvalidInput);
    }
    Ok(reply)
}
pub struct BoundEnvClient {
    actions: BoundActionsClient,
}
impl BoundEnvClient {
    pub fn connect(
        endpoint: Endpoint,
        loaded: crate::credential_file::LoadedCredential,
    ) -> std::result::Result<Self, ErrorCode> {
        Ok(Self {
            actions: BoundActionsClient::connect(endpoint, loaded)?,
        })
    }
    fn project(&self) -> &Id {
        &self.actions.bound.project
    }
    fn transport(&mut self) -> &mut ServiceClient {
        &mut self.actions.bound.runtime.inner
    }
    pub fn read(
        &mut self,
        id: Id,
        input: ExternalInput,
    ) -> std::result::Result<ExternalReply, ErrorCode> {
        self.actions.read(id, input)
    }
    pub fn document_state(
        &mut self,
        id: Id,
    ) -> std::result::Result<Response<crate::service_control::EnvDocumentState>, ErrorCode> {
        let project = self.project().clone();
        env_document_state(self.transport(), id, &project)
    }
    pub fn plan(
        &mut self,
        request: &Request<MutationInput>,
    ) -> std::result::Result<Response<crate::service_control::EnvPreview>, ErrorCode> {
        if &request.project_id != self.project() {
            return Err(ErrorCode::ProjectMismatch);
        }
        env_plan(self.transport(), request)
    }
    pub fn apply(
        &mut self,
        request: &Request<MutationInput>,
        review: &Digest,
    ) -> std::result::Result<Response<MutationReceipt>, ErrorCode> {
        if &request.project_id != self.project() {
            return Err(ErrorCode::ProjectMismatch);
        }
        env_apply(self.transport(), request, review)
    }
    pub fn lookup(
        &mut self,
        id: Id,
        key: &Id,
    ) -> std::result::Result<Response<Option<MutationReceipt>>, ErrorCode> {
        let project = self.project().clone();
        env_lookup(self.transport(), id, &project, key)
    }
}
/// Explicit MCP composition retains one credential and finite validators. The
/// protocol's independent flags choose which of these surfaces is exposed.
pub struct BoundEnvActionsClient {
    env: BoundEnvClient,
}
impl BoundEnvActionsClient {
    pub fn connect(
        endpoint: Endpoint,
        loaded: crate::credential_file::LoadedCredential,
    ) -> std::result::Result<Self, ErrorCode> {
        Ok(Self {
            env: BoundEnvClient::connect(endpoint, loaded)?,
        })
    }
    pub fn environment(&mut self) -> &mut BoundEnvClient {
        &mut self.env
    }
    pub fn actions(&mut self) -> &mut BoundActionsClient {
        &mut self.env.actions
    }
}

/// Finite reviewed DB binding/restoration client. Public inputs contain only
/// opaque instance/backup IDs; credentials, paths and backup checksums stay at
/// the manager/native-plan boundary.
pub struct DatabaseClient {
    inner: ServiceClient,
}
impl std::fmt::Debug for DatabaseClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DatabaseClient")
            .field("owner_pid", &self.inner.client.peer.process_id)
            .finish_non_exhaustive()
    }
}
impl DatabaseClient {
    pub fn connect(
        endpoint: Endpoint,
        credential: ExternalCredential,
    ) -> std::result::Result<Self, ErrorCode> {
        Ok(Self {
            inner: ServiceClient::connect(endpoint, credential)?,
        })
    }
    pub fn choices(
        &mut self,
        id: Id,
        project: &Id,
    ) -> std::result::Result<Response<crate::service_control::DatabaseChoices>, ErrorCode> {
        database_choices(&mut self.inner, id, project)
    }
    pub fn plan(
        &mut self,
        request: &Request<MutationInput>,
    ) -> std::result::Result<Response<PlanReceipt>, ErrorCode> {
        database_plan(&mut self.inner, request)
    }
    pub fn apply(
        &mut self,
        request: &Request<MutationInput>,
        review: &Digest,
    ) -> std::result::Result<Response<MutationReceipt>, ErrorCode> {
        database_apply(&mut self.inner, request, review)
    }
    pub fn lookup(
        &mut self,
        id: Id,
        project: &Id,
        key: &Id,
    ) -> std::result::Result<Response<Option<MutationReceipt>>, ErrorCode> {
        database_lookup(&mut self.inner, id, project, key)
    }
}
fn database_request(request: &Request<MutationInput>) -> std::result::Result<(), ErrorCode> {
    if !matches!(
        request.input,
        MutationInput::DatabaseBind(_) | MutationInput::DatabaseRestore(_)
    ) {
        return Err(ErrorCode::UnsupportedCapability);
    }
    if !request
        .expected_revision
        .is_some_and(|revision| revision <= devforge_control::limits::SafeInt::MAX)
        || request.idempotency_key.is_none()
    {
        return Err(ErrorCode::InvalidInput);
    }
    Ok(())
}
fn database_choices(
    client: &mut ServiceClient,
    id: Id,
    project: &Id,
) -> std::result::Result<Response<crate::service_control::DatabaseChoices>, ErrorCode> {
    let reply: Response<crate::service_control::DatabaseChoices> = client.exchange(
        &id,
        json!({"method":"db_choices","request_id":id,"project_id":project}),
        false,
    )?;
    if reply.data().is_some_and(|choices| {
        &choices.project_id != project
            || choices
                .current_instance_id
                .as_ref()
                .is_some_and(|current| !choices.instance_ids.contains(current))
    }) {
        return Err(ErrorCode::InvalidInput);
    }
    Ok(reply)
}
fn database_plan(
    client: &mut ServiceClient,
    request: &Request<MutationInput>,
) -> std::result::Result<Response<PlanReceipt>, ErrorCode> {
    database_request(request)?;
    let reply: Response<PlanReceipt> = client.exchange(
        &request.request_id,
        json!({"method":"db_plan","request":request}),
        false,
    )?;
    if reply.data().is_some_and(|plan| {
        plan.project_id != request.project_id
            || Some(plan.revision.get()) != request.expected_revision
    }) {
        return Err(ErrorCode::InvalidInput);
    }
    Ok(reply)
}
fn database_apply(
    client: &mut ServiceClient,
    request: &Request<MutationInput>,
    review: &Digest,
) -> std::result::Result<Response<MutationReceipt>, ErrorCode> {
    database_request(request)?;
    let reply: Response<MutationReceipt> = client.exchange(
        &request.request_id,
        json!({"method":"db_apply","request":request,"review":review}),
        true,
    )?;
    if reply.data().is_some_and(|receipt| {
        receipt.project_id != request.project_id
            || Some(receipt.revision.get()) != request.expected_revision
            || reply
                .operation_id()
                .is_some_and(|id| id != &receipt.operation_id)
    }) {
        return Err(ErrorCode::AdmissionUnconfirmed);
    }
    Ok(reply)
}
fn database_lookup(
    client: &mut ServiceClient,
    id: Id,
    project: &Id,
    key: &Id,
) -> std::result::Result<Response<Option<MutationReceipt>>, ErrorCode> {
    let reply: Response<Option<MutationReceipt>> = client.exchange(
        &id,
        json!({"method":"db_lookup","request_id":id,"project_id":project,"idempotency_key":key}),
        false,
    )?;
    if reply
        .data()
        .and_then(Option::as_ref)
        .is_some_and(|receipt| {
            &receipt.project_id != project
                || reply
                    .operation_id()
                    .is_some_and(|id| id != &receipt.operation_id)
        })
    {
        return Err(ErrorCode::InvalidInput);
    }
    Ok(reply)
}
pub struct BoundDatabaseClient {
    previous: BoundEnvActionsClient,
}
impl BoundDatabaseClient {
    pub fn connect(
        endpoint: Endpoint,
        loaded: crate::credential_file::LoadedCredential,
    ) -> std::result::Result<Self, ErrorCode> {
        Ok(Self {
            previous: BoundEnvActionsClient::connect(endpoint, loaded)?,
        })
    }
    fn project(&self) -> &Id {
        self.previous.env.project()
    }
    fn transport(&mut self) -> &mut ServiceClient {
        self.previous.env.transport()
    }
    pub fn read(
        &mut self,
        id: Id,
        input: ExternalInput,
    ) -> std::result::Result<ExternalReply, ErrorCode> {
        self.previous.environment().read(id, input)
    }
    pub fn choices(
        &mut self,
        id: Id,
    ) -> std::result::Result<Response<crate::service_control::DatabaseChoices>, ErrorCode> {
        let project = self.project().clone();
        database_choices(self.transport(), id, &project)
    }
    pub fn plan(
        &mut self,
        request: &Request<MutationInput>,
    ) -> std::result::Result<Response<PlanReceipt>, ErrorCode> {
        if &request.project_id != self.project() {
            return Err(ErrorCode::ProjectMismatch);
        }
        database_plan(self.transport(), request)
    }
    pub fn apply(
        &mut self,
        request: &Request<MutationInput>,
        review: &Digest,
    ) -> std::result::Result<Response<MutationReceipt>, ErrorCode> {
        if &request.project_id != self.project() {
            return Err(ErrorCode::ProjectMismatch);
        }
        database_apply(self.transport(), request, review)
    }
    pub fn lookup(
        &mut self,
        id: Id,
        key: &Id,
    ) -> std::result::Result<Response<Option<MutationReceipt>>, ErrorCode> {
        let project = self.project().clone();
        database_lookup(self.transport(), id, &project, key)
    }
}
/// Explicit composition for independently selected MCP surfaces. Existing
/// service/runtime/environment/read-only wrappers retain their finite APIs.
pub struct BoundDatabaseActionsClient {
    database: BoundDatabaseClient,
}
impl BoundDatabaseActionsClient {
    pub fn connect(
        endpoint: Endpoint,
        loaded: crate::credential_file::LoadedCredential,
    ) -> std::result::Result<Self, ErrorCode> {
        Ok(Self {
            database: BoundDatabaseClient::connect(endpoint, loaded)?,
        })
    }
    pub fn database(&mut self) -> &mut BoundDatabaseClient {
        &mut self.database
    }
    pub fn previous(&mut self) -> &mut BoundEnvActionsClient {
        &mut self.database.previous
    }
}

struct SecretRequest(Value);
impl Drop for SecretRequest {
    fn drop(&mut self) {
        if let Some(Value::String(value)) = self.0.get_mut("credential") {
            value.zeroize();
        }
        if self
            .0
            .pointer("/request/input/tool")
            .and_then(Value::as_str)
            == Some("environment.yaml_save")
            && let Some(Value::String(text)) = self.0.pointer_mut("/request/input/arguments/text")
        {
            text.zeroize();
        }
    }
}
fn observe(endpoint: &Endpoint) -> Result<(PeerIdentity, Health)> {
    let (peer, value) = Client::new(endpoint.clone())
        .request_with_peer(&json!({"method":"health"}))
        .map_err(transport_error)?;
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Reply {
        ok: bool,
        data: Health,
    }
    let response: Reply =
        serde_json::from_value(value).map_err(|_| Error(ErrorCode::InvalidInput))?;
    if !response.ok
        || response.data.api_version != 1
        || peer.process_id != Some(response.data.owner_pid)
        || response.data.owner_pid == 0
    {
        return Err(Error(ErrorCode::Unauthorized));
    }
    Ok((peer, response.data))
}
fn transport_error(error: crate::Error) -> Error {
    Error(match error {
        crate::Error::Busy => ErrorCode::Busy,
        crate::Error::UnauthorizedPeer => ErrorCode::Unauthorized,
        crate::Error::FrameTooLarge => ErrorCode::OutputLimitExceeded,
        crate::Error::TimedOut => ErrorCode::TimedOut,
        _ => ErrorCode::StorageUnavailable,
    })
}
