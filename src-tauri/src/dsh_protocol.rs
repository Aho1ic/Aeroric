use serde_json::Value;

/// Decode the stable DSH HTTP RPC result envelope while preserving its legacy
/// defaults and error wording.
pub(crate) fn decode_result_value(body: Value, method: &str) -> Result<Value, String> {
    let result = body
        .get("result")
        .ok_or_else(|| format!("DSH API {method} response has no result"))?;
    if result.get("ok").and_then(Value::as_bool) != Some(true) {
        let message = result
            .get("error")
            .and_then(|error| error.get("message"))
            .and_then(Value::as_str)
            .unwrap_or("unknown DSH API error");
        return Err(format!("DSH API {method} rejected the request: {message}"));
    }
    Ok(result.get("value").cloned().unwrap_or(Value::Null))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn decodes_success_value_and_defaults_missing_value_to_null() {
        assert_eq!(
            decode_result_value(
                json!({ "result": { "ok": true, "value": { "id": 7 } } }),
                "test"
            )
            .unwrap(),
            json!({ "id": 7 })
        );
        assert_eq!(
            decode_result_value(json!({ "result": { "ok": true } }), "test").unwrap(),
            Value::Null
        );
    }

    #[test]
    fn preserves_missing_result_and_rejection_errors() {
        assert_eq!(
            decode_result_value(json!({}), "session.list").unwrap_err(),
            "DSH API session.list response has no result"
        );
        assert_eq!(
            decode_result_value(
                json!({ "result": { "ok": false, "error": { "message": "denied" } } }),
                "session.list",
            )
            .unwrap_err(),
            "DSH API session.list rejected the request: denied"
        );
    }
}
