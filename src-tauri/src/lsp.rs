use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::Command;
use tokio::time::{timeout, Duration};

const LSP_TIMEOUT: Duration = Duration::from_secs(8);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LspServerCommand {
    pub program: String,
    pub args: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LspServerStatus {
    pub supported: bool,
    pub available: bool,
    pub language_id: Option<String>,
    pub command: Option<LspServerCommand>,
    pub install_hint: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LspPosition {
    pub line: u32,
    pub character: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LspRange {
    pub start: LspPosition,
    pub end: LspPosition,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LspLocation {
    pub uri: String,
    pub path: String,
    pub range: LspRange,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LspHover {
    pub contents: String,
    pub range: Option<LspRange>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LspCompletionItem {
    pub label: String,
    pub detail: Option<String>,
    pub documentation: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LspDocumentRequest {
    pub project_path: String,
    pub file_path: String,
    pub content: String,
    pub line: u32,
    pub character: u32,
}

pub fn language_id_for_path(path: impl AsRef<Path>) -> Option<&'static str> {
    let ext = path.as_ref().extension()?.to_str()?.to_ascii_lowercase();
    match ext.as_str() {
        "ts" => Some("typescript"),
        "tsx" => Some("typescriptreact"),
        "js" | "mjs" | "cjs" => Some("javascript"),
        "jsx" => Some("javascriptreact"),
        _ => None,
    }
}

pub fn default_server_command(language_id: &str) -> Option<LspServerCommand> {
    match language_id {
        "typescript" | "typescriptreact" | "javascript" | "javascriptreact" => {
            Some(LspServerCommand {
                program: "typescript-language-server".to_string(),
                args: vec!["--stdio".to_string()],
            })
        }
        _ => None,
    }
}

pub fn validate_document_path(project_path: &Path, file_path: &Path) -> Result<PathBuf, String> {
    let project_root = project_path
        .canonicalize()
        .map_err(|err| format!("failed to resolve project root: {err}"))?;
    let document_path = file_path
        .canonicalize()
        .map_err(|err| format!("failed to resolve document path: {err}"))?;
    if !document_path.starts_with(&project_root) {
        return Err("document path is outside project root".to_string());
    }
    if language_id_for_path(&document_path).is_none() {
        return Err("language server is not supported for this file type".to_string());
    }
    Ok(document_path)
}

#[tauri::command]
pub fn lsp_server_status(
    project_path: String,
    file_path: String,
) -> Result<LspServerStatus, String> {
    let document_path = validate_document_path(Path::new(&project_path), Path::new(&file_path))?;
    let language_id = language_id_for_path(&document_path).map(str::to_string);
    let command = language_id.as_deref().and_then(default_server_command);
    let available = command
        .as_ref()
        .map(|command| command_available(&command.program))
        .unwrap_or(false);

    Ok(LspServerStatus {
        supported: language_id.is_some() && command.is_some(),
        available,
        language_id,
        command,
        install_hint: if available {
            None
        } else {
            Some("pnpm add -D typescript-language-server typescript".to_string())
        },
    })
}

#[tauri::command]
pub async fn lsp_hover(request: LspDocumentRequest) -> Result<Option<LspHover>, String> {
    let result = request_lsp_feature(&request, "textDocument/hover").await?;
    Ok(parse_hover(Some(&result)))
}

#[tauri::command]
pub async fn lsp_definition(request: LspDocumentRequest) -> Result<Vec<LspLocation>, String> {
    let result = request_lsp_feature(&request, "textDocument/definition").await?;
    Ok(parse_locations(Some(&result)))
}

#[tauri::command]
pub async fn lsp_completion(request: LspDocumentRequest) -> Result<Vec<LspCompletionItem>, String> {
    let result = request_lsp_feature(&request, "textDocument/completion").await?;
    Ok(parse_completion_items(Some(&result)))
}

async fn request_lsp_feature(request: &LspDocumentRequest, method: &str) -> Result<Value, String> {
    let project_root = Path::new(&request.project_path)
        .canonicalize()
        .map_err(|err| format!("failed to resolve project root: {err}"))?;
    let document_path = validate_document_path(&project_root, Path::new(&request.file_path))?;
    let language_id = language_id_for_path(&document_path)
        .ok_or_else(|| "language server is not supported for this file type".to_string())?;
    let server = default_server_command(language_id)
        .ok_or_else(|| "language server is not configured for this file type".to_string())?;
    run_lsp_request(
        &project_root,
        &document_path,
        language_id,
        &server,
        request,
        method,
    )
    .await
}

async fn run_lsp_request(
    project_root: &Path,
    document_path: &Path,
    language_id: &str,
    server: &LspServerCommand,
    request: &LspDocumentRequest,
    method: &str,
) -> Result<Value, String> {
    let run = async {
        let mut command = Command::new(&server.program);
        command
            .args(&server.args)
            .current_dir(project_root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        let mut child = command.spawn().map_err(|err| {
            if err.kind() == std::io::ErrorKind::NotFound {
                "typescript-language-server is not installed. Run: pnpm add -D typescript-language-server typescript".to_string()
            } else {
                format!("failed to start language server: {err}")
            }
        })?;

        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| "failed to open language server stdin".to_string())?;
        let mut stdout = child
            .stdout
            .take()
            .ok_or_else(|| "failed to open language server stdout".to_string())?;

        let document_uri = file_uri(document_path);
        write_message(
            &mut stdin,
            &json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "initialize",
                "params": {
                    "processId": std::process::id(),
                    "rootUri": file_uri(project_root),
                    "capabilities": {},
                    "workspaceFolders": [{
                        "uri": file_uri(project_root),
                        "name": project_root.file_name().and_then(|name| name.to_str()).unwrap_or("workspace")
                    }]
                }
            }),
        )
        .await?;
        read_response_with_id(&mut stdout, 1).await?;

        write_message(
            &mut stdin,
            &json!({
                "jsonrpc": "2.0",
                "method": "initialized",
                "params": {}
            }),
        )
        .await?;

        write_message(
            &mut stdin,
            &json!({
                "jsonrpc": "2.0",
                "method": "textDocument/didOpen",
                "params": {
                    "textDocument": {
                        "uri": document_uri,
                        "languageId": language_id,
                        "version": 1,
                        "text": request.content
                    }
                }
            }),
        )
        .await?;

        write_message(
            &mut stdin,
            &json!({
                "jsonrpc": "2.0",
                "id": 2,
                "method": method,
                "params": {
                    "textDocument": { "uri": document_uri },
                    "position": {
                        "line": request.line,
                        "character": request.character
                    }
                }
            }),
        )
        .await?;

        let response = read_response_with_id(&mut stdout, 2).await?;
        let _ = write_message(
            &mut stdin,
            &json!({
                "jsonrpc": "2.0",
                "id": 3,
                "method": "shutdown",
                "params": null
            }),
        )
        .await;
        let _ = write_message(
            &mut stdin,
            &json!({
                "jsonrpc": "2.0",
                "method": "exit"
            }),
        )
        .await;
        let _ = child.kill().await;

        if let Some(error) = response.get("error") {
            return Err(format!("language server request failed: {error}"));
        }
        Ok(response.get("result").cloned().unwrap_or(Value::Null))
    };

    timeout(LSP_TIMEOUT, run)
        .await
        .map_err(|_| "language server request timed out".to_string())?
}

async fn write_message<W: AsyncWriteExt + Unpin>(
    writer: &mut W,
    value: &Value,
) -> Result<(), String> {
    let body = serde_json::to_vec(value).map_err(|err| err.to_string())?;
    let header = format!("Content-Length: {}\r\n\r\n", body.len());
    writer
        .write_all(header.as_bytes())
        .await
        .map_err(|err| err.to_string())?;
    writer
        .write_all(&body)
        .await
        .map_err(|err| err.to_string())?;
    writer.flush().await.map_err(|err| err.to_string())
}

async fn read_response_with_id<R: AsyncReadExt + Unpin>(
    reader: &mut R,
    id: i64,
) -> Result<Value, String> {
    loop {
        let value = read_message(reader).await?;
        if value.get("id").and_then(Value::as_i64) == Some(id) {
            return Ok(value);
        }
    }
}

async fn read_message<R: AsyncReadExt + Unpin>(reader: &mut R) -> Result<Value, String> {
    let mut header = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        reader
            .read_exact(&mut byte)
            .await
            .map_err(|err| format!("failed to read language server header: {err}"))?;
        header.push(byte[0]);
        if header.ends_with(b"\r\n\r\n") {
            break;
        }
        if header.len() > 8192 {
            return Err("language server header is too large".to_string());
        }
    }

    let header = String::from_utf8(header).map_err(|err| err.to_string())?;
    let content_length = header
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            if name.eq_ignore_ascii_case("content-length") {
                value.trim().parse::<usize>().ok()
            } else {
                None
            }
        })
        .ok_or_else(|| "language server response missing Content-Length".to_string())?;
    let mut body = vec![0u8; content_length];
    reader
        .read_exact(&mut body)
        .await
        .map_err(|err| format!("failed to read language server body: {err}"))?;
    serde_json::from_slice(&body).map_err(|err| err.to_string())
}

fn command_available(program: &str) -> bool {
    std::process::Command::new(program)
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn file_uri(path: &Path) -> String {
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

fn path_from_file_uri(uri: &str) -> String {
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

fn parse_hover(value: Option<&Value>) -> Option<LspHover> {
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

fn parse_locations(value: Option<&Value>) -> Vec<LspLocation> {
    match value {
        Some(Value::Array(items)) => items.iter().filter_map(parse_location).collect(),
        Some(value) => parse_location(value).into_iter().collect(),
        None => Vec::new(),
    }
}

fn parse_location(value: &Value) -> Option<LspLocation> {
    let uri = value.get("uri")?.as_str()?.to_string();
    let range = parse_range(value.get("range")?)?;
    Some(LspLocation {
        path: path_from_file_uri(&uri),
        uri,
        range,
    })
}

fn parse_range(value: &Value) -> Option<LspRange> {
    Some(LspRange {
        start: parse_position(value.get("start")?)?,
        end: parse_position(value.get("end")?)?,
    })
}

fn parse_position(value: &Value) -> Option<LspPosition> {
    Some(LspPosition {
        line: value.get("line")?.as_u64()? as u32,
        character: value.get("character")?.as_u64()? as u32,
    })
}

fn parse_completion_items(value: Option<&Value>) -> Vec<LspCompletionItem> {
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

fn markdown_text(value: &Value) -> Option<String> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_project(name: &str) -> std::path::PathBuf {
        let root =
            std::env::temp_dir().join(format!("aeroric-lsp-test-{}-{}", name, std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn detects_ts_js_language_ids() {
        assert_eq!(language_id_for_path("src/App.tsx"), Some("typescriptreact"));
        assert_eq!(language_id_for_path("src/App.ts"), Some("typescript"));
        assert_eq!(language_id_for_path("src/App.jsx"), Some("javascriptreact"));
        assert_eq!(language_id_for_path("src/App.js"), Some("javascript"));
        assert_eq!(language_id_for_path("src/App.rs"), None);
    }

    #[test]
    fn builds_default_typescript_language_server_command() {
        let command = default_server_command("typescript").unwrap();
        assert_eq!(command.program, "typescript-language-server");
        assert_eq!(command.args, vec!["--stdio"]);
    }

    #[test]
    fn rejects_document_outside_project_root() {
        let project = temp_project("outside");
        let outside =
            std::env::temp_dir().join(format!("aeroric-lsp-outside-{}.ts", std::process::id()));
        fs::write(&outside, "const value = 1;\n").unwrap();

        let err = validate_document_path(&project, &outside).unwrap_err();

        assert!(err.contains("outside project root"));
    }
}
