use super::*;
use serde_json::json;

fn schema() -> Value {
    json!({
        "type": "object",
        "required": ["status", "items"],
        "additionalProperties": false,
        "properties": {
            "status": {"type": "string", "enum": ["ok", "attention"]},
            "count": {"type": "integer"},
            "items": {"type": "array", "items": {
                "type": "object", "required": ["title"],
                "properties": {"title": {"type": "string"}, "score": {"type": "number"}}
            }}
        }
    })
}

/// Only the keywords the checker checks are taken, so nothing written is
/// silently not checked.
#[test]
fn a_structure_is_taken_only_when_every_part_of_it_is_checked() {
    assert_eq!(check_schema(&schema()), Ok(()));
    assert!(check_schema(&json!({"properties": {}}))
        .unwrap_err()
        .contains("type"));
    let pattern = check_schema(&json!({"type": "object", "properties": {
        "name": {"type": "string", "pattern": "^a"}}}))
    .unwrap_err();
    assert!(
        pattern.contains("$.name") && pattern.contains("pattern"),
        "{pattern}"
    );
    assert!(check_schema(&json!({"type": "date"})).is_err());
    assert!(check_schema(&json!({"type": "object", "additionalProperties": true})).is_err());
    let long = json!({"type": "string", "description": "长".repeat(SCHEMA_MAX_CHARS)});
    assert!(check_schema(&long).unwrap_err().contains("太长"));
}

/// The last JSON block counts; a reply that is JSON itself counts too.
#[test]
fn the_result_is_the_last_json_block_of_the_reply() {
    let reply = "先看了一下。\n```json\n{\"draft\": true}\n```\n结论：\n```json\n{\"status\": \"ok\", \"items\": []}\n```\n就这些。";
    assert_eq!(extract(reply), Ok(json!({"status": "ok", "items": []})));
    assert_eq!(extract("  {\"a\": 1} "), Ok(json!({"a": 1})));
    assert!(extract("```sh\nls\n```\n一切正常").is_err());
    assert!(extract("一切正常，没有发现问题").is_err());
}

/// The first place a value does not fit is named, with its path.
#[test]
fn a_value_that_does_not_fit_says_where() {
    let fits = json!({"status": "attention", "count": 2,
        "items": [{"title": "a", "score": 1.5}, {"title": "b", "score": 2}]});
    assert_eq!(validate(&schema(), &fits), Ok(()));
    let cases = [
        (json!({"status": "ok"}), "缺少字段 items"),
        (json!({"status": "fine", "items": []}), "$.status 只能是"),
        (
            json!({"status": "ok", "items": [{"score": 1}]}),
            "$.items[0] 缺少字段 title",
        ),
        (
            json!({"status": "ok", "items": [], "count": 1.5}),
            "$.count 应是整数",
        ),
        (
            json!({"status": "ok", "items": [], "extra": 1}),
            "多了字段 extra",
        ),
        (json!(["ok"]), "$ 应是对象"),
    ];
    for (value, says) in cases {
        let why = validate(&schema(), &value).unwrap_err();
        assert!(why.contains(says), "{value}: {why}");
    }
}

#[test]
fn the_verdict_comes_from_the_last_reply() {
    let ok = judge(
        &schema(),
        Some("```json\n{\"status\": \"ok\", \"items\": []}\n```"),
    );
    assert_eq!(ok, Ok(json!({"status": "ok", "items": []})));
    assert!(judge(&schema(), None)
        .unwrap_err()
        .contains("没有 Agent 的回复"));
    assert!(judge(&schema(), Some("一切正常"))
        .unwrap_err()
        .contains("没有可以读的 JSON"));
    assert!(judge(&schema(), Some("{\"status\": \"ok\"}"))
        .unwrap_err()
        .starts_with("结果不符合结构："));
    assert!(
        instruction(&schema()).contains("```json") && instruction(&schema()).contains("\"enum\"")
    );
}
