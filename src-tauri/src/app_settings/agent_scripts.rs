use super::*;

pub(super) fn normalize_model_list(models: Vec<String>) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for model in models
        .into_iter()
        .map(|model| model.trim().to_string())
        .filter(|model| !model.is_empty())
    {
        if seen.insert(model.to_ascii_lowercase()) {
            out.push(model);
        }
    }
    out
}

pub(super) fn normalize_setup_models(draft: &AgentSetupDraft) -> Vec<String> {
    let source = if draft.models.is_empty() {
        vec![draft.model.clone()]
    } else {
        draft.models.clone()
    };
    normalize_model_list(source)
}

pub(super) fn validate_model_name(model: &str) -> bool {
    !model.is_empty()
        && !model
            .chars()
            .any(|ch| matches!(ch, '\0' | '\n' | '\r' | '"' | '\\'))
}

#[cfg(not(windows))]
pub(super) fn model_picker_shell(selected_models: &[String]) -> String {
    let default_model = selected_models.first().cloned().unwrap_or_default();
    format!(
        r#"selected_model="${{AERORIC_AGENT_MODEL:-}}"
if [ -z "$selected_model" ]; then
  selected_model={default_model}
fi
"#,
        default_model = shell_quote(&default_model),
    )
}

/// 新建 Agent 未显式选择推理强度时写入的默认值。
///
/// Aeroric 的使用场景以复杂工程任务为主，默认 `high` 经常需要用户手动再调高一档；
/// 统一默认到 `xhigh`，需要更低强度的用户仍可在 Agent 配置里显式选择。
pub(super) const DEFAULT_MODEL_REASONING_EFFORT: &str = "xhigh";

pub(super) fn codex_config_for_draft(draft: &AgentSetupDraft) -> String {
    let provider = sanitize_custom_agent_id(&draft.id);
    format!(
        r#"model_provider = {provider}
model_reasoning_effort = "{DEFAULT_MODEL_REASONING_EFFORT}"
model_context_window = 258400
model_auto_compact_token_limit = 219640

[model_providers.{provider_key}]
name = {label}
base_url = {base_url}
env_key = "OPENAI_API_KEY"
wire_api = "responses"
request_max_retries = 3
stream_max_retries = 3
stream_idle_timeout_ms = 300000
supports_websockets = false
"#,
        provider = toml_string(&provider),
        provider_key = toml_table_key(&provider),
        label = toml_string(&draft.label),
        base_url = toml_string(&normalize_base_url(&draft.base_url)),
    )
}

pub(super) fn fallback_codex_model(model: &str, priority: usize) -> serde_json::Value {
    serde_json::json!({
        "slug": model,
        "display_name": model,
        "description": "Custom model configured in Aeroric.",
        // 目录必须声明 xhigh，否则默认写入的 model_reasoning_effort = "xhigh"
        // 会被 Codex 当成不支持的取值。
        "default_reasoning_level": DEFAULT_MODEL_REASONING_EFFORT,
        "supported_reasoning_levels": [
            {
                "effort": "high",
                "description": "Greater reasoning depth for complex problems"
            },
            {
                "effort": "xhigh",
                "description": "Maximum reasoning depth for the hardest problems"
            }
        ],
        "shell_type": "shell_command",
        "visibility": "list",
        "supported_in_api": true,
        "priority": priority,
        "upgrade": null,
        "base_instructions": "",
        "supports_reasoning_summaries": true,
        "default_reasoning_summary": "none",
        "support_verbosity": true,
        "default_verbosity": "low",
        "apply_patch_tool_type": "freeform",
        "web_search_tool_type": "text_and_image",
        "truncation_policy": { "mode": "tokens", "limit": 10000 },
        "supports_parallel_tool_calls": true,
        "context_window": 258400,
        "experimental_supported_tools": [],
        "input_modalities": ["text", "image"],
        "supports_search_tool": true
    })
}

pub(super) fn build_codex_model_catalog(
    selected_models: &[String],
    bundled: Option<&str>,
) -> String {
    let bundled_models = bundled
        .and_then(|raw| serde_json::from_str::<serde_json::Value>(raw).ok())
        .and_then(|value| {
            value
                .get("models")
                .and_then(|models| models.as_array())
                .cloned()
        })
        .unwrap_or_default();
    let template = selected_models
        .iter()
        .find_map(|selected| {
            bundled_models.iter().find(|model| {
                model.get("slug").and_then(|slug| slug.as_str()) == Some(selected.as_str())
            })
        })
        .or_else(|| bundled_models.first())
        .cloned();

    let models = selected_models
        .iter()
        .enumerate()
        .map(|(priority, selected)| {
            let mut model = bundled_models
                .iter()
                .find(|model| {
                    model.get("slug").and_then(|slug| slug.as_str()) == Some(selected.as_str())
                })
                .cloned()
                .or_else(|| template.clone())
                .unwrap_or_else(|| fallback_codex_model(selected, priority));
            if let Some(object) = model.as_object_mut() {
                object.insert("slug".to_string(), selected.clone().into());
                object.insert("display_name".to_string(), selected.clone().into());
                object.insert(
                    "description".to_string(),
                    "Custom model configured in Aeroric.".into(),
                );
                object.insert("visibility".to_string(), "list".into());
                object.insert("priority".to_string(), priority.into());
                object.insert("availability_nux".to_string(), serde_json::Value::Null);
                object.insert("upgrade".to_string(), serde_json::Value::Null);
            }
            model
        })
        .collect::<Vec<_>>();

    serde_json::to_string_pretty(&serde_json::json!({ "models": models }))
        .unwrap_or_else(|_| "{\"models\":[]}".to_string())
}

pub(super) fn load_bundled_codex_catalog(codex_bin: &str) -> Option<String> {
    let mut command = Command::new(codex_bin);
    crate::subprocess::configure_background_command(&mut command);
    let output = command
        .args(["debug", "models", "--bundled"])
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

pub(super) fn split_codex_config_for_dynamic_base_url(config: &str) -> (&str, &str) {
    let marker = "base_url = ";
    let Some(index) = config.find(marker) else {
        return (config, "");
    };
    let after_base_url = config[index..]
        .find('\n')
        .map(|offset| index + offset + 1)
        .unwrap_or(config.len());
    (&config[..index], &config[after_base_url..])
}

pub(super) fn is_aeroric_codex_wrapper(content: &str) -> bool {
    content.contains("# AERORIC_CODEX_WRAPPER_VERSION=")
        || is_aeroric_codex_chat_proxy_wrapper(content)
        || (content.contains("export CODEX_HOME=")
            && content.contains("model_catalog_json = \"model-catalog.json\""))
}

pub(super) fn is_aeroric_codex_chat_proxy_wrapper(content: &str) -> bool {
    content.contains("# AERORIC_CODEX_CHAT_PROXY_VERSION=")
}

#[cfg(not(windows))]
pub(super) fn build_codex_agent_shell_script(draft: &AgentSetupDraft) -> String {
    let id = sanitize_custom_agent_id(&draft.id);
    let models = normalize_setup_models(draft);
    let picker = model_picker_shell(&models);
    let config = codex_config_for_draft(draft);
    let codex_bin = detect_path("codex");
    let codex_bin = if codex_bin.is_empty() {
        "codex".to_string()
    } else {
        codex_bin
    };
    let bundled_catalog = load_bundled_codex_catalog(&codex_bin);
    let model_catalog = build_codex_model_catalog(&models, bundled_catalog.as_deref());
    let use_proxy = draft.enable_chat_completions_proxy;
    let proxy_marker = if use_proxy {
        CODEX_CHAT_PROXY_MARKER
    } else {
        CODEX_AGENT_SCRIPT_MARKER
    };
    let upstream_environment = if use_proxy {
        format!(
            "export AERORIC_UPSTREAM_BASE_URL={}\n",
            shell_quote(&normalize_base_url(&draft.base_url))
        )
    } else {
        String::new()
    };
    // Codex talks to the bridge over 127.0.0.1.  A process-level HTTP proxy
    // must never receive that request: many proxy servers interpret its own
    // loopback address and reply with a 502 before the local bridge is reached.
    let local_proxy_bypass_environment = if use_proxy {
        format!(
            r#"existing_no_proxy="${{NO_PROXY:-${{no_proxy:-}}}}"
if [ -n "$existing_no_proxy" ]; then
  export NO_PROXY="${{existing_no_proxy}},{local_proxy_bypass}"
else
  export NO_PROXY="{local_proxy_bypass}"
fi
export no_proxy="$NO_PROXY"
"#,
            local_proxy_bypass = LOCAL_CHAT_PROXY_BYPASS,
        )
    } else {
        String::new()
    };
    let proxy_setup = if use_proxy {
        let (config_before_base_url, config_after_base_url) =
            split_codex_config_for_dynamic_base_url(&config);
        format!(
            r#"proxy_script="$AGENT_HOME/codex-chat-proxy.py"
cat <<'AERORIC_CODEX_CHAT_PROXY' > "$proxy_script"
{proxy_script}
AERORIC_CODEX_CHAT_PROXY
chmod 700 "$proxy_script"

port_file="$AGENT_HOME/codex-chat-proxy.port"
rm -f "$port_file"
python_bin=""
if command -v python3 >/dev/null 2>&1; then
  python_bin="python3"
elif command -v python >/dev/null 2>&1; then
  python_bin="python"
fi
if [ -z "$python_bin" ]; then
  echo "This custom Codex agent requires Python 3 to bridge Responses to Chat Completions." >&2
  exit 1
fi
proxy_log="$AGENT_HOME/codex-chat-proxy.log"
export AERORIC_PROXY_LOG_LEVEL="${{AERORIC_PROXY_LOG_LEVEL:-INFO}}"
"$python_bin" "$proxy_script" --port-file "$port_file" >"$proxy_log" 2>&1 &
proxy_pid=$!
cleanup_proxy() {{
  kill "$proxy_pid" 2>/dev/null || true
  rm -f "$port_file"
}}
trap cleanup_proxy EXIT

proxy_port=""
for _ in $(seq 1 100); do
  if [ -s "$port_file" ]; then
    proxy_port="$(cat "$port_file")"
    break
  fi
  sleep 0.02
done
if [ -z "$proxy_port" ]; then
  echo "Failed to start the local Chat Completions bridge." >&2
  exit 1
fi

{{
  printf 'model = "%s"\n' "$selected_model"
  printf 'model_catalog_json = "model-catalog.json"\n'
  cat <<'AERORIC_CODEX_CONFIG_BEFORE_BASE_URL'
{config_before_base_url}AERORIC_CODEX_CONFIG_BEFORE_BASE_URL
  printf 'base_url = "http://127.0.0.1:%s/v1"\n' "$proxy_port"
  cat <<'AERORIC_CODEX_CONFIG'
{config_after_base_url}AERORIC_CODEX_CONFIG
}} > "$CODEX_HOME/config.toml"
"#,
            proxy_script = CODEX_CHAT_PROXY_SCRIPT,
            config_before_base_url = config_before_base_url,
            config_after_base_url = config_after_base_url,
        )
    } else {
        let (config_before_base_url, config_after_base_url) =
            split_codex_config_for_dynamic_base_url(&config);
        format!(
            r#"{{
  printf 'model = "%s"\n' "$selected_model"
  printf 'model_catalog_json = "model-catalog.json"\n'
  cat <<'AERORIC_CODEX_CONFIG_BEFORE_BASE_URL'
{config_before_base_url}AERORIC_CODEX_CONFIG_BEFORE_BASE_URL
  printf 'base_url = "%s"\n' "${{OPENAI_BASE_URL:-{fallback_base_url}}}"
  cat <<'AERORIC_CODEX_CONFIG_AFTER_BASE_URL'
{config_after_base_url}AERORIC_CODEX_CONFIG_AFTER_BASE_URL
}} > "$CODEX_HOME/config.toml"
"#,
            config_before_base_url = config_before_base_url,
            config_after_base_url = config_after_base_url,
            fallback_base_url = normalize_base_url(&draft.base_url),
        )
    };
    format!(
        r#"#!/bin/bash
set -euo pipefail
{proxy_marker}

AGENT_HOME="${{AERORIC_AGENT_HOME:-$HOME/.aeroric/agent-homes/{id}}}"
mkdir -p "$AGENT_HOME"
export CODEX_HOME="$AGENT_HOME"
API_KEY_FILE="${{AERORIC_AGENT_API_KEY_FILE:-$HOME/.aeroric/agent-credentials/{id}}}"
if [ ! -r "$API_KEY_FILE" ]; then
  echo "Aeroric API key file is missing: $API_KEY_FILE" >&2
  exit 1
fi
api_key="$(cat -- "$API_KEY_FILE")"
if [ -z "$api_key" ]; then
  echo "Aeroric API key file is empty: $API_KEY_FILE" >&2
  exit 1
fi
export OPENAI_API_KEY="$api_key"
export ANTHROPIC_API_KEY="$api_key"
{upstream_environment}
{local_proxy_bypass_environment}
{picker}

cat <<'AERORIC_CODEX_MODELS' > "$CODEX_HOME/model-catalog.json"
{model_catalog}
AERORIC_CODEX_MODELS

{proxy_setup}

{codex_bin} "$@" || codex_status=$?
codex_status="${{codex_status:-0}}"
unset api_key
exit "$codex_status"
"#,
        id = id,
        proxy_marker = proxy_marker,
        upstream_environment = upstream_environment,
        local_proxy_bypass_environment = local_proxy_bypass_environment,
        picker = picker,
        model_catalog = model_catalog,
        proxy_setup = proxy_setup,
        codex_bin = shell_quote(&codex_bin),
    )
}

