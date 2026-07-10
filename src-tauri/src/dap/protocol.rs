use serde_json::Value;
use std::io::{BufRead, Write};

use super::{DebugCallFrame, DebugEvaluateResult, DebugVariable};

pub(super) fn write_debug_adapter_message<W: Write>(
    writer: &mut W,
    message: &Value,
) -> Result<(), String> {
    let body = message.to_string();
    write!(writer, "Content-Length: {}\r\n\r\n{}", body.len(), body).map_err(|e| e.to_string())?;
    writer.flush().map_err(|e| e.to_string())
}

pub(super) fn read_debug_adapter_message<R: BufRead>(
    reader: &mut R,
) -> Result<Option<Value>, String> {
    let mut content_length = None;
    let mut line = String::new();
    loop {
        line.clear();
        let count = reader.read_line(&mut line).map_err(|e| e.to_string())?;
        if count == 0 {
            return Ok(None);
        }
        let header = line.trim_end_matches(['\r', '\n']);
        if header.is_empty() {
            break;
        }
        if let Some((name, value)) = header.split_once(':') {
            if name.eq_ignore_ascii_case("content-length") {
                content_length = Some(
                    value
                        .trim()
                        .parse::<usize>()
                        .map_err(|e| format!("Invalid DAP content length: {e}"))?,
                );
            }
        }
    }
    let length = content_length.ok_or_else(|| "Missing DAP content length".to_string())?;
    let mut body = vec![0; length];
    reader.read_exact(&mut body).map_err(|e| e.to_string())?;
    serde_json::from_slice(&body)
        .map(Some)
        .map_err(|e| format!("Invalid DAP message JSON: {e}"))
}

pub(super) fn debug_adapter_variable_object_id(reference: i64) -> String {
    format!("dap:{reference}")
}

pub(super) fn parse_debug_adapter_variable_reference(object_id: &str) -> Result<i64, String> {
    object_id
        .strip_prefix("dap:")
        .ok_or_else(|| "Debug variable object id is not a DAP reference".to_string())?
        .parse::<i64>()
        .map_err(|e| format!("Invalid DAP variable reference: {e}"))
}

