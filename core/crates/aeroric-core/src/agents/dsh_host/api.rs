//! `DshApiClient` —— DSH Web 的 HTTP RPC transport 与方法群。
//!
//! P3-05 方法群片(补记二百六十五 §3)自宿主 verbatim 上收:三块(`api_client.rs`
//! 的 struct/`new`/`call`/`bounded_utf8_prefix`、`dsh_webui.rs` 的两个
//! `impl DshApiClient` 块)合并进本文件,方法体一行没改。机械项只有四处:
//! `use super::*` 拆成显式 import、`crate::dsh_protocol::decode_result_value`
//! 指回同 crate 的 [`client`]、dto 类型走 [`super::dto`]、块内私有辅助
//! (`default_true`/`supported_dsh_reasoning_effort`/`DshSettings*`)按新地盘取
//! `pub(crate)` 可见性。orphan 规则决定 impl 与 struct 不可跨 crate 分片,故整体迁移。

use serde_json::{json, Value};
use std::collections::HashMap;
use tokio::time::Duration;
use uuid::Uuid;

use super::attachments::{normalize_prompt_mode, parse_prompt_image};
use super::client::decode_result_value;
use super::dto::{
    DshCredentialView, DshDirectoryListing, DshDiscoveredModel, DshGlobalModels, DshGoal,
    DshHostInfo, DshModelCatalogFailure, DshModelGroup, DshModelSelection, DshPresetInfo,
    DshPresetList, DshPresetReadResult, DshProviderInfo, DshSessionHistory, DshSessionModels,
    DshSessionSummary, DshSettingsDescription, DshSkillEntry, DshSubagentSummary, DshWorkspace,
    DshWorkspaceList,
};
use super::remote_mux::DshRemoteMux;

/// `commands/execute` 的 wire 参数。
///
/// gateway 按 remote 的**参数名**逐一精确匹配 wire 字段(assertExactArguments):
/// 多一个是 `unexpected`,少一个是 `missing`,只有声明了 optional 的参数可省略。
/// 上游签名是 `execute(agent, line, images, signal)`,`images` 是必填
/// `readonly EncodedImageAttachment[]` —— 不带附件也必须显式给空数组,否则整个
/// 请求被拒:`args fields do not match the descriptor: missing "images"`。
/// 这条路径同时承载启动时下发的 `/permission <preset>`,所以漏掉它的表现是
/// "一开终端就报错"。
fn execute_command_args(session_id: &str, line: &str) -> Value {
    json!({ "agentId": session_id, "line": line, "images": [] })
}

pub const DSH_HTTP_ERROR_SNIPPET_BYTES: usize = 200;

#[derive(Clone)]
pub struct DshApiClient {
    pub(crate) client: reqwest::Client,
    pub base_url: String,
}

pub fn bounded_utf8_prefix(value: &str, max_bytes: usize) -> &str {
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
    pub fn new(base_url: String) -> Result<Self, String> {
        let client = reqwest::Client::builder()
            .no_proxy()
            .build()
            .map_err(|error| format!("Failed to create DSH API client: {error}"))?;
        Ok(Self { client, base_url })
    }

    pub async fn call(&self, method: &str, payload: Value) -> Result<Value, String> {
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
        decode_result_value(body, method)
    }
}

// ── 方法群(自宿主 `dsh_webui.rs` 两个 impl 块合并,顺序与原文一致) ──

impl DshApiClient {
    pub async fn create_session(
        &self,
        cwd: &str,
        session_id: Option<&str>,
        workspace_id: Option<&str>,
        agent_preset: Option<&str>,
    ) -> Result<(String, Option<String>), String> {
        let mut payload = json!({ "cwd": cwd });
        if let Some(session_id) = session_id.filter(|value| !value.trim().is_empty()) {
            payload["sessionId"] = Value::String(session_id.to_string());
        }
        if let Some(workspace_id) = workspace_id.filter(|value| !value.trim().is_empty()) {
            payload["workspaceId"] = Value::String(workspace_id.to_string());
        }
        if let Some(agent_preset) = agent_preset.filter(|value| !value.trim().is_empty()) {
            payload["agentPreset"] = Value::String(agent_preset.to_string());
        }
        let value = self.remote_request("session/create", payload).await?;
        let id = value
            .get("sessionId")
            .and_then(Value::as_str)
            .ok_or_else(|| "DSH session.create returned no sessionId".to_string())?
            .to_string();
        Ok((
            id,
            value
                .get("agentPreset")
                .and_then(Value::as_str)
                .map(str::to_string),
        ))
    }