#[cfg(not(windows))]
pub(super) fn build_claude_code_agent_shell_script(draft: &AgentSetupDraft) -> String {
    let id = sanitize_custom_agent_id(&draft.id);
    let models = normalize_setup_models(draft);
    let picker = model_picker_shell(&models);
    let context_setup = if draft.enable_1m_context {
        r#"
if [[ "$selected_model" != *"[1m]" ]]; then
  selected_model="${selected_model}[1m]"
fi
"#
    } else {
        ""
    };
    format!(
        r#"#!/bin/bash
set -euo pipefail
{script_marker}

AGENT_HOME="${{AERORIC_AGENT_HOME:-$HOME/.aeroric/agent-homes/{id}}}"
mkdir -p "$AGENT_HOME" "$AGENT_HOME/tmp" "$AGENT_HOME/session-env"

export CLAUDE_CONFIG_DIR="$AGENT_HOME"
export CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC="1"
export CLAUDE_CODE_ATTRIBUTION_HEADER="0"
export CLAUDE_CODE_SESSION_ENV_DIR="$AGENT_HOME/session-env"
export TMPDIR="$AGENT_HOME/tmp"

unset ANTHROPIC_API_KEY
unset ANTHROPIC_AUTH_TOKEN
unset ANTHROPIC_BASE_URL
unset ANTHROPIC_DEFAULT_OPUS_MODEL
unset ANTHROPIC_DEFAULT_SONNET_MODEL
unset ANTHROPIC_DEFAULT_HAIKU_MODEL
unset ANTHROPIC_MODEL
unset AGENT_ROUTER_TOKEN

API_KEY_FILE="${{AERORIC_AGENT_API_KEY_FILE:-$HOME/.aeroric/agent-credentials/{id}}}"
if [ ! -r "$API_KEY_FILE" ]; then
  echo "Aeroric API key file is missing: $API_KEY_FILE" >&2
  exit 1
fi
api_key="$(cat -- "$API_KEY_FILE")"
if [ -z "$api_key" ]; then
  echo "Aeroric API key file is empty: $API_KEY_FILE" >&2
  exit 1
fi

{picker}
{context_setup}

export ANTHROPIC_BASE_URL={base_url}
export ANTHROPIC_AUTH_TOKEN="$api_key"
export AGENT_ROUTER_TOKEN="$ANTHROPIC_AUTH_TOKEN"
export ANTHROPIC_DEFAULT_OPUS_MODEL="$selected_model"
export ANTHROPIC_DEFAULT_SONNET_MODEL="$selected_model"
export ANTHROPIC_DEFAULT_HAIKU_MODEL="$selected_model"

exec claude --model "$selected_model" "$@"
"#,
        id = id,
        script_marker = CLAUDE_AGENT_SCRIPT_MARKER,
        picker = picker,
        context_setup = context_setup,
        base_url = shell_quote(&normalize_base_url(&draft.base_url)),
    )
}

#[cfg(any(windows, test))]
pub(super) fn powershell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

#[cfg(any(windows, test))]
pub(super) fn powershell_literal_block(value: &str) -> String {
    format!("@'\n{}\n'@", value.replace("\r\n", "\n"))
}

#[cfg(any(windows, test))]
pub(super) fn powershell_recovery_values(draft: &AgentSetupDraft) -> String {
    let model = normalize_setup_models(draft)
        .first()
        .cloned()
        .unwrap_or_default();
    let base_url = normalize_base_url(&draft.base_url);
    format!(
        "# AERORIC_RECOVERY selected_model={}\n# AERORIC_RECOVERY ANTHROPIC_BASE_URL={}\n# AERORIC_RECOVERY AERORIC_UPSTREAM_BASE_URL={}\n",
        shell_quote(&model),
        shell_quote(&base_url),
        shell_quote(&base_url),
    )
}

#[cfg(any(windows, test))]
pub(super) fn build_codex_agent_powershell_script(draft: &AgentSetupDraft) -> String {
    let id = sanitize_custom_agent_id(&draft.id);
    let models = normalize_setup_models(draft);
    let default_model = models.first().cloned().unwrap_or_default();
    let config = codex_config_for_draft(draft);
    let codex_bin = detect_path("codex");
    let codex_bin = if codex_bin.is_empty() {
        "codex".to_string()
    } else {
        codex_bin
    };
    let bundled_catalog = load_bundled_codex_catalog(&codex_bin);
    let model_catalog = build_codex_model_catalog(&models, bundled_catalog.as_deref());
    let use_proxy = draft.enable_chat_completions_proxy;
    let marker = if use_proxy {
        CODEX_CHAT_PROXY_MARKER
    } else {
        CODEX_AGENT_SCRIPT_MARKER
    };
    // Keep the Responses bridge on the local loopback interface even when the
    // terminal inherited HTTP(S)_PROXY from Aeroric or the parent shell.
    let local_proxy_bypass_environment = if use_proxy {
        format!(
            r#"$existingNoProxy = $env:NO_PROXY
if ([string]::IsNullOrWhiteSpace($existingNoProxy)) {{
  $existingNoProxy = $env:no_proxy
}}
$localProxyBypass = '{local_proxy_bypass}'
if ([string]::IsNullOrWhiteSpace($existingNoProxy)) {{
  $env:NO_PROXY = $localProxyBypass
}} else {{
  $env:NO_PROXY = "$existingNoProxy,$localProxyBypass"
}}
$env:no_proxy = $env:NO_PROXY
"#,
            local_proxy_bypass = LOCAL_CHAT_PROXY_BYPASS,
        )
    } else {
        String::new()
    };
    let config_setup = if use_proxy {
        let (before, after) = split_codex_config_for_dynamic_base_url(&config);
        format!(
            r#"$proxyScript = Join-Path $agentHome 'codex-chat-proxy.py'
[System.IO.File]::WriteAllText($proxyScript, {proxy_script}, $utf8NoBom)
$portFile = Join-Path $agentHome 'codex-chat-proxy.port'
Remove-Item -LiteralPath $portFile -Force -ErrorAction SilentlyContinue
$pythonCommand = Get-Command python3, python, py -ErrorAction SilentlyContinue | Select-Object -First 1
if ($null -eq $pythonCommand) {{
  throw 'This custom Codex agent requires Python 3 to bridge Responses to Chat Completions.'
}}
$pythonArgs = @()
if ($pythonCommand.Name -eq 'py.exe' -or $pythonCommand.Name -eq 'py') {{ $pythonArgs += '-3' }}
$pythonArgs += @(('"' + $proxyScript + '"'), '--port-file', ('"' + $portFile + '"'))
$proxyLog = Join-Path $agentHome 'codex-chat-proxy.log'
$env:AERORIC_PROXY_LOG_LEVEL = if ($env:AERORIC_PROXY_LOG_LEVEL) {{ $env:AERORIC_PROXY_LOG_LEVEL }} else {{ 'INFO' }}
$proxyProcess = Start-Process -FilePath $pythonCommand.Source -ArgumentList $pythonArgs -PassThru -WindowStyle Hidden -RedirectStandardOutput $proxyLog -RedirectStandardError (Join-Path $agentHome 'codex-chat-proxy-err.log')
for ($attempt = 0; $attempt -lt 100; $attempt++) {{
  if ((Test-Path -LiteralPath $portFile) -and (Get-Item -LiteralPath $portFile).Length -gt 0) {{ break }}
  Start-Sleep -Milliseconds 20
}}
if (-not (Test-Path -LiteralPath $portFile)) {{
  Stop-Process -Id $proxyProcess.Id -Force -ErrorAction SilentlyContinue
  throw 'Failed to start the local Chat Completions bridge.'
}}
$proxyPort = (Get-Content -LiteralPath $portFile -Raw).Trim()
$configContent = 'model = "' + $selectedModel + '"' + [Environment]::NewLine +
  'model_catalog_json = "model-catalog.json"' + [Environment]::NewLine +
  {before} + [Environment]::NewLine +
  'base_url = "http://127.0.0.1:' + $proxyPort + '/v1"' + [Environment]::NewLine +
  {after}
"#,
            proxy_script = powershell_literal_block(CODEX_CHAT_PROXY_SCRIPT),
            before = powershell_literal_block(before),
            after = powershell_literal_block(after),
        )
    } else {
        let (before, after) = split_codex_config_for_dynamic_base_url(&config);
        format!(
            r#"$proxyProcess = $null
$portFile = $null
$configContent = 'model = "' + $selectedModel + '"' + [Environment]::NewLine +
  'model_catalog_json = "model-catalog.json"' + [Environment]::NewLine +
  {before} + [Environment]::NewLine +
  'base_url = "' + (if ([string]::IsNullOrWhiteSpace($env:OPENAI_BASE_URL)) {{ {fallback_base_url} }} else {{ $env:OPENAI_BASE_URL }}) + '"' + [Environment]::NewLine +
  {after}
"#,
            before = powershell_literal_block(before),
            after = powershell_literal_block(after),
            fallback_base_url = powershell_quote(&normalize_base_url(&draft.base_url)),
        )
    };
    format!(
        r#"$ErrorActionPreference = 'Stop'
{marker}
{recovery}
$agentHome = if ($env:AERORIC_AGENT_HOME) {{ $env:AERORIC_AGENT_HOME }} else {{ Join-Path $HOME {relative_home} }}
New-Item -ItemType Directory -Force -Path $agentHome | Out-Null
$env:CODEX_HOME = $agentHome
$apiKeyFile = if ($env:AERORIC_AGENT_API_KEY_FILE) {{ $env:AERORIC_AGENT_API_KEY_FILE }} else {{ Join-Path $HOME {api_key_file} }}
if (-not (Test-Path -LiteralPath $apiKeyFile -PathType Leaf)) {{
  throw "Aeroric API key file is missing: $apiKeyFile"
}}
$apiKey = [System.IO.File]::ReadAllText($apiKeyFile).Trim()
if ([string]::IsNullOrEmpty($apiKey)) {{
  throw "Aeroric API key file is empty: $apiKeyFile"
}}
$env:OPENAI_API_KEY = $apiKey
$env:ANTHROPIC_API_KEY = $apiKey
$env:AERORIC_UPSTREAM_BASE_URL = {base_url}
{local_proxy_bypass_environment}
$selectedModel = if ($env:AERORIC_AGENT_MODEL) {{ $env:AERORIC_AGENT_MODEL }} else {{ {default_model} }}
$utf8NoBom = New-Object System.Text.UTF8Encoding($false)
[System.IO.File]::WriteAllText((Join-Path $agentHome 'model-catalog.json'), {model_catalog}, $utf8NoBom)
{config_setup}
[System.IO.File]::WriteAllText((Join-Path $agentHome 'config.toml'), $configContent, $utf8NoBom)
try {{
  & {codex_bin} @args
  exit $LASTEXITCODE
}} finally {{
  if ($null -ne $proxyProcess) {{
    Stop-Process -Id $proxyProcess.Id -Force -ErrorAction SilentlyContinue
  }}
  if ($null -ne $portFile) {{
    Remove-Item -LiteralPath $portFile -Force -ErrorAction SilentlyContinue
  }}
}}
"#,
        marker = marker,
        recovery = powershell_recovery_values(draft),
        relative_home = powershell_quote(&format!(".aeroric\\agent-homes\\{id}")),
        api_key_file = powershell_quote(&format!(".aeroric\\agent-credentials\\{id}")),
        base_url = powershell_quote(&normalize_base_url(&draft.base_url)),
        local_proxy_bypass_environment = local_proxy_bypass_environment,
        default_model = powershell_quote(&default_model),
        model_catalog = powershell_literal_block(&model_catalog),
        config_setup = config_setup,
        codex_bin = powershell_quote(&codex_bin),
    )
}

