//! E16 (2026-11-21): a rule can ask for its result in a JSON structure.
//! The prompt says so; once the session ends, the agent's last reply must
//! hold a JSON value that fits, or the run is marked failed — never kept as
//! prose that looks like a result.
//!
//! The structure is a small part of JSON Schema, checked here without a
//! library: `type` (one or a list of `object` `array` `string` `number`
//! `integer` `boolean` `null`), `properties`, `required`,
//! `additionalProperties: false`, `items` and `enum`. Other keywords are
//! refused when the rule is saved, so nothing the user wrote is silently
//! not checked.

use serde_json::Value;

/// How long a structure may be, written out.
pub const SCHEMA_MAX_CHARS: usize = 4000;

const TYPES: &[&str] = &[
    "object", "array", "string", "number", "integer", "boolean", "null",
];
const KEYWORDS: &[&str] = &[
    "type",
    "properties",
    "required",
    "additionalProperties",
    "items",
    "enum",
    "description",
    "title",
    "$schema",
];

/// A structure the checker can check in full, or why not.
pub fn check_schema(schema: &Value) -> Result<(), String> {
    if serde_json::to_string(schema).map_or(0, |s| s.chars().count()) > SCHEMA_MAX_CHARS {
        return Err(format!("结果结构太长，最多 {SCHEMA_MAX_CHARS} 字"));
    }
    if schema.get("type").is_none() {
        return Err("结果结构最外层要写 type，例如 \"object\"".into());
    }
    check_node(schema, "$")
}

fn check_node(node: &Value, at: &str) -> Result<(), String> {
    let Some(fields) = node.as_object() else {
        return Err(format!("结果结构 {at} 应是一个对象"));
    };
    if let Some(key) = fields.keys().find(|k| !KEYWORDS.contains(&k.as_str())) {
        return Err(format!(
            "结果结构 {at} 用了不支持的 {key}；支持 type、properties、required、additionalProperties: false、items、enum"
        ));
    }
    if let Some(kind) = fields.get("type") {
        let names: Vec<&Value> = match kind {
            Value::Array(list) => list.iter().collect(),
            one => vec![one],
        };
        if names.is_empty()
            || names
                .iter()
                .any(|n| !n.as_str().is_some_and(|n| TYPES.contains(&n)))
        {
            return Err(format!("结果结构 {at} 的 type 不认识"));
        }
    }
    if let Some(extra) = fields.get("additionalProperties") {
        if extra != &Value::Bool(false) {
            return Err(format!(
                "结果结构 {at} 的 additionalProperties 只支持 false"
            ));
        }
    }
    if let Some(required) = fields.get("required") {
        if !required
            .as_array()
            .is_some_and(|r| r.iter().all(Value::is_string))
        {
            return Err(format!("结果结构 {at} 的 required 应是字段名的列表"));
        }
    }
    if let Some(choices) = fields.get("enum") {
        if !choices.as_array().is_some_and(|c| !c.is_empty()) {
            return Err(format!("结果结构 {at} 的 enum 应是非空列表"));
        }
    }
    if let Some(properties) = fields.get("properties") {
        let Some(properties) = properties.as_object() else {
            return Err(format!("结果结构 {at} 的 properties 应是一个对象"));
        };
        for (name, child) in properties {
            check_node(child, &format!("{at}.{name}"))?;
        }
    }
    if let Some(items) = fields.get("items") {
        check_node(items, &format!("{at}[]"))?;
    }
    Ok(())
}

/// What the prompt adds: where the result goes and what it must fit.
pub fn instruction(schema: &Value) -> String {
    let shown = serde_json::to_string_pretty(schema).unwrap_or_else(|_| schema.to_string());
    format!(
        "\n\n这是一个要交结果的定时任务：在你最后一条回复里，用一个 ```json 代码块给出这次的结果，它必须符合下面的结构（JSON Schema）。Aeroric 会按它检查，不符合这次运行就记为失败。\n```json\n{shown}\n```"
    )
}