    /// Session model selector data.
    ///
    /// 0.1.5 replaced the session-scoped `session.models` with the
    /// deployment-wide `session/modelCatalog`, which takes no wire arguments
    /// and renamed two fields: `default` (was `current`) and
    /// `routableProviders: string[]` (was `routable: bool`).  The session's own
    /// live selection moved to the `modelSelection` projection, which only
    /// reaches clients through the `session/follow` baseline frame -- there is
    /// no unary read for it.  That is fine for the one caller: it reads
    /// `current` purely as the fallback when the launch did not name a model,
    /// and at session start the deployment default *is* the effective
    /// selection (the session has not selected anything yet).
    pub async fn models(&self, _session_id: &str) -> Result<DshSessionModels, String> {
        let catalog = self.remote_unit("session/modelCatalog").await?;
        let default = catalog
            .get("default")
            .cloned()
            .ok_or_else(|| "DSH session/modelCatalog returned no default selection".to_string())?;
        let routable = catalog
            .get("routableProviders")
            .and_then(Value::as_array)
            .is_some_and(|providers| !providers.is_empty());
        serde_json::from_value(json!({
            "current": default,
            "routable": routable,
            "groups": catalog.get("groups").cloned().unwrap_or_else(|| Value::Array(Vec::new())),
            "failures": catalog.get("failures").cloned().unwrap_or_else(|| Value::Array(Vec::new())),
        }))
        .map_err(|error| format!("DSH session/modelCatalog payload was invalid: {error}"))
    }

    pub async fn select_model(
        &self,
        session_id: &str,
        selection: &DshModelSelection,
    ) -> Result<(), String> {
        let mut payload = json!({
            "sessionId": session_id,
            "provider": selection.provider,
            "model": selection.model,
        });
        if let Some(effort) = &selection.reasoning_effort {
            payload["reasoningEffort"] = Value::String(effort.clone());
        }
        self.remote_request("session/selectModel", payload)
            .await
            .map(|_| ())
    }

    pub async fn cancel(&self, session_id: &str) -> Result<(), String> {
        self.remote_request("session/cancel", json!({ "sessionId": session_id }))
            .await
            .map(|_| ())
    }

    /// Invoke a generated Typert remote.  The official Web client carries
    /// these through the same JSON RPC gateway (`payload.args`) even though
    /// they are not part of the static apiproxy domain map.
    pub async fn remote_call(&self, method: &str, args: Value) -> Result<Value, String> {
        self.call(method, json!({ "args": args })).await
    }

    /// Call a Remote whose sole wire parameter is named `request` -- the shape
    /// most Typert endpoints use (`session/page`, `workspace/create`, ...).
    pub async fn remote_request(&self, method: &str, request: Value) -> Result<Value, String> {
        self.remote_call(method, json!({ "request": request }))
            .await
    }

    /// Call a Remote that declares no wire parameters.  The gateway's
    /// `assertExactArguments` rejects a missing argument object, so the empty
    /// object is required rather than optional.
    pub async fn remote_unit(&self, method: &str) -> Result<Value, String> {
        self.remote_call(method, json!({})).await
    }

    pub async fn list_commands(&self, session_id: &str) -> Result<Value, String> {
        self.remote_call("commands/list", json!({ "agentId": session_id }))
            .await
    }

    pub async fn execute_command(&self, session_id: &str, line: &str) -> Result<Value, String> {
        self.remote_call("commands/execute", execute_command_args(session_id, line))
            .await
    }

    pub async fn list_message_feedback(&self, session_id: &str) -> Result<Value, String> {
        self.remote_call(
            "messageFeedback/list",
            json!({ "request": { "sessionId": session_id } }),
        )
        .await
    }

    pub async fn put_message_feedback(
        &self,
        session_id: &str,
        message_id: &str,
        rating: &str,
        note: Option<&str>,
        if_version: Option<&str>,
    ) -> Result<Value, String> {
        let mut request = json!({
            "sessionId": session_id,
            "messageId": message_id,
            "rating": rating,
            "ifVersion": if_version,
        });
        if let Some(note) = note {
            request["note"] = Value::String(note.to_string());
        }
        self.remote_call("messageFeedback/put", json!({ "request": request }))
            .await
    }

    pub async fn delete_message_feedback(
        &self,
        session_id: &str,
        message_id: &str,
        if_version: &str,
    ) -> Result<Value, String> {
        self.remote_call(
            "messageFeedback/delete",
            json!({
                "request": {
                    "sessionId": session_id,
                    "messageId": message_id,
                    "ifVersion": if_version,
                },
            }),
        )
        .await
    }

