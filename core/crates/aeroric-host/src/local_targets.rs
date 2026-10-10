//! Pure target refusal for definition services that require an original local
//! projectPath. Not search's canonical/registered-root policy or a remote owner.

use aeroric_protocol::envelope::{Frame, Request};
use aeroric_protocol::error::ErrorCode;
use aeroric_protocol::project::ProjectTarget;

use crate::dispatcher::failure;

/// Call after decoding all required typed fields and before any filesystem
/// work. An omitted target preserves legacy local behavior; a present value
/// must be a matching local protocol target, never an implicit local fallback.
pub(crate) fn local_target_error(
    request: &Request,
    project_path: &str,
    unsupported_message: &str,
) -> Option<Frame> {
    let value = request.params.get("target")?;
    let (code, message) = match serde_json::from_value::<ProjectTarget>(value.clone()) {
        Ok(ProjectTarget::Local { path }) if path == project_path => return None,
        Ok(ProjectTarget::Local { .. }) => (
            ErrorCode::InvalidArgument,
            "target.path must match the original projectPath".to_string(),
        ),
        Ok(_) => (
            ErrorCode::UnsupportedCapability,
            unsupported_message.to_string(),
        ),
        Err(error) => (
            ErrorCode::InvalidArgument,
            format!("invalid target: {error}"),
        ),
    };
    Some(failure(request.id.clone(), code, &message))
}
