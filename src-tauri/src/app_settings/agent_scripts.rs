use super::*;

/// 把 Aeroric 的 codex hook 片段追加进本 Agent 隔离 `CODEX_HOME` 的 config.toml。
///
/// 自定义 Agent 读不到 `~/.codex/config.toml`,而本脚本每次启动都整体重写自己的
/// config,所以必须在这里追加。片段路径固定、内容由桌面端维护(node 路径变化
/// 不需要重刷脚本);文件缺失时静默跳过,hook 脚本本身在缺少 AERORIC_TASK_ID 时
/// 也会零副作用退出,故用户手动运行本脚本同样安全。
#[cfg(not(windows))]
const CODEX_HOOKS_FRAGMENT_SHELL: &str = r#"aeroric_hooks_fragment="${AERORIC_CODEX_HOOKS_FRAGMENT:-$HOME/.aeroric/hooks/codex-hooks.toml}"
if [ -r "$aeroric_hooks_fragment" ]; then
  printf '\n' >> "$CODEX_HOME/config.toml"
  cat -- "$aeroric_hooks_fragment" >> "$CODEX_HOME/config.toml"
fi
"#;

/// PowerShell 版本,语义同 `CODEX_HOOKS_FRAGMENT_SHELL`。
#[cfg(any(windows, test))]
const CODEX_HOOKS_FRAGMENT_POWERSHELL: &str = r#"$aeroricHooksFragment = if ($env:AERORIC_CODEX_HOOKS_FRAGMENT) { $env:AERORIC_CODEX_HOOKS_FRAGMENT } else { Join-Path $HOME '.aeroric/hooks/codex-hooks.toml' }
if (Test-Path -LiteralPath $aeroricHooksFragment) {
  Add-Content -LiteralPath (Join-Path $env:CODEX_HOME 'config.toml') -Value ([Environment]::NewLine + (Get-Content -LiteralPath $aeroricHooksFragment -Raw))
}
"#;

/// Chat Completions bridge 对 Python 的最低要求。`codex_chat_proxy.py` 用了 PEP 585
/// 的 `dict[str, Any]` 作模块级别名,3.8 及更早会在 import 期直接 TypeError。
///
/// 这个下限同时被三处引用:桌面端预检、生成的 bash 脚本、生成的 PowerShell 脚本。
/// 三处必须一致,否则预检说"可用"而启动仍然失败。
pub(super) const CHAT_BRIDGE_PYTHON_MIN_MINOR: u32 = 9;

/// 自动探测时的候选顺序。必须按这个顺序逐个实跑,不能依赖 `Get-Command a, b, c`
/// 的返回顺序——它不保证按入参排序,可能先给出没装运行时的 `py.exe`。
pub(super) const CHAT_BRIDGE_PYTHON_CANDIDATES: &[&str] = &["python3", "python", "py"];

/// 一个 Python 候选的探测结论。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ChatBridgePythonProbe {
    /// 解释器的可执行路径(自动探测时是解析后的结果)。
    pub program: String,
    /// 需要追加在脚本参数之前的固定参数,目前只有 `py -3`。
    pub leading_args: Vec<String>,
    /// 探到的版本,形如 `3.12`。
    pub version: Option<String>,
    /// 不可用的原因;`None` 表示这个候选可用。
    pub failure: Option<String>,
}

impl ChatBridgePythonProbe {
    pub fn is_usable(&self) -> bool {
        self.failure.is_none()
    }
}

/// 实跑一个解释器并判断它能不能承载 bridge。
///
/// 只看"文件存在"或"命令能找到"是不够的:Windows 预置的 Microsoft Store 别名桩
/// (`%LOCALAPPDATA%\Microsoft\WindowsApps\python*.exe`)存在且能被 `where` 找到,
/// 但一运行就跳商店并以非零码退出。所以这里必须真的执行一次取版本号。
pub(super) fn probe_chat_bridge_python_program(program: &str) -> ChatBridgePythonProbe {
    let leading_args: Vec<String> = if program_is_py_launcher(program) {
        vec!["-3".to_string()]
    } else {
        Vec::new()
    };
    let mut probe = ChatBridgePythonProbe {
        program: program.to_string(),
        leading_args: leading_args.clone(),
        version: None,
        failure: None,
    };

    let mut command = Command::new(program);
    crate::subprocess::configure_background_command(&mut command);
    command
        .args(&leading_args)
        .arg("-c")
        .arg("import sys; print(sys.version_info[0], sys.version_info[1])")
        .env("PATH", get_login_shell_path())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let output = match command.output() {
        Ok(output) => output,
        Err(error) => {
            probe.failure = Some(error.to_string());
            return probe;
        }
    };
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if !output.status.success() {
        let detail = if !stderr.is_empty() {
            stderr
        } else if !stdout.is_empty() {
            stdout
        } else {
            format!("exited with status {}", output.status)
        };
        probe.failure = Some(detail);
        return probe;
    }

    let Some((major, minor)) = parse_python_version_probe(&stdout) else {
        let detail = if stdout.is_empty() { stderr } else { stdout };
        probe.failure = Some(format!("unexpected version output: {detail}"));
        return probe;
    };
    probe.version = Some(format!("{major}.{minor}"));
    if major < 3 || (major == 3 && minor < CHAT_BRIDGE_PYTHON_MIN_MINOR) {
        probe.failure = Some(format!(
            "Python {major}.{minor} is too old (need 3.{CHAT_BRIDGE_PYTHON_MIN_MINOR}+)"
        ));
    }
    probe
}