#[cfg(any(windows, test))]
pub(super) fn powershell_claude_resolution_block(configured_path: &str) -> String {
    format!(
        r#"{resolution_marker}
$nodeDirectories = @(
  $env:NODE_HOME,
  $env:NVM_SYMLINK,
  [Environment]::ExpandEnvironmentVariables('%ProgramFiles%\nodejs'),
  [Environment]::ExpandEnvironmentVariables('%ProgramFiles(x86)%\nodejs'),
  [Environment]::ExpandEnvironmentVariables('%LOCALAPPDATA%\Programs\nodejs')
)
foreach ($nodeDirectory in $nodeDirectories) {{
  if (-not [string]::IsNullOrWhiteSpace($nodeDirectory) -and (Test-Path -LiteralPath (Join-Path $nodeDirectory 'node.exe') -PathType Leaf)) {{
    $env:PATH = "$nodeDirectory;$env:PATH"
    break
  }}
}}

$claudeExecutable = $null
$configuredClaude = {configured_path}
if (-not [string]::IsNullOrWhiteSpace($configuredClaude)) {{
  $configuredCommand = Get-Command $configuredClaude -CommandType Application,ExternalScript -ErrorAction SilentlyContinue | Select-Object -First 1
  if ($null -ne $configuredCommand) {{
    $claudeExecutable = if ($configuredCommand.Path) {{ $configuredCommand.Path }} else {{ $configuredCommand.Source }}
  }} elseif (Test-Path -LiteralPath $configuredClaude -PathType Leaf) {{
    $claudeExecutable = (Resolve-Path -LiteralPath $configuredClaude).Path
  }}
}}
if ([string]::IsNullOrWhiteSpace($claudeExecutable)) {{
  $claudeCommand = Get-Command 'claude' -CommandType Application,ExternalScript -ErrorAction SilentlyContinue | Select-Object -First 1
  if ($null -ne $claudeCommand) {{
    $claudeExecutable = if ($claudeCommand.Path) {{ $claudeCommand.Path }} else {{ $claudeCommand.Source }}
  }}
}}

$claudeCandidates = @(
  [Environment]::ExpandEnvironmentVariables('%USERPROFILE%\.aeroric\tools\claude\current\claude.exe'),
  [Environment]::ExpandEnvironmentVariables('%APPDATA%\npm\claude.cmd'),
  [Environment]::ExpandEnvironmentVariables('%APPDATA%\npm\claude.ps1'),
  [Environment]::ExpandEnvironmentVariables('%USERPROFILE%\.local\bin\claude.exe'),
  [Environment]::ExpandEnvironmentVariables('%USERPROFILE%\.local\bin\claude.cmd'),
  [Environment]::ExpandEnvironmentVariables('%USERPROFILE%\.local\bin\claude.ps1'),
  [Environment]::ExpandEnvironmentVariables('%USERPROFILE%\.npm-global\bin\claude.cmd'),
  [Environment]::ExpandEnvironmentVariables('%USERPROFILE%\.npm-global\bin\claude.ps1'),
  [Environment]::ExpandEnvironmentVariables('%USERPROFILE%\scoop\shims\claude.cmd'),
  [Environment]::ExpandEnvironmentVariables('%USERPROFILE%\scoop\shims\claude.ps1'),
  [Environment]::ExpandEnvironmentVariables('%LOCALAPPDATA%\Programs\claude-code\claude.exe'),
  [Environment]::ExpandEnvironmentVariables('%LOCALAPPDATA%\Programs\Claude\claude.exe')
)
$npmCommand = Get-Command 'npm' -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1
if ($null -ne $npmCommand) {{
  $npmExecutable = if ($npmCommand.Path) {{ $npmCommand.Path }} else {{ $npmCommand.Source }}
  $npmPrefix = (& $npmExecutable prefix -g 2>$null | Select-Object -First 1)
  if ($null -ne $npmPrefix) {{
    $npmPrefix = $npmPrefix.ToString().Trim()
    if ($npmPrefix) {{
      $claudeCandidates += Join-Path $npmPrefix 'claude.cmd'
      $claudeCandidates += Join-Path $npmPrefix 'claude.ps1'
    }}
  }}
}}
if ([string]::IsNullOrWhiteSpace($claudeExecutable)) {{
  foreach ($candidate in $claudeCandidates) {{
    if (-not [string]::IsNullOrWhiteSpace($candidate) -and (Test-Path -LiteralPath $candidate -PathType Leaf)) {{
      $claudeExecutable = (Resolve-Path -LiteralPath $candidate).Path
      break
    }}
  }}
}}
if ([string]::IsNullOrWhiteSpace($claudeExecutable)) {{
  throw 'AERORIC_CLAUDE_CLI_NOT_FOUND: Claude Code CLI was not found. Install Node.js and Claude Code, or add its executable directory to PATH.'
}}
"#,
        configured_path = powershell_quote(configured_path),
        resolution_marker = CLAUDE_CLI_RESOLUTION_MARKER,
    )
}

#[cfg(any(windows, test))]
pub(super) fn build_claude_code_agent_powershell_script(draft: &AgentSetupDraft) -> String {
    let id = sanitize_custom_agent_id(&draft.id);
    let models = normalize_setup_models(draft);
    let default_model = models.first().cloned().unwrap_or_default();
    let claude_bin = detect_path("claude");
    let claude_bin = if claude_bin.is_empty() {
        "claude".to_string()
    } else {
        claude_bin
    };
    let context_setup = if draft.enable_1m_context {
        r#"
if (-not $selectedModel.EndsWith('[1m]')) { $selectedModel += '[1m]' }
"#
    } else {
        ""
    };
    format!(
        r#"$ErrorActionPreference = 'Stop'
{marker}
{recovery}
$agentHome = if ($env:AERORIC_AGENT_HOME) {{ $env:AERORIC_AGENT_HOME }} else {{ Join-Path $HOME {relative_home} }}
New-Item -ItemType Directory -Force -Path $agentHome, (Join-Path $agentHome 'tmp'), (Join-Path $agentHome 'session-env') | Out-Null
$env:CLAUDE_CONFIG_DIR = $agentHome
$env:CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC = '1'
$env:CLAUDE_CODE_ATTRIBUTION_HEADER = '0'
$env:CLAUDE_CODE_SESSION_ENV_DIR = Join-Path $agentHome 'session-env'
$env:TMP = Join-Path $agentHome 'tmp'
$env:TEMP = $env:TMP
Remove-Item Env:ANTHROPIC_API_KEY, Env:ANTHROPIC_AUTH_TOKEN, Env:ANTHROPIC_BASE_URL -ErrorAction SilentlyContinue
$selectedModel = if ($env:AERORIC_AGENT_MODEL) {{ $env:AERORIC_AGENT_MODEL }} else {{ {default_model} }}
{context_setup}
$apiKeyFile = if ($env:AERORIC_AGENT_API_KEY_FILE) {{ $env:AERORIC_AGENT_API_KEY_FILE }} else {{ Join-Path $HOME {api_key_file} }}
if (-not (Test-Path -LiteralPath $apiKeyFile -PathType Leaf)) {{
  throw "Aeroric API key file is missing: $apiKeyFile"
}}
$apiKey = [System.IO.File]::ReadAllText($apiKeyFile).Trim()
if ([string]::IsNullOrEmpty($apiKey)) {{
  throw "Aeroric API key file is empty: $apiKeyFile"
}}
$env:ANTHROPIC_BASE_URL = {base_url}
$env:ANTHROPIC_AUTH_TOKEN = $apiKey
$env:AGENT_ROUTER_TOKEN = $env:ANTHROPIC_AUTH_TOKEN
$env:ANTHROPIC_DEFAULT_OPUS_MODEL = $selectedModel
$env:ANTHROPIC_DEFAULT_SONNET_MODEL = $selectedModel
$env:ANTHROPIC_DEFAULT_HAIKU_MODEL = $selectedModel
{cli_resolution}
& $claudeExecutable --model $selectedModel @args
exit $LASTEXITCODE
"#,
        marker = CLAUDE_AGENT_SCRIPT_MARKER,
        recovery = powershell_recovery_values(draft),
        relative_home = powershell_quote(&format!(".aeroric\\agent-homes\\{id}")),
        default_model = powershell_quote(&default_model),
        context_setup = context_setup,
        api_key_file = powershell_quote(&format!(".aeroric\\agent-credentials\\{id}")),
        base_url = powershell_quote(&normalize_base_url(&draft.base_url)),
        cli_resolution = powershell_claude_resolution_block(&claude_bin),
    )
}

pub(super) fn build_codex_agent_script(draft: &AgentSetupDraft) -> String {
    #[cfg(windows)]
    {
        build_codex_agent_powershell_script(draft)
    }
    #[cfg(not(windows))]
    {
        build_codex_agent_shell_script(draft)
    }
}

pub(super) fn build_claude_code_agent_script(draft: &AgentSetupDraft) -> String {
    #[cfg(windows)]
    {
        build_claude_code_agent_powershell_script(draft)
    }
    #[cfg(not(windows))]
    {
        build_claude_code_agent_shell_script(draft)
    }
}

pub(super) fn build_agent_script(draft: &AgentSetupDraft) -> String {
    match draft.kind {
        AgentSetupKind::Codex => build_codex_agent_script(draft),
        AgentSetupKind::ClaudeCode => build_claude_code_agent_script(draft),
        // dsh-like 档案不走 wrapper 脚本(setup_agent_profile 单独分支处理)。
        AgentSetupKind::Dsh => String::new(),
    }
}

