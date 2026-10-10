//! Explicit file targets, request-time writer admission and file side-effect ports.
//! RPC parameters cannot replace the ports or grant authority.

use std::path::Path;
use std::sync::Arc;

use aeroric_core::context::CoreContext;
use aeroric_core::projects::mutations;
use aeroric_core::storage::Record;
use aeroric_protocol::envelope::{Failure, Frame, Request};
use aeroric_protocol::error::{ErrorCode, ErrorDetail, RpcError};
use aeroric_protocol::project::ProjectTarget;

use crate::dispatcher::failure;

/// Root-resolver errors must retain their category across the host-owned port.
#[derive(Debug)]
pub enum FileProjectError {
    NotFound(String),
    InvalidLocation,
    UnsupportedLocation,
}

impl FileProjectError {
    pub(crate) fn into_frame(self, request: &Request) -> Frame {
        let error = match self {
            Self::NotFound(message) => RpcError::new(ErrorCode::NotFound, message),
            Self::InvalidLocation => RpcError::new(
                ErrorCode::InvalidArgument,
                "Project location is malformed or unrecognised; local file access is refused",
            )
            .with_error_detail(&ErrorDetail {
                field: Some("location".into()),
                ..Default::default()
            }),
            Self::UnsupportedLocation => RpcError::new(
                ErrorCode::UnsupportedCapability,
                "Project location is not hosted: headless SSH/WSL file transport is unavailable",
            )
            .with_error_detail(&ErrorDetail {
                field: Some("location".into()),
                ..Default::default()
            }),
        };
        Frame::Failure(Failure {
            v: 1,
            id: request.id.clone(),
            ok: false,
            error,
            extra: Default::default(),
        })
    }
}

/// Stored transport/addressability admission, before inspecting its raw path.
/// resolve_location alone falls back on malformed data: always guard it first.
/// Local records retain the existing raw-path contract, not a new identity rule.
pub(crate) fn require_local_record(record: &Record) -> Result<(), FileProjectError> {
    if !mutations::is_addressable(record) {
        return Err(FileProjectError::InvalidLocation);
    }
    if !mutations::resolve_location(record).is_local() {
        return Err(FileProjectError::UnsupportedLocation);
    }
    Ok(())
}

/// An optional local-root assertion, never a replacement for projectId ownership.
/// Remote targets are refused before any writer or filesystem checks.
pub(crate) struct FileTarget {
    local_path: Option<String>,
}

impl FileTarget {
    /// Pure protocol decoding. A present null/malformed target is not omission.
    pub(crate) fn decode(request: &Request) -> Result<Self, Box<Frame>> {
        let Some(value) = request.params.get("target") else {
            return Ok(Self { local_path: None });
        };
        match serde_json::from_value::<ProjectTarget>(value.clone()) {
            Ok(ProjectTarget::Local { path }) => Ok(Self {
                local_path: Some(path),
            }),
            Ok(_) => Err(Box::new(failure(
                request.id.clone(),
                ErrorCode::UnsupportedCapability,
                "File target is not hosted: headless SSH/WSL file transport is unavailable",
            ))),
            Err(error) => Err(Box::new(failure(
                request.id.clone(),
                ErrorCode::InvalidArgument,
                &format!("invalid target: {error}"),
            ))),
        }
    }

    /// Call only after writer admission (for mutations) and authoritative
    /// projectId lookup/canonicalization. Allow equivalent aliases of that root,
    /// not a second root, worktree or vault selected by the request.
    pub(crate) fn root_refusal(&self, request: &Request, canonical_root: &Path) -> Option<Frame> {
        let local = self.local_path.as_deref()?;
        match aeroric_core::files::path_policy::validate_project_root(local) {
            Ok(path) if path == canonical_root => None,
            Ok(_) => Some(failure(
                request.id.clone(),
                ErrorCode::InvalidArgument,
                "projectId and target.path disagree",
            )),
            Err(error) => Some(failure(
                request.id.clone(),
                ErrorCode::InvalidArgument,
                &format!("invalid local file target: {error}"),
            )),
        }
    }
}

/// Authentication/root ownership is not a writer lease. Recheck the current
/// lock identity on every mutation, before project lookup or file side effects.
pub(crate) fn write_refusal(request: &Request, context: &CoreContext) -> Option<Frame> {
    context
        .require_write()
        .err()
        .map(|error| failure(request.id.clone(), ErrorCode::IoError, &error.to_string()))
}

pub type TrashDelete = dyn Fn(&Path, &str) -> Result<(), String> + Send + Sync;

/// Production delegates to the existing core trash policy. Tests inject the
/// same core path validation with a scratch quarantine, never the OS Trash.
#[derive(Clone)]
pub struct FilePorts {
    pub trash: Arc<TrashDelete>,
}

impl Default for FilePorts {
    fn default() -> Self {
        Self {
            trash: Arc::new(aeroric_core::files::read_write::delete_path),
        }
    }
}
