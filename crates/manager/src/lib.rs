//! User-scoped single-owner IPC infrastructure, not a public tool dispatcher.
//!
//! A composition root supplies its real workspace handler. Peer identity proves
//! an OS user, never a client role/grant or permission to execute project code.
//! A Workspace is constructed only through a one-shot actor permit; no MCP tool is advertised.
mod endpoint;
mod framing;
pub mod launch;
mod transport;

pub use endpoint::{Endpoint, LegacyExclusion, Owner, WorkspacePermit};
pub use transport::{Client, PeerIdentity, Server, StopSignal};

use std::{fmt, io};

pub const MAX_REQUEST_BYTES: usize = 65_536;
pub const MAX_RESPONSE_BYTES: usize = 262_144;
pub const MAX_CONNECTIONS: usize = 16;
pub const IO_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

#[derive(Debug)]
pub enum Error {
    Io(io::Error),
    Busy,
    UnsafeEndpoint,
    UnauthorizedPeer,
    InvalidFrame,
    FrameTooLarge,
    TimedOut,
    UnsupportedPlatform,
    RestrictedJob,
    InvalidLaunch,
    HandlerFailed,
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Never include a path, incoming payload or OS error text in transport
        // errors. Detailed diagnostics belong in an explicitly redacted adapter.
        f.write_str(match self {
            Self::Io(_) => "manager_io",
            Self::Busy => "manager_busy",
            Self::UnsafeEndpoint => "manager_unsafe_endpoint",
            Self::UnauthorizedPeer => "manager_unauthorized_peer",
            Self::InvalidFrame => "manager_invalid_frame",
            Self::FrameTooLarge => "manager_frame_too_large",
            Self::TimedOut => "manager_timeout",
            Self::UnsupportedPlatform => "manager_unsupported_platform",
            Self::RestrictedJob => "manager_restricted_parent_job",
            Self::InvalidLaunch => "manager_invalid_launch",
            Self::HandlerFailed => "manager_handler_failed",
        })
    }
}
impl std::error::Error for Error {}
impl From<io::Error> for Error {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}
pub type Result<T> = std::result::Result<T, Error>;

pub mod auth;

pub mod actor;

pub mod bootstrap;

pub mod controller;

pub mod native_client;

pub mod payload;

mod subjects;

pub mod external_auth;

#[cfg(test)]
extern crate self as devforge_manager;

pub mod desktop;

pub mod report_reads;
