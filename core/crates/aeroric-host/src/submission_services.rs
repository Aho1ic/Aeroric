//! Native composer catalog and receipt queries. All IO stays in blocking workers.
use std::sync::Arc;

use aeroric_core::context::CoreContext;
use aeroric_core::settings::session_catalog;
use aeroric_core::tasks::{submissions::SubmissionError, TaskService};
use aeroric_protocol::envelope::{Frame, Request};
use aeroric_protocol::error::ErrorCode;
use aeroric_protocol::submission::SubmissionRequest;
use serde::Deserialize;
use serde_json::Value;

use crate::dispatcher::{failure, ServiceSet};
use crate::services::ok;

fn submission_error(id: String, error: SubmissionError) -> Frame {
    let code = match &error {
        SubmissionError::Invalid => ErrorCode::InvalidArgument,
        SubmissionError::Conflict => ErrorCode::RevisionConflict,
        SubmissionError::Storage(_) => ErrorCode::IoError,
    };
    failure(id, code, &error.to_string())
}

pub fn register(
    set: &mut ServiceSet,
    context: Arc<CoreContext>,
    service: Option<Arc<TaskService>>,
) {
    let can_execute = service.is_some();
    let catalog_context = context;
    set.register(
        "agent.configurations",
        Arc::new(move |request, _call| {
            let context = Arc::clone(&catalog_context);
            Box::pin(async move {
                let id = request.id.clone();
                match tokio::task::spawn_blocking(move || {
                    #[derive(Deserialize)]
                    #[serde(rename_all = "camelCase", deny_unknown_fields)]
                    struct Params {
                        project_id: String,
                    }
                    let params: Params = match serde_json::from_value(request.params.clone()) {
                        Ok(params) => params,
                        Err(_) => {
                            return failure(
                                request.id,
                                ErrorCode::InvalidArgument,
                                "requires an explicit projectId",
                            )
                        }
                    };
                    let _settings = aeroric_core::storage::settings::settings_lock().lock();
                    let result = session_catalog::for_project(&context, &params.project_id).map(
                        |(project, mut catalog)| {
                            crate::remote_sessions::mark_remote_availability(
                                &context,
                                &project,
                                &mut catalog.configurations,
                            );
                            catalog
                        },
                    );
                    match result {
                        Ok(mut directory) => {
                            if !can_execute {
                                for configuration in &mut directory.configurations {
                                    configuration
                                        .blocked_reasons
                                        .push("当前宿主没有装配任务执行服务。".into());
                                }
                            }
                            ok(&request, serde_json::json!(directory))
                        }
                        Err(reason) => failure(request.id, ErrorCode::IoError, &reason),
                    }
                })
                .await
                {
                    Ok(frame) => frame,
                    Err(_) => failure(
                        id,
                        ErrorCode::IoError,
                        "configuration catalog worker failed",
                    ),
                }
            })
        }),
    );
    if let Some(service) = service {
        set.register(
            "task.submission.get",
            Arc::new(move |request, _call| {
                let service = Arc::clone(&service);
                Box::pin(async move {
                    let id = request.id.clone();
                    match tokio::task::spawn_blocking(move || {
                        #[derive(Deserialize)]
                        #[serde(rename_all = "camelCase", deny_unknown_fields)]
                        struct Params {
                            submission_id: String,
                            fingerprint: String,
                        }
                        let params: Params = match serde_json::from_value(request.params.clone()) {
                            Ok(params) => params,
                            Err(_) => {
                                return failure(
                                    request.id,
                                    ErrorCode::InvalidArgument,
                                    "requires submissionId and fingerprint",
                                )
                            }
                        };
                        match service.submission(&params.submission_id, &params.fingerprint) {
                            Ok(lookup) => ok(&request, serde_json::json!(lookup)),
                            Err(error) => submission_error(request.id, error),
                        }
                    })
                    .await
                    {
                        Ok(frame) => frame,
                        Err(_) => {
                            failure(id, ErrorCode::IoError, "submission lookup worker failed")
                        }
                    }
                })
            }),
        );
    }
}

/// Called only by the opt-in branch of the existing task.run handler.
pub(crate) fn run(request: Request, service: Arc<TaskService>) -> Frame {
    let payload: SubmissionRequest = match serde_json::from_value(request.params.clone()) {
        Ok(payload) => payload,
        Err(_) => {
            return failure(
                request.id,
                ErrorCode::InvalidArgument,
                "invalid versioned task submission",
            )
        }
    };
    match service.submit(&payload) {
        Ok(receipt) => ok(&request, serde_json::json!(receipt)),
        Err(error) => submission_error(request.id, error),
    }
}

pub(crate) fn is_versioned(params: &Value) -> bool {
    [
        "schemaVersion",
        "submissionId",
        "configurationId",
        "configurationVersion",
        "fingerprint",
    ]
    .iter()
    .any(|key| params.get(key).is_some())
}
