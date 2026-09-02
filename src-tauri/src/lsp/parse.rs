//! LSP 响应 JSON → 本仓库的强类型结构。
//!
//! 从 `lsp.rs` 整块搬出来,内容一行没改。全是纯函数:`Option<&Value>` 进,
//! `Option<T>` / `Vec<T>` 出,不读盘、不发请求、不碰全局会话表。
//!
//! 每个函数都在**解析别的进程(语言服务器)吐出来的东西**,形状不受我们控制 ——
//! 所以一律走 `?` 短路 + 逐字段 `get()`,少一个字段就整条跳过,不 panic 也不报错。
//! `lsp/protocol.rs` 里那批(`parse_position` / `parse_range` / `parse_hover` 等)
//! 是同一层的更基础件,这里直接靠 `use super::*;` 拿到。

use super::*;

pub(super) fn parse_signature_help(value: Option<&Value>) -> Option<LspSignatureHelp> {
    let value = value?;
    if value.is_null() {
        return None;
    }
    let signatures = value
        .get("signatures")
        .and_then(Value::as_array)?
        .iter()
        .filter_map(parse_signature_information)
        .collect::<Vec<_>>();
    if signatures.is_empty() {
        return None;
    }
    Some(LspSignatureHelp {
        signatures,
        active_signature: value
            .get("activeSignature")
            .and_then(Value::as_u64)
            .map(|value| value as u32),
        active_parameter: value
            .get("activeParameter")
            .and_then(Value::as_u64)
            .map(|value| value as u32),
    })
}

pub(super) fn parse_signature_information(value: &Value) -> Option<LspSignatureInformation> {
    let label = value.get("label")?.as_str()?.to_string();
    let parameters = value
        .get("parameters")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| parse_parameter_information(item, &label))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    Some(LspSignatureInformation {
        label,
        documentation: value.get("documentation").and_then(markdown_text),
        parameters,
    })
}

pub(super) fn parse_parameter_information(
    value: &Value,
    signature_label: &str,
) -> Option<LspParameterInformation> {
    let label_value = value.get("label")?;
    let label = if let Some(text) = label_value.as_str() {
        text.to_string()
    } else {
        let range = label_value.as_array()?;
        let start = range.first()?.as_u64()? as usize;
        let end = range.get(1)?.as_u64()? as usize;
        signature_label.get(start..end)?.to_string()
    };
    Some(LspParameterInformation {
        label,
        documentation: value.get("documentation").and_then(markdown_text),
    })
}

pub(super) fn parse_code_actions(value: Option<&Value>) -> Vec<LspCodeAction> {
    let Some(Value::Array(items)) = value else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| {
            let title = item.get("title")?.as_str()?.to_string();
            Some(LspCodeAction {
                title,
                kind: item.get("kind").and_then(Value::as_str).map(str::to_string),
                edit: parse_workspace_edit(item.get("edit")),
                command: parse_lsp_command(item.get("command")),
            })
        })
        .collect()
}

pub(super) fn parse_lsp_command(value: Option<&Value>) -> Option<LspCommand> {
    let value = value?;
    let command = value.get("command")?.as_str()?.to_string();
    Some(LspCommand {
        title: value
            .get("title")
            .and_then(Value::as_str)
            .map(str::to_string),
        command,
        arguments: value
            .get("arguments")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default(),
    })
}

pub(super) fn parse_inlay_hints(value: Option<&Value>) -> Vec<LspInlayHint> {
    let Some(Value::Array(items)) = value else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| {
            let label = inlay_hint_label(item.get("label")?)?;
            if label.trim().is_empty() {
                return None;
            }
            Some(LspInlayHint {
                label,
                position: parse_position(item.get("position")?)?,
                kind: item
                    .get("kind")
                    .and_then(Value::as_u64)
                    .map(|value| value as u32),
                tooltip: item.get("tooltip").and_then(markdown_text),
                padding_left: item
                    .get("paddingLeft")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                padding_right: item
                    .get("paddingRight")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
            })
        })
        .collect()
}

pub(super) fn inlay_hint_label(value: &Value) -> Option<String> {
    match value {
        Value::String(label) => Some(label.clone()),
        Value::Array(parts) => {
            let label: String = parts
                .iter()
                .filter_map(|part| part.get("value").and_then(Value::as_str))
                .collect();
            Some(label)
        }
        _ => None,
    }
}