pub(super) fn validate_agent_setup_draft(draft: &AgentSetupDraft) -> Result<String, String> {
    let id = sanitize_custom_agent_id(&draft.id);
    if id.is_empty() {
        return Err("Agent ID is required".to_string());
    }
    if draft.label.trim().is_empty() {
        return Err("Agent name is required".to_string());
    }
    let is_dsh = matches!(draft.kind, AgentSetupKind::Dsh);
    // dsh 官方 provider 无需 base_url;提供 base_url 即自定义 OpenAI 兼容网关。
    if !is_dsh && normalize_base_url(&draft.base_url).is_empty() {
        return Err("Base URL is required".to_string());
    }
    if draft.api_key.trim().is_empty() {
        return Err("API key is required".to_string());
    }
    if draft.api_key.contains('\0') || draft.base_url.contains('\0') {
        return Err("API key and base URL cannot contain NUL bytes".to_string());
    }
    let models = normalize_setup_models(draft);
    if models.is_empty() && !(is_dsh && normalize_base_url(&draft.base_url).is_empty()) {
        return Err("At least one model is required".to_string());
    }
    if models.iter().any(|model| !validate_model_name(model)) {
        return Err("Model names cannot contain quotes, backslashes, or newlines".to_string());
    }
    Ok(id)
}

pub(super) fn setup_agent_kind_suffix(kind: &AgentSetupKind) -> &'static str {
    match kind {
        AgentSetupKind::Codex => "codex",
        AgentSetupKind::ClaudeCode => "claude",
        AgentSetupKind::Dsh => "dsh",
    }
}

pub(super) fn allocate_setup_agent_id(
    requested_id: &str,
    kind: &AgentSetupKind,
    settings: &AppSettings,
) -> Result<String, String> {
    let requested = sanitize_custom_agent_id(requested_id);
    if requested.is_empty() {
        return Err("Agent ID is required".to_string());
    }
    let suffix = setup_agent_kind_suffix(kind);
    let base = requested
        .strip_suffix("_codex")
        .or_else(|| requested.strip_suffix("_claude"))
        .or_else(|| requested.strip_suffix("_dsh"))
        .unwrap_or(&requested);
    let base = if base.is_empty() { "agent" } else { base };
    let preferred = sanitize_custom_agent_id(&format!("{base}_{suffix}"));
    // 内置 agent id 全部保留。当前 `preferred` 总会带上 `_{suffix}` 后缀,拼不出
    // 裸 id,所以这里只是把不变量写全:内置集合一旦变化,自定义档案也不该占用。
    let is_used = |candidate: &str| {
        matches!(candidate, "claude" | "claude_gpt55" | "codex" | "dsh")
            || settings
                .custom_agents
                .iter()
                .any(|profile| profile.id == candidate)
    };
    if !is_used(&preferred) {
        return Ok(preferred);
    }
    for index in 2..=10_000 {
        let candidate = sanitize_custom_agent_id(&format!("{preferred}_{index}"));
        if !is_used(&candidate) {
            return Ok(candidate);
        }
    }
    Err("Could not allocate a unique Agent ID".to_string())
}

pub(super) fn write_agent_script_at_path(path: &Path, content: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    atomic_write(path, content)?;
    #[cfg(not(windows))]
    {
        let mut permissions = fs::metadata(path).map_err(|e| e.to_string())?.permissions();
        permissions.set_mode(0o700);
        fs::set_permissions(path, permissions).map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub(super) fn native_agent_script_extension() -> &'static str {
    if cfg!(windows) {
        "ps1"
    } else {
        "sh"
    }
}

pub(super) fn default_agent_script_path(id: &str) -> Result<PathBuf, String> {
    Ok(agent_scripts_dir()?.join(format!("{id}.{}", native_agent_script_extension())))
}

pub(super) fn generated_agent_script_target_path(
    id: &str,
    current_path: &str,
) -> Result<PathBuf, String> {
    let current_path = normalize_config_path(current_path.to_string());
    if current_path.trim().is_empty() {
        return default_agent_script_path(id);
    }
    let current = PathBuf::from(current_path);
    if current
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case(native_agent_script_extension()))
    {
        return Ok(current);
    }
    Ok(current.with_extension(native_agent_script_extension()))
}

pub(super) fn is_aeroric_generated_agent_wrapper(content: &str) -> bool {
    is_aeroric_codex_wrapper(content)
        || content.contains(CLAUDE_AGENT_SCRIPT_MARKER_PREFIX)
        || (content.contains("export CLAUDE_CONFIG_DIR=")
            && content.contains("CLAUDE_CODE_SESSION_ENV_DIR"))
}

pub(super) fn write_generated_agent_script(
    id: &str,
    current_path: &str,
    content: &str,
    api_key: &str,
) -> Result<PathBuf, String> {
    let target = generated_agent_script_target_path(id, current_path)?;
    write_agent_script_at_path(&target, content)?;
    write_agent_api_key(id, api_key)?;

    let previous = PathBuf::from(normalize_config_path(current_path.to_string()));
    if !current_path.trim().is_empty() && previous != target {
        let remove_previous = fs::read_to_string(&previous)
            .map(|existing| is_aeroric_generated_agent_wrapper(&existing))
            .unwrap_or(false);
        if remove_previous {
            let _ = fs::remove_file(previous);
        }
    }
    Ok(target)
}

pub(super) fn write_agent_script(
    id: &str,
    content: &str,
    api_key: &str,
) -> Result<PathBuf, String> {
    let dir = agent_scripts_dir()?;
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = default_agent_script_path(id)?;
    write_agent_script_at_path(&path, content)?;
    write_agent_api_key(id, api_key)?;
    Ok(path)
}

pub(super) fn remove_agent_profile_file(path: &str) -> Result<(), String> {
    let path = normalize_config_path(path.to_string());
    if path.trim().is_empty() {
        return Ok(());
    }
    let path = Path::new(&path);
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.to_string()),
    };
    if metadata.is_dir() {
        return Err(format!(
            "Refusing to delete directory as agent config: {}",
            path.display()
        ));
    }
    fs::remove_file(path).map_err(|error| error.to_string())
}

pub(super) fn profile_uses_aeroric_generated_wrapper(profile: &CustomAgentProfile) -> bool {
    let normalized_path = normalize_config_path(profile.path.clone());
    if fs::read_to_string(&normalized_path)
        .map(|content| is_aeroric_generated_agent_wrapper(&content))
        .unwrap_or(false)
    {
        return true;
    }
    let expected_path = default_agent_script_path(&profile.id)
        .ok()
        .map(|path| normalize_config_path(path.to_string_lossy().into_owned()));
    expected_path.as_deref() == Some(normalized_path.as_str())
        && profile.config_lang == "shellscript"
        && !profile.base_url.trim().is_empty()
        && !profile.api_key.trim().is_empty()
        && !profile.models.is_empty()
}

pub(super) fn remove_exact_generated_agent_home_at(
    homes_root: &Path,
    id: &str,
) -> Result<(), String> {
    let normalized_id = sanitize_custom_agent_id(id);
    if normalized_id.is_empty() || normalized_id != id {
        return Err("Refusing to delete an invalid Agent home path".to_string());
    }
    let target = homes_root.join(&normalized_id);
    if target.parent() != Some(homes_root) {
        return Err("Refusing to delete an Agent home outside the isolation directory".to_string());
    }
    let metadata = match fs::symlink_metadata(&target) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.to_string()),
    };
    if metadata.file_type().is_symlink() || metadata.is_file() {
        fs::remove_file(&target).map_err(|error| error.to_string())
    } else if metadata.is_dir() {
        fs::remove_dir_all(&target).map_err(|error| error.to_string())
    } else {
        Err(format!(
            "Refusing to delete unsupported Agent home entry: {}",
            target.display()
        ))
    }
}

pub(super) fn remove_exact_generated_agent_home(id: &str) -> Result<(), String> {
    remove_exact_generated_agent_home_at(&aeroric_dir()?.join("agent-homes"), id)
}

pub(super) fn parse_generated_shell_value(content: &str, key: &str) -> Option<String> {
    for line in content.lines() {
        let trimmed = line.trim();
        let line = trimmed
            .strip_prefix("# AERORIC_RECOVERY ")
            .or_else(|| trimmed.strip_prefix("export "))
            .unwrap_or(trimmed);
        let Some(value) = line
            .strip_prefix(key)
            .and_then(|value| value.strip_prefix('='))
            .map(str::trim)
        else {
            continue;
        };
        if value.starts_with('$') || value.contains("${") {
            continue;
        }
        if let Some(single_quoted) = value.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')) {
            return Some(single_quoted.replace("'\"'\"'", "'"));
        }
        if let Some(double_quoted) = value.strip_prefix('"').and_then(|v| v.strip_suffix('"')) {
            if !double_quoted.contains('$') {
                return Some(double_quoted.to_string());
            }
            continue;
        }
        if !value.is_empty() && !value.chars().any(char::is_whitespace) {
            return Some(value.to_string());
        }
    }
    None
}

pub(super) fn parse_generated_toml_string(content: &str, key: &str) -> Option<String> {
    content.lines().find_map(|line| {
        let line = line.trim();
        if !line.starts_with(key) {
            return None;
        }
        let table = toml::from_str::<toml::Table>(line).ok()?;
        table.get(key)?.as_str().map(str::to_string)
    })
}

pub(super) fn push_builtin_model(credentials: &mut BuiltInAgentCredentials, value: &str) {
    let value = value.trim();
    if value.is_empty() {
        return;
    }
    let (model, uses_1m_context) = value
        .strip_suffix("[1m]")
        .map(|model| (model.trim(), true))
        .unwrap_or((value, false));
    if model.is_empty() {
        return;
    }
    credentials.enable_1m_context |= uses_1m_context;
    if !credentials.models.iter().any(|existing| existing == model) {
        credentials.models.push(model.to_string());
    }
}

pub(super) fn parse_claude_builtin_credentials(content: &str) -> BuiltInAgentCredentials {
    let mut credentials = BuiltInAgentCredentials::default();
    let Ok(value) = serde_json::from_str::<serde_json::Value>(content) else {
        return credentials;
    };
    let env = value.get("env").and_then(serde_json::Value::as_object);
    let env_value = |key: &str| {
        env.and_then(|values| values.get(key))
            .and_then(serde_json::Value::as_str)
    };

    credentials.base_url = env_value("ANTHROPIC_BASE_URL")
        .map(normalize_base_url)
        .unwrap_or_default();
    credentials.api_key = ["ANTHROPIC_AUTH_TOKEN", "ANTHROPIC_API_KEY"]
        .into_iter()
        .find_map(env_value)
        .unwrap_or_default()
        .trim()
        .to_string();
    if let Some(model) = value.get("model").and_then(serde_json::Value::as_str) {
        push_builtin_model(&mut credentials, model);
    }
    for key in [
        "ANTHROPIC_MODEL",
        "ANTHROPIC_DEFAULT_OPUS_MODEL",
        "ANTHROPIC_DEFAULT_SONNET_MODEL",
        "ANTHROPIC_DEFAULT_HAIKU_MODEL",
    ] {
        if let Some(model) = env_value(key) {
            push_builtin_model(&mut credentials, model);
        }
    }
    credentials
}

pub(super) fn parse_claude_credentials_file(content: &str) -> BuiltInAgentCredentials {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(content) else {
        return BuiltInAgentCredentials::default();
    };

    // Claude's official credentials file normally contains OAuth access tokens.
    // Only accept fields that explicitly identify an API key; never treat an
    // accessToken, refreshToken, or account token as an API key.
    fn visit(value: &serde_json::Value, credentials: &mut BuiltInAgentCredentials) {
        let Some(object) = value.as_object() else {
            return;
        };
        for (key, value) in object {
            if let Some(text) = value.as_str() {
                match key.as_str() {
                    "ANTHROPIC_BASE_URL" | "baseUrl" | "base_url" => {
                        if credentials.base_url.is_empty() {
                            credentials.base_url = normalize_base_url(text);
                        }
                    }
                    "ANTHROPIC_API_KEY" | "ANTHROPIC_AUTH_TOKEN" | "apiKey" | "api_key"
                        if credentials.api_key.is_empty() && !text.trim().is_empty() =>
                    {
                        credentials.api_key = text.trim().to_string();
                    }
                    _ => {}
                }
            }
            if credentials.base_url.is_empty() || credentials.api_key.is_empty() {
                visit(value, credentials);
            }
        }
    }

    let mut credentials = BuiltInAgentCredentials::default();
    visit(&value, &mut credentials);
    credentials
}

