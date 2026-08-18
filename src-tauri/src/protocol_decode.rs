use serde_json::Value;

/// Read an optional string field shared by session and remote protocol
/// decoders. Wrong JSON types remain absent to preserve existing compatibility.
pub(crate) fn string_field<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn reads_optional_string_fields_without_panicking_on_wrong_types() {
        let value = json!({ "sessionId": "s-1", "count": 1 });
        assert_eq!(string_field(&value, "sessionId"), Some("s-1"));
        assert_eq!(string_field(&value, "count"), None);
    }
}