fn program_is_py_launcher(program: &str) -> bool {
    let file_name = Path::new(program)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(program);
    let stem = file_name
        .rsplit_once('.')
        .map(|(stem, _)| stem)
        .unwrap_or(file_name);
    stem.eq_ignore_ascii_case("py")
}

/// 解析 `print(sys.version_info[0], sys.version_info[1])` 的输出。
///
/// 只认最后一行的两个整数:某些环境会在 stdout 前面插入告警,取最后一行最稳。
fn parse_python_version_probe(text: &str) -> Option<(u32, u32)> {
    text.lines().rev().find_map(|line| {
        let mut parts = line.split_whitespace();
        let major = parts.next()?.parse::<u32>().ok()?;
        let minor = parts.next()?.parse::<u32>().ok()?;
        if parts.next().is_some() {
            return None;
        }
        Some((major, minor))
    })
}

/// 按候选顺序找出第一个可用的解释器;全都不可用时返回逐个候选的失败原因。
pub(super) fn resolve_chat_bridge_python(
) -> Result<ChatBridgePythonProbe, Vec<ChatBridgePythonProbe>> {
    let mut failures = Vec::new();
    for candidate in CHAT_BRIDGE_PYTHON_CANDIDATES {
        let resolved = detect_path(candidate);
        let program = if resolved.is_empty() {
            (*candidate).to_string()
        } else {
            resolved
        };
        let probe = probe_chat_bridge_python_program(&program);
        if probe.is_usable() {
            return Ok(probe);
        }
        failures.push(probe);
    }
    Err(failures)
}

/// 启动 Responses→Chat Completions bridge(PowerShell)。
///
/// 版本下限 3.9:`codex_chat_proxy.py` 用了 PEP 585 的 `dict[str, Any]` 作模块级
/// 别名,3.8 及更早会在 import 期直接 TypeError。
///
/// 三处硬性要求,少一处都会在干净的 Windows 机器上表现为 bridge 启动失败:
/// 1. 必须实际探测 `--version`。Windows 预置的 Microsoft Store 别名桩
///    (`%LOCALAPPDATA%\Microsoft\WindowsApps\python*.exe`)能被 `Get-Command`
///    找到,但一运行就跳商店并以非零码退出,只判断"命令存在"必然误判。
/// 2. 候选必须按 python3→python→py 定序自己遍历。`Get-Command a, b, c` 不保证
///    按入参顺序返回,`Select-Object -First 1` 可能先拿到没装运行时的 py.exe。
/// 3. 等待窗口要够长,并在失败时带上 bridge 日志。首次冷启动叠加 Defender 实时
///    扫描,Python 解释器起步经常超过原先的 2s 预算。
///
/// 配置了固定解释器时的选择逻辑(PowerShell)。
///
/// 不回退自动探测:用户明确指定了某个 conda 环境的 Python,却静默换用 PATH 上的另
/// 一个,会让"为什么装的包不生效"变得极难排查。宁可直接报错说这条路径不可用。
#[cfg(any(windows, test))]
const CODEX_CHAT_BRIDGE_CONFIGURED_POWERSHELL: &str = r#"$bridgeInterpreter = $null
$bridgeInterpreterArgs = @()
$configuredBridgePython = BRIDGE_PYTHON_PATH_LITERAL
if ($env:AERORIC_BRIDGE_PYTHON) { $configuredBridgePython = $env:AERORIC_BRIDGE_PYTHON }
$configuredProbe = ''
$configuredExit = -1
$configuredArgs = @()
if ([System.IO.Path]::GetFileNameWithoutExtension($configuredBridgePython) -eq 'py') { $configuredArgs += '-3' }
$previousErrorAction = $ErrorActionPreference
$ErrorActionPreference = 'Continue'
try {
  $configuredProbe = (& $configuredBridgePython @configuredArgs '-c' 'import sys; print(sys.version_info[0], sys.version_info[1])' 2>&1 | Out-String)
  $configuredExit = $LASTEXITCODE
} catch {
  $configuredProbe = $_.Exception.Message
} finally {
  $ErrorActionPreference = $previousErrorAction
}
$configuredProbe = $configuredProbe.Trim()
$configuredMatch = [regex]::Match($configuredProbe, '(?m)^\s*(\d+)\s+(\d+)\s*$')
if ($configuredExit -ne 0 -or -not $configuredMatch.Success) {
  throw ('The Python configured for this agent cannot run the Chat Completions bridge: ' + $configuredBridgePython + [Environment]::NewLine + $configuredProbe)
}
$configuredMajor = [int]$configuredMatch.Groups[1].Value
$configuredMinor = [int]$configuredMatch.Groups[2].Value
if ($configuredMajor -lt 3 -or ($configuredMajor -eq 3 -and $configuredMinor -lt 9)) {
  throw ('The Python configured for this agent is too old for the Chat Completions bridge (need 3.9+): ' + $configuredBridgePython + ' is ' + $configuredMajor + '.' + $configuredMinor)
}
$bridgeInterpreter = $configuredBridgePython
$bridgeInterpreterArgs = $configuredArgs
"#;