pub(super) fn parse_shell_builtin_credentials(content: &str) -> BuiltInAgentCredentials {
    let mut credentials = BuiltInAgentCredentials {
        base_url: ["ANTHROPIC_BASE_URL", "OPENAI_BASE_URL"]
            .into_iter()
            .find_map(|key| parse_generated_shell_value(content, key))
            .map(|value| normalize_base_url(&value))
            .unwrap_or_default(),
        api_key: [
            "ANTHROPIC_AUTH_TOKEN",
            "ANTHROPIC_API_KEY",
            "OPENAI_API_KEY",
            "CODEX_API_KEY",
        ]
        .into_iter()
        .find_map(|key| parse_generated_shell_value(content, key))
        .unwrap_or_default()
        .trim()
        .to_string(),
        ..Default::default()
    };
    for key in [
        "selected_model",
        "ANTHROPIC_MODEL",
        "ANTHROPIC_DEFAULT_OPUS_MODEL",
        "ANTHROPIC_DEFAULT_SONNET_MODEL",
        "ANTHROPIC_DEFAULT_HAIKU_MODEL",
    ] {
        if let Some(model) = parse_generated_shell_value(content, key) {
            push_builtin_model(&mut credentials, &model);
        }
    }
    credentials
}

pub(super) fn parse_codex_builtin_credentials_with_env(
    content: &str,
    env: &impl Fn(&str) -> Option<String>,
) -> BuiltInAgentCredentials {
    let mut credentials = BuiltInAgentCredentials::default();
    let Ok(table) = toml::from_str::<toml::Table>(content) else {
        return credentials;
    };
    if let Some(model) = table.get("model").and_then(toml::Value::as_str) {
        push_builtin_model(&mut credentials, model);
    }

    let provider = table.get("model_provider").and_then(toml::Value::as_str);
    let provider_table = provider.and_then(|provider| {
        table
            .get("model_providers")
            .and_then(toml::Value::as_table)
            .and_then(|providers| providers.get(provider))
            .and_then(toml::Value::as_table)
    });
    credentials.base_url = provider_table
        .and_then(|provider| provider.get("base_url"))
        .or_else(|| table.get("base_url"))
        .and_then(toml::Value::as_str)
        .map(normalize_base_url)
        .unwrap_or_default();
    credentials.api_key = provider_table
        .and_then(|provider| provider.get("api_key"))
        .or_else(|| table.get("api_key"))
        .and_then(toml::Value::as_str)
        .map(str::to_string)
        .or_else(|| {
            provider_table
                .and_then(|provider| provider.get("env_key"))
                .and_then(toml::Value::as_str)
                .and_then(env)
        })
        .or_else(|| {
            ["OPENAI_API_KEY", "CODEX_API_KEY", "ANTHROPIC_API_KEY"]
                .into_iter()
                .find_map(env)
        })
        .unwrap_or_default()
        .trim()
        .to_string();
    credentials
}

#[cfg(test)]
pub(super) fn parse_codex_builtin_credentials(content: &str) -> BuiltInAgentCredentials {
    parse_codex_builtin_credentials_with_env(content, &|key| std::env::var(key).ok())
}

pub(super) fn codex_auth_path(config_path: &Path) -> Option<PathBuf> {
    config_path.parent().map(|parent| parent.join("auth.json"))
}