    pub async fn prompt(
        &self,
        session_id: &str,
        prompt: &str,
        mode: &str,
        images: Option<Vec<String>>,
        client_time_zone: Option<&str>,
    ) -> Result<Value, String> {
        let mut content = vec![json!({ "type": "text", "text": prompt })];
        for image in images.unwrap_or_default() {
            let (media_type, data) = parse_prompt_image(&image)?;
            content.push(json!({
                "type": "image",
                "mediaType": media_type,
                "data": data,
            }));
        }
        let mut request = json!({
            // 0.1.5 requires a client-minted request id on the exact accepted
            // user message; the official Web client mints a plain randomUUID.
            "requestId": Uuid::new_v4().to_string(),
            "sessionId": session_id,
            "mode": normalize_prompt_mode(mode)?,
            "content": content,
        });
        if let Some(zone) = client_time_zone.filter(|value| !value.trim().is_empty()) {
            request["clientTimeZone"] = Value::String(zone.to_string());
        }
        let value = self.remote_request("session/prompt", request).await?;
        if value.get("accepted").and_then(Value::as_bool) != Some(true) {
            return Err("DSH session/prompt did not acknowledge the prompt".to_string());
        }
        Ok(value)
    }

    pub async fn list_presets(&self) -> Result<Vec<DshPresetInfo>, String> {
        Ok(self.list_preset_details().await?.presets)
    }

    pub async fn list_preset_details(&self) -> Result<DshPresetList, String> {
        let value = self.remote_unit("agentPresets/list").await?;
        let presets = serde_json::from_value(
            value
                .get("presets")
                .cloned()
                .unwrap_or(Value::Array(Vec::new())),
        )
        .map_err(|error| format!("DSH agentPresets/list payload was invalid: {error}"))?;
        // 0.1.5 split the roster from the opener capability: `AgentPresetRoster`
        // no longer carries `hasDocument`, and whether a preset directory can be
        // opened natively is now the settings controller's own capability.
        let has_document = self
            .remote_unit("settings/canOpenAgentPresetDirectory")
            .await?
            .as_bool()
            .unwrap_or(false);
        Ok(DshPresetList {
            presets,
            authorable: value
                .get("authorable")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            has_document,
        })
    }

    pub(crate) async fn describe_settings(&self) -> Result<DshSettingsDescription, String> {
        serde_json::from_value(self.remote_unit("settings/describe").await?)
            .map_err(|error| format!("DSH settings.describe payload was invalid: {error}"))
    }

    pub async fn set_default_preset(&self, preset: &str) -> Result<(), String> {
        // The DSH Web API deliberately owns this setting. Read the namespace
        // first so an old/stale Web process produces an actionable diagnostic
        // instead of the settings picker collapsing into a generic failure.
        let description = self.describe_settings().await?;
        if !description.writable {
            return Err(
                "DSH settings are read-only; make the DSH settings document writable and retry"
                    .to_string(),
            );
        }
        let namespace = description
            .namespaces
            .iter()
            .find(|namespace| namespace.ns == "agent-presets")
            .ok_or_else(|| {
                "DSH Web does not expose the agent-presets settings namespace; restart DSH from the current DeepSeek Harness source".to_string()
            })?;
        self.call(
            "settings.update",
            json!({
                "ns": "agent-presets",
                "patch": { "default": preset },
                "expectedRevision": namespace.revision,
            }),
        )
        .await
        .map(|_| ())
    }
    // ── Session extended ──────────────────────────────────────────────────────

    pub async fn list_sessions(&self) -> Result<Vec<DshSessionSummary>, String> {
        let value = self.call("session.list", json!({})).await?;
        let items = value
            .get("items")
            .cloned()
            .unwrap_or(Value::Array(Vec::new()));
        serde_json::from_value(items)
            .map_err(|e| format!("DSH session.list payload was invalid: {e}"))
    }

    pub async fn session_history(
        &self,
        session_id: &str,
        before_seq: Option<u64>,
        max_messages: Option<u32>,
    ) -> Result<DshSessionHistory, String> {
        let mut payload = json!({ "sessionId": session_id });
        if let Some(seq) = before_seq {
            payload["beforeSeq"] = Value::Number(seq.into());
        }
        if let Some(max) = max_messages {
            payload["maxMessages"] = Value::Number(max.into());
        }
        let value = self.call("session.history", payload).await?;
        let events = value
            .get("events")
            .cloned()
            .unwrap_or(Value::Array(Vec::new()));
        let events: Vec<Value> = serde_json::from_value(events)
            .map_err(|e| format!("DSH session.history events was invalid: {e}"))?;
        let has_more = value
            .get("hasMore")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        Ok(DshSessionHistory {
            events,
            has_more,
            projections: value.get("projections").cloned(),
        })
    }

    pub async fn rename_session(
        &self,
        session_id: &str,
        title: &str,
    ) -> Result<(String, u64), String> {
        let value = self
            .call(
                "session.rename",
                json!({ "sessionId": session_id, "title": title }),
            )
            .await?;
        let resolved_title = value
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or(title)
            .to_string();
        let seq = value.get("seq").and_then(Value::as_u64).unwrap_or(0);
        Ok((resolved_title, seq))
    }

