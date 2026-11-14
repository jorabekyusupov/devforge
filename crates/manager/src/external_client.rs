//! Restricted external-grant SDK. No NativeClient, native credential provider,
//! manager launch, grant issuer, project path or generic RPC escape hatch.
use crate::{
    Client, Endpoint, PeerIdentity,
    external_auth::ExternalCredential,
    external_control::{ExternalInput, ExternalReply, ExternalRequest},
};
use devforge_control::{ErrorCode, Id};
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
struct SecretRequest(Value);
impl Drop for SecretRequest {
    fn drop(&mut self) {
        if let Some(Value::String(value)) = self.0.get_mut("credential") {
            value.zeroize();
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