#[cfg(any(windows, test))]
const CODEX_CHAT_BRIDGE_AUTODETECT_POWERSHELL: &str = r#"$bridgeInterpreter = $null
$bridgeInterpreterArgs = @()
$bridgeProbeNotes = @()
foreach ($candidateName in @('python3', 'python', 'py')) {
  $candidates = @(Get-Command $candidateName -CommandType Application -ErrorAction SilentlyContinue)
  foreach ($candidate in $candidates) {
    $candidateArgs = @()
    if ($candidate.Name -eq 'py' -or $candidate.Name -eq 'py.exe') { $candidateArgs += '-3' }
    $probeOutput = ''
    $probeExit = -1
    $previousErrorAction = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try {
      $probeOutput = (& $candidate.Source @candidateArgs '-c' 'import sys; print(sys.version_info[0], sys.version_info[1])' 2>&1 | Out-String)
      $probeExit = $LASTEXITCODE
    } catch {
      $probeOutput = $_.Exception.Message
    } finally {
      $ErrorActionPreference = $previousErrorAction
    }
    $probeOutput = $probeOutput.Trim()
    $versionMatch = [regex]::Match($probeOutput, '(?m)^\s*(\d+)\s+(\d+)\s*$')
    if ($probeExit -ne 0 -or -not $versionMatch.Success) {
      $bridgeProbeNotes += ($candidate.Source + ' -> ' + $probeOutput)
      continue
    }
    $probeMajor = [int]$versionMatch.Groups[1].Value
    $probeMinor = [int]$versionMatch.Groups[2].Value
    if ($probeMajor -lt 3 -or ($probeMajor -eq 3 -and $probeMinor -lt 9)) {
      $bridgeProbeNotes += ($candidate.Source + ' -> Python ' + $probeMajor + '.' + $probeMinor + ' is too old')
      continue
    }
    $bridgeInterpreter = $candidate.Source
    $bridgeInterpreterArgs = $candidateArgs
    break
  }
  if ($null -ne $bridgeInterpreter) { break }
}
if ($null -eq $bridgeInterpreter) {
  $bridgeHint = 'This custom Codex agent requires Python 3.9+ to bridge Responses to Chat Completions. Install it from python.org (tick "Add python.exe to PATH"); the Microsoft Store alias stub does not count.'
  if ($bridgeProbeNotes.Count -gt 0) {
    $bridgeHint = $bridgeHint + [Environment]::NewLine + 'Checked: ' + ($bridgeProbeNotes -join '; ')
  }
  throw $bridgeHint
}
"#;

/// 解释器选好之后的启动与等待,两种选择方式共用。
#[cfg(any(windows, test))]
const CODEX_CHAT_BRIDGE_WAIT_POWERSHELL: &str = r#"$proxyLog = Join-Path $agentHome 'codex-chat-proxy.log'
$proxyErrorLog = Join-Path $agentHome 'codex-chat-proxy-err.log'
if (-not $env:AERORIC_PROXY_LOG_LEVEL) { $env:AERORIC_PROXY_LOG_LEVEL = 'INFO' }
$bridgeArgs = $bridgeInterpreterArgs + @(('"' + $proxyScript + '"'), '--port-file', ('"' + $portFile + '"'))
$proxyProcess = Start-Process -FilePath $bridgeInterpreter -ArgumentList $bridgeArgs -PassThru -WindowStyle Hidden -RedirectStandardOutput $proxyLog -RedirectStandardError $proxyErrorLog
$proxyPort = ''
for ($attempt = 0; $attempt -lt 400; $attempt++) {
  $proxyPort = Read-AeroricBridgePort $portFile
  if ($proxyPort) { break }
  if ($proxyProcess.HasExited) { break }
  Start-Sleep -Milliseconds 50
}
if (-not $proxyPort) {
  # 进程可能在最后一次轮询之后才落盘,退出前再确认一次。
  $proxyPort = Read-AeroricBridgePort $portFile
}
if (-not $proxyPort) {
  Stop-Process -Id $proxyProcess.Id -Force -ErrorAction SilentlyContinue
  $bridgeFailure = 'Failed to start the local Chat Completions bridge.'
  $bridgeFailure = $bridgeFailure + [Environment]::NewLine + 'Bridge: ' + $bridgeInterpreter
  if ($proxyProcess.HasExited) {
    $bridgeFailure = $bridgeFailure + ' (exited with code ' + $proxyProcess.ExitCode + ')'
  }
  foreach ($logPath in @($proxyErrorLog, $proxyLog)) {
    $logTail = ''
    try {
      if (Test-Path -LiteralPath $logPath) {
        $logTail = ((Get-Content -LiteralPath $logPath -Tail 20 -ErrorAction SilentlyContinue) -join [Environment]::NewLine).Trim()
      }
    } catch {
      $logTail = ''
    }
    if ($logTail) {
      $bridgeFailure = $bridgeFailure + [Environment]::NewLine + $logPath + ':' + [Environment]::NewLine + $logTail
    }
  }
  throw $bridgeFailure
}"#;