pub(super) fn parse_document_symbols(value: Option<&Value>, document_uri: &str) -> Vec<LspSymbol> {
    let Some(Value::Array(items)) = value else {
        return Vec::new();
    };
    let mut symbols = Vec::new();
    for item in items {
        collect_document_symbol(item, document_uri, None, &mut symbols);
    }
    symbols
}

pub(super) fn collect_document_symbol(
    value: &Value,
    document_uri: &str,
    container_name: Option<&str>,
    symbols: &mut Vec<LspSymbol>,
) {
    if let Some(symbol) = parse_document_symbol(value, document_uri, container_name) {
        let next_container = symbol.name.clone();
        symbols.push(symbol);
        if let Some(children) = value.get("children").and_then(Value::as_array) {
            for child in children {
                collect_document_symbol(child, document_uri, Some(&next_container), symbols);
            }
        }
    } else if let Some(symbol) = parse_symbol_information(value) {
        symbols.push(symbol);
    }
}

pub(super) fn parse_document_symbol(
    value: &Value,
    document_uri: &str,
    container_name: Option<&str>,
) -> Option<LspSymbol> {
    let name = value.get("name")?.as_str()?.to_string();
    let range = parse_range(value.get("range")?)?;
    let selection_range =
        parse_range(value.get("selectionRange")?).unwrap_or_else(|| range.clone());
    Some(LspSymbol {
        name,
        kind: value.get("kind")?.as_u64()? as u32,
        detail: value
            .get("detail")
            .and_then(Value::as_str)
            .map(str::to_string),
        container_name: container_name.map(str::to_string),
        uri: document_uri.to_string(),
        path: path_from_file_uri(document_uri),
        range,
        selection_range,
    })
}

pub(super) fn parse_workspace_symbols(value: Option<&Value>) -> Vec<LspSymbol> {
    let Some(Value::Array(items)) = value else {
        return Vec::new();
    };
    items.iter().filter_map(parse_symbol_information).collect()
}

pub(super) fn parse_symbol_information(value: &Value) -> Option<LspSymbol> {
    let name = value.get("name")?.as_str()?.to_string();
    let location = value.get("location")?;
    let uri = location.get("uri")?.as_str()?.to_string();
    let range = parse_range(location.get("range")?)?;
    Some(LspSymbol {
        name,
        kind: value.get("kind")?.as_u64()? as u32,
        detail: value
            .get("detail")
            .and_then(Value::as_str)
            .map(str::to_string),
        container_name: value
            .get("containerName")
            .and_then(Value::as_str)
            .map(str::to_string),
        uri: uri.clone(),
        path: path_from_file_uri(&uri),
        selection_range: range.clone(),
        range,
    })
}

pub(super) fn parse_workspace_edit(value: Option<&Value>) -> Option<LspWorkspaceEdit> {
    let value = value?;
    if value.is_null() {
        return None;
    }

    let mut files: BTreeMap<String, LspFileEdit> = BTreeMap::new();
    if let Some(changes) = value.get("changes").and_then(Value::as_object) {
        for (uri, edits_value) in changes {
            let Some(edits) = parse_text_edits(edits_value) else {
                continue;
            };
            files.insert(
                uri.clone(),
                LspFileEdit {
                    uri: uri.clone(),
                    path: path_from_file_uri(uri),
                    edits,
                },
            );
        }
    }

    if let Some(document_changes) = value.get("documentChanges").and_then(Value::as_array) {
        for change in document_changes {
            let Some(uri) = change
                .get("textDocument")
                .and_then(|doc| doc.get("uri"))
                .and_then(Value::as_str)
            else {
                continue;
            };
            let Some(edits_value) = change.get("edits") else {
                continue;
            };
            let Some(edits) = parse_text_edits(edits_value) else {
                continue;
            };
            files
                .entry(uri.to_string())
                .or_insert_with(|| LspFileEdit {
                    uri: uri.to_string(),
                    path: path_from_file_uri(uri),
                    edits: Vec::new(),
                })
                .edits
                .extend(edits);
        }
    }

    let files: Vec<_> = files
        .into_values()
        .filter(|file| !file.edits.is_empty())
        .collect();
    (!files.is_empty()).then_some(LspWorkspaceEdit { files })
}

