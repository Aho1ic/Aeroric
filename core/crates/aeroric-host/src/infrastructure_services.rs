//! Local infrastructure queries. Process policy comes from trusted composition;
//! request params cannot choose executables, environment or remote ownership.

use std::sync::Arc;

use aeroric_core::infrastructure::conda_face::CondaProbe;
use aeroric_protocol::envelope::Request;
use aeroric_protocol::error::ErrorCode;

use crate::dispatcher::{failure, ServiceSet};
use crate::services::ok;

pub fn register_infrastructure_services(set: &mut ServiceSet, conda: CondaProbe) {
    set.register(
        "infra.detect_conda_environments",
        Arc::new(move |request: Request, _call| {
            let conda = conda.clone();
            Box::pin(async move {
                // Omitted params decode to null. Everything else must be the
                // empty object; in particular, SSH hints never trigger a local
                // substitute. Admission happens before the first process/IO.
                if !request.params.is_null()
                    && !request
                        .params
                        .as_object()
                        .is_some_and(|args| args.is_empty())
                {
                    return failure(
                        request.id,
                        ErrorCode::InvalidArgument,
                        "Conda discovery takes no arguments",
                    );
                }
                // A read-only query needs authentication (dispatcher), not the
                // data-directory writer. Missing/bad candidates still mean [].
                match tokio::task::spawn_blocking(move || conda.detect()).await {
                    Ok(environments) => match serde_json::to_value(environments) {
                        Ok(value) => ok(&request, value),
                        Err(error) => failure(request.id, ErrorCode::IoError, &error.to_string()),
                    },
                    Err(error) => failure(request.id, ErrorCode::IoError, &error.to_string()),
                }
            })
        }),
    );
}