/// 决定 bash 侧要探测哪些解释器候选。
///
/// 配置了固定路径就只试这一个,不把自动探测的候选拼进去——静默回退到 PATH 上的另一个
/// Python 会让"为什么装的包不生效"极难排查。`AERORIC_BRIDGE_PYTHON` 可临时覆盖。
///
/// 路径必须走 `shell_quote`:conda 环境路径常带空格,裸插会被 shell 重新分词。
#[cfg(not(windows))]
fn codex_chat_bridge_configured_python_shell(configured_python: &str) -> String {
    let configured_python = configured_python.trim();
    if configured_python.is_empty() {
        // 仍然读环境变量:未配置时也允许临时指定一个解释器。
        return "configured_python=\"${AERORIC_BRIDGE_PYTHON:-}\"\n\
                if [ -n \"$configured_python\" ]; then\n  \
                python_candidates=(\"$configured_python\")\n\
                else\n  \
                python_candidates=(\"python3\" \"python\")\n\
                fi\n"
            .to_string();
    }
    format!(
        "configured_python={configured}\n\
         if [ -n \"${{AERORIC_BRIDGE_PYTHON:-}}\" ]; then\n  \
         configured_python=\"$AERORIC_BRIDGE_PYTHON\"\n\
         fi\n\
         python_candidates=(\"$configured_python\")\n",
        configured = shell_quote(configured_python),
    )
}

/// 纯版本/帮助探测直接短路,不碰 bridge(PowerShell)。
///
/// 桌面端的 `detect_agent_version` 会用 `--version` 跑这个包装脚本。`codex --version`
/// 既不需要 bridge 也不需要 config.toml,但旧结构会先把 bridge 拉起来——于是没装
/// Python 的机器上版本探测直接失败,装了的机器上也要白等一次解释器冷启动。
#[cfg(any(windows, test))]
const CODEX_CHAT_BRIDGE_VERSION_BYPASS_POWERSHELL: &str = r#"if ($args.Count -eq 1 -and @('--version', '-V', '--help', '-h') -contains $args[0]) {
  & CODEX_BIN_LITERAL @args
  exit $LASTEXITCODE
}
"#;

/// 拼出完整的 bridge 启动段(PowerShell):先选解释器,再启动并等端口。
///
/// `AERORIC_BRIDGE_PYTHON` 环境变量可临时覆盖配置里的路径,便于用户在不改设置的
/// 情况下试另一个解释器。
#[cfg(any(windows, test))]
fn codex_chat_bridge_launch_powershell(configured_python: &str) -> String {
    let configured_python = configured_python.trim();
    let select = if configured_python.is_empty() {
        CODEX_CHAT_BRIDGE_AUTODETECT_POWERSHELL.to_string()
    } else {
        CODEX_CHAT_BRIDGE_CONFIGURED_POWERSHELL.replace(
            "BRIDGE_PYTHON_PATH_LITERAL",
            &powershell_quote(configured_python),
        )
    };
    format!("{select}{}", CODEX_CHAT_BRIDGE_WAIT_POWERSHELL)
}