/// The result in `reply`: the last fenced block that is JSON, or the whole
/// reply when it is JSON itself.
pub fn extract(reply: &str) -> Result<Value, String> {
    let mut found = None;
    let mut rest = reply;
    while let Some(open) = rest.find("```") {
        let after = &rest[open + 3..];
        let Some(line_end) = after.find('\n') else {
            break;
        };
        let body_start = &after[line_end + 1..];
        let Some(close) = body_start.find("```") else {
            break;
        };
        let lang = after[..line_end].trim().to_ascii_lowercase();
        if lang.is_empty() || lang == "json" {
            if let Ok(value) = serde_json::from_str::<Value>(body_start[..close].trim()) {
                found = Some(value);
            }
        }
        rest = &body_start[close + 3..];
    }
    if let Some(value) = found {
        return Ok(value);
    }
    serde_json::from_str::<Value>(reply.trim())
        .map_err(|_| "最后一条回复里没有可以读的 JSON 结果".to_string())
}

/// `value` fits `schema`, or the first place it does not.
pub fn validate(schema: &Value, value: &Value) -> Result<(), String> {
    check_value(schema, value, "$")
}

fn type_of(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(n) if n.is_i64() || n.is_u64() => "integer",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

fn type_name(kind: &str) -> &'static str {
    match kind {
        "object" => "对象",
        "array" => "列表",
        "string" => "字符串",
        "number" => "数字",
        "integer" => "整数",
        "boolean" => "true 或 false",
        _ => "null",
    }
}

fn check_value(schema: &Value, value: &Value, at: &str) -> Result<(), String> {
    if let Some(kind) = schema.get("type") {
        let wanted: Vec<&str> = match kind {
            Value::Array(list) => list.iter().filter_map(Value::as_str).collect(),
            one => one.as_str().into_iter().collect(),
        };
        let actual = type_of(value);
        let fits = wanted
            .iter()
            .any(|w| *w == actual || (*w == "number" && actual == "integer"));
        if !fits {
            let names: Vec<&str> = wanted.iter().map(|w| type_name(w)).collect();
            return Err(format!("{at} 应是{}", names.join("或")));
        }
    }
    if let Some(choices) = schema.get("enum").and_then(Value::as_array) {
        if !choices.contains(value) {
            let shown: Vec<String> = choices.iter().map(Value::to_string).collect();
            return Err(format!("{at} 只能是 {} 之一", shown.join("、")));
        }
    }
    if let Some(fields) = value.as_object() {
        let properties = schema.get("properties").and_then(Value::as_object);
        for name in schema
            .get("required")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            if !fields.contains_key(name) {
                return Err(format!("{at} 缺少字段 {name}"));
            }
        }
        for (name, child) in fields {
            match properties.and_then(|p| p.get(name)) {
                Some(rule) => check_value(rule, child, &format!("{at}.{name}"))?,
                None if schema.get("additionalProperties") == Some(&Value::Bool(false)) => {
                    return Err(format!("{at} 多了字段 {name}"));
                }
                None => {}
            }
        }
    }
    if let (Some(items), Some(list)) = (schema.get("items"), value.as_array()) {
        for (i, item) in list.iter().enumerate() {
            check_value(items, item, &format!("{at}[{i}]"))?;
        }
    }
    Ok(())
}

/// The run's verdict from the session's last reply (`None`: no reply).
pub fn judge(schema: &Value, reply: Option<&str>) -> Result<Value, String> {
    let reply = reply
        .map(str::trim)
        .filter(|r| !r.is_empty())
        .ok_or("会话结束时没有 Agent 的回复，没有结果")?;
    let value = extract(reply)?;
    validate(schema, &value).map_err(|why| format!("结果不符合结构：{why}"))?;
    Ok(value)
}

#[cfg(test)]
#[path = "result_tests.rs"]
mod tests;
