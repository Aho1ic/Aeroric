//! Moonshot / Kimi 的 Chat Completions schema 兼容(Codex 桥接路径专用)。
//!
//! 镜像 cc-switch db41d701(`transform_codex_chat_moonshot_schema.rs`)。
//!
//! Moonshot 的 Chat Completions 端点(`api.moonshot.cn`、`api.moonshot.ai`,以及
//! Kimi For Coding 的 `api.kimi.com`)按 2019-09 之前的读法校验
//! `tools[].function.parameters`:`$ref` 只要带任何兄弟键就直接 400
//! (`tools.function.parameters is not a valid moonshot flavored json schema …
//! when using $ref, type should be defined in the referenced schema instead of
//! the parent schema`)。Codex 内置工具(schemars 生成的 `$defs.__schemaN`)正是
//! 这个形状,于是每个走本地路由打到 Kimi 的 Codex 回合都失败。
//!
//! 改写用的是 draft-07 表达同一约束的写法:把 `$ref` 挪进 `allOf`,兄弟键原地不动,
//! `{"$ref": P, "type": "string"}` → `{"allOf": [{"$ref": P}], "type": "string"}`。
//! 在 2020-12 下带兄弟键的 `$ref` 本身就是合取,所以不解引用、不合并、不丢键,
//! schema 语义保持精确。
//!
//! 作用面刻意收窄:只在 Responses → Chat 这条桥接路径上,且只在解析出的上游 host
//! 命中 Moonshot/Kimi 时执行。其它供应商的工具 schema 字节不变,prompt 缓存前缀
//! 不受扰动。以自有域名转发到 Moonshot 的中转不匹配;真出现这种报告再扩
//! [`MOONSHOT_HOST_SUFFIXES`]。

use serde_json::{json, Map, Value};
use url::Url;

/// Chat Completions 校验器拒收 `$ref` 兄弟键的上游 host。按 host 本身或其子域匹配。
const MOONSHOT_HOST_SUFFIXES: &[&str] = &["moonshot.cn", "moonshot.ai", "kimi.com"];

/// 取值是单个 schema 的关键字(draft-07 的 `items` 还可以是 schema 元组)。
/// 布尔 schema(`additionalProperties: false`)由 [`wrap_ref_siblings`] 的对象判断跳过。
const SINGLE_SCHEMA_KEYWORDS: &[&str] = &[
    "items",
    "additionalItems",
    "unevaluatedItems",
    "contains",
    "additionalProperties",
    "unevaluatedProperties",
    "propertyNames",
    "not",
    "if",
    "then",
    "else",
    "contentSchema",
];

/// 取值是 schema 数组的关键字。
const SCHEMA_ARRAY_KEYWORDS: &[&str] = &["allOf", "anyOf", "oneOf", "prefixItems"];

/// 取值是「名字 → schema」映射的关键字。draft-07 的 `dependencies` 混着 schema 值与
/// 字符串数组,数组那种由对象判断放过。
const SCHEMA_MAP_KEYWORDS: &[&str] = &[
    "properties",
    "patternProperties",
    "$defs",
    "definitions",
    "dependentSchemas",
    "dependencies",
];

/// 解析出的上游 `base_url` 是否指向 Moonshot / Kimi。
///
/// URL 解析不出来时按「不是」处理:不改写,请求原样出去,与这个模块不存在时一致。
pub(super) fn upstream_requires_ref_sibling_all_of(base_url: &str) -> bool {
    let Ok(url) = Url::parse(base_url.trim()) else {
        return false;
    };
    let Some(host) = url.host_str() else {
        return false;
    };
    let host = host.to_ascii_lowercase();
    MOONSHOT_HOST_SUFFIXES.iter().any(|suffix| {
        host == *suffix
            || host
                .strip_suffix(suffix)
                .is_some_and(|prefix| prefix.ends_with('.'))
    })
}

/// 改写 Chat Completions 请求体里 `tools[].function.parameters` 的每个
/// 「带兄弟键的 `$ref`」节点。返回被改动的工具数。
pub(super) fn wrap_ref_siblings_in_chat_tools(chat_body: &mut Value) -> usize {
    let Some(tools) = chat_body.get_mut("tools").and_then(Value::as_array_mut) else {
        return 0;
    };
    let mut changed = 0;
    for tool in tools.iter_mut() {
        let Some(parameters) = tool
            .get_mut("function")
            .and_then(|function| function.get_mut("parameters"))
        else {
            continue;
        };
        if wrap_ref_siblings(parameters) > 0 {
            changed += 1;
        }
    }
    changed
}