/// 读取 bridge 落盘的端口号;文件不存在、还没写完或内容不是纯数字都返回空串。
///
/// 旧版只用 `Test-Path` 判断,文件已创建但还是空的时候会拿到空端口,继续拼出
/// `http://127.0.0.1:/v1` 这种无效 base_url,把启动失败推迟成难查的请求错误。
#[cfg(any(windows, test))]
const CODEX_CHAT_BRIDGE_PORT_READER_POWERSHELL: &str = r#"function Read-AeroricBridgePort([string] $path) {
  try {
    if (-not (Test-Path -LiteralPath $path)) { return '' }
    $raw = [System.IO.File]::ReadAllText($path).Trim()
    if ($raw -match '^[0-9]+$') { return $raw }
  } catch {
  }
  return ''
}
"#;

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
# 只判断"命令存在"不够:PATH 上的 python 可能是 2.x,也可能是指向已删除运行时的
# 陈旧 shim。bridge 用了 PEP 585 的 dict[str, Any],低于 3.9 会在 import 期报错。
python_bin=""
python_probe_notes=""
{configured_python_setup}for candidate in "${{python_candidates[@]}}"; do
  # 绝对路径用 -x 判断,裸名才走 command -v:配置里填的是具体解释器路径。
  case "$candidate" in
    */*) [ -x "$candidate" ] || {{
      python_probe_notes="${{python_probe_notes}}${{candidate}} -> not an executable file; "
      continue
    }} ;;
    *) command -v "$candidate" >/dev/null 2>&1 || continue ;;
  esac
  probe_output="$("$candidate" -c 'import sys; print(sys.version_info[0], sys.version_info[1])' 2>&1)" || {{
    python_probe_notes="${{python_probe_notes}}${{candidate}} -> ${{probe_output}}; "
    continue
  }}
  probe_major="${{probe_output%% *}}"
  probe_minor="${{probe_output##* }}"
  case "$probe_major$probe_minor" in
    *[!0-9]*|"")
      python_probe_notes="${{python_probe_notes}}${{candidate}} -> ${{probe_output}}; "
      continue
      ;;
  esac
  if [ "$probe_major" -gt 3 ] || {{ [ "$probe_major" -eq 3 ] && [ "$probe_minor" -ge 9 ]; }}; then
    python_bin="$candidate"
    break
  fi
  python_probe_notes="${{python_probe_notes}}${{candidate}} -> Python ${{probe_major}}.${{probe_minor}} is too old; "
done
if [ -z "$python_bin" ]; then
  if [ -n "$configured_python" ]; then
    # 配置了固定解释器就不回退自动探测:用户明确指定了某个 conda 环境的 Python,
    # 却静默换用 PATH 上的另一个,会让"为什么装的包不生效"极难排查。
    echo "The Python configured for this agent cannot run the Chat Completions bridge: $configured_python" >&2
  else
    echo "This custom Codex agent requires Python 3.9+ to bridge Responses to Chat Completions." >&2
  fi
  if [ -n "$python_probe_notes" ]; then
    echo "Checked: ${{python_probe_notes%; }}" >&2
  fi
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

# 冷启动叠加杀毒扫描时解释器起步可能远超 2s,窗口放宽到 20s,但进程一退出就立刻
# 收敛,不白等。端口必须是纯数字,否则空文件会拼出 http://127.0.0.1:/v1。
proxy_port=""
for _ in $(seq 1 400); do
  if [ -s "$port_file" ]; then
    candidate_port="$(cat "$port_file" 2>/dev/null || true)"
    candidate_port="$(printf '%s' "$candidate_port" | tr -d '[:space:]')"
    case "$candidate_port" in
      ""|*[!0-9]*) ;;
      *)
        proxy_port="$candidate_port"
        break
        ;;
    esac
  fi
  kill -0 "$proxy_pid" 2>/dev/null || break
  sleep 0.05
done
if [ -z "$proxy_port" ] && [ -s "$port_file" ]; then
  # 进程可能在最后一次轮询之后才落盘,退出前再确认一次。
  candidate_port="$(printf '%s' "$(cat "$port_file" 2>/dev/null || true)" | tr -d '[:space:]')"
  case "$candidate_port" in
    ""|*[!0-9]*) ;;
    *) proxy_port="$candidate_port" ;;
  esac
fi
if [ -z "$proxy_port" ]; then
  echo "Failed to start the local Chat Completions bridge." >&2
  echo "Bridge: $python_bin $proxy_script" >&2
  if [ -s "$proxy_log" ]; then
    echo "$proxy_log:" >&2
    tail -n 20 "$proxy_log" >&2
  fi
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
            configured_python_setup =
                codex_chat_bridge_configured_python_shell(&draft.bridge_python_path),
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
{version_bypass}API_KEY_FILE="${{AERORIC_AGENT_API_KEY_FILE:-$HOME/.aeroric/agent-credentials/{id}}}"
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

{hooks_fragment}
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
        hooks_fragment = CODEX_HOOKS_FRAGMENT_SHELL,
        version_bypass = if use_proxy {
            format!(
                r#"# 纯版本/帮助探测直接短路,不碰 bridge:桌面端的版本探测会用 `--version`
# 跑这个脚本,而 `codex --version` 既不需要 bridge 也不需要 config.toml。
if [ "$#" -eq 1 ]; then
  case "$1" in
    --version|-V|--help|-h)
      exec {codex_bin} "$1"
      ;;
  esac
fi
"#,
                codex_bin = shell_quote(&codex_bin),
            )
        } else {
            String::new()
        },
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
            r#"{port_reader}$proxyScript = Join-Path $agentHome 'codex-chat-proxy.py'
[System.IO.File]::WriteAllText($proxyScript, {proxy_script}, $utf8NoBom)
$portFile = Join-Path $agentHome 'codex-chat-proxy.port'
Remove-Item -LiteralPath $portFile -Force -ErrorAction SilentlyContinue
$proxyProcess = $null
{bridge_launch}
$configContent = 'model = "' + $selectedModel + '"' + [Environment]::NewLine +
  'model_catalog_json = "model-catalog.json"' + [Environment]::NewLine +
  {before} + [Environment]::NewLine +
  'base_url = "http://127.0.0.1:' + $proxyPort + '/v1"' + [Environment]::NewLine +
  {after}
"#,
            port_reader = CODEX_CHAT_BRIDGE_PORT_READER_POWERSHELL,
            proxy_script = powershell_literal_block(CODEX_CHAT_PROXY_SCRIPT),
            bridge_launch = codex_chat_bridge_launch_powershell(&draft.bridge_python_path),
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
{version_bypass}$apiKeyFile = if ($env:AERORIC_AGENT_API_KEY_FILE) {{ $env:AERORIC_AGENT_API_KEY_FILE }} else {{ Join-Path $HOME {api_key_file} }}
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
{hooks_fragment}
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
        hooks_fragment = CODEX_HOOKS_FRAGMENT_POWERSHELL,
        version_bypass = if use_proxy {
            CODEX_CHAT_BRIDGE_VERSION_BYPASS_POWERSHELL
                .replace("CODEX_BIN_LITERAL", &powershell_quote(&codex_bin))
        } else {
            String::new()
        },
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
            bridge_python_path: String::new(),
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
            bridge_python_path: String::new(),
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
            bridge_python_path: String::new(),
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

    fn hooks_fragment_test_draft(enable_chat_completions_proxy: bool) -> AgentSetupDraft {
        bridge_test_draft(enable_chat_completions_proxy, "")
    }

    fn bridge_test_draft(
        enable_chat_completions_proxy: bool,
        bridge_python_path: &str,
    ) -> AgentSetupDraft {
        AgentSetupDraft {
            id: "hooked".to_string(),
            label: "Hooked".to_string(),
            kind: AgentSetupKind::Codex,
            base_url: "https://example.com/v1/".to_string(),
            api_key: "sk-test".to_string(),
            model: "gpt-5.6".to_string(),
            models: vec!["gpt-5.6".to_string()],
            enable_1m_context: false,
            enable_chat_completions_proxy,
            bridge_python_path: bridge_python_path.to_string(),
            dsh_api_protocol: String::new(),
            proxy_enabled: false,
        }
    }

    /// 自定义 codex Agent 的隔离 CODEX_HOME 读不到 `~/.codex/config.toml`,包装脚本
    /// 必须自己追加 Aeroric 的 hook 片段;否则 hook 事件为零,状态判定会退回到
    /// 噪声极大的 transcript 猜测,把每次工具调用都误报成"等待用户输入"。
    #[cfg(not(windows))]
    #[test]
    fn codex_wrapper_appends_aeroric_hooks_fragment() {
        for proxy in [false, true] {
            let script = build_codex_agent_shell_script(&hooks_fragment_test_draft(proxy));
            assert!(
                script.contains(".aeroric/hooks/codex-hooks.toml"),
                "proxy={proxy} 缺少 hook 片段默认路径"
            );
            assert!(
                script
                    .contains("cat -- \"$aeroric_hooks_fragment\" >> \"$CODEX_HOME/config.toml\""),
                "proxy={proxy} 未把片段追加进本 Agent 的 config.toml"
            );
            // 必须在 config.toml 整体重写之后追加,否则会被覆盖掉。
            // 锚点用重写分支特有的 `} > `,避免匹配到片段自身的 `>>` 追加。
            let write = script
                .rfind("} > \"$CODEX_HOME/config.toml\"")
                .expect("config.toml 写入位置");
            let append = script
                .find("cat -- \"$aeroric_hooks_fragment\"")
                .expect("片段追加位置");
            assert!(append > write, "proxy={proxy} 片段追加早于 config 重写");
            // 片段缺失时不能让 `set -e` 中断启动。
            assert!(script.contains("if [ -r \"$aeroric_hooks_fragment\" ]; then"));
        }
    }

    #[cfg(any(windows, test))]
    #[test]
    fn codex_powershell_wrapper_appends_aeroric_hooks_fragment() {
        for proxy in [false, true] {
            let script = build_codex_agent_powershell_script(&hooks_fragment_test_draft(proxy));
            assert!(
                script.contains(".aeroric/hooks/codex-hooks.toml"),
                "proxy={proxy} 缺少 hook 片段默认路径"
            );
            let write = script
                .rfind("'config.toml'), $configContent")
                .expect("config.toml 写入位置");
            let append = script
                .find("Add-Content -LiteralPath (Join-Path $env:CODEX_HOME 'config.toml')")
                .expect("片段追加位置");
            assert!(append > write, "proxy={proxy} 片段追加早于 config 重写");
            assert!(script.contains("if (Test-Path -LiteralPath $aeroricHooksFragment)"));
        }
    }

    /// bridge 启动必须先探到一个真正能跑的 Python 3.9+。Windows 预置的 Microsoft
    /// Store 别名桩能被 `Get-Command` 找到却一运行就退出,只判断"命令存在"会让
    /// 干净装机的用户卡在无线索的 "Failed to start the local Chat Completions
    /// bridge."。
    #[cfg(any(windows, test))]
    #[test]
    fn powershell_chat_bridge_probes_a_usable_python_before_launching() {
        let script = build_codex_agent_powershell_script(&hooks_fragment_test_draft(true));
        // 必须真的执行解释器取版本,而不是只看命令是否存在。
        assert!(script.contains("import sys; print(sys.version_info[0], sys.version_info[1])"));
        assert!(script.contains("if ($probeExit -ne 0 -or -not $versionMatch.Success) {"));
        assert!(script.contains("($probeMajor -eq 3 -and $probeMinor -lt 9)"));
        // 候选要按 python3→python→py 定序自查:Get-Command 多入参不保证顺序,
        // Select-Object -First 1 可能先拿到没装运行时的 py.exe。
        assert!(script.contains("foreach ($candidateName in @('python3', 'python', 'py'))"));
        assert!(!script.contains("Get-Command python3, python, py"));
        // 失败时要给出可执行的下一步,并点名商店桩不算。
        assert!(script.contains("requires Python 3.9+"));
        assert!(script.contains("Microsoft Store alias stub does not count"));
        assert!(script.contains("'Checked: ' + ($bridgeProbeNotes -join '; ')"));
    }

    /// bridge 起不来时必须把日志尾部带进报错,否则用户只看到一句没有线索的失败。
    /// 等待窗口也要够长:首次冷启动叠加 Defender 实时扫描经常超过原先的 2s。
    #[cfg(any(windows, test))]
    #[test]
    fn powershell_chat_bridge_waits_longer_and_reports_the_log_tail() {
        let script = build_codex_agent_powershell_script(&hooks_fragment_test_draft(true));
        assert!(script.contains("for ($attempt = 0; $attempt -lt 400; $attempt++) {"));
        assert!(script.contains("Start-Sleep -Milliseconds 50"));
        // 进程已经死了就别把 20s 白等完。
        assert!(script.contains("if ($proxyProcess.HasExited) { break }"));
        assert!(script.contains("Get-Content -LiteralPath $logPath -Tail 20"));
        assert!(script.contains("' (exited with code ' + $proxyProcess.ExitCode + ')'"));
        assert!(script.contains("Failed to start the local Chat Completions bridge."));
    }

    /// 端口必须校验成纯数字:旧版只用 `Test-Path`,文件已创建但还没写完时会拿到空
    /// 端口,拼出 `http://127.0.0.1:/v1`,把启动失败推迟成难查的请求错误。
    #[cfg(any(windows, test))]
    #[test]
    fn powershell_chat_bridge_rejects_a_half_written_port_file() {
        let script = build_codex_agent_powershell_script(&hooks_fragment_test_draft(true));
        assert!(script.contains("function Read-AeroricBridgePort([string] $path) {"));
        assert!(script.contains("if ($raw -match '^[0-9]+$') { return $raw }"));
        // 端口只能来自这个校验过的读取函数。
        assert!(!script.contains("$proxyPort = (Get-Content -LiteralPath $portFile -Raw).Trim()"));
        assert!(script.contains("$proxyPort = Read-AeroricBridgePort $portFile"));
        // 端口读取函数要定义在使用之前。
        let definition = script
            .find("function Read-AeroricBridgePort")
            .expect("端口读取函数定义");
        let usage = script
            .find("$proxyPort = Read-AeroricBridgePort")
            .expect("端口读取函数调用");
        assert!(definition < usage, "端口读取函数定义晚于调用");
    }

    /// 上面几个 bridge 测试都是字符串匹配,匹配得再全也证明不了脚本能被 shell 解析。
    /// bridge 分支里有嵌套的 `case`/`$(...)`/花括号转义,漏一个反引号或大括号就会让
    /// 生成的包装脚本在用户机器上直接语法错误,所以这里真的跑一次 `bash -n`。
    #[cfg(not(windows))]
    #[test]
    fn generated_codex_shell_wrapper_parses_under_bash() {
        let dir = std::env::temp_dir().join(format!(
            "aeroric-codex-shell-syntax-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&dir).expect("临时目录");
        for proxy in [false, true] {
            let script = build_codex_agent_shell_script(&hooks_fragment_test_draft(proxy));
            let path = dir.join(format!("wrapper-proxy-{proxy}.sh"));
            fs::write(&path, &script).expect("写入包装脚本");
            let output = std::process::Command::new("bash")
                .arg("-n")
                .arg(&path)
                .output()
                .expect("执行 bash -n");
            assert!(
                output.status.success(),
                "proxy={proxy} 生成的包装脚本语法错误: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        let _ = fs::remove_dir_all(&dir);
    }

    /// 配置了固定解释器后不能再回退自动探测:用户明确指定某个 conda 环境的 Python,
    /// 却静默换用 PATH 上的另一个,会让"为什么装的包不生效"极难排查。
    #[cfg(any(windows, test))]
    #[test]
    fn powershell_chat_bridge_pins_the_configured_interpreter() {
        let script =
            build_codex_agent_powershell_script(&bridge_test_draft(true, r"C:\Py 3.12\python.exe"));
        assert!(script.contains(r"$configuredBridgePython = 'C:\Py 3.12\python.exe'"));
        // 固定路径分支下不能出现自动探测的候选遍历。
        assert!(!script.contains("foreach ($candidateName in @('python3', 'python', 'py'))"));
        // 固定路径同样要实跑校验版本,并且失败就报错而不是换一个。
        assert!(script.contains("$configuredMatch = [regex]::Match($configuredProbe"));
        assert!(script.contains("cannot run the Chat Completions bridge"));
        assert!(script.contains("is too old for the Chat Completions bridge (need 3.9+)"));
        // 环境变量可临时覆盖,便于不改设置就试另一个解释器。
        assert!(script.contains("if ($env:AERORIC_BRIDGE_PYTHON)"));
        // 启动与等待段两种选择方式共用。
        assert!(script.contains("for ($attempt = 0; $attempt -lt 400; $attempt++) {"));
    }

    /// 没配置就仍然走自动探测,行为与 v7 一致。
    #[cfg(any(windows, test))]
    #[test]
    fn powershell_chat_bridge_autodetects_when_unconfigured() {
        let script = build_codex_agent_powershell_script(&bridge_test_draft(true, ""));
        assert!(script.contains("foreach ($candidateName in @('python3', 'python', 'py'))"));
        assert!(!script.contains("$configuredBridgePython"));
    }

    /// 解释器路径必须用单引号转义后再插进 PowerShell。conda 环境路径常带空格,而
    /// `$` 之类字符裸插会被 PowerShell 重新解析。
    #[cfg(any(windows, test))]
    #[test]
    fn powershell_chat_bridge_quotes_the_configured_interpreter() {
        let script = build_codex_agent_powershell_script(&bridge_test_draft(
            true,
            r"C:\it's\py$x\python.exe",
        ));
        // 单引号翻倍,`$` 在单引号里不插值。
        assert!(script.contains(r"'C:\it''s\py$x\python.exe'"));
    }

    /// bash 侧固定解释器:只试这一个候选,且报错要点名是配置的那条路径。
    #[cfg(not(windows))]
    #[test]
    fn shell_chat_bridge_pins_the_configured_interpreter() {
        let script =
            build_codex_agent_shell_script(&bridge_test_draft(true, "/opt/my env/bin/python3"));
        // 路径要经过 shell_quote,带空格也不会被重新分词。
        assert!(script.contains("configured_python='/opt/my env/bin/python3'"));
        assert!(script.contains("python_candidates=(\"$configured_python\")"));
        // 固定路径分支不能把自动探测的候选也拼进去。
        assert!(!script.contains("python_candidates=(\"python3\" \"python\")"));
        assert!(script.contains("The Python configured for this agent cannot run"));
        // 绝对路径要用 -x 判断可执行,而不是 command -v。
        assert!(script.contains("*/*) [ -x \"$candidate\" ]"));
    }

    #[cfg(not(windows))]
    #[test]
    fn shell_chat_bridge_autodetects_when_unconfigured() {
        let script = build_codex_agent_shell_script(&bridge_test_draft(true, ""));
        assert!(script.contains("python_candidates=(\"python3\" \"python\")"));
        // 未配置时也允许用环境变量临时指定。
        assert!(script.contains("configured_python=\"${AERORIC_BRIDGE_PYTHON:-}\""));
    }

    /// `--version` 探测不能拉起 bridge。桌面端的 `detect_agent_version` 就是用
    /// `--version` 跑这个包装脚本,旧结构会让没装 Python 的机器连版本都测不出来,
    /// 装了的机器也要白等一次解释器冷启动。
    #[cfg(not(windows))]
    #[test]
    fn shell_chat_bridge_short_circuits_version_probes() {
        let script = build_codex_agent_shell_script(&bridge_test_draft(true, ""));
        let bypass = script
            .find("--version|-V|--help|-h)")
            .expect("版本短路分支");
        let bridge = script.find("codex-chat-proxy.py").expect("bridge 启动位置");
        assert!(bypass < bridge, "版本短路必须在 bridge 启动之前");
        // 也要在读凭据之前:`codex --version` 不需要 API key。
        let api_key = script.find("API_KEY_FILE=").expect("凭据读取位置");
        assert!(bypass < api_key, "版本短路必须在读凭据之前");
        // 不开 bridge 的脚本不需要这段短路。
        assert!(
            !build_codex_agent_shell_script(&bridge_test_draft(false, ""))
                .contains("--version|-V|--help|-h)")
        );
    }

    #[cfg(any(windows, test))]
    #[test]
    fn powershell_chat_bridge_short_circuits_version_probes() {
        let script = build_codex_agent_powershell_script(&bridge_test_draft(true, ""));
        let bypass = script
            .find("@('--version', '-V', '--help', '-h') -contains $args[0]")
            .expect("版本短路分支");
        let bridge = script.find("codex-chat-proxy.py").expect("bridge 启动位置");
        assert!(bypass < bridge, "版本短路必须在 bridge 启动之前");
        let api_key = script.find("$apiKeyFile = ").expect("凭据读取位置");
        assert!(bypass < api_key, "版本短路必须在读凭据之前");
        assert!(
            !build_codex_agent_powershell_script(&bridge_test_draft(false, ""))
                .contains("-contains $args[0]")
        );
    }

    /// 预检、bash、PowerShell 三处的版本下限必须一致,否则会出现"预检说可用、启动仍
    /// 然失败"这种最难排查的组合。
    #[test]
    fn chat_bridge_python_floor_is_consistent_across_all_three_sites() {
        assert_eq!(CHAT_BRIDGE_PYTHON_MIN_MINOR, 9);
        #[cfg(not(windows))]
        {
            let script = build_codex_agent_shell_script(&bridge_test_draft(true, ""));
            assert!(script.contains(&format!(
                "[ \"$probe_minor\" -ge {CHAT_BRIDGE_PYTHON_MIN_MINOR} ]"
            )));
        }
        #[cfg(any(windows, test))]
        {
            let script = build_codex_agent_powershell_script(&bridge_test_draft(true, ""));
            assert!(script.contains(&format!("$probeMinor -lt {CHAT_BRIDGE_PYTHON_MIN_MINOR})")));
        }
    }

    /// bash 侧与 PowerShell 侧共用同一个 bridge,校验强度必须保持一致。
    #[cfg(not(windows))]
    #[test]
    fn shell_chat_bridge_probes_python_and_reports_the_log_tail() {
        let script = build_codex_agent_shell_script(&hooks_fragment_test_draft(true));
        assert!(script.contains("import sys; print(sys.version_info[0], sys.version_info[1])"));
        assert!(script.contains("requires Python 3.9+"));
        assert!(script.contains("[ \"$probe_minor\" -ge 9 ]"));
        assert!(script.contains("for _ in $(seq 1 400); do"));
        // 进程已经退出就立刻收敛,不白等满窗口。
        assert!(script.contains("kill -0 \"$proxy_pid\" 2>/dev/null || break"));
        assert!(script.contains("tail -n 20 \"$proxy_log\" >&2"));
        // 空端口不能被当成有效端口用去拼 base_url。
        assert!(script.contains("\"\"|*[!0-9]*) ;;"));
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
            bridge_python_path: String::new(),
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
            bridge_python_path: String::new(),
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
            bridge_python_path: String::new(),
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
            bridge_python_path: String::new(),
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
            bridge_python_path: String::new(),
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
            bridge_python_path: String::new(),
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
            bridge_python_path: String::new(),
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
            bridge_python_path: String::new(),
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
            bridge_python_path: String::new(),
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
            bridge_python_path: String::new(),
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
                bridge_python_path: String::new(),
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
            bridge_python_path: String::new(),
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
                bridge_python_path: String::new(),
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
                bridge_python_path: String::new(),
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
                bridge_python_path: String::new(),
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
                bridge_python_path: String::new(),
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