pub(super) fn parse_debug_adapter_variables(response: &Value, limit: usize) -> Vec<DebugVariable> {
    response
        .get("body")
        .and_then(|body| body.get("variables"))
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .take(limit)
                .filter_map(|item| {
                    let name = item.get("name").and_then(Value::as_str)?.to_string();
                    let value = item
                        .get("value")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string();
                    let type_name = item.get("type").and_then(Value::as_str).map(str::to_string);
                    let reference = item
                        .get("variablesReference")
                        .and_then(Value::as_i64)
                        .unwrap_or(0);
                    Some(DebugVariable {
                        name,
                        value,
                        type_name,
                        object_id: (reference > 0)
                            .then(|| debug_adapter_variable_object_id(reference)),
                        has_children: reference > 0,
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

fn debug_adapter_source_path(source: &Value) -> String {
    source
        .get("path")
        .and_then(Value::as_str)
        .or_else(|| source.get("name").and_then(Value::as_str))
        .unwrap_or_default()
        .to_string()
}

pub(super) fn parse_debug_adapter_stack_frames(response: &Value) -> Vec<(DebugCallFrame, i64)> {
    response
        .get("body")
        .and_then(|body| body.get("stackFrames"))
        .and_then(Value::as_array)
        .map(|frames| {
            frames
                .iter()
                .take(32)
                .filter_map(|frame| {
                    let id = frame.get("id").and_then(Value::as_i64)?;
                    let function_name = frame
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or("<module>")
                        .to_string();
                    let file = frame
                        .get("source")
                        .map(debug_adapter_source_path)
                        .unwrap_or_default();
                    let line = frame.get("line").and_then(Value::as_u64).unwrap_or(1) as u32;
                    let column = frame.get("column").and_then(Value::as_u64).unwrap_or(1) as u32;
                    Some((
                        DebugCallFrame {
                            function_name,
                            file,
                            line,
                            column,
                            frame_id: Some(format!("dap:{id}")),
                        },
                        id,
                    ))
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

pub(super) fn debug_adapter_request_success(response: &Value) -> bool {
    response
        .get("success")
        .and_then(Value::as_bool)
        .unwrap_or(true)
}

pub(super) fn debug_adapter_response_error(response: &Value) -> String {
    response
        .get("message")
        .and_then(Value::as_str)
        .or_else(|| {
            response
                .get("body")
                .and_then(|body| body.get("error"))
                .and_then(|error| error.get("format"))
                .and_then(Value::as_str)
        })
        .unwrap_or("Debug adapter request failed")
        .to_string()
}

pub(super) fn parse_debug_adapter_evaluate_result(
    response: &Value,
    expression: &str,
) -> DebugEvaluateResult {
    let body = response.get("body").cloned().unwrap_or_default();
    let result = body
        .get("result")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let type_name = body.get("type").and_then(Value::as_str).map(str::to_string);
    let reference = body
        .get("variablesReference")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    DebugEvaluateResult {
        expression: expression.to_string(),
        result,
        type_name,
        object_id: (reference > 0).then(|| debug_adapter_variable_object_id(reference)),
        has_children: reference > 0,
    }
}

fn property_value_to_string(value: &Value) -> (String, Option<String>) {
    let value_type = value
        .get("type")
        .and_then(Value::as_str)
        .map(str::to_string);
    let rendered = value
        .get("value")
        .map(|value| match value {
            Value::String(text) => text.clone(),
            Value::Null => "null".to_string(),
            Value::Bool(flag) => flag.to_string(),
            Value::Number(number) => number.to_string(),
            _ => value
                .get("description")
                .and_then(Value::as_str)
                .map(str::to_string)
                .unwrap_or_else(|| value.to_string()),
        })
        .or_else(|| {
            value
                .get("description")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .unwrap_or_else(|| "undefined".to_string());
    (rendered, value_type)
}

fn debug_variable_from_property(item: &Value) -> Option<DebugVariable> {
    let name = item.get("name").and_then(Value::as_str)?.to_string();
    let value = item.get("value")?;
    let (value_text, value_type) = property_value_to_string(value);
    let object_id = value
        .get("objectId")
        .and_then(Value::as_str)
        .map(str::to_string);
    Some(DebugVariable {
        name,
        value: value_text,
        type_name: value_type,
        has_children: object_id.is_some(),
        object_id,
    })
}

pub(super) fn debug_evaluate_result_from_remote_object(
    expression: &str,
    value: &Value,
) -> DebugEvaluateResult {
    let (result, type_name) = property_value_to_string(value);
    let object_id = value
        .get("objectId")
        .and_then(Value::as_str)
        .map(str::to_string);
    DebugEvaluateResult {
        expression: expression.to_string(),
        result,
        type_name,
        has_children: object_id.is_some(),
        object_id,
    }
}

pub(super) fn cdp_error_message(response: &Value) -> Option<String> {
    response
        .get("error")
        .and_then(|error| {
            error
                .get("message")
                .and_then(Value::as_str)
                .or_else(|| error.get("data").and_then(Value::as_str))
        })
        .map(str::to_string)
}

pub(super) fn cdp_exception_message(response: &Value) -> Option<String> {
    let details = response.get("result")?.get("exceptionDetails")?;
    details
        .get("exception")
        .and_then(|exception| exception.get("description").and_then(Value::as_str))
        .or_else(|| details.get("text").and_then(Value::as_str))
        .map(str::to_string)
}

pub(super) fn parse_debug_variables_from_properties(
    result: &Value,
    limit: usize,
) -> Vec<DebugVariable> {
    result
        .get("result")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .take(limit)
                .filter_map(debug_variable_from_property)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}