/// 只沿 schema 取值的关键字走一遍 JSON Schema,把每个带兄弟键的 `$ref` 挪进
/// `allOf`,返回改写的节点数。数据取值的关键字(`default`、`examples`、`enum`、
/// `const`)与 `x-…` 这类未知关键字一律不进入,所以它们里面字面量的 `$ref` 键不受影响。
fn wrap_ref_siblings(schema: &mut Value) -> usize {
    let Value::Object(map) = schema else {
        return 0;
    };
    let mut rewritten = 0;
    if map.len() > 1 && map.get("$ref").is_some_and(Value::is_string) {
        move_ref_into_all_of(map);
        rewritten += 1;
    }
    for (key, child) in map.iter_mut() {
        let key = key.as_str();
        if SCHEMA_MAP_KEYWORDS.contains(&key) {
            if let Value::Object(entries) = child {
                rewritten += entries.values_mut().map(wrap_ref_siblings).sum::<usize>();
            }
        } else if SCHEMA_ARRAY_KEYWORDS.contains(&key) {
            if let Value::Array(entries) = child {
                rewritten += entries.iter_mut().map(wrap_ref_siblings).sum::<usize>();
            }
        } else if SINGLE_SCHEMA_KEYWORDS.contains(&key) {
            match child {
                // draft-07 的元组校验:`items` 可以是 schema 数组。
                Value::Array(entries) => {
                    rewritten += entries.iter_mut().map(wrap_ref_siblings).sum::<usize>();
                }
                other => rewritten += wrap_ref_siblings(other),
            }
        }
    }
    rewritten
}

