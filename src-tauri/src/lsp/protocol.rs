use serde_json::Value;
use std::path::Path;

use super::{LspCompletionItem, LspHover, LspLocation, LspPosition, LspRange};

pub(super) fn file_uri(path: &Path) -> String {
    let path = path.to_string_lossy();
    #[cfg(windows)]
    {
        format!("file:///{}", percent_encode_path(&path.replace('\\', "/")))
    }
    #[cfg(not(windows))]
    {
        format!("file://{}", percent_encode_path(&path))
    }
}

fn percent_encode_path(path: &str) -> String {
    let mut encoded = String::new();
    for byte in path.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'/' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(byte as char)
            }
            _ => encoded.push_str(&format!("%{byte:02X}")),
        }
    }
    encoded
}

pub(super) fn path_from_file_uri(uri: &str) -> String {
    let without_scheme = uri.strip_prefix("file://").unwrap_or(uri);
    percent_decode_path(without_scheme)
}

fn percent_decode_path(path: &str) -> String {
    let mut output = Vec::new();
    let bytes = path.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            if let Ok(value) = u8::from_str_radix(&path[index + 1..index + 3], 16) {
                output.push(value);
                index += 3;
                continue;
            }
        }
        output.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&output).into_owned()
}

pub(super) fn parse_hover(value: Option<&Value>) -> Option<LspHover> {
    let value = value?;
    if value.is_null() {
        return None;
    }
    let contents = markdown_text(value.get("contents")?)?;
    Some(LspHover {
        contents,
        range: value.get("range").and_then(parse_range),
    })
}

pub(super) fn parse_locations(value: Option<&Value>) -> Vec<LspLocation> {
    match value {
        Some(Value::Array(items)) => items.iter().filter_map(parse_location).collect(),
        Some(value) => parse_location(value).into_iter().collect(),
        None => Vec::new(),
    }
}

fn parse_location(value: &Value) -> Option<LspLocation> {
    let uri = value
        .get("uri")
        .or_else(|| value.get("targetUri"))?
        .as_str()?
        .to_string();
    let range_value = value
        .get("range")
        .or_else(|| value.get("targetSelectionRange"))
        .or_else(|| value.get("targetRange"))?;
    let range = parse_range(range_value)?;
    Some(LspLocation {
        path: path_from_file_uri(&uri),
        uri,
        range,
    })
}

pub(super) fn parse_range(value: &Value) -> Option<LspRange> {
    Some(LspRange {
        start: parse_position(value.get("start")?)?,
        end: parse_position(value.get("end")?)?,
    })
}

pub(super) fn parse_position(value: &Value) -> Option<LspPosition> {
    Some(LspPosition {
        line: value.get("line")?.as_u64()? as u32,
        character: value.get("character")?.as_u64()? as u32,
    })
}

pub(super) fn parse_completion_items(value: Option<&Value>) -> Vec<LspCompletionItem> {
    let Some(value) = value else {
        return Vec::new();
    };
    let items = if let Some(items) = value.get("items").and_then(Value::as_array) {
        items
    } else if let Some(items) = value.as_array() {
        items
    } else {
        return Vec::new();
    };

    items
        .iter()
        .filter_map(|item| {
            Some(LspCompletionItem {
                label: item.get("label")?.as_str()?.to_string(),
                detail: item
                    .get("detail")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                documentation: item.get("documentation").and_then(markdown_text),
            })
        })
        .collect()
}

pub(super) fn markdown_text(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Object(map) => map
            .get("value")
            .and_then(Value::as_str)
            .map(str::to_string)
            .or_else(|| map.get("contents").and_then(markdown_text)),
        Value::Array(items) => {
            let parts: Vec<_> = items.iter().filter_map(markdown_text).collect();
            (!parts.is_empty()).then(|| parts.join("\n\n"))
        }
        _ => None,
    }
}