    pub async fn fork_session(
        &self,
        session_id: &str,
        at_seq: Option<u64>,
    ) -> Result<String, String> {
        let mut payload = json!({ "sessionId": session_id });
        if let Some(seq) = at_seq {
            payload["atSeq"] = Value::Number(seq.into());
        }
        let value = self.call("session.fork", payload).await?;
        value
            .get("sessionId")
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| "DSH session.fork returned no sessionId".to_string())
    }

    pub async fn search_sessions(&self, query: &str) -> Result<(Vec<Value>, bool), String> {
        let value = self
            .call("session.search", json!({ "query": query }))
            .await?;
        let items = value
            .get("items")
            .cloned()
            .unwrap_or(Value::Array(Vec::new()));
        let items: Vec<Value> = serde_json::from_value(items)
            .map_err(|e| format!("DSH session.search items was invalid: {e}"))?;
        let has_more = value
            .get("hasMore")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        Ok((items, has_more))
    }

    pub async fn update_session_queue(
        &self,
        session_id: &str,
        item_id: &str,
        action: Value,
    ) -> Result<(), String> {
        self.call(
            "session.updateQueue",
            json!({
                "sessionId": session_id,
                "itemId": item_id,
                "action": action,
            }),
        )
        .await
        .map(|_| ())
    }

    // ── Workspace ─────────────────────────────────────────────────────────────

    /// Workspace roster.
    ///
    /// 0.1.5 has no unary equivalent: the roster is the `{type:'baseline'}`
    /// opening frame of the `workspace/follow` stream, and the gateway rejects
    /// opening a stream remote over unary POST outright. We open one throwaway
    /// mux generation, take the baseline, then cancel the stream — the roster
    /// is a point-in-time read, and live invalidation rides the long-lived
    /// host subscription (`consume_host_events`).
    pub async fn list_workspaces(&self) -> Result<DshWorkspaceList, String> {
        let mux = DshRemoteMux::connect(&self.base_url).await?;
        let result = {
            let mut stream = mux.open("workspace/follow", json!({}))?;
            match stream.next().await {
                Some(Ok(item)) => {
                    let value = item.get("value").cloned().ok_or_else(|| {
                        "DSH workspace/follow baseline carried no value".to_string()
                    })?;
                    serde_json::from_value(value)
                        .map_err(|e| format!("DSH workspace baseline was invalid: {e}"))
                }
                Some(Err(error)) => Err(error),
                None => Err("DSH workspace/follow stream ended before its baseline".to_string()),
            }
        };
        drop(mux);
        result
    }

    pub async fn create_workspace(&self, path: &str) -> Result<(DshWorkspace, bool), String> {
        let value = self
            .remote_request("workspace/create", json!({ "path": path }))
            .await?;
        let workspace: DshWorkspace = serde_json::from_value(
            value
                .get("workspace")
                .cloned()
                .ok_or_else(|| "DSH workspace.create returned no workspace".to_string())?,
        )
        .map_err(|e| format!("DSH workspace.create payload was invalid: {e}"))?;
        let created = value
            .get("created")
            .and_then(Value::as_bool)
            .unwrap_or(true);
        Ok((workspace, created))
    }

    pub async fn rename_workspace(
        &self,
        workspace_id: &str,
        title: &str,
    ) -> Result<DshWorkspace, String> {
        let value = self
            .remote_request(
                "workspace/rename",
                json!({ "workspaceId": workspace_id, "title": title }),
            )
            .await?;
        serde_json::from_value(
            value
                .get("workspace")
                .cloned()
                .ok_or_else(|| "DSH workspace.rename returned no workspace".to_string())?,
        )
        .map_err(|e| format!("DSH workspace.rename payload was invalid: {e}"))
    }

    pub async fn delete_workspace(&self, workspace_id: &str) -> Result<(), String> {
        self.remote_request("workspace/delete", json!({ "workspaceId": workspace_id }))
            .await
            .map(|_| ())
    }

    pub async fn workspace_insert_before(
        &self,
        workspace_id: &str,
        before_workspace_id: Option<&str>,
    ) -> Result<Vec<String>, String> {
        let mut payload = json!({ "workspaceId": workspace_id });
        if let Some(before) = before_workspace_id {
            payload["beforeWorkspaceId"] = Value::String(before.to_string());
        }
        let value = self
            .remote_request("workspace/insertBefore", payload)
            .await?;
        let ids: Vec<String> = serde_json::from_value(
            value
                .get("workspaceIds")
                .cloned()
                .unwrap_or(Value::Array(Vec::new())),
        )
        .map_err(|e| format!("DSH workspace.insertBefore payload was invalid: {e}"))?;
        Ok(ids)
    }

    pub async fn workspace_insert_session_before(
        &self,
        workspace_id: &str,
        session_id: &str,
        before_session_id: Option<&str>,
    ) -> Result<DshWorkspace, String> {
        let mut payload = json!({ "workspaceId": workspace_id, "sessionId": session_id });
        if let Some(before) = before_session_id {
            payload["beforeSessionId"] = Value::String(before.to_string());
        }
        let value = self
            .remote_request("workspace/insertSessionBefore", payload)
            .await?;
        serde_json::from_value(
            value.get("workspace").cloned().ok_or_else(|| {
                "DSH workspace.insertSessionBefore returned no workspace".to_string()
            })?,
        )
        .map_err(|e| format!("DSH workspace.insertSessionBefore payload was invalid: {e}"))
    }

    pub async fn workspace_archive_session(&self, session_id: &str) -> Result<Vec<String>, String> {
        let value = self
            .remote_request(
                "workspace/archiveSession",
                json!({ "sessionId": session_id }),
            )
            .await?;
        let ids: Vec<String> = serde_json::from_value(
            value
                .get("archivedSessionIds")
                .cloned()
                .unwrap_or(Value::Array(Vec::new())),
        )
        .map_err(|e| format!("DSH workspace.archiveSession payload was invalid: {e}"))?;
        Ok(ids)
    }

    // ── Credentials ───────────────────────────────────────────────────────────

    pub async fn describe_credentials(
        &self,
        refs: &[String],
    ) -> Result<HashMap<String, DshCredentialView>, String> {
        let value = self
            .call("credentials.describe", json!({ "refs": refs }))
            .await?;
        let creds: HashMap<String, DshCredentialView> = serde_json::from_value(
            value
                .get("credentials")
                .cloned()
                .unwrap_or(Value::Object(serde_json::Map::new())),
        )
        .map_err(|e| format!("DSH credentials.describe payload was invalid: {e}"))?;
        Ok(creds)
    }

    pub async fn set_credential(&self, ref_: &str, value_str: &str) -> Result<(), String> {
        self.call(
            "credentials.set",
            json!({ "ref": ref_, "value": value_str }),
        )
        .await
        .map(|_| ())
    }

    pub async fn unset_credential(&self, ref_: &str) -> Result<(), String> {
        self.call("credentials.unset", json!({ "ref": ref_ }))
            .await
            .map(|_| ())
    }

    // ── LLM ───────────────────────────────────────────────────────────────────

    pub async fn list_llm_providers(&self) -> Result<Vec<DshProviderInfo>, String> {
        let value = self.call("llm.providers", json!({})).await?;
        let providers: Vec<DshProviderInfo> = serde_json::from_value(
            value
                .get("providers")
                .cloned()
                .unwrap_or(Value::Array(Vec::new())),
        )
        .map_err(|e| format!("DSH llm.providers payload was invalid: {e}"))?;
        Ok(providers)
    }

    pub async fn list_llm_models(&self) -> Result<DshGlobalModels, String> {
        let value = self.call("llm.models", json!({})).await?;
        let groups: Vec<DshModelGroup> = serde_json::from_value(
            value
                .get("groups")
                .cloned()
                .unwrap_or(Value::Array(Vec::new())),
        )
        .map_err(|e| format!("DSH llm.models groups was invalid: {e}"))?;
        let failures: Vec<DshModelCatalogFailure> = serde_json::from_value(
            value
                .get("failures")
                .cloned()
                .unwrap_or(Value::Array(Vec::new())),
        )
        .map_err(|e| format!("DSH llm.models failures was invalid: {e}"))?;
        Ok(DshGlobalModels { groups, failures })
    }

    pub async fn discover_llm_models(
        &self,
        settings_ns: &str,
        provider: Option<&str>,
        base_url: Option<&str>,
        api: Option<&str>,
        api_key: Option<&str>,
    ) -> Result<Vec<DshDiscoveredModel>, String> {
        let mut payload = json!({ "settingsNs": settings_ns });
        if let Some(p) = provider {
            payload["provider"] = Value::String(p.to_string());
        }
        if let Some(u) = base_url {
            payload["baseURL"] = Value::String(u.to_string());
        }
        if let Some(a) = api {
            payload["api"] = Value::String(a.to_string());
        }
        if let Some(k) = api_key {
            payload["apiKey"] = Value::String(k.to_string());
        }
        let value = self.call("llm.discoverModels", payload).await?;
        let models: Vec<DshDiscoveredModel> = serde_json::from_value(
            value
                .get("models")
                .cloned()
                .unwrap_or(Value::Array(Vec::new())),
        )
        .map_err(|e| format!("DSH llm.discoverModels payload was invalid: {e}"))?;
        Ok(models)
    }

    // ── Subagent ──────────────────────────────────────────────────────────────

    pub async fn list_subagents(
        &self,
        session_id: &str,
    ) -> Result<Vec<DshSubagentSummary>, String> {
        let value = self
            .call("subagent.list", json!({ "parentSessionId": session_id }))
            .await?;
        let entries = value
            .get("entries")
            .or_else(|| value.get("subagents"))
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let mut items = Vec::new();
        for entry in entries {
            if entry.get("kind").and_then(Value::as_str) == Some("diagnostic") {
                continue;
            }
            let child_id = entry
                .get("id")
                .or_else(|| entry.get("sessionId"))
                .and_then(Value::as_str);
            let Some(child_id) = child_id else { continue };
            items.push(DshSubagentSummary {
                session_id: child_id.to_string(),
                parent_session_id: session_id.to_string(),
                running: entry
                    .get("activity")
                    .and_then(Value::as_str)
                    .map(|activity| activity == "running")
                    .or_else(|| entry.get("running").and_then(Value::as_bool))
                    .unwrap_or(false),
                cwd: entry.get("cwd").and_then(Value::as_str).map(str::to_string),
                mode: entry
                    .get("mode")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                label: entry
                    .get("label")
                    .and_then(Value::as_str)
                    .map(str::to_string),
            });
        }
        Ok(items)
    }

    pub async fn subagent_history(
        &self,
        session_id: &str,
        parent_session_id: Option<&str>,
        mode: Option<&str>,
        before_seq: Option<u64>,
        max_messages: Option<u32>,
    ) -> Result<DshSessionHistory, String> {
        let mut payload = if let Some(parent) = parent_session_id {
            json!({
                "parentSessionId": parent,
                "childSessionId": session_id,
                "mode": mode.unwrap_or("continuable"),
            })
        } else {
            json!({ "sessionId": session_id })
        };
        if let Some(seq) = before_seq {
            payload["beforeSeq"] = Value::Number(seq.into());
        }
        if let Some(max) = max_messages {
            payload["maxMessages"] = Value::Number(max.into());
        }
        let value = self.call("subagent.history", payload).await?;
        let events: Vec<Value> = serde_json::from_value(
            value
                .get("events")
                .cloned()
                .unwrap_or(Value::Array(Vec::new())),
        )
        .map_err(|e| format!("DSH subagent.history events was invalid: {e}"))?;
        let has_more = value
            .get("hasMore")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        Ok(DshSessionHistory {
            events,
            has_more,
            projections: value.get("projections").cloned(),
        })
    }

    pub async fn subagent_prompt(
        &self,
        session_id: &str,
        parent_session_id: Option<&str>,
        mode: Option<&str>,
        content: &str,
        client_time_zone: Option<&str>,
    ) -> Result<Value, String> {
        let mut payload = if let Some(parent) = parent_session_id {
            json!({
                "parentSessionId": parent,
                "childSessionId": session_id,
                "mode": mode.unwrap_or("continuable"),
                "content": [{ "type": "text", "text": content }],
            })
        } else {
            json!({
                "sessionId": session_id,
                "content": [{ "type": "text", "text": content }],
            })
        };
        if let Some(zone) = client_time_zone.filter(|zone| !zone.trim().is_empty()) {
            payload["clientTimeZone"] = Value::String(zone.to_string());
        }
        self.call("subagent.prompt", payload).await
    }

    pub async fn interrupt_subagent(
        &self,
        session_id: &str,
        parent_session_id: Option<&str>,
        mode: Option<&str>,
    ) -> Result<Value, String> {
        let payload = if let Some(parent) = parent_session_id {
            json!({
                "parentSessionId": parent,
                "childSessionId": session_id,
                "mode": mode.unwrap_or("continuable"),
            })
        } else {
            json!({ "sessionId": session_id })
        };
        self.call("subagent.interrupt", payload).await
    }

    // ── Goals ─────────────────────────────────────────────────────────────────

    pub fn goal_result(
        value: &Value,
        title: Option<&str>,
        status: &str,
    ) -> Result<DshGoal, String> {
        let reference = value
            .get("ref")
            .ok_or_else(|| "DSH goal response returned no ref".to_string())?;
        let goal_id = reference
            .get("id")
            .and_then(Value::as_str)
            .ok_or_else(|| "DSH goal response ref has no id".to_string())?;
        let revision = reference
            .get("revision")
            .and_then(Value::as_u64)
            .ok_or_else(|| "DSH goal response ref has no revision".to_string())?;
        Ok(DshGoal {
            goal_id: goal_id.to_string(),
            title: title.unwrap_or("").to_string(),
            revision,
            status: status.to_string(),
            created_at: None,
        })
    }

    pub async fn create_goal(
        &self,
        session_id: &str,
        title: &str,
        max_goal_rounds: Option<u32>,
    ) -> Result<DshGoal, String> {
        let mut payload = json!({ "sessionId": session_id, "objective": title });
        if let Some(rounds) = max_goal_rounds {
            payload["maxGoalRounds"] = Value::Number(rounds.into());
        }
        let value = self.call("goal.create", payload).await?;
        Self::goal_result(&value, Some(title), "active")
    }

    pub async fn edit_goal(
        &self,
        session_id: &str,
        goal_id: &str,
        revision: u64,
        objective: Option<&str>,
        max_goal_rounds: Option<u32>,
    ) -> Result<DshGoal, String> {
        let mut payload = json!({
            "sessionId": session_id,
            "ref": { "id": goal_id, "revision": revision },
        });
        if let Some(objective) = objective {
            payload["objective"] = Value::String(objective.to_string());
        }
        if let Some(rounds) = max_goal_rounds {
            payload["maxGoalRounds"] = Value::Number(rounds.into());
        }
        let value = self.call("goal.edit", payload).await?;
        Self::goal_result(&value, objective, "active")
    }

    pub async fn pause_goal(
        &self,
        session_id: &str,
        goal_id: &str,
        revision: u64,
    ) -> Result<DshGoal, String> {
        let value = self
            .call(
                "goal.pause",
                json!({ "sessionId": session_id, "ref": { "id": goal_id, "revision": revision } }),
            )
            .await?;
        Self::goal_result(&value, None, "paused")
    }

    pub async fn resume_goal(
        &self,
        session_id: &str,
        goal_id: &str,
        revision: u64,
    ) -> Result<DshGoal, String> {
        let value = self
            .call(
                "goal.resume",
                json!({ "sessionId": session_id, "ref": { "id": goal_id, "revision": revision } }),
            )
            .await?;
        Self::goal_result(&value, None, "active")
    }

    pub async fn complete_goal(
        &self,
        session_id: &str,
        goal_id: &str,
        revision: u64,
    ) -> Result<DshGoal, String> {
        let value = self
            .call(
                "goal.complete",
                json!({ "sessionId": session_id, "ref": { "id": goal_id, "revision": revision } }),
            )
            .await?;
        Self::goal_result(&value, None, "complete")
    }

    pub async fn clear_goals(
        &self,
        session_id: &str,
        goal_id: &str,
        revision: u64,
    ) -> Result<(), String> {
        self.call(
            "goal.clear",
            json!({ "sessionId": session_id, "ref": { "id": goal_id, "revision": revision } }),
        )
        .await
        .map(|_| ())
    }

    // ── Skill ─────────────────────────────────────────────────────────────────

    pub async fn list_skills(&self, session_id: &str) -> Result<Vec<DshSkillEntry>, String> {
        let value = self
            .call("skill.list", json!({ "sessionId": session_id }))
            .await?;
        let raw = value
            .get("skills")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let skills = raw
            .into_iter()
            .filter_map(|skill| {
                let id = skill
                    .get("id")
                    .or_else(|| skill.get("name"))
                    .and_then(Value::as_str)?
                    .to_string();
                Some(DshSkillEntry {
                    name: skill
                        .get("name")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                    description: skill
                        .get("description")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                    when_to_use: skill
                        .get("whenToUse")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                    model_invocable: skill.get("modelInvocable").and_then(Value::as_bool),
                    id,
                })
            })
            .collect();
        Ok(skills)
    }

    // ── Host ──────────────────────────────────────────────────────────────────

    pub async fn describe_host(&self) -> Result<DshHostInfo, String> {
        let value = self.call("host.describe", json!({})).await?;
        let cwd = value
            .get("cwd")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        Ok(DshHostInfo {
            version: value
                .get("version")
                .and_then(Value::as_str)
                .map(str::to_string),
            cwd,
            provider: value
                .get("provider")
                .and_then(Value::as_str)
                .map(str::to_string),
            model: value
                .get("model")
                .and_then(Value::as_str)
                .map(str::to_string),
            attached_sessions: value.get("attachedSessions").and_then(Value::as_u64),
            can_open_path: value
                .get("canOpenPath")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        })
    }

    pub async fn host_list_directory(
        &self,
        path: Option<&str>,
    ) -> Result<DshDirectoryListing, String> {
        let mut payload = json!({});
        if let Some(p) = path {
            payload["path"] = Value::String(p.to_string());
        }
        let value = self.call("host.listDirectory", payload).await?;
        serde_json::from_value(value)
            .map_err(|e| format!("DSH host.listDirectory payload was invalid: {e}"))
    }

    pub async fn host_create_directory(&self, path: &str, name: &str) -> Result<Value, String> {
        if name.trim().is_empty() {
            return Err("DSH host.createDirectory requires a non-empty name".to_string());
        }
        self.call(
            "host.createDirectory",
            json!({ "path": path, "name": name }),
        )
        .await
    }

    pub async fn host_open_path(&self, path: &str) -> Result<Value, String> {
        self.call("host.openPath", json!({ "path": path })).await
    }

    // ── AgentPreset extended ──────────────────────────────────────────────────

    pub async fn select_preset(
        &self,
        session_id: &str,
        agent_preset: &str,
    ) -> Result<String, String> {
        let value = self
            .call(
                "agentPreset.select",
                json!({ "sessionId": session_id, "agentPreset": agent_preset }),
            )
            .await?;
        Ok(value
            .get("agentPreset")
            .and_then(Value::as_str)
            .unwrap_or(agent_preset)
            .to_string())
    }

    pub async fn read_preset(&self, preset: &str) -> Result<DshPresetReadResult, String> {
        let value = self
            .call("agentPreset.read", json!({ "agentPreset": preset }))
            .await?;
        let content = value
            .get("content")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        Ok(DshPresetReadResult {
            content,
            preset: value
                .get("agentPreset")
                .or_else(|| value.get("preset"))
                .and_then(Value::as_str)
                .unwrap_or(preset)
                .to_string(),
            trust: value
                .get("trust")
                .and_then(Value::as_str)
                .map(str::to_string),
            name: value
                .get("name")
                .and_then(Value::as_str)
                .map(str::to_string),
            description: value
                .get("description")
                .and_then(Value::as_str)
                .map(str::to_string),
        })
    }

    pub async fn copy_preset(
        &self,
        from: &str,
        target_preset: &str,
        name: Option<&str>,
    ) -> Result<DshPresetInfo, String> {
        let mut payload = json!({ "from": from, "agentPreset": target_preset });
        if let Some(n) = name {
            payload["name"] = Value::String(n.to_string());
        }
        let value = self.call("agentPreset.copy", payload).await?;
        let id = value
            .get("agentPreset")
            .or_else(|| value.get("preset").and_then(|preset| preset.get("id")))
            .and_then(Value::as_str)
            .unwrap_or(target_preset)
            .to_string();
        Ok(DshPresetInfo {
            id,
            trust: "user".to_string(),
            is_default: false,
            name: name.map(str::to_string),
            description: None,
            broken: None,
        })
    }

    pub async fn open_preset_document(&self, preset: &str) -> Result<Value, String> {
        self.call("agentPreset.openDocument", json!({ "agentPreset": preset }))
            .await
    }

    pub async fn remove_preset(&self, preset: &str) -> Result<(), String> {
        self.call("agentPreset.remove", json!({ "agentPreset": preset }))
            .await
            .map(|_| ())
    }

    // ── Settings extended ─────────────────────────────────────────────────────

    pub async fn open_settings_document(&self) -> Result<Value, String> {
        self.call("settings.openDocument", json!({})).await
    }

    pub async fn update_settings(
        &self,
        ns: &str,
        patch: Value,
        expected_revision: Option<u64>,
    ) -> Result<Value, String> {
        let mut payload = json!({ "ns": ns, "patch": patch });
        if let Some(revision) = expected_revision {
            payload["expectedRevision"] = Value::Number(revision.into());
        }
        self.call("settings.update", payload).await
    }

    pub async fn replace_settings(
        &self,
        ns: &str,
        value: Value,
        expected_revision: Option<u64>,
    ) -> Result<Value, String> {
        let mut payload = json!({ "ns": ns, "section": value });
        if let Some(revision) = expected_revision {
            payload["expectedRevision"] = Value::Number(revision.into());
        }
        self.call("settings.replace", payload).await
    }

    pub async fn mutate_settings(
        &self,
        ns: &str,
        ops: Value,
        expected_revision: Option<u64>,
    ) -> Result<Value, String> {
        let mut payload = json!({ "ns": ns, "ops": ops });
        if let Some(revision) = expected_revision {
            payload["expectedRevision"] = Value::Number(revision.into());
        }
        self.call("settings.mutate", payload).await
    }
}

#[cfg(test)]
mod tests {
    use super::execute_command_args;
    use serde_json::json;

    #[test]
    fn sends_an_empty_image_list_with_every_command() {
        let args = execute_command_args("session-1", "/permission read-only");
        assert_eq!(args["agentId"], "session-1");
        assert_eq!(args["line"], "/permission read-only");
        // gateway 精确匹配 descriptor 参数名。`images` 缺席会让整个请求被拒
        // (missing "images"),而这条路径也承载启动时的 `/permission`,
        // 所以缺了它连终端都起不来。
        assert_eq!(args["images"], json!([]));
        // 多余字段同样会被拒(unexpected),所以字段集必须正好是这三个。
        assert_eq!(
            args.as_object().map(|fields| fields.len()),
            Some(3),
            "wire 字段必须与 execute(agent, line, images, signal) 的可传参数一一对应"
        );
    }
}