pub(super) fn read_codex_auth_api_key(config_path: &Path) -> Option<String> {
    let content = fs::read_to_string(codex_auth_path(config_path)?).ok()?;
    let value = serde_json::from_str::<serde_json::Value>(&content).ok()?;
    ["OPENAI_API_KEY", "CODEX_API_KEY", "ANTHROPIC_API_KEY"]
        .into_iter()
        .find_map(|key| value.get(key).and_then(serde_json::Value::as_str))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

pub(super) fn write_codex_auth_api_key(config_path: &Path, api_key: &str) -> Result<(), String> {
    let Some(auth_path) = codex_auth_path(config_path) else {
        return Err("Codex configuration path has no parent directory".to_string());
    };
    if let Some(parent) = auth_path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let mut value = fs::read_to_string(&auth_path)
        .ok()
        .and_then(|content| serde_json::from_str::<serde_json::Value>(&content).ok())
        .filter(serde_json::Value::is_object)
        .unwrap_or_else(|| serde_json::json!({}));
    let object = value
        .as_object_mut()
        .ok_or_else(|| "Invalid Codex authentication file".to_string())?;
    object.insert(
        "auth_mode".to_string(),
        serde_json::Value::String("apikey".to_string()),
    );
    object.insert(
        "OPENAI_API_KEY".to_string(),
        serde_json::Value::String(api_key.trim().to_string()),
    );
    let content = serde_json::to_string_pretty(&value).map_err(|error| error.to_string())?;
    atomic_write_private(&auth_path, &content)
}

pub(super) fn parse_builtin_agent_credentials_with_env(
    agent: &str,
    content: &str,
    env: &impl Fn(&str) -> Option<String>,
) -> BuiltInAgentCredentials {
    match agent {
        "claude" => parse_claude_builtin_credentials(content),
        "claude_gpt55" => parse_shell_builtin_credentials(content),
        "codex" => parse_codex_builtin_credentials_with_env(content, env),
        _ => BuiltInAgentCredentials::default(),
    }
}

pub(super) fn parse_builtin_agent_credentials(
    agent: &str,
    content: &str,
) -> BuiltInAgentCredentials {
    parse_builtin_agent_credentials_with_env(agent, content, &|key| std::env::var(key).ok())
}

pub(super) fn merged_builtin_agent_credentials_with_env(
    settings: &AppSettings,
    agent: &str,
    config_content: &str,
    env: &impl Fn(&str) -> Option<String>,
) -> BuiltInAgentCredentials {
    let mut credentials = settings
        .builtin_agent_credentials
        .get(agent)
        .cloned()
        .unwrap_or_default();
    let recovered = parse_builtin_agent_credentials_with_env(agent, config_content, env);
    if credentials.base_url.is_empty() {
        credentials.base_url = recovered.base_url;
    }
    if credentials.api_key.is_empty() {
        credentials.api_key = recovered.api_key;
    }
    if credentials.models.is_empty() {
        credentials.models = recovered.models;
    }
    credentials.enable_1m_context |= recovered.enable_1m_context;
    credentials.base_url = normalize_base_url(&credentials.base_url);
    credentials.api_key = credentials.api_key.trim().to_string();
    credentials.models = normalize_model_list(credentials.models);
    credentials
}

pub(super) fn detect_builtin_agent_credentials_with_env(
    settings: &AppSettings,
    agent: &str,
    config_path: &Path,
    config_content: &str,
    env: &impl Fn(&str) -> Option<String>,
) -> BuiltInAgentCredentials {
    let mut credentials =
        merged_builtin_agent_credentials_with_env(settings, agent, config_content, env);
    if agent == "claude" && (credentials.base_url.is_empty() || credentials.api_key.is_empty()) {
        if let Some(parent) = config_path.parent() {
            if let Ok(content) = fs::read_to_string(parent.join(".credentials.json")) {
                let recovered = parse_claude_credentials_file(&content);
                if credentials.base_url.is_empty() {
                    credentials.base_url = recovered.base_url;
                }
                if credentials.api_key.is_empty() {
                    credentials.api_key = recovered.api_key;
                }
            }
        }
    }
    if agent == "codex" && credentials.api_key.is_empty() {
        credentials.api_key = read_codex_auth_api_key(config_path).unwrap_or_default();
    }
    credentials.base_url = normalize_base_url(&credentials.base_url);
    credentials.api_key = credentials.api_key.trim().to_string();
    credentials.models = normalize_model_list(credentials.models);
    credentials
}

pub(super) fn detect_builtin_agent_credentials(
    settings: &AppSettings,
    agent: &str,
    config_path: &Path,
    config_content: &str,
) -> BuiltInAgentCredentials {
    detect_builtin_agent_credentials_with_env(
        settings,
        agent,
        config_path,
        config_content,
        &|key| std::env::var(key).ok(),
    )
}

pub(super) fn recover_custom_agent_credentials(profile: &mut CustomAgentProfile) {
    if !profile.base_url.is_empty() && !profile.api_key.is_empty() {
        return;
    }
    let Ok(content) = fs::read_to_string(&profile.path) else {
        return;
    };
    if profile.base_url.is_empty() {
        let recovered = if profile.codex_like {
            parse_generated_shell_value(&content, "AERORIC_UPSTREAM_BASE_URL")
                .or_else(|| parse_generated_toml_string(&content, "base_url"))
        } else {
            parse_generated_shell_value(&content, "ANTHROPIC_BASE_URL")
        };
        if let Some(base_url) = recovered {
            profile.base_url = normalize_base_url(&base_url);
        }
    }
    if profile.api_key.is_empty() {
        let keys: &[&str] = if profile.codex_like {
            &["OPENAI_API_KEY", "ANTHROPIC_API_KEY"]
        } else {
            &["ANTHROPIC_AUTH_TOKEN", "ANTHROPIC_API_KEY"]
        };
        if let Some(api_key) = keys
            .iter()
            .find_map(|key| parse_generated_shell_value(&content, key))
        {
            profile.api_key = api_key.trim().to_string();
        }
    }
}

pub(super) fn recover_custom_agent_models(profile: &mut CustomAgentProfile) {
    if !profile.models.is_empty() {
        return;
    }
    let Ok(content) = fs::read_to_string(&profile.path) else {
        return;
    };
    let Some(model) = parse_generated_shell_value(&content, "selected_model") else {
        return;
    };
    let model = model.trim();
    if !model.is_empty() {
        profile.models.push(model.to_string());
    }
}

pub(super) fn recover_custom_agent_settings(settings: &mut AppSettings) {
    for profile in &mut settings.custom_agents {
        recover_custom_agent_credentials(profile);
        recover_custom_agent_models(profile);
    }
}

pub(super) fn refresh_stale_codex_agent_scripts(settings: &mut AppSettings) {
    for profile in &mut settings.custom_agents {
        if !profile.codex_like
            || profile.config_lang != "shellscript"
            || profile.models.is_empty()
            || profile.base_url.trim().is_empty()
            || profile.api_key.trim().is_empty()
        {
            continue;
        }
        // Always sync credentials file to ensure API key is up to date,
        // even if the script itself doesn't need regeneration.
        let _ = sync_agent_credentials(&profile.id, &profile.api_key);

        let script_path = normalize_config_path(profile.path.clone());
        let script_content = fs::read_to_string(&script_path).unwrap_or_default();
        // The saved profile is the only source of truth for the bridge. Earlier
        // builds wrote the bridge into every Codex wrapper unconditionally, so a
        // bridge marker in the script does not imply the user opted in — agents
        // stay on the direct Responses endpoint until the setting is turned on.
        let expected_marker = if profile.enable_chat_completions_proxy {
            CODEX_CHAT_PROXY_MARKER
        } else {
            CODEX_AGENT_SCRIPT_MARKER
        };
        let requires_native_script_migration =
            generated_agent_script_target_path(&profile.id, &script_path)
                .map(|target| target.as_path() != Path::new(&script_path))
                .unwrap_or(false);
        if script_content.contains(expected_marker) && !requires_native_script_migration {
            continue;
        }
        // Do not replace arbitrary user-authored shell scripts during startup.
        // Empty/missing scripts can be regenerated, while legacy Aeroric Codex
        // wrappers are recognized by their CODEX_HOME/model-catalog signature.
        if !script_content.is_empty() && !is_aeroric_codex_wrapper(&script_content) {
            continue;
        }
        let draft = AgentSetupDraft {
            id: profile.id.clone(),
            label: profile.label.clone(),
            kind: AgentSetupKind::Codex,
            base_url: profile.base_url.clone(),
            api_key: profile.api_key.clone(),
            model: profile.models[0].clone(),
            models: profile.models.clone(),
            enable_1m_context: profile.enable_1m_context,
            enable_chat_completions_proxy: profile.enable_chat_completions_proxy,
            dsh_api_protocol: String::new(),
            proxy_enabled: false,
        };
        if validate_agent_setup_draft(&draft).is_err() {
            continue;
        }
        let script = build_codex_agent_script(&draft);
        if let Ok(path) =
            write_generated_agent_script(&profile.id, &script_path, &script, &profile.api_key)
        {
            profile.path = path.to_string_lossy().into_owned();
        }
    }
}

pub(super) fn refresh_stale_claude_agent_scripts(settings: &mut AppSettings) {
    for profile in &mut settings.custom_agents {
        if profile.codex_like
            || profile.config_lang != "shellscript"
            || profile.models.is_empty()
            || profile.base_url.trim().is_empty()
            || profile.api_key.trim().is_empty()
        {
            continue;
        }
        // Always sync credentials file to ensure API key is up to date,
        // even if the script itself doesn't need regeneration.
        let _ = sync_agent_credentials(&profile.id, &profile.api_key);

        let script_path = normalize_config_path(profile.path.clone());
        let script_content = fs::read_to_string(&script_path).unwrap_or_default();
        let is_current = claude_agent_script_is_current(&script_path, &script_content);
        let requires_native_script_migration =
            generated_agent_script_target_path(&profile.id, &script_path)
                .map(|target| target.as_path() != Path::new(&script_path))
                .unwrap_or(false);
        if is_current && !requires_native_script_migration {
            continue;
        }
        if !script_content.is_empty() && !is_aeroric_generated_agent_wrapper(&script_content) {
            continue;
        }
        let draft = AgentSetupDraft {
            id: profile.id.clone(),
            label: profile.label.clone(),
            kind: AgentSetupKind::ClaudeCode,
            base_url: profile.base_url.clone(),
            api_key: profile.api_key.clone(),
            model: profile.models[0].clone(),
            models: profile.models.clone(),
            enable_1m_context: profile.enable_1m_context,
            enable_chat_completions_proxy: profile.enable_chat_completions_proxy,
            dsh_api_protocol: String::new(),
            proxy_enabled: false,
        };
        if validate_agent_setup_draft(&draft).is_err() {
            continue;
        }
        let script = build_claude_code_agent_script(&draft);
        if let Ok(path) =
            write_generated_agent_script(&profile.id, &script_path, &script, &profile.api_key)
        {
            profile.path = path.to_string_lossy().into_owned();
        }
    }
}

fn claude_agent_script_is_current(script_path: &str, content: &str) -> bool {
    if !content.contains(CLAUDE_AGENT_SCRIPT_MARKER) {
        return false;
    }
    let is_powershell = Path::new(script_path)
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("ps1"));
    !is_powershell || content.contains(CLAUDE_CLI_RESOLUTION_MARKER)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_codex_builtin_provider_credentials() {
        let credentials = parse_codex_builtin_credentials(
            r#"
model = "gpt-5.5-codex"
model_provider = "work"

[model_providers.work]
base_url = "https://codex.example.com/v1/"
api_key = "sk-codex"
"#,
        );

        assert_eq!(credentials.base_url, "https://codex.example.com/v1");
        assert_eq!(credentials.api_key, "sk-codex");
        assert_eq!(credentials.models, vec!["gpt-5.5-codex"]);
    }

    #[test]
    fn reads_and_writes_codex_api_key_next_to_target_config() {
        let root =
            std::env::temp_dir().join(format!("aeroric-codex-auth-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let config_path = root.join("config.toml");
        std::fs::write(
            root.join("auth.json"),
            r#"{"auth_mode":"chatgpt","tokens":{"access_token":"keep"}}"#,
        )
        .unwrap();

        assert_eq!(read_codex_auth_api_key(&config_path), None);
        write_codex_auth_api_key(&config_path, "sk-target").unwrap();
        assert_eq!(
            read_codex_auth_api_key(&config_path),
            Some("sk-target".to_string())
        );
        let auth: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(root.join("auth.json")).unwrap())
                .unwrap();
        assert_eq!(
            auth.pointer("/tokens/access_token")
                .and_then(|v| v.as_str()),
            Some("keep")
        );
        assert_eq!(
            auth.get("auth_mode").and_then(|v| v.as_str()),
            Some("apikey")
        );

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn detects_codex_api_key_from_auth_without_promoting_oauth_tokens() {
        let root =
            std::env::temp_dir().join(format!("aeroric-codex-detect-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let config_path = root.join("config.toml");
        std::fs::write(
            root.join("auth.json"),
            r#"{"auth_mode":"chatgpt","tokens":{"access_token":"oauth-token"}}"#,
        )
        .unwrap();

        let credentials = detect_builtin_agent_credentials_with_env(
            &AppSettings::default(),
            "codex",
            &config_path,
            "model = \"gpt-5.6\"\n",
            &|_| None,
        );
        assert_eq!(credentials.api_key, "");
        assert_eq!(credentials.models, vec!["gpt-5.6"]);

        std::fs::write(
            root.join("auth.json"),
            r#"{"auth_mode":"apikey","OPENAI_API_KEY":"sk-detected"}"#,
        )
        .unwrap();
        let credentials = detect_builtin_agent_credentials_with_env(
            &AppSettings::default(),
            "codex",
            &config_path,
            "",
            &|_| None,
        );
        assert_eq!(credentials.api_key, "sk-detected");

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn claude_credentials_parser_ignores_oauth_tokens() {
        let oauth = parse_claude_credentials_file(
            r#"{"claudeAiOauth":{"accessToken":"oauth-token","refreshToken":"refresh"}}"#,
        );
        assert_eq!(oauth.api_key, "");

        let api = parse_claude_credentials_file(
            r#"{"apiKey":"sk-claude","baseUrl":"https://api.example.com/v1/"}"#,
        );
        assert_eq!(api.api_key, "sk-claude");
        assert_eq!(api.base_url, "https://api.example.com/v1");
    }

    #[test]
    fn builds_codex_agent_script_without_chat_bridge_by_default() {
        let draft = AgentSetupDraft {
            id: "gpt55".to_string(),
            label: "GPT55".to_string(),
            kind: AgentSetupKind::Codex,
            base_url: "https://example.com/v1/".to_string(),
            api_key: "sk-test".to_string(),
            model: "gpt-5.6".to_string(),
            models: vec!["gpt-5.6".to_string(), "gpt-5.6-sol".to_string()],
            enable_1m_context: false,
            enable_chat_completions_proxy: false,
            dsh_api_protocol: String::new(),
            proxy_enabled: false,
        };

        let script = build_agent_script(&draft);

        assert!(script.contains("CODEX_HOME"));
        assert!(script.contains(CODEX_AGENT_SCRIPT_MARKER));
        assert!(script.contains(
            "printf 'base_url = \"%s\"\\n' \"${OPENAI_BASE_URL:-https://example.com/v1}\""
        ));
        assert!(!script.contains(CODEX_CHAT_PROXY_MARKER));
        assert!(!script.contains("codex-chat-proxy.py"));
        assert!(script.contains("selected_model='gpt-5.6'"));
        assert!(script.contains("printf 'model = \"%s\"\\n' \"$selected_model\""));
        assert!(script.contains("model_catalog_json = \"model-catalog.json\""));
        assert!(script.contains("\"slug\": \"gpt-5.6-sol\""));
        assert!(script.contains("env_key = \"OPENAI_API_KEY\""));
        assert!(script.contains("API_KEY_FILE=\"${AERORIC_AGENT_API_KEY_FILE:-$HOME/.aeroric/agent-credentials/gpt55}\""));
        assert!(!script.contains("existing_no_proxy="));
        assert!(!script.contains("sk-test"));
    }

    /// 没有显式设置过推理强度的 Agent 一律默认 xhigh；模型目录必须同时声明该等级，
    /// 否则 Codex 会拒绝这个取值。
    #[test]
    fn new_agents_default_to_xhigh_reasoning_effort() {
        let draft = AgentSetupDraft {
            id: "defaults".to_string(),
            label: "Defaults".to_string(),
            kind: AgentSetupKind::Codex,
            base_url: "https://example.com/v1/".to_string(),
            api_key: "sk-test".to_string(),
            model: "gpt-5.6-sol".to_string(),
            models: vec!["gpt-5.6-sol".to_string()],
            enable_1m_context: false,
            enable_chat_completions_proxy: false,
            dsh_api_protocol: String::new(),
            proxy_enabled: false,
        };

        let config = codex_config_for_draft(&draft);
        assert!(
            config.contains(r#"model_reasoning_effort = "xhigh""#),
            "unexpected config: {config}"
        );

        // 目录既可能来自 `codex debug models --bundled`，也可能来自内置兜底；
        // 两条路径都必须声明 xhigh，否则 Codex 会拒绝上面写入的取值。
        let script = build_agent_script(&draft);
        assert!(
            script.contains("\"effort\": \"xhigh\""),
            "catalog missing xhigh"
        );
        let fallback = fallback_codex_model("custom-model", 0);
        assert_eq!(
            fallback
                .get("default_reasoning_level")
                .and_then(|v| v.as_str()),
            Some("xhigh")
        );
        assert!(fallback
            .get("supported_reasoning_levels")
            .and_then(|levels| levels.as_array())
            .is_some_and(|levels| levels
                .iter()
                .any(
                    |level| level.get("effort").and_then(|effort| effort.as_str()) == Some("xhigh")
                )));
    }

    #[cfg(not(windows))]
    #[test]
    fn builds_codex_agent_script_with_chat_completions_bridge() {
        let draft = AgentSetupDraft {
            id: "gpt55".to_string(),
            label: "GPT55".to_string(),
            kind: AgentSetupKind::Codex,
            base_url: "https://example.com/v1/".to_string(),
            api_key: "sk-test".to_string(),
            model: "gpt-5.6".to_string(),
            models: vec!["gpt-5.6".to_string(), "gpt-5.6-sol".to_string()],
            enable_1m_context: false,
            enable_chat_completions_proxy: true,
            dsh_api_protocol: String::new(),
            proxy_enabled: false,
        };

        let script = build_agent_script(&draft);

        assert!(script.contains("export AERORIC_UPSTREAM_BASE_URL='https://example.com/v1'"));
        assert!(script.contains("printf 'base_url = \"http://127.0.0.1:%s/v1\"\\n'"));
        assert!(!script.contains("base_url = \"https://example.com/v1\""));
        assert!(script.contains(CODEX_CHAT_PROXY_MARKER));
        assert!(script.contains("codex-chat-proxy.py"));
        assert!(
            script.contains(r#"export NO_PROXY="${existing_no_proxy},127.0.0.1,localhost,::1""#)
        );
        assert!(script.contains("export no_proxy=\"$NO_PROXY\""));
    }

    #[test]
    fn builds_native_windows_powershell_agent_launchers() {
        let codex = AgentSetupDraft {
            id: "gpt55".to_string(),
            label: "GPT55".to_string(),
            kind: AgentSetupKind::Codex,
            base_url: "https://example.com/v1/".to_string(),
            api_key: "sk'test".to_string(),
            model: "gpt-5.6".to_string(),
            models: vec!["gpt-5.6".to_string()],
            enable_1m_context: false,
            enable_chat_completions_proxy: true,
            dsh_api_protocol: String::new(),
            proxy_enabled: false,
        };
        let codex_script = build_codex_agent_powershell_script(&codex);
        assert!(codex_script.contains("$env:CODEX_HOME"));
        assert!(codex_script.contains("agent-credentials\\gpt55"));
        assert!(!codex_script.contains("sk''test"));
        assert!(codex_script.contains("Start-Process"));
        assert!(codex_script.contains("codex-chat-proxy.py"));
        assert!(codex_script.contains("$localProxyBypass = '127.0.0.1,localhost,::1'"));
        assert!(codex_script.contains("$env:no_proxy = $env:NO_PROXY"));
        assert!(codex_script.contains(" @args"));
        assert!(codex_script.contains("# AERORIC_RECOVERY selected_model='gpt-5.6'"));

        let claude = AgentSetupDraft {
            id: "agentrouter".to_string(),
            label: "AgentRouter".to_string(),
            kind: AgentSetupKind::ClaudeCode,
            base_url: "https://agentrouter.org".to_string(),
            api_key: "sk'test".to_string(),
            model: "claude-opus-4-8".to_string(),
            models: vec!["claude-opus-4-8".to_string()],
            enable_1m_context: true,
            enable_chat_completions_proxy: false,
            dsh_api_protocol: String::new(),
            proxy_enabled: false,
        };
        let claude_script = build_claude_code_agent_powershell_script(&claude);
        assert!(claude_script.contains("$env:CLAUDE_CONFIG_DIR"));
        assert!(claude_script.contains(CLAUDE_AGENT_SCRIPT_MARKER));
        assert!(claude_script.contains(CLAUDE_CLI_RESOLUTION_MARKER));
        assert!(claude_script.contains("agent-credentials\\agentrouter"));
        assert!(!claude_script.contains("sk''test"));
        assert!(
            claude_script.contains("Get-Command 'claude' -CommandType Application,ExternalScript")
        );
        assert!(claude_script.contains("%ProgramFiles%\\nodejs"));
        assert!(claude_script.contains("$env:PATH = \"$nodeDirectory;$env:PATH\""));
        assert!(
            claude_script.contains("%USERPROFILE%\\.aeroric\\tools\\claude\\current\\claude.exe")
        );
        assert!(claude_script.contains("%APPDATA%\\npm\\claude.cmd"));
        assert!(claude_script.contains("%APPDATA%\\npm\\claude.ps1"));
        assert!(claude_script.contains("npmExecutable prefix -g"));
        assert!(claude_script.contains("AERORIC_CLAUDE_CLI_NOT_FOUND"));
        assert!(claude_script.contains("$selectedModel += '[1m]'"));
        assert!(claude_script.contains("--model $selectedModel @args"));
        assert!(!claude_script.contains("& 'claude'"));
    }

    #[test]
    fn powershell_claude_resolution_prefers_configured_paths_and_supports_shims() {
        let block = powershell_claude_resolution_block(r"C:\Program Files\Claude\claude.exe");

        assert!(block.contains("$configuredClaude = 'C:\\Program Files\\Claude\\claude.exe'"));
        assert!(block.contains("Test-Path -LiteralPath $configuredClaude -PathType Leaf"));
        assert!(block.contains("Resolve-Path -LiteralPath $configuredClaude"));
        assert!(block.contains("Application,ExternalScript"));
        assert!(block.contains("claude.exe"));
        assert!(block.contains("claude.cmd"));
        assert!(block.contains("claude.ps1"));
        assert!(block.contains("AERORIC_CLAUDE_CLI_NOT_FOUND"));
    }

    #[test]
    fn powershell_claude_wrapper_requires_the_resolution_capability_marker() {
        let path = r"C:\Users\test\.aeroric\agents\deepseek_claude.ps1";
        let incomplete = format!("{CLAUDE_AGENT_SCRIPT_MARKER}\n& 'claude' @args\n");
        let current = format!(
            "{CLAUDE_AGENT_SCRIPT_MARKER}\n{CLAUDE_CLI_RESOLUTION_MARKER}\n& $claudeExecutable @args\n"
        );

        assert!(!claude_agent_script_is_current(path, &incomplete));
        assert!(claude_agent_script_is_current(path, &current));
        assert!(claude_agent_script_is_current(
            "/tmp/deepseek_claude.sh",
            CLAUDE_AGENT_SCRIPT_MARKER
        ));
    }

    #[test]
    fn codex_model_catalog_contains_only_selected_models() {
        let bundled = serde_json::json!({
            "models": [
                {
                    "slug": "gpt-5.5",
                    "display_name": "GPT-5.5",
                    "description": "Bundled model",
                    "default_reasoning_level": "medium",
                    "supported_reasoning_levels": [],
                    "shell_type": "shell_command",
                    "visibility": "list",
                    "supported_in_api": true,
                    "priority": 0,
                    "upgrade": null,
                    "base_instructions": "bundled instructions",
                    "supports_reasoning_summaries": true,
                    "default_reasoning_summary": "none",
                    "support_verbosity": true,
                    "default_verbosity": "low",
                    "apply_patch_tool_type": "freeform",
                    "web_search_tool_type": "text_and_image",
                    "truncation_policy": { "mode": "tokens", "limit": 10000 },
                    "supports_parallel_tool_calls": true,
                    "context_window": 272000,
                    "experimental_supported_tools": [],
                    "input_modalities": ["text", "image"],
                    "supports_search_tool": true
                },
                {
                    "slug": "gpt-5.3",
                    "display_name": "GPT-5.3",
                    "description": "Unselected model"
                }
            ]
        })
        .to_string();
        let selected = vec!["gpt-5.6-sol".to_string(), "gpt-5.5".to_string()];

        let catalog = build_codex_model_catalog(&selected, Some(&bundled));
        let value: serde_json::Value = serde_json::from_str(&catalog).unwrap();
        let models = value["models"].as_array().unwrap();

        assert_eq!(models.len(), 2);
        assert_eq!(models[0]["slug"], "gpt-5.6-sol");
        assert_eq!(models[1]["slug"], "gpt-5.5");
        assert_eq!(models[0]["base_instructions"], "bundled instructions");
        assert!(!catalog.contains("gpt-5.3"));
    }

    #[cfg(not(windows))]
    #[test]
    fn builds_claude_code_agent_script_with_anthropic_env() {
        let draft = AgentSetupDraft {
            id: "agentrouter".to_string(),
            label: "AgentRouter".to_string(),
            kind: AgentSetupKind::ClaudeCode,
            base_url: "https://agentrouter.org".to_string(),
            api_key: "sk-test".to_string(),
            model: "claude-opus-4-8".to_string(),
            models: vec!["claude-opus-4-8".to_string(), "claude-opus-4-6".to_string()],
            enable_1m_context: true,
            enable_chat_completions_proxy: false,
            dsh_api_protocol: String::new(),
            proxy_enabled: false,
        };

        let script = build_agent_script(&draft);

        assert!(script.contains("CLAUDE_CONFIG_DIR"));
        assert!(script.contains("export ANTHROPIC_BASE_URL='https://agentrouter.org'"));
        assert!(script.contains("export ANTHROPIC_AUTH_TOKEN=\"$api_key\""));
        assert!(!script.contains("sk-test"));
        assert!(!script.contains("export ANTHROPIC_API_KEY"));
        assert!(script.contains("selected_model='claude-opus-4-8'"));
        assert!(script.contains("selected_model=\"${selected_model}[1m]\""));
        assert!(script.contains("exec claude --model \"$selected_model\" \"$@\""));
    }

    #[test]
    fn builds_claude_code_agent_script_without_1m_suffix_when_disabled() {
        let draft = AgentSetupDraft {
            id: "agentrouter".to_string(),
            label: "AgentRouter".to_string(),
            kind: AgentSetupKind::ClaudeCode,
            base_url: "https://agentrouter.org".to_string(),
            api_key: "sk-test".to_string(),
            model: "claude-opus-4-6".to_string(),
            models: vec!["claude-opus-4-6".to_string()],
            enable_1m_context: false,
            enable_chat_completions_proxy: false,
            dsh_api_protocol: String::new(),
            proxy_enabled: false,
        };

        let script = build_agent_script(&draft);

        assert!(script.contains(CLAUDE_AGENT_SCRIPT_MARKER));
        assert!(!script.contains("selected_model=\"${selected_model}[1m]\""));
    }

    #[cfg(not(windows))]
    #[test]
    fn custom_agent_script_model_selection_is_non_interactive() {
        let draft = AgentSetupDraft {
            id: "gpt55".to_string(),
            label: "GPT55".to_string(),
            kind: AgentSetupKind::Codex,
            base_url: "https://example.com/v1/".to_string(),
            api_key: "sk-test".to_string(),
            model: "gpt-5.6".to_string(),
            models: vec!["gpt-5.6".to_string(), "gpt-5.6-sol".to_string()],
            enable_1m_context: false,
            enable_chat_completions_proxy: false,
            dsh_api_protocol: String::new(),
            proxy_enabled: false,
        };

        let script = build_agent_script(&draft);

        assert!(script.contains("selected_model=\"${AERORIC_AGENT_MODEL:-}\""));
        assert!(script.contains("selected_model='gpt-5.6'"));
        assert!(!script.contains("read -r -p"));
        assert!(!script.contains("请选择模型"));
        assert!(!script.contains("已选择"));
        assert!(!script.contains("AERORIC_AGENT_MODEL_CHOICE"));
    }

    #[test]
    fn recovers_generated_agent_credentials_from_scripts() {
        let dir =
            std::env::temp_dir().join(format!("aeroric-agent-recover-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        let codex_path = dir.join("codex.sh");
        fs::write(
            &codex_path,
            "export ANTHROPIC_API_KEY='sk-test'\nbase_url = \"https://example.com/v1\"\n",
        )
        .unwrap();
        let mut profile = CustomAgentProfile {
            id: "custom".to_string(),
            label: "Custom".to_string(),
            path: codex_path.to_string_lossy().into_owned(),
            codex_like: true,
            family: String::new(),
            config_lang: "shellscript".to_string(),
            base_url: String::new(),
            api_key: String::new(),
            models: Vec::new(),
            enable_1m_context: false,
            enable_chat_completions_proxy: false,
            username: String::new(),
            password: String::new(),
        };

        recover_custom_agent_credentials(&mut profile);

        assert_eq!(profile.base_url, "https://example.com/v1");
        assert_eq!(profile.api_key, "sk-test");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn recovers_generated_agent_model_from_script_default() {
        let dir = std::env::temp_dir().join(format!(
            "aeroric-agent-model-recover-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&dir).unwrap();
        let script_path = dir.join("agent.sh");
        fs::write(
            &script_path,
            r#"selected_model="${AERORIC_AGENT_MODEL:-}"
if [ -z "$selected_model" ]; then
  selected_model='GLM-5.2'
fi
"#,
        )
        .unwrap();
        let mut profile = CustomAgentProfile {
            id: "custom".to_string(),
            label: "Custom".to_string(),
            path: script_path.to_string_lossy().into_owned(),
            codex_like: false,
            family: String::new(),
            config_lang: "shellscript".to_string(),
            base_url: String::new(),
            api_key: String::new(),
            models: Vec::new(),
            enable_1m_context: false,
            enable_chat_completions_proxy: false,
            username: String::new(),
            password: String::new(),
        };

        recover_custom_agent_models(&mut profile);

        assert_eq!(profile.models, vec!["GLM-5.2"]);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn recovers_chat_proxy_upstream_credentials_from_generated_script() {
        let dir = std::env::temp_dir().join(format!(
            "aeroric-codex-proxy-credentials-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&dir).unwrap();
        let script_path = dir.join("proxy.sh");
        fs::write(
            &script_path,
            "#!/bin/bash\nexport OPENAI_API_KEY='sk-test'\nexport AERORIC_UPSTREAM_BASE_URL='https://example.com/v1'\n",
        )
        .unwrap();
        let mut profile = CustomAgentProfile {
            id: "proxy".to_string(),
            label: "Proxy".to_string(),
            path: script_path.to_string_lossy().into_owned(),
            codex_like: true,
            family: String::new(),
            config_lang: "shellscript".to_string(),
            base_url: String::new(),
            api_key: String::new(),
            models: vec!["gpt-5.6".to_string()],
            enable_1m_context: false,
            enable_chat_completions_proxy: true,
            username: String::new(),
            password: String::new(),
        };

        recover_custom_agent_credentials(&mut profile);

        assert_eq!(profile.base_url, "https://example.com/v1");
        assert_eq!(profile.api_key, "sk-test");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn recognizes_legacy_aeroric_codex_wrappers() {
        let script = r#"#!/bin/bash
export CODEX_HOME="$AGENT_HOME"
printf 'model_catalog_json = "model-catalog.json"\n'
"#;

        assert!(is_aeroric_codex_wrapper(script));
        assert!(!is_aeroric_codex_chat_proxy_wrapper(script));
    }

    #[test]
    fn refreshes_stale_codex_agent_scripts_to_chat_bridge() {
        let dir =
            std::env::temp_dir().join(format!("aeroric-codex-refresh-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        let script_path = dir.join("liwan.sh");
        fs::write(
            &script_path,
            "#!/bin/bash\n# AERORIC_CODEX_CHAT_PROXY_VERSION=2\n",
        )
        .unwrap();
        let mut settings = AppSettings {
            custom_agents: vec![CustomAgentProfile {
                id: "liwan".to_string(),
                label: "liwan".to_string(),
                path: script_path.to_string_lossy().into_owned(),
                codex_like: true,
                family: String::new(),
                config_lang: "shellscript".to_string(),
                base_url: "https://metapi.example/v1".to_string(),
                api_key: "sk-test".to_string(),
                models: vec!["gpt-5.6-sol".to_string()],
                enable_1m_context: false,
                enable_chat_completions_proxy: true,
                username: String::new(),
                password: String::new(),
            }],
            ..AppSettings::default()
        };

        refresh_stale_codex_agent_scripts(&mut settings);

        assert!(settings.custom_agents[0].enable_chat_completions_proxy);
        let script = fs::read_to_string(&script_path).unwrap();
        assert!(script.contains(CODEX_CHAT_PROXY_MARKER));
        assert!(script.contains("export AERORIC_UPSTREAM_BASE_URL='https://metapi.example/v1'"));
        assert!(script.contains("codex-chat-proxy.py"));
        assert!(script.contains("export no_proxy=\"$NO_PROXY\""));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn resets_codex_chat_bridge_wrappers_when_setting_is_off() {
        let dir =
            std::env::temp_dir().join(format!("aeroric-codex-unbridge-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        let script_path = dir.join("muyuan.sh");
        let bridged = build_codex_agent_script(&AgentSetupDraft {
            id: "muyuan".to_string(),
            label: "muyuan".to_string(),
            kind: AgentSetupKind::Codex,
            base_url: "https://muyuan.example/v1".to_string(),
            api_key: "sk-test".to_string(),
            model: "gpt-5.6-sol".to_string(),
            models: vec!["gpt-5.6-sol".to_string()],
            enable_1m_context: false,
            enable_chat_completions_proxy: true,
            dsh_api_protocol: String::new(),
            proxy_enabled: false,
        });
        fs::write(&script_path, &bridged).unwrap();
        let mut settings = AppSettings {
            custom_agents: vec![CustomAgentProfile {
                id: "muyuan".to_string(),
                label: "muyuan".to_string(),
                path: script_path.to_string_lossy().into_owned(),
                codex_like: true,
                family: String::new(),
                config_lang: "shellscript".to_string(),
                base_url: "https://muyuan.example/v1".to_string(),
                api_key: "sk-test".to_string(),
                models: vec!["gpt-5.6-sol".to_string()],
                enable_1m_context: false,
                enable_chat_completions_proxy: false,
                username: String::new(),
                password: String::new(),
            }],
            ..AppSettings::default()
        };

        refresh_stale_codex_agent_scripts(&mut settings);

        assert!(!settings.custom_agents[0].enable_chat_completions_proxy);
        let script = fs::read_to_string(&script_path).unwrap();
        assert!(script.contains(CODEX_AGENT_SCRIPT_MARKER));
        assert!(!script.contains(CODEX_CHAT_PROXY_MARKER));
        assert!(!script.contains("codex-chat-proxy.py"));
        assert!(script.contains(
            "printf 'base_url = \"%s\"\\n' \"${OPENAI_BASE_URL:-https://muyuan.example/v1}\""
        ));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn preserves_user_authored_codex_shell_scripts_during_refresh() {
        let dir =
            std::env::temp_dir().join(format!("aeroric-codex-preserve-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        let script_path = dir.join("custom.sh");
        let original = "#!/bin/bash\necho custom-codex-wrapper\n";
        fs::write(&script_path, original).unwrap();
        let mut settings = AppSettings {
            custom_agents: vec![CustomAgentProfile {
                id: "custom".to_string(),
                label: "Custom".to_string(),
                path: script_path.to_string_lossy().into_owned(),
                codex_like: true,
                family: String::new(),
                config_lang: "shellscript".to_string(),
                base_url: "https://example.com/v1".to_string(),
                api_key: "sk-test".to_string(),
                models: vec!["gpt-5.6".to_string()],
                enable_1m_context: false,
                enable_chat_completions_proxy: false,
                username: String::new(),
                password: String::new(),
            }],
            ..AppSettings::default()
        };

        refresh_stale_codex_agent_scripts(&mut settings);

        assert_eq!(fs::read_to_string(&script_path).unwrap(), original);
        assert!(!settings.custom_agents[0].enable_chat_completions_proxy);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn refreshes_stale_claude_agent_scripts() {
        let dir =
            std::env::temp_dir().join(format!("aeroric-claude-refresh-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        let script_path = dir.join("agentrouter.sh");
        fs::write(
            &script_path,
            "#!/bin/bash\n# AERORIC_CLAUDE_WRAPPER_VERSION=5\nset -euo pipefail\nAGENT_HOME=\"$HOME/.aeroric/agent-homes/agentrouter\"\nexport CLAUDE_CONFIG_DIR=\"$AGENT_HOME\"\nexport CLAUDE_CODE_SESSION_ENV_DIR=\"$AGENT_HOME/session-env\"\nexport ANTHROPIC_AUTH_TOKEN='sk-test'\nexport ANTHROPIC_API_KEY=\"$ANTHROPIC_AUTH_TOKEN\"\nexec claude \"$@\"\n",
        )
        .unwrap();
        let mut settings = AppSettings {
            custom_agents: vec![CustomAgentProfile {
                id: "agentrouter".to_string(),
                label: "AgentRouter".to_string(),
                path: script_path.to_string_lossy().into_owned(),
                codex_like: false,
                family: String::new(),
                config_lang: "shellscript".to_string(),
                base_url: "https://agentrouter.org".to_string(),
                api_key: "sk-test".to_string(),
                models: vec!["claude-opus-4-6".to_string()],
                enable_1m_context: true,
                enable_chat_completions_proxy: false,
                username: String::new(),
                password: String::new(),
            }],
            ..AppSettings::default()
        };

        refresh_stale_claude_agent_scripts(&mut settings);

        let script = fs::read_to_string(&script_path).unwrap();
        assert!(script.contains(CLAUDE_AGENT_SCRIPT_MARKER));
        assert!(!script.contains("# AERORIC_CLAUDE_WRAPPER_VERSION=5"));
        assert!(script.contains("selected_model=\"${selected_model}[1m]\""));
        assert!(!script.contains("export ANTHROPIC_API_KEY"));
        let _ = fs::remove_dir_all(dir);
    }

    #[cfg(windows)]
    #[test]
    fn refreshes_legacy_powershell_claude_wrappers_after_resolution_fix() {
        let dir = std::env::temp_dir().join(format!(
            "aeroric-claude-powershell-refresh-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&dir).unwrap();
        let script_path = dir.join("deepseek_claude.ps1");
        fs::write(
            &script_path,
            "# AERORIC_CLAUDE_WRAPPER_VERSION=5\n& 'claude' @args\n",
        )
        .unwrap();
        let mut settings = AppSettings {
            custom_agents: vec![CustomAgentProfile {
                id: "deepseek_claude".to_string(),
                label: "DeepSeek Claude".to_string(),
                path: script_path.to_string_lossy().into_owned(),
                codex_like: false,
                family: String::new(),
                config_lang: "shellscript".to_string(),
                base_url: "https://example.com".to_string(),
                api_key: "sk-test".to_string(),
                models: vec!["claude-sonnet".to_string()],
                enable_1m_context: false,
                enable_chat_completions_proxy: false,
                username: String::new(),
                password: String::new(),
            }],
            ..AppSettings::default()
        };

        refresh_stale_claude_agent_scripts(&mut settings);

        let script = fs::read_to_string(&script_path).unwrap();
        assert!(script.contains(CLAUDE_AGENT_SCRIPT_MARKER));
        assert!(script.contains("$claudeExecutable"));
        assert!(!script.contains("& 'claude' @args"));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn preserves_user_authored_claude_shell_scripts_during_refresh() {
        let dir =
            std::env::temp_dir().join(format!("aeroric-claude-preserve-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        let script_path = dir.join("custom.sh");
        let original = "#!/bin/bash\nexport ANTHROPIC_AUTH_TOKEN='custom'\necho custom-wrapper\n";
        fs::write(&script_path, original).unwrap();
        let mut settings = AppSettings {
            custom_agents: vec![CustomAgentProfile {
                id: "custom".to_string(),
                label: "Custom".to_string(),
                path: script_path.to_string_lossy().into_owned(),
                codex_like: false,
                family: String::new(),
                config_lang: "shellscript".to_string(),
                base_url: "https://example.com".to_string(),
                api_key: "sk-test".to_string(),
                models: vec!["claude-opus-4-6".to_string()],
                enable_1m_context: false,
                enable_chat_completions_proxy: false,
                username: String::new(),
                password: String::new(),
            }],
            ..AppSettings::default()
        };

        refresh_stale_claude_agent_scripts(&mut settings);

        assert_eq!(fs::read_to_string(&script_path).unwrap(), original);
        assert_eq!(
            settings.custom_agents[0].path,
            script_path.to_string_lossy()
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn builtin_claude_model_aliases_are_available_for_model_dropdowns() {
        assert_eq!(
            claude_builtin_model_aliases(),
            vec!["fable", "opus", "sonnet"]
        );
    }

    #[test]
    fn removes_agent_profile_file_but_refuses_directories() {
        let dir = std::env::temp_dir().join(format!("aeroric-agent-delete-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let script = dir.join("agent.sh");
        fs::write(&script, "#!/bin/sh\n").unwrap();

        remove_agent_profile_file(&script.to_string_lossy()).unwrap();
        assert!(!script.exists());

        let directory_result = remove_agent_profile_file(&dir.to_string_lossy());
        assert!(directory_result
            .unwrap_err()
            .contains("Refusing to delete directory"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn makes_user_agent_script_executable_when_possible() {
        use std::io::Write;
        use std::os::unix::fs::PermissionsExt;

        let dir = std::env::temp_dir().join(format!("aeroric-agent-exec-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let script = dir.join("agent.sh");
        let mut file = fs::File::create(&script).unwrap();
        writeln!(file, "#!/bin/sh").unwrap();
        writeln!(file, "echo ok").unwrap();
        fs::set_permissions(&script, fs::Permissions::from_mode(0o644)).unwrap();

        ensure_user_agent_script_executable(&script).unwrap();

        let mode = fs::metadata(&script).unwrap().permissions().mode();
        assert_ne!(mode & 0o100, 0);
        let _ = fs::remove_dir_all(&dir);
    }
}