pub(super) fn parse_text_edits(value: &Value) -> Option<Vec<LspTextEdit>> {
    let edits = value.as_array()?;
    let parsed: Vec<_> = edits
        .iter()
        .filter_map(|edit| {
            Some(LspTextEdit {
                range: parse_range(edit.get("range")?)?,
                new_text: edit.get("newText")?.as_str()?.to_string(),
            })
        })
        .collect();
    Some(parsed)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_signature_help_with_parameter_ranges() {
        let help = parse_signature_help(Some(&json!({
            "signatures": [
                {
                    "label": "helper(name: string, count: number): string",
                    "documentation": {
                        "kind": "markdown",
                        "value": "Builds a label."
                    },
                    "parameters": [
                        {
                            "label": [7, 19],
                            "documentation": "Display name."
                        },
                        {
                            "label": "count: number",
                            "documentation": {
                                "kind": "markdown",
                                "value": "Repeat count."
                            }
                        }
                    ]
                }
            ],
            "activeSignature": 0,
            "activeParameter": 1
        })))
        .expect("signature help should parse");

        assert_eq!(
            help,
            LspSignatureHelp {
                signatures: vec![LspSignatureInformation {
                    label: "helper(name: string, count: number): string".to_string(),
                    documentation: Some("Builds a label.".to_string()),
                    parameters: vec![
                        LspParameterInformation {
                            label: "name: string".to_string(),
                            documentation: Some("Display name.".to_string())
                        },
                        LspParameterInformation {
                            label: "count: number".to_string(),
                            documentation: Some("Repeat count.".to_string())
                        }
                    ]
                }],
                active_signature: Some(0),
                active_parameter: Some(1)
            }
        );
    }

    #[test]
    fn parses_workspace_edit_changes_for_rename_preview() {
        let edit = parse_workspace_edit(Some(&json!({
            "changes": {
                "file:///repo/src/app.ts": [
                    {
                        "range": {
                            "start": { "line": 0, "character": 6 },
                            "end": { "line": 0, "character": 12 }
                        },
                        "newText": "renamed"
                    }
                ]
            }
        })))
        .unwrap();

        assert_eq!(
            edit,
            LspWorkspaceEdit {
                files: vec![LspFileEdit {
                    uri: "file:///repo/src/app.ts".to_string(),
                    path: "/repo/src/app.ts".to_string(),
                    edits: vec![LspTextEdit {
                        range: LspRange {
                            start: LspPosition {
                                line: 0,
                                character: 6
                            },
                            end: LspPosition {
                                line: 0,
                                character: 12
                            }
                        },
                        new_text: "renamed".to_string()
                    }]
                }]
            }
        );
    }

    #[test]
    fn parses_code_actions_with_workspace_edits() {
        let actions = parse_code_actions(Some(&json!([
            {
                "title": "Add missing import",
                "kind": "quickfix",
                "edit": {
                    "changes": {
                        "file:///repo/src/app.ts": [
                            {
                                "range": {
                                    "start": { "line": 0, "character": 0 },
                                    "end": { "line": 0, "character": 0 }
                                },
                                "newText": "import { helper } from './helper';\n"
                            }
                        ]
                    }
                }
            }
        ])));

        assert_eq!(
            actions,
            vec![LspCodeAction {
                title: "Add missing import".to_string(),
                kind: Some("quickfix".to_string()),
                edit: Some(LspWorkspaceEdit {
                    files: vec![LspFileEdit {
                        uri: "file:///repo/src/app.ts".to_string(),
                        path: "/repo/src/app.ts".to_string(),
                        edits: vec![LspTextEdit {
                            range: LspRange {
                                start: LspPosition {
                                    line: 0,
                                    character: 0
                                },
                                end: LspPosition {
                                    line: 0,
                                    character: 0
                                }
                            },
                            new_text: "import { helper } from './helper';\n".to_string()
                        }]
                    }]
                }),
                command: None
            }]
        );
    }

    #[test]
    fn parses_code_actions_with_commands() {
        let actions = parse_code_actions(Some(&json!([
            {
                "title": "Organize imports",
                "kind": "source.organizeImports",
                "command": {
                    "title": "Organize Imports",
                    "command": "_typescript.organizeImports",
                    "arguments": [
                        "file:///repo/src/app.ts",
                        { "skipDestructiveCodeActions": true }
                    ]
                }
            }
        ])));

        assert_eq!(
            actions,
            vec![LspCodeAction {
                title: "Organize imports".to_string(),
                kind: Some("source.organizeImports".to_string()),
                edit: None,
                command: Some(LspCommand {
                    title: Some("Organize Imports".to_string()),
                    command: "_typescript.organizeImports".to_string(),
                    arguments: vec![
                        json!("file:///repo/src/app.ts"),
                        json!({ "skipDestructiveCodeActions": true })
                    ]
                })
            }]
        );
    }

    #[test]
    fn parses_inlay_hints_with_string_and_label_parts() {
        let hints = parse_inlay_hints(Some(&json!([
            {
                "position": { "line": 2, "character": 18 },
                "label": ": string",
                "kind": 1,
                "tooltip": { "kind": "markdown", "value": "Return type" },
                "paddingLeft": true,
                "paddingRight": false
            },
            {
                "position": { "line": 4, "character": 9 },
                "label": [{ "value": "name" }, { "value": ": " }],
                "paddingRight": true
            },
            {
                "position": { "line": 5, "character": 0 },
                "label": []
            }
        ])));

        assert_eq!(
            hints,
            vec![
                LspInlayHint {
                    label: ": string".to_string(),
                    position: LspPosition {
                        line: 2,
                        character: 18
                    },
                    kind: Some(1),
                    tooltip: Some("Return type".to_string()),
                    padding_left: true,
                    padding_right: false,
                },
                LspInlayHint {
                    label: "name: ".to_string(),
                    position: LspPosition {
                        line: 4,
                        character: 9
                    },
                    kind: None,
                    tooltip: None,
                    padding_left: false,
                    padding_right: true,
                }
            ]
        );
    }
    #[test]
    fn parses_document_symbols_and_flattens_children() {
        let symbols = parse_document_symbols(
            Some(&json!([
                {
                    "name": "App",
                    "kind": 12,
                    "range": {
                        "start": { "line": 1, "character": 0 },
                        "end": { "line": 8, "character": 1 }
                    },
                    "selectionRange": {
                        "start": { "line": 1, "character": 9 },
                        "end": { "line": 1, "character": 12 }
                    },
                    "children": [
                        {
                            "name": "helper",
                            "kind": 12,
                            "detail": "function",
                            "range": {
                                "start": { "line": 3, "character": 2 },
                                "end": { "line": 5, "character": 3 }
                            },
                            "selectionRange": {
                                "start": { "line": 3, "character": 11 },
                                "end": { "line": 3, "character": 17 }
                            }
                        }
                    ]
                }
            ])),
            "file:///repo/src/App.tsx",
        );

        assert_eq!(symbols.len(), 2);
        assert_eq!(symbols[0].name, "App");
        assert_eq!(symbols[0].path, "/repo/src/App.tsx");
        assert_eq!(symbols[1].name, "helper");
        assert_eq!(symbols[1].container_name, Some("App".to_string()));
        assert_eq!(
            symbols[1].selection_range,
            LspRange {
                start: LspPosition {
                    line: 3,
                    character: 11
                },
                end: LspPosition {
                    line: 3,
                    character: 17
                }
            }
        );
    }

    #[test]
    fn parses_workspace_symbol_information() {
        let symbols = parse_workspace_symbols(Some(&json!([
            {
                "name": "createService",
                "kind": 12,
                "containerName": "services",
                "location": {
                    "uri": "file:///repo/src/services.ts",
                    "range": {
                        "start": { "line": 4, "character": 7 },
                        "end": { "line": 4, "character": 20 }
                    }
                }
            }
        ])));

        assert_eq!(
            symbols,
            vec![LspSymbol {
                name: "createService".to_string(),
                kind: 12,
                detail: None,
                container_name: Some("services".to_string()),
                uri: "file:///repo/src/services.ts".to_string(),
                path: "/repo/src/services.ts".to_string(),
                range: LspRange {
                    start: LspPosition {
                        line: 4,
                        character: 7
                    },
                    end: LspPosition {
                        line: 4,
                        character: 20
                    }
                },
                selection_range: LspRange {
                    start: LspPosition {
                        line: 4,
                        character: 7
                    },
                    end: LspPosition {
                        line: 4,
                        character: 20
                    }
                }
            }]
        );
    }
}
