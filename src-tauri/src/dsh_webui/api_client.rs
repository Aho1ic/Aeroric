use serde_json::{json, Value};
use tokio::time::Duration;
use uuid::Uuid;

pub(super) const DSH_HTTP_ERROR_SNIPPET_BYTES: usize = 200;

#[derive(Clone)]
pub(crate) struct DshApiClient {
    pub(super) client: reqwest::Client,
    pub(super) base_url: String,
}

pub(super) fn bounded_utf8_prefix(value: &str, max_bytes: usize) -> &str {
    if value.len() <= max_bytes {
        return value;
    }
    let mut boundary = max_bytes;
    while !value.is_char_boundary(boundary) {
        boundary -= 1;
    }
    &value[..boundary]
}

impl DshApiClient {
    pub(crate) fn new(base_url: String) -> Result<Self, String> {
        let client = reqwest::Client::builder()
            .no_proxy()
            .build()
            .map_err(|error| format!("Failed to create DSH API client: {error}"))?;
        Ok(Self { client, base_url })
    }

    pub(super) async fn call(&self, method: &str, payload: Value) -> Result<Value, String> {
        let request = json!({
            "type": "client-request",
            "rpcId": Uuid::new_v4().to_string(),
            "method": method,
            "payload": payload,
        });
        let response = self
            .client
            .post(format!("{}/api/{method}", self.base_url))
            .header("content-type", "application/json")
            .json(&request)
            .timeout(Duration::from_secs(30))
            .send()
            .await
            .map_err(|error| format!("DSH API request {method} failed: {error}"))?;
        let status = response.status();
        // Non-success responses may be HTML; preserve the HTTP status instead
        // of turning them into a misleading JSON decoder failure.
        if !status.is_success() {
            let text = response.text().await.unwrap_or_default();
            let trimmed = text.trim();
            return if trimmed.is_empty() {
                Err(format!("DSH API {method} returned HTTP {status}"))
            } else {
                let snippet = bounded_utf8_prefix(trimmed, DSH_HTTP_ERROR_SNIPPET_BYTES);
                Err(format!(
                    "DSH API {method} returned HTTP {status}: {snippet}"
                ))
            };
        }
        let body: Value = response
            .json()
            .await
            .map_err(|error| format!("DSH API response {method} was invalid JSON: {error}"))?;
        crate::dsh_protocol::decode_result_value(body, method)
    }
}