fn move_ref_into_all_of(map: &mut Map<String, Value>) {
    let Some(reference) = map.remove("$ref") else {
        return;
    };
    let branch = json!({ "$ref": reference });
    match map.get_mut("allOf") {
        Some(Value::Array(branches)) => branches.push(branch),
        _ => {
            map.insert("allOf".to_string(), Value::Array(vec![branch]));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Codex 内置工具的形状,缩到 Moonshot 拒收的两个位置:带 description 的属性
    /// `$ref`,以及带 `type`/`minLength` 兄弟键的 `$defs` 条目 `$ref`。
    fn desktop_like_schema() -> Value {
        json!({
            "type": "object",
            "properties": {
                "prompt": { "$ref": "#/$defs/__schema20", "description": "Prompt to run" },
                "mode": { "type": "string", "enum": ["fast", "slow"] }
            },
            "required": ["prompt"],
            "$defs": {
                "__schema20": { "$ref": "#/$defs/__schema2", "type": "string", "minLength": 1 },
                "__schema2": { "type": "string" }
            }
        })
    }

    fn has_ref_with_siblings(value: &Value) -> bool {
        match value {
            Value::Object(map) => {
                (map.len() > 1 && map.contains_key("$ref"))
                    || map.values().any(has_ref_with_siblings)
            }
            Value::Array(items) => items.iter().any(has_ref_with_siblings),
            _ => false,
        }
    }

    #[test]
    fn gate_matches_moonshot_and_kimi_hosts_only() {
        for url in [
            "https://api.moonshot.cn/v1",
            "https://api.moonshot.ai/v1/",
            "https://api.kimi.com/coding/v1",
            "https://API.KIMI.COM/coding/v1",
            " https://api.moonshot.cn/v1/chat/completions ",
        ] {
            assert!(upstream_requires_ref_sibling_all_of(url), "{url}");
        }
        for url in [
            "https://api.openai.com/v1",
            "https://api.x.ai/v1",
            "https://kimi-relay.example.com/v1",
            "https://api.kimi.com.evil.net/v1",
            "https://notmoonshot.cn/v1",
            "api.moonshot.cn/v1",
            "",
        ] {
            assert!(!upstream_requires_ref_sibling_all_of(url), "{url}");
        }
    }

    #[test]
    fn wraps_ref_siblings_in_properties_and_defs() {
        let mut schema = desktop_like_schema();
        assert_eq!(wrap_ref_siblings(&mut schema), 2);
        assert_eq!(
            schema,
            json!({
                "type": "object",
                "properties": {
                    "prompt": {
                        "description": "Prompt to run",
                        "allOf": [{ "$ref": "#/$defs/__schema20" }]
                    },
                    "mode": { "type": "string", "enum": ["fast", "slow"] }
                },
                "required": ["prompt"],
                "$defs": {
                    "__schema20": {
                        "type": "string",
                        "minLength": 1,
                        "allOf": [{ "$ref": "#/$defs/__schema2" }]
                    },
                    "__schema2": { "type": "string" }
                }
            })
        );
        assert!(!has_ref_with_siblings(&schema));
    }

    #[test]
    fn bare_refs_and_ref_free_schemas_are_untouched() {
        let original = json!({
            "type": "object",
            "properties": {
                "a": { "$ref": "#/$defs/A" },
                "b": { "type": "integer", "minimum": 0 }
            },
            "$defs": { "A": { "type": "string" } }
        });
        let mut schema = original.clone();
        assert_eq!(wrap_ref_siblings(&mut schema), 0);
        assert_eq!(schema, original);

        let original = json!({ "type": "object", "properties": { "a": { "type": "string" } } });
        let mut schema = original.clone();
        assert_eq!(wrap_ref_siblings(&mut schema), 0);
        assert_eq!(schema, original);
    }

    #[test]
    fn appends_ref_to_existing_all_of() {
        let mut schema = json!({
            "allOf": [{ "type": "string" }],
            "$ref": "#/$defs/A",
            "description": "d"
        });
        assert_eq!(wrap_ref_siblings(&mut schema), 1);
        assert_eq!(
            schema,
            json!({
                "allOf": [{ "type": "string" }, { "$ref": "#/$defs/A" }],
                "description": "d"
            })
        );
    }

    #[test]
    fn root_ref_keeps_type_and_defs() {
        let mut schema = json!({
            "type": "object",
            "$ref": "#/$defs/Root",
            "$defs": { "Root": { "properties": { "a": { "type": "string" } } } }
        });
        assert_eq!(wrap_ref_siblings(&mut schema), 1);
        assert_eq!(
            schema,
            json!({
                "type": "object",
                "$defs": { "Root": { "properties": { "a": { "type": "string" } } } },
                "allOf": [{ "$ref": "#/$defs/Root" }]
            })
        );
    }

    #[test]
    fn does_not_enter_data_values_or_unknown_keywords() {
        let original = json!({
            "type": "object",
            "properties": {
                // 名字就叫 `$ref` 的属性是属性,不是引用。
                "$ref": { "type": "string", "description": "literal name" },
                "cfg": {
                    "type": "object",
                    "default": { "$ref": "literal", "note": "data" },
                    "examples": [{ "$ref": "literal", "note": "data" }],
                    "enum": [{ "$ref": "literal", "note": "data" }],
                    "const": { "$ref": "literal", "note": "data" }
                }
            },
            "x-metadata": { "$ref": "literal", "note": "vendor extension" },
            "$ref_like": { "$ref": "literal", "note": "unknown keyword" }
        });
        let mut schema = original.clone();
        assert_eq!(wrap_ref_siblings(&mut schema), 0);
        assert_eq!(schema, original);
    }

    #[test]
    fn covers_every_schema_valued_keyword() {
        let node = || json!({ "$ref": "#/$defs/A", "description": "d" });
        let mut schema = json!({
            "type": "object",
            "properties": { "p": node() },
            "patternProperties": { "^x": node() },
            "additionalProperties": node(),
            "unevaluatedProperties": node(),
            "propertyNames": node(),
            "dependentSchemas": { "p": node() },
            "dependencies": { "p": node(), "q": ["p"] },
            "items": node(),
            "prefixItems": [node()],
            "contains": node(),
            "not": node(),
            "if": node(),
            "then": node(),
            "else": node(),
            "allOf": [node()],
            "anyOf": [node()],
            "oneOf": [node()],
            "$defs": { "A": { "type": "string" }, "B": node() },
            "definitions": { "C": node() }
        });
        assert_eq!(wrap_ref_siblings(&mut schema), 19);
        assert!(!has_ref_with_siblings(&schema));
        assert_eq!(schema["dependencies"]["q"], json!(["p"]));

        // draft-07 的 `items` 元组形式。
        let mut tuple = json!({ "type": "array", "items": [node(), { "type": "string" }] });
        assert_eq!(wrap_ref_siblings(&mut tuple), 1);
        assert!(!has_ref_with_siblings(&tuple));
    }

    #[test]
    fn rewrite_is_idempotent() {
        let mut once = desktop_like_schema();
        wrap_ref_siblings(&mut once);
        let mut twice = once.clone();
        assert_eq!(wrap_ref_siblings(&mut twice), 0);
        assert_eq!(twice, once);
    }

    #[test]
    fn chat_body_helper_counts_changed_tools_and_skips_non_function_tools() {
        let clean = json!({
            "type": "function",
            "function": {
                "name": "clean",
                "parameters": { "type": "object", "properties": { "a": { "type": "string" } } }
            }
        });
        let mut body = json!({
            "model": "k3",
            "messages": [],
            "tools": [
                { "type": "function", "function": { "name": "desktop", "parameters": desktop_like_schema() } },
                clean.clone(),
                { "type": "web_search" }
            ]
        });
        assert_eq!(wrap_ref_siblings_in_chat_tools(&mut body), 1);
        assert!(!has_ref_with_siblings(
            &body["tools"][0]["function"]["parameters"]
        ));
        assert_eq!(body["tools"][1], clean);
        assert_eq!(body["tools"][2], json!({ "type": "web_search" }));

        let mut no_tools = json!({ "model": "k3", "messages": [] });
        assert_eq!(wrap_ref_siblings_in_chat_tools(&mut no_tools), 0);
    }
}
