// ── Session metrics ───────────────────────────────────────────────────────────

use chrono::Timelike;
use parking_lot::Mutex;
use serde_json::Value;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::time::SystemTime;

#[derive(serde::Serialize, Clone, Default)]
pub(crate) struct SessionMetrics {
    pub(crate) tool_calls: u64,
    pub(crate) duration_secs: f64,
    /// 任务累计 token 消耗（包含缓存命中 / reasoning），用于 UI"总消耗"。
    pub(crate) total_tokens: u64,
    /// 当前上下文占用（最后一轮 prompt 大小）。Codex 直读，Claude 由最后一条 assistant 推导。
    pub(crate) context_tokens: u64,
    /// 模型上下文窗口大小。仅 Codex 自带；Claude session 不暴露此值，留 0 让前端隐藏。
    pub(crate) context_window: u64,
}

/// 缓存：session_path → (file_modified_time, SessionMetrics)
static METRICS_CACHE: LazyLock<Mutex<HashMap<String, (SystemTime, SessionMetrics)>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn parse_rfc3339_secs(ts: &str) -> Option<f64> {
    chrono::DateTime::parse_from_rfc3339(ts)
        .ok()
        .map(|dt| dt.timestamp() as f64 + dt.timestamp_subsec_millis() as f64 / 1000.0)
}

fn track_timestamp(val: &Value, first: &mut Option<f64>, last: &mut Option<f64>) {
    if let Some(ts_str) = val.get("timestamp").and_then(|v| v.as_str()) {
        if let Some(ts) = parse_rfc3339_secs(ts_str) {
            if first.is_none() {
                *first = Some(ts);
            }
            *last = Some(ts);
        }
    }
}

fn duration_from(first: Option<f64>, last: Option<f64>) -> f64 {
    match (first, last) {
        (Some(a), Some(b)) => (b - a).max(0.0),
        _ => 0.0,
    }
}

/// 探测格式：与 `session.rs::is_codex_format` 保持一致——探测窗口内出现
/// `type=session_meta` 或 `type=event_msg` 即视为 Codex。
/// Why: Codex 各版本 `payload.originator` 取值漂移（codex_cli_rs / codex-tui / ...），
/// 仅靠 originator 前缀判定会让部分可正常回放的 Codex session 被错走 Claude 解析，
/// token/tool_calls 全部归零；判定标准必须与会话查看器保持一致。
const SESSION_FORMAT_DETECTION_LINES: usize = 200;

pub(crate) fn is_codex_session(content: &str) -> bool {
    for line in content.lines().take(SESSION_FORMAT_DETECTION_LINES) {
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        match v.get("type").and_then(|t| t.as_str()) {
            Some("session_meta") | Some("event_msg") => return true,
            _ => {}
        }
    }
    false
}

/// dsh transcript 以 header 行开头(`type: "session"` + 数值 version + id)。
pub(crate) fn is_dsh_session(content: &str) -> bool {
    content
        .lines()
        .next()
        .map(crate::session_dsh::line_is_dsh_header)
        .unwrap_or(false)
}

/// Claude transcript 不像 Codex 那样自带 `model_context_window`，只在每条 assistant
/// 消息上记录 model。为了让终端顶栏能显示"上下文占用 / 窗口"，这里按 model 推导窗口。
///
/// 覆盖不到的 model（第三方中转、自定义 slug）返回 None，前端据此隐藏上下文这一项——
/// 宁可少显示一项，也不要给出一个编造的百分比。
pub(crate) fn claude_context_window_for_model(model: &str) -> Option<u64> {
    let slug = model.trim().to_ascii_lowercase();
    if slug.is_empty() {
        return None;
    }
    // Anthropic 官方模型：Sonnet 4 起支持 1M beta，但 transcript 不记录是否启用，
    // 因此统一按默认 200K 计算，避免把已用量显示成"看起来还很空"。
    if slug.starts_with("claude-") {
        return Some(200_000);
    }
    None
}

fn parse_claude_metrics(content: &str) -> SessionMetrics {
    let mut input_tokens: u64 = 0;
    let mut output_tokens: u64 = 0;
    let mut cache_creation: u64 = 0;
    let mut cache_read: u64 = 0;
    let mut tool_calls: u64 = 0;
    let mut last_context: u64 = 0;
    let mut last_model: Option<String> = None;
    let mut first_ts: Option<f64> = None;
    let mut last_ts: Option<f64> = None;

    for line in content.lines() {
        let Ok(val) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        track_timestamp(&val, &mut first_ts, &mut last_ts);

        if val.get("type").and_then(|v| v.as_str()) != Some("assistant") {
            continue;
        }
        let Some(message) = val.get("message") else {
            continue;
        };
        if let Some(model) = message.get("model").and_then(|v| v.as_str()) {
            if !model.trim().is_empty() {
                last_model = Some(model.to_string());
            }
        }

        if let Some(usage) = message.get("usage") {
            let inp = usage
                .get("input_tokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let out = usage
                .get("output_tokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let cc = usage
                .get("cache_creation_input_tokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let cr = usage
                .get("cache_read_input_tokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            input_tokens += inp;
            output_tokens += out;
            cache_creation += cc;
            cache_read += cr;
            // 最后一条 assistant 的 prompt 总大小 ≈ 当前上下文占用
            last_context = inp + cc + cr;
        }

        if let Some(arr) = message.get("content").and_then(|v| v.as_array()) {
            for item in arr {
                if item.get("type").and_then(|v| v.as_str()) == Some("tool_use") {
                    tool_calls += 1;
                }
            }
        }
    }

    SessionMetrics {
        tool_calls,
        duration_secs: duration_from(first_ts, last_ts),
        total_tokens: input_tokens + output_tokens + cache_creation + cache_read,
        context_tokens: last_context,
        // Claude session 不带窗口大小，按最后一条 assistant 的 model 推导。
        context_window: last_model
            .as_deref()
            .and_then(claude_context_window_for_model)
            .unwrap_or(0),
    }
}

fn parse_codex_metrics(content: &str) -> SessionMetrics {
    let mut tool_calls: u64 = 0;
    let mut last_token_info: Option<Value> = None;
    let mut first_ts: Option<f64> = None;
    let mut last_ts: Option<f64> = None;

    for line in content.lines() {
        let Ok(val) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        track_timestamp(&val, &mut first_ts, &mut last_ts);

        let t = val.get("type").and_then(|v| v.as_str()).unwrap_or("");
        let payload = val.get("payload");
        let pt = payload
            .and_then(|p| p.get("type"))
            .and_then(|v| v.as_str())
            .unwrap_or("");

        match (t, pt) {
            ("event_msg", "token_count") => {
                if let Some(info) = payload.and_then(|p| p.get("info")) {
                    if !info.is_null() {
                        last_token_info = Some(info.clone());
                    }
                }
            }
            ("response_item", "function_call") | ("response_item", "custom_tool_call") => {
                tool_calls += 1;
            }
            _ => {}
        }
    }

    let (total_tokens, context_tokens, context_window) =
        if let Some(info) = last_token_info.as_ref() {
            let total = info.get("total_token_usage");
            let last = info.get("last_token_usage");
            let tot = total
                .and_then(|t| t.get("total_tokens"))
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let ctx = last
                .and_then(|l| l.get("total_tokens"))
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let win = info
                .get("model_context_window")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            (tot, ctx, win)
        } else {
            (0, 0, 0)
        };

    SessionMetrics {
        tool_calls,
        duration_secs: duration_from(first_ts, last_ts),
        total_tokens,
        context_tokens,
        context_window,
    }
}

pub(crate) fn parse_session_metrics_from_path(path: &std::path::Path) -> SessionMetrics {
    let Ok(content) = std::fs::read_to_string(path) else {
        return SessionMetrics::default();
    };
    if is_codex_session(&content) {
        parse_codex_metrics(&content)
    } else {
        parse_claude_metrics(&content)
    }
}

/// 带缓存的 session 指标解析
/// 通过文件修改时间判断缓存是否有效，避免重复解析未变更的文件
pub(crate) fn parse_session_metrics_cached(path: &std::path::Path) -> SessionMetrics {
    let path_str = path.to_string_lossy().to_string();

    // 获取文件修改时间
    let modified = match std::fs::metadata(path).and_then(|m| m.modified()) {
        Ok(t) => t,
        Err(_) => return SessionMetrics::default(),
    };

    // 检查缓存
    {
        let cache = METRICS_CACHE.lock();
        if let Some((cached_time, cached_metrics)) = cache.get(&path_str) {
            if *cached_time == modified {
                return cached_metrics.clone();
            }
        }
    }

    // 缓存未命中，完整解析
    let metrics = parse_session_metrics_from_path(path);

    // 更新缓存
    {
        let mut cache = METRICS_CACHE.lock();
        cache.insert(path_str, (modified, metrics.clone()));
    }

    metrics
}

#[tauri::command]
pub async fn read_session_metrics(session_path: String) -> Result<SessionMetrics, String> {
    tokio::task::spawn_blocking(move || {
        let path = std::path::Path::new(&session_path);
        // dsh transcripts come in two encodings under the same directory, and the
        // path was pinned at session registration — before the first flush decided
        // which one. Try the sibling before declaring the session unreadable, the
        // same redirect `validate_session_path_for` applies for the session view.
        let resolved = if path.exists() {
            path.to_path_buf()
        } else {
            path.parent()
                .and_then(crate::session_dsh::dsh_transcript_in)
                .ok_or_else(|| format!("Session file not found: {}", session_path))?
        };
        Ok(parse_session_metrics_cached(&resolved))
    })
    .await
    .map_err(|e| format!("read_session_metrics join error: {}", e))?
}

// ── Local usage statistics ──────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum UsageAgent {
    Codex,
    Claude,
    Dsh,
}

#[derive(Clone, Debug)]
pub(crate) struct UsageRequest {
    pub(crate) timestamp: f64,
    pub(crate) date: chrono::NaiveDate,
    pub(crate) agent: UsageAgent,
    pub(crate) model: String,
    pub(crate) input_tokens: u64,
    pub(crate) output_tokens: u64,
    pub(crate) cache_creation_tokens: u64,
    pub(crate) cache_read_tokens: u64,
}

#[derive(serde::Serialize, Clone, Default, Debug)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UsageStatisticsTotals {
    pub(crate) total_tokens: u64,
    pub(crate) input_tokens: u64,
    pub(crate) output_tokens: u64,
    pub(crate) cache_creation_tokens: u64,
    pub(crate) cache_read_tokens: u64,
    pub(crate) cache_hit_rate: f64,
    pub(crate) request_count: u64,
    pub(crate) total_cost: f64,
    /// 命中公开价目表的请求数。
    pub(crate) priced_request_count: u64,
    /// 无公开价目、按同档模型推算单价的请求数(成本仍计入 total_cost)。
    pub(crate) estimated_request_count: u64,
}

#[derive(serde::Serialize, Clone, Default, Debug)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UsageStatisticsDay {
    pub(crate) date: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) hour: Option<u32>,
    #[serde(flatten)]
    pub(crate) totals: UsageStatisticsTotals,
}

#[derive(serde::Serialize, Clone, Default, Debug)]
pub(crate) struct UsageStatisticsBreakdown {
    pub(crate) codex: UsageStatisticsTotals,
    pub(crate) claude: UsageStatisticsTotals,
    #[serde(default)]
    pub(crate) dsh: UsageStatisticsTotals,
}

#[derive(serde::Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UsageStatistics {
    pub(crate) range_days: u32,
    pub(crate) from: String,
    pub(crate) to: String,
    pub(crate) agent: String,
    pub(crate) updated_at: i64,
    pub(crate) totals: UsageStatisticsTotals,
    pub(crate) series: Vec<UsageStatisticsDay>,
    pub(crate) breakdown: UsageStatisticsBreakdown,
}

/// 每 100 万 token 的美元单价。
#[derive(Clone, Copy)]
struct ModelPricing {
    input: f64,
    cached_input: f64,
    cache_write: f64,
    output: f64,
}

const fn price(input: f64, cached_input: f64, cache_write: f64, output: f64) -> ModelPricing {
    ModelPricing {
        input,
        cached_input,
        cache_write,
        output,
    }
}

/// 单价来源:命中价目表还是按同档推算。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PricingSource {
    Listed,
    Estimated,
}

fn request_date(val: &Value) -> Option<(f64, chrono::NaiveDate)> {
    let timestamp = val.get("timestamp")?.as_str()?;
    let parsed = chrono::DateTime::parse_from_rfc3339(timestamp).ok()?;
    let local = parsed.with_timezone(&chrono::Local);
    Some((
        parsed.timestamp() as f64 + parsed.timestamp_subsec_millis() as f64 / 1000.0,
        local.date_naive(),
    ))
}

pub(crate) fn parse_claude_usage_requests(content: &str, source_key: &str) -> Vec<UsageRequest> {
    let mut requests = HashMap::<String, UsageRequest>::new();

    for (line_index, line) in content.lines().enumerate() {
        let Ok(val) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if val.get("type").and_then(Value::as_str) != Some("assistant") {
            continue;
        }
        let Some(message) = val.get("message") else {
            continue;
        };
        let Some(usage) = message.get("usage") else {
            continue;
        };
        let Some((timestamp, date)) = request_date(&val) else {
            continue;
        };

        let request_id = message
            .get("id")
            .and_then(Value::as_str)
            .or_else(|| val.get("uuid").and_then(Value::as_str))
            .map(str::to_owned)
            .unwrap_or_else(|| format!("{source_key}:{line_index}"));
        let request = UsageRequest {
            timestamp,
            date,
            agent: UsageAgent::Claude,
            model: message
                .get("model")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            input_tokens: usage
                .get("input_tokens")
                .and_then(Value::as_u64)
                .unwrap_or(0),
            output_tokens: usage
                .get("output_tokens")
                .and_then(Value::as_u64)
                .unwrap_or(0),
            cache_creation_tokens: usage
                .get("cache_creation_input_tokens")
                .and_then(Value::as_u64)
                .unwrap_or(0),
            cache_read_tokens: usage
                .get("cache_read_input_tokens")
                .and_then(Value::as_u64)
                .unwrap_or(0),
        };

        let replace = requests
            .get(&request_id)
            .map(|current| request.timestamp >= current.timestamp)
            .unwrap_or(true);
        if replace {
            requests.insert(request_id, request);
        }
    }

    requests.into_values().collect()
}

pub(crate) fn parse_codex_usage_requests(content: &str) -> Vec<UsageRequest> {
    let mut requests = Vec::new();
    let mut model = String::new();

    for line in content.lines() {
        let Ok(val) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let item_type = val.get("type").and_then(Value::as_str).unwrap_or_default();
        let payload = val.get("payload");
        let payload_type = payload
            .and_then(|item| item.get("type"))
            .and_then(Value::as_str)
            .unwrap_or_default();

        if item_type == "turn_context" {
            if let Some(next_model) = payload
                .and_then(|item| item.get("model"))
                .and_then(Value::as_str)
            {
                model = next_model.to_owned();
            }
            continue;
        }

        if item_type != "event_msg" || payload_type != "token_count" {
            continue;
        }
        let Some(last_usage) = payload
            .and_then(|item| item.get("info"))
            .and_then(|item| item.get("last_token_usage"))
        else {
            continue;
        };
        let Some((timestamp, date)) = request_date(&val) else {
            continue;
        };
        let raw_input = last_usage
            .get("input_tokens")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        let cached_input = last_usage
            .get("cached_input_tokens")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        let visible_output = last_usage
            .get("output_tokens")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        let reasoning_output = last_usage
            .get("reasoning_output_tokens")
            .and_then(Value::as_u64)
            .unwrap_or(0);

        requests.push(UsageRequest {
            timestamp,
            date,
            agent: UsageAgent::Codex,
            model: model.clone(),
            input_tokens: raw_input.saturating_sub(cached_input),
            output_tokens: visible_output.saturating_add(reasoning_output),
            cache_creation_tokens: 0,
            cache_read_tokens: cached_input,
        });
    }

    requests
}

/// dsh 会话 token 聚合:每条带 `usage` 的 `assistant/message` 事件计一次请求。
/// model 取最近一条 `request/context` 事件(路由变化时才记录);时间戳来自事件
/// 的 `time`(Unix 毫秒)。
pub(crate) fn parse_dsh_usage_line(line: &str, current_model: &mut String) -> Option<UsageRequest> {
    let val = serde_json::from_str::<Value>(line).ok()?;
    match val.get("type").and_then(Value::as_str) {
        Some("request/context") => {
            if let Some(model) = val
                .get("data")
                .and_then(|data| data.get("model"))
                .and_then(Value::as_str)
            {
                *current_model = model.to_owned();
            }
            None
        }
        Some("assistant/message") => {
            let data = val.get("data")?;
            let usage = data.get("usage")?;
            let time_ms = val.get("time").and_then(Value::as_i64)?;
            let timestamp = time_ms as f64 / 1000.0;
            let date = chrono::DateTime::from_timestamp_millis(time_ms)
                .map(|dt| dt.with_timezone(&chrono::Local).date_naive())?;
            Some(UsageRequest {
                timestamp,
                date,
                agent: UsageAgent::Dsh,
                model: current_model.clone(),
                input_tokens: usage
                    .get("inputTokens")
                    .and_then(Value::as_u64)
                    .unwrap_or(0),
                output_tokens: usage
                    .get("outputTokens")
                    .and_then(Value::as_u64)
                    .unwrap_or(0),
                cache_creation_tokens: usage
                    .get("cacheWriteTokens")
                    .and_then(Value::as_u64)
                    .unwrap_or(0),
                cache_read_tokens: usage
                    .get("cacheReadTokens")
                    .and_then(Value::as_u64)
                    .unwrap_or(0),
            })
        }
        _ => None,
    }
}

pub(crate) fn parse_dsh_usage_requests(content: &str) -> Vec<UsageRequest> {
    let mut requests = Vec::new();
    let mut current_model = String::new();
    for line in content.lines() {
        if let Some(request) = parse_dsh_usage_line(line, &mut current_model) {
            requests.push(request);
        }
    }
    requests
}

/// 剥掉结尾的 ISO 发布日期,例如 `gpt-5-4-mini-2026-03-17`。
fn strip_release_date(name: &str) -> Option<&str> {
    let numeric = |part: &str, len: usize| {
        part.len() == len && part.chars().all(|character| character.is_ascii_digit())
    };
    let (head, day) = name.rsplit_once('-')?;
    let (head, month) = head.rsplit_once('-')?;
    let (head, year) = head.rsplit_once('-')?;
    (numeric(day, 2) && numeric(month, 2) && numeric(year, 4)).then_some(head)
}

/// 归一化模型名,让一张价目表同时覆盖所有 harness(同一模型会以不同写法出现在
/// codex / claude / dsh 三种会话里):
/// - 去掉路由前缀:`anthropic/claude-opus-5` → `claude-opus-5`
/// - `.` 统一成 `-`:`claude-opus-4.8` 与 `claude-opus-4-8` 归为同族
/// - 去掉发布批次与推理档位后缀:`deepseek-v4-pro-0813`、`claude-opus-4-6-thinking`
fn normalize_model(model: &str) -> String {
    // 部署位置与推理档位不改变单价。
    const SUFFIXES: [&str; 13] = [
        "-non-reasoning",
        "-reasoning",
        "-thinking",
        "-xhigh",
        "-high",
        "-medium",
        "-low",
        "-minimal",
        "-aws",
        "-gcp",
        "-azure",
        "-vertex",
        "-preview",
    ];

    let mut name = model
        .rsplit('/')
        .next()
        .unwrap_or(model)
        .trim()
        .to_ascii_lowercase()
        .replace('.', "-");
    if let Some(stripped) = strip_release_date(&name) {
        name = stripped.to_owned();
    }
    loop {
        // 4 位及以上的纯数字尾段是发布批次(`deepseek-v4-pro-0813`)。
        if let Some((head, batch)) = name.rsplit_once('-') {
            if batch.len() >= 4 && batch.chars().all(|character| character.is_ascii_digit()) {
                name = head.to_owned();
                continue;
            }
        }
        match SUFFIXES.iter().find_map(|suffix| name.strip_suffix(suffix)) {
            Some(head) => name = head.to_owned(),
            None => return name,
        }
    }
}
/// 公开 API 单价(USD / 1M token),按归一化模型名的子串匹配。
///
/// 顺序敏感:更长的族名必须排在其前缀之前,否则 `opus-4-8` 会先被 `opus-4` 命中。
/// 不单独公示 cache write 单价的供应商(GLM / Grok / Kimi / MiniMax)保守按 input
/// 计价;OpenAI 不对 cache write 计费,故取 0。
const MODEL_PRICES: &[(&str, ModelPricing)] = &[
    // Anthropic —— cache write = 1.25x input,cache read = 0.1x input。
    ("claude-mythos-5", price(10.0, 1.0, 12.5, 50.0)),
    ("mythos-5", price(10.0, 1.0, 12.5, 50.0)),
    ("fable-5", price(10.0, 1.0, 12.5, 50.0)),
    ("opus-5", price(5.0, 0.5, 6.25, 25.0)),
    ("opus-4-8", price(5.0, 0.5, 6.25, 25.0)),
    ("opus-4-7", price(5.0, 0.5, 6.25, 25.0)),
    ("opus-4-6", price(5.0, 0.5, 6.25, 25.0)),
    ("opus-4-5", price(5.0, 0.5, 6.25, 25.0)),
    ("opus-4-1", price(15.0, 1.5, 18.75, 75.0)),
    ("opus-4", price(15.0, 1.5, 18.75, 75.0)),
    ("sonnet-5", price(2.0, 0.2, 2.5, 10.0)),
    ("sonnet-4-6", price(3.0, 0.3, 3.75, 15.0)),
    ("sonnet-4-5", price(3.0, 0.3, 3.75, 15.0)),
    ("sonnet-4", price(3.0, 0.3, 3.75, 15.0)),
    ("haiku-4-5", price(1.0, 0.1, 1.25, 5.0)),
    ("haiku-3-5", price(0.8, 0.08, 1.0, 4.0)),
    // OpenAI
    ("gpt-5-6-sol", price(5.0, 0.5, 6.25, 30.0)),
    ("gpt-5-6-terra", price(2.5, 0.25, 3.125, 15.0)),
    ("gpt-5-6-luna", price(1.0, 0.1, 1.25, 6.0)),
    ("gpt-5-5-pro", price(30.0, 30.0, 0.0, 180.0)),
    ("gpt-5-5", price(5.0, 0.5, 0.0, 30.0)),
    ("gpt-5-4-pro", price(30.0, 30.0, 0.0, 180.0)),
    ("gpt-5-4-mini", price(0.75, 0.075, 0.0, 4.5)),
    ("gpt-5-4-nano", price(0.2, 0.02, 0.0, 1.25)),
    ("gpt-5-4", price(2.5, 0.25, 0.0, 15.0)),
    ("gpt-5-3-codex", price(1.75, 0.175, 0.0, 14.0)),
    // DeepSeek —— 表内为非高峰价,高峰时段(UTC 01–04、06–10)乘 2,见
    // `deepseek_peak_multiplier`。cache write 按 cache miss 计价。
    ("deepseek-v4-pro", price(0.66, 0.022, 0.66, 1.98)),
    ("deepseek-v4-flash", price(0.22, 0.007, 0.22, 0.66)),
    // Z.ai GLM
    ("glm-5-3", price(1.4, 0.26, 1.4, 4.4)),
    ("glm-5-2", price(1.4, 0.26, 1.4, 4.4)),
    ("glm-5-1", price(1.4, 0.26, 1.4, 4.4)),
    ("glm-5-turbo", price(1.2, 0.24, 1.2, 4.0)),
    ("glm-5", price(1.0, 0.2, 1.0, 3.2)),
    ("glm-4-7-flashx", price(0.07, 0.01, 0.07, 0.4)),
    ("glm-4-7-flash", price(0.0, 0.0, 0.0, 0.0)),
    ("glm-4-7", price(0.6, 0.11, 0.6, 2.2)),
    ("glm-4-6", price(0.6, 0.11, 0.6, 2.2)),
    ("glm-4-5-airx", price(1.1, 0.22, 1.1, 4.5)),
    ("glm-4-5-air", price(0.2, 0.03, 0.2, 1.1)),
    ("glm-4-5-x", price(2.2, 0.45, 2.2, 8.9)),
    ("glm-4-5-flash", price(0.0, 0.0, 0.0, 0.0)),
    ("glm-4-5", price(0.6, 0.11, 0.6, 2.2)),
    // xAI Grok
    ("grok-4-6", price(2.0, 0.5, 2.0, 6.0)),
    ("grok-4-5", price(2.0, 0.3, 2.0, 6.0)),
    ("grok-4-3", price(1.25, 0.2, 1.25, 2.5)),
    ("grok-4-20", price(1.25, 0.2, 1.25, 2.5)),
    ("grok-build", price(1.0, 0.2, 1.0, 2.0)),
    // Moonshot Kimi
    ("kimi-k3", price(3.0, 0.3, 3.0, 15.0)),
    // Xiaomi MiMo —— cache write 目前免费。
    ("mimo-v2-5-pro", price(0.435, 0.0036, 0.0, 0.87)),
    ("mimo-v2-5", price(0.14, 0.0028, 0.0, 0.28)),
    // MiniMax
    ("minimax-m3", price(0.3, 0.06, 0.3, 1.2)),
];

/// 未收录模型的兜底单价:按名字里的档位关键字推算,保证任何模型都有成本估算,
/// 而不是静默算作 0。命中此路径的请求计入 `estimated_request_count`。
fn estimated_pricing_for(model: &str) -> ModelPricing {
    if ["nano", "flash", "lite", "tiny", "air"]
        .iter()
        .any(|hint| model.contains(hint))
    {
        return price(0.2, 0.02, 0.2, 0.8);
    }
    if ["mini", "small", "haiku", "turbo"]
        .iter()
        .any(|hint| model.contains(hint))
    {
        return price(1.0, 0.1, 1.25, 5.0);
    }
    if ["opus", "fable", "pro", "max", "ultra", "sol", "sota"]
        .iter()
        .any(|hint| model.contains(hint))
    {
        return price(5.0, 0.5, 6.25, 25.0);
    }
    // 其余按主力档(Sonnet 5 / GPT-5.4 量级)估算。
    price(2.5, 0.25, 3.125, 15.0)
}

/// DeepSeek 高峰时段(UTC 01:00–04:00、06:00–10:00)单价是非高峰的 2 倍。
fn deepseek_peak_multiplier(timestamp: f64) -> f64 {
    let Some(utc) = chrono::DateTime::from_timestamp(timestamp.trunc() as i64, 0) else {
        return 1.0;
    };
    let hour = utc.hour();
    if (1..4).contains(&hour) || (6..10).contains(&hour) {
        2.0
    } else {
        1.0
    }
}

fn pricing_for_request(request: &UsageRequest) -> (ModelPricing, PricingSource) {
    let model = normalize_model(&request.model);
    let listed = MODEL_PRICES
        .iter()
        .find(|(family, _)| model.contains(family))
        .map(|(_, pricing)| *pricing);

    let (mut pricing, source) = match listed {
        Some(pricing) => (pricing, PricingSource::Listed),
        None => (estimated_pricing_for(&model), PricingSource::Estimated),
    };

    if model.contains("deepseek") {
        let multiplier = deepseek_peak_multiplier(request.timestamp);
        pricing = price(
            pricing.input * multiplier,
            pricing.cached_input * multiplier,
            pricing.cache_write * multiplier,
            pricing.output * multiplier,
        );
    }

    (pricing, source)
}

fn estimated_request_cost(request: &UsageRequest) -> (f64, PricingSource) {
    let (pricing, source) = pricing_for_request(request);
    let cost = (request.input_tokens as f64 * pricing.input
        + request.cache_read_tokens as f64 * pricing.cached_input
        + request.cache_creation_tokens as f64 * pricing.cache_write
        + request.output_tokens as f64 * pricing.output)
        / 1_000_000.0;
    (cost, source)
}

fn add_request(totals: &mut UsageStatisticsTotals, request: &UsageRequest) {
    totals.input_tokens = totals.input_tokens.saturating_add(request.input_tokens);
    totals.output_tokens = totals.output_tokens.saturating_add(request.output_tokens);
    totals.cache_creation_tokens = totals
        .cache_creation_tokens
        .saturating_add(request.cache_creation_tokens);
    totals.cache_read_tokens = totals
        .cache_read_tokens
        .saturating_add(request.cache_read_tokens);
    totals.request_count = totals.request_count.saturating_add(1);

    let (cost, source) = estimated_request_cost(request);
    totals.total_cost += cost;
    // 零 token 的占位请求(如 Claude 的 `<synthetic>` 消息)不影响单价可信度,
    // 不计入"按同档推算"的提示计数。
    let billable = request.input_tokens
        | request.output_tokens
        | request.cache_creation_tokens
        | request.cache_read_tokens;
    match source {
        PricingSource::Listed => {
            totals.priced_request_count = totals.priced_request_count.saturating_add(1);
        }
        PricingSource::Estimated if billable > 0 => {
            totals.estimated_request_count = totals.estimated_request_count.saturating_add(1);
        }
        PricingSource::Estimated => {}
    }
}

fn finalize_totals(totals: &mut UsageStatisticsTotals) {
    totals.total_tokens = totals
        .input_tokens
        .saturating_add(totals.output_tokens)
        .saturating_add(totals.cache_creation_tokens)
        .saturating_add(totals.cache_read_tokens);
    let cache_eligible = totals
        .input_tokens
        .saturating_add(totals.cache_creation_tokens)
        .saturating_add(totals.cache_read_tokens);
    totals.cache_hit_rate = if cache_eligible == 0 {
        0.0
    } else {
        totals.cache_read_tokens as f64 / cache_eligible as f64
    };
}

fn aggregate_requests(
    requests: &[UsageRequest],
    from: chrono::NaiveDate,
    to: chrono::NaiveDate,
    agent: Option<UsageAgent>,
    hourly: bool,
    min_timestamp: Option<f64>,
) -> (UsageStatisticsTotals, Vec<UsageStatisticsDay>) {
    let mut totals = UsageStatisticsTotals::default();

    if hourly {
        // 过去 24 小时滚动窗口:按 (日期, 小时) 聚合,零消耗小时不占位。
        let mut hours = BTreeMap::<(chrono::NaiveDate, u32), UsageStatisticsTotals>::new();
        for request in requests {
            if let Some(min_ts) = min_timestamp {
                if request.timestamp < min_ts {
                    continue;
                }
            }
            if request.date < from || request.date > to {
                continue;
            }
            if agent.is_some_and(|selected| selected != request.agent) {
                continue;
            }
            add_request(&mut totals, request);
            if let Some(local) =
                chrono::DateTime::from_timestamp(request.timestamp.trunc() as i64, 0)
                    .map(|timestamp| timestamp.with_timezone(&chrono::Local))
            {
                let key = (local.date_naive(), local.hour());
                add_request(hours.entry(key).or_default(), request);
            }
        }

        finalize_totals(&mut totals);
        let series = hours
            .into_iter()
            .filter_map(|((date, hour), mut bucket)| {
                finalize_totals(&mut bucket);
                // 跳过消耗为 0 的小时,图表只展示有用量的时段。
                if bucket.total_tokens == 0 {
                    return None;
                }
                Some(UsageStatisticsDay {
                    date: date.to_string(),
                    hour: Some(hour),
                    totals: bucket,
                })
            })
            .collect();
        return (totals, series);
    }

    let mut days = BTreeMap::<chrono::NaiveDate, UsageStatisticsTotals>::new();
    let mut date = from;
    while date <= to {
        days.insert(date, UsageStatisticsTotals::default());
        date += chrono::Duration::days(1);
    }

    for request in requests {
        if let Some(min_ts) = min_timestamp {
            if request.timestamp < min_ts {
                continue;
            }
        }
        if request.date < from || request.date > to {
            continue;
        }
        if agent.is_some_and(|selected| selected != request.agent) {
            continue;
        }
        add_request(&mut totals, request);
        if let Some(day) = days.get_mut(&request.date) {
            add_request(day, request);
        }
    }

    finalize_totals(&mut totals);
    let series = days
        .into_iter()
        .map(|(date, mut day)| {
            finalize_totals(&mut day);
            UsageStatisticsDay {
                date: date.to_string(),
                hour: None,
                totals: day,
            }
        })
        .collect();
    (totals, series)
}

pub(crate) fn collect_jsonl_files(root: &Path, files: &mut HashSet<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() {
            collect_jsonl_files(&path, files);
        } else if file_type.is_file() && is_usage_log(&path) {
            files.insert(path.canonicalize().unwrap_or(path));
        }
    }
}

fn is_usage_log(path: &Path) -> bool {
    path.extension().and_then(|ext| ext.to_str()) == Some("jsonl") || is_zstd_usage_log(path)
}

pub(crate) fn is_zstd_usage_log(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.ends_with(".jsonl.zstd"))
}

pub(crate) fn usage_roots() -> Vec<PathBuf> {
    let Some(home) = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
    else {
        return Vec::new();
    };
    usage_roots_for_home(&home)
}

fn usage_roots_for_home(home: &Path) -> Vec<PathBuf> {
    let mut roots = HashSet::new();

    if let Ok(entries) = std::fs::read_dir(home) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name == ".codex" || name.starts_with(".codex-") {
                roots.insert(entry.path().join("sessions"));
            }
            if name == ".claude" || name.starts_with(".claude-") {
                roots.insert(entry.path().join("projects"));
            }
            if name == ".dsh" {
                roots.insert(entry.path().join("sessions"));
            }
        }
    }

    let agent_homes = home.join(".aeroric").join("agent-homes");
    if let Ok(entries) = std::fs::read_dir(agent_homes) {
        for entry in entries.flatten() {
            roots.insert(entry.path().join("sessions"));
            roots.insert(entry.path().join("projects"));
        }
    }

    roots.into_iter().filter(|path| path.is_dir()).collect()
}

fn read_usage_statistics_sync(range_days: u32, agent: String) -> Result<UsageStatistics, String> {
    if !matches!(range_days, 1 | 7 | 14 | 30) {
        return Err("range_days must be one of 1, 7, 14, or 30".to_owned());
    }
    let selected_agent = match agent.as_str() {
        "all" => None,
        "codex" => Some(UsageAgent::Codex),
        "claude" => Some(UsageAgent::Claude),
        "dsh" => Some(UsageAgent::Dsh),
        _ => return Err("agent must be all, codex, claude, or dsh".to_owned()),
    };

    let now = chrono::Local::now();
    let to = now.date_naive();
    // range_days == 1 表示滚动过去 24 小时,而不是自然日"当天"。
    let (from, min_timestamp, hourly) = if range_days == 1 {
        let window_start = now - chrono::Duration::hours(24);
        (
            window_start.date_naive(),
            Some(window_start.timestamp() as f64),
            true,
        )
    } else {
        (
            to - chrono::Duration::days(i64::from(range_days - 1)),
            None,
            false,
        )
    };
    let requests = crate::usage_index::load_requests(from, to)?;

    let (totals, series) =
        aggregate_requests(&requests, from, to, selected_agent, hourly, min_timestamp);
    let (codex, _) = aggregate_requests(
        &requests,
        from,
        to,
        Some(UsageAgent::Codex),
        false,
        min_timestamp,
    );
    let (claude, _) = aggregate_requests(
        &requests,
        from,
        to,
        Some(UsageAgent::Claude),
        false,
        min_timestamp,
    );
    let (dsh, _) = aggregate_requests(
        &requests,
        from,
        to,
        Some(UsageAgent::Dsh),
        false,
        min_timestamp,
    );

    Ok(UsageStatistics {
        range_days,
        from: from.to_string(),
        to: to.to_string(),
        agent,
        updated_at: crate::usage_index::latest_updated_at()?,
        totals,
        series,
        breakdown: UsageStatisticsBreakdown { codex, claude, dsh },
    })
}

#[tauri::command]
pub async fn read_usage_statistics(
    range_days: u32,
    agent: String,
) -> Result<UsageStatistics, String> {
    tokio::task::spawn_blocking(move || read_usage_statistics_sync(range_days, agent))
        .await
        .map_err(|error| format!("read_usage_statistics join error: {error}"))?
}

#[cfg(test)]
mod session_metrics_tests {
    use super::*;

    #[test]
    fn dsh_usage_requests_track_route_model_and_usage_fields() {
        let content = concat!(
            r#"{"type":"session","version":0,"id":"s1","createdAt":1755100000000,"delegationDepth":0}"#,
            "\n",
            r#"{"type":"request/context","seq":0,"time":1755100000100,"data":{"provider":"deepseek-official","model":"deepseek-v4-flash"}}"#,
            "\n",
            r#"{"type":"assistant/message","seq":1,"time":1755100001000,"data":{"turn":0,"step":0,"message":{"role":"assistant","content":[]},"usage":{"inputTokens":100,"outputTokens":20,"cacheReadTokens":50,"cacheWriteTokens":5,"reasoningTokens":7}}}"#,
            "\n",
            r#"{"type":"request/context","seq":2,"time":1755100002000,"data":{"provider":"deepseek-official","model":"deepseek-v4-pro"}}"#,
            "\n",
            r#"{"type":"assistant/message","seq":3,"time":1755100003000,"data":{"turn":0,"step":1,"message":{"role":"assistant","content":[]},"usage":{"inputTokens":10,"outputTokens":2}}}"#,
            "\n",
            r#"{"type":"assistant/message","seq":4,"time":1755100004000,"data":{"turn":0,"step":2,"message":{"role":"assistant","content":[]}}}"#,
            "\n",
        );
        assert!(is_dsh_session(content));
        assert!(!is_codex_session(content));
        let requests = parse_dsh_usage_requests(content);
        // 无 usage 的 assistant/message 不计请求。
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0].agent, UsageAgent::Dsh);
        assert_eq!(requests[0].model, "deepseek-v4-flash");
        assert_eq!(requests[0].input_tokens, 100);
        assert_eq!(requests[0].output_tokens, 20);
        assert_eq!(requests[0].cache_read_tokens, 50);
        assert_eq!(requests[0].cache_creation_tokens, 5);
        assert_eq!(requests[1].model, "deepseek-v4-pro");
        // dsh 请求同样按公开价目计价,不再落进"未定价"。
        let mut totals = UsageStatisticsTotals::default();
        add_request(&mut totals, &requests[0]);
        assert!(totals.total_cost > 0.0);
        assert_eq!(totals.priced_request_count, 1);
        assert_eq!(totals.estimated_request_count, 0);
    }

    #[test]
    fn usage_log_discovery_includes_jsonl_and_jsonl_zstd_only() {
        let root =
            std::env::temp_dir().join(format!("aeroric-usage-discovery-{}", uuid::Uuid::new_v4()));
        let nested = root.join("nested");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(root.join("plain.jsonl"), b"{}\n").unwrap();
        std::fs::write(nested.join("session.jsonl.zstd"), b"compressed").unwrap();
        std::fs::write(root.join("ignored.zstd"), b"compressed").unwrap();
        std::fs::write(root.join("ignored.json"), b"{}").unwrap();

        let mut files = HashSet::new();
        collect_jsonl_files(&root, &mut files);
        assert_eq!(files.len(), 2);
        assert!(files.iter().any(|path| path.ends_with("plain.jsonl")));
        assert!(files
            .iter()
            .any(|path| path.ends_with("session.jsonl.zstd")));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn usage_roots_include_official_and_managed_dsh_sessions() {
        let home =
            std::env::temp_dir().join(format!("aeroric-usage-roots-{}", uuid::Uuid::new_v4()));
        let official = home.join(".dsh").join("sessions");
        let managed = home
            .join(".aeroric")
            .join("agent-homes")
            .join("dsh")
            .join("sessions");
        std::fs::create_dir_all(&official).unwrap();
        std::fs::create_dir_all(&managed).unwrap();

        let roots = usage_roots_for_home(&home);
        assert!(roots.contains(&official));
        assert!(roots.contains(&managed));

        let _ = std::fs::remove_dir_all(home);
    }

    /// Claude 顶栏必须能拿到时长 / TOKENS / 上下文三项。窗口大小 transcript 里没有，
    /// 由 model 推导。
    #[test]
    fn claude_metrics_expose_duration_tokens_and_context_window() {
        let content = concat!(
            r#"{"type":"user","timestamp":"2026-08-13T05:58:04.000Z","message":{"role":"user"}}"#,
            "\n",
            r#"{"type":"assistant","timestamp":"2026-08-13T05:58:10.000Z","message":{"model":"claude-opus-5","usage":{"input_tokens":100,"output_tokens":20,"cache_creation_input_tokens":5,"cache_read_input_tokens":1000},"content":[{"type":"tool_use"}]}}"#,
            "\n",
            r#"{"type":"assistant","timestamp":"2026-08-13T05:59:04.000Z","message":{"model":"claude-opus-5","usage":{"input_tokens":200,"output_tokens":30,"cache_creation_input_tokens":0,"cache_read_input_tokens":4000}}}"#,
            "\n",
        );

        let metrics = parse_claude_metrics(content);
        assert_eq!(metrics.duration_secs, 60.0);
        assert_eq!(metrics.total_tokens, 100 + 20 + 5 + 1000 + 200 + 30 + 4000);
        // 上下文 = 最后一轮 prompt 大小（input + cache_creation + cache_read）
        assert_eq!(metrics.context_tokens, 200 + 4000);
        assert_eq!(metrics.context_window, 200_000);
        assert_eq!(metrics.tool_calls, 1);
    }

    /// 第三方中转的 model slug 推导不出窗口时保持 0，让前端只显示占用量而不是编造百分比。
    #[test]
    fn unknown_claude_model_leaves_the_context_window_unset() {
        let content = concat!(
            r#"{"type":"assistant","timestamp":"2026-08-13T05:58:10.000Z","message":{"model":"z-ai/glm-5.2","usage":{"input_tokens":10,"output_tokens":2,"cache_read_input_tokens":30}}}"#,
            "\n",
        );
        let metrics = parse_claude_metrics(content);
        assert_eq!(metrics.context_window, 0);
        assert_eq!(metrics.context_tokens, 40);
        assert_eq!(metrics.total_tokens, 42);
    }

    #[test]
    fn codex_metrics_still_read_the_window_from_the_transcript() {
        let content = concat!(
            r#"{"type":"session_meta","timestamp":"2026-08-13T05:58:00.000Z","payload":{"type":"session_meta"}}"#,
            "\n",
            r#"{"type":"event_msg","timestamp":"2026-08-13T05:58:30.000Z","payload":{"type":"token_count","info":{"total_token_usage":{"total_tokens":5000},"last_token_usage":{"total_tokens":1200},"model_context_window":258400}}}"#,
            "\n",
        );
        let metrics = parse_codex_metrics(content);
        assert_eq!(metrics.total_tokens, 5000);
        assert_eq!(metrics.context_tokens, 1200);
        assert_eq!(metrics.context_window, 258_400);
    }
}

#[cfg(test)]
mod usage_statistics_tests {
    use super::*;

    fn date(year: i32, month: u32, day: u32) -> chrono::NaiveDate {
        chrono::NaiveDate::from_ymd_opt(year, month, day).unwrap()
    }

    #[test]
    fn claude_duplicate_message_ids_count_once_and_keep_latest_record() {
        let content = r#"
{"timestamp":"2026-07-15T01:00:00Z","type":"assistant","uuid":"outer-1","message":{"id":"msg-1","model":"claude-opus-4.8","usage":{"input_tokens":10,"output_tokens":2,"cache_creation_input_tokens":3,"cache_read_input_tokens":4}}}
{"timestamp":"2026-07-15T01:00:01Z","type":"assistant","uuid":"outer-2","message":{"id":"msg-1","model":"claude-opus-4.8","usage":{"input_tokens":10,"output_tokens":5,"cache_creation_input_tokens":3,"cache_read_input_tokens":4}}}
"#;
        let requests = parse_claude_usage_requests(content, "test");

        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].output_tokens, 5);
    }

    #[test]
    fn codex_uses_last_token_usage_and_splits_cached_input() {
        let content = r#"
{"timestamp":"2026-07-15T01:00:00Z","type":"turn_context","payload":{"model":"gpt-5.5"}}
{"timestamp":"2026-07-15T01:00:01Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":9999,"cached_input_tokens":5000,"output_tokens":999,"reasoning_output_tokens":100},"last_token_usage":{"input_tokens":120,"cached_input_tokens":80,"output_tokens":20,"reasoning_output_tokens":7}}}}
"#;
        let requests = parse_codex_usage_requests(content);

        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].input_tokens, 40);
        assert_eq!(requests[0].cache_read_tokens, 80);
        assert_eq!(requests[0].output_tokens, 27);
    }

    #[test]
    fn aggregation_filters_dates_and_calculates_cache_hit_rate() {
        let requests = vec![
            UsageRequest {
                timestamp: 1.0,
                date: date(2026, 7, 14),
                agent: UsageAgent::Codex,
                model: "gpt-5.5".to_owned(),
                input_tokens: 50,
                output_tokens: 10,
                cache_creation_tokens: 0,
                cache_read_tokens: 50,
            },
            UsageRequest {
                timestamp: 2.0,
                date: date(2026, 7, 1),
                agent: UsageAgent::Codex,
                model: "gpt-5.5".to_owned(),
                input_tokens: 1000,
                output_tokens: 1000,
                cache_creation_tokens: 0,
                cache_read_tokens: 0,
            },
        ];

        let (totals, series) = aggregate_requests(
            &requests,
            date(2026, 7, 14),
            date(2026, 7, 15),
            None,
            false,
            None,
        );

        assert_eq!(totals.request_count, 1);
        assert_eq!(totals.total_tokens, 110);
        assert_eq!(totals.cache_hit_rate, 0.5);
        assert_eq!(series.len(), 2);
        assert_eq!(series[1].totals.request_count, 0);
    }

    #[test]
    fn hourly_aggregation_ends_with_the_current_local_hour() {
        let now = chrono::Local::now();
        let today = now.date_naive();
        let requests = vec![UsageRequest {
            timestamp: now.timestamp() as f64,
            date: today,
            agent: UsageAgent::Codex,
            model: "gpt-5.5".to_owned(),
            input_tokens: 50,
            output_tokens: 10,
            cache_creation_tokens: 0,
            cache_read_tokens: 20,
        }];

        let window_start = now - chrono::Duration::hours(24);
        let (totals, series) = aggregate_requests(
            &requests,
            window_start.date_naive(),
            today,
            None,
            true,
            Some(window_start.timestamp() as f64),
        );

        assert_eq!(totals.request_count, 1);
        assert_eq!(series.len(), 1);
        assert_eq!(
            series.last().and_then(|bucket| bucket.hour),
            Some(now.hour())
        );
        assert_eq!(
            series.last().map(|bucket| bucket.totals.request_count),
            Some(1)
        );
        assert_eq!(
            series.last().map(|bucket| bucket.date.clone()),
            Some(today.to_string())
        );
    }

    #[test]
    fn hourly_aggregation_uses_rolling_24h_window_and_skips_empty_hours() {
        let now = chrono::Local::now();
        let today = now.date_naive();
        let yesterday = today - chrono::Duration::days(1);
        let window_start = now - chrono::Duration::hours(24);
        let in_window_old = (now - chrono::Duration::hours(20)).timestamp() as f64;
        let out_of_window = (now - chrono::Duration::hours(30)).timestamp() as f64;
        let current = now.timestamp() as f64;
        let old_local = chrono::DateTime::from_timestamp(in_window_old.trunc() as i64, 0)
            .unwrap()
            .with_timezone(&chrono::Local);

        let requests = vec![
            UsageRequest {
                timestamp: out_of_window,
                date: yesterday,
                agent: UsageAgent::Codex,
                model: "gpt-5.5".to_owned(),
                input_tokens: 999,
                output_tokens: 999,
                cache_creation_tokens: 0,
                cache_read_tokens: 0,
            },
            UsageRequest {
                timestamp: in_window_old,
                date: old_local.date_naive(),
                agent: UsageAgent::Codex,
                model: "gpt-5.5".to_owned(),
                input_tokens: 40,
                output_tokens: 10,
                cache_creation_tokens: 0,
                cache_read_tokens: 0,
            },
            UsageRequest {
                timestamp: current,
                date: today,
                agent: UsageAgent::Claude,
                model: "claude-sonnet-4".to_owned(),
                input_tokens: 20,
                output_tokens: 5,
                cache_creation_tokens: 0,
                cache_read_tokens: 0,
            },
        ];

        let (totals, series) = aggregate_requests(
            &requests,
            window_start.date_naive(),
            today,
            None,
            true,
            Some(window_start.timestamp() as f64),
        );

        assert_eq!(totals.request_count, 2);
        assert_eq!(totals.input_tokens, 60);
        assert!(!series.is_empty());
        assert!(series.iter().all(|bucket| bucket.totals.total_tokens > 0));
        assert!(series
            .iter()
            .any(|bucket| { bucket.date == today.to_string() && bucket.hour == Some(now.hour()) }));
        // 零消耗小时不会出现在 series 中。
        assert!(series.iter().all(|bucket| bucket.totals.request_count > 0));
    }

    #[test]
    fn unknown_models_still_get_an_estimated_cost() {
        let requests = vec![UsageRequest {
            timestamp: 1.0,
            date: date(2026, 7, 15),
            agent: UsageAgent::Codex,
            model: "custom-private-model".to_owned(),
            input_tokens: 100,
            output_tokens: 20,
            cache_creation_tokens: 0,
            cache_read_tokens: 0,
        }];

        let (totals, _) = aggregate_requests(
            &requests,
            date(2026, 7, 15),
            date(2026, 7, 15),
            None,
            false,
            None,
        );

        assert_eq!(totals.priced_request_count, 0);
        assert_eq!(totals.estimated_request_count, 1);
        assert!(totals.total_cost > 0.0);
    }

    fn pricing_of(model: &str, agent: UsageAgent) -> (ModelPricing, PricingSource) {
        pricing_for_request(&UsageRequest {
            timestamp: 0.0,
            date: date(2026, 8, 21),
            agent,
            model: model.to_owned(),
            input_tokens: 0,
            output_tokens: 0,
            cache_creation_tokens: 0,
            cache_read_tokens: 0,
        })
    }

    #[test]
    fn model_names_normalize_across_harnesses() {
        assert_eq!(
            normalize_model("anthropic/claude-opus-5-aws"),
            "claude-opus-5"
        );
        assert_eq!(normalize_model("claude-opus-4.8"), "claude-opus-4-8");
        assert_eq!(
            normalize_model("claude-opus-4-6-thinking"),
            "claude-opus-4-6"
        );
        assert_eq!(normalize_model("gpt-5.4-mini-2026-03-17"), "gpt-5-4-mini");
        assert_eq!(normalize_model("deepseek-v4-pro-0813"), "deepseek-v4-pro");
        assert_eq!(
            normalize_model("DeepSeek-V4-Flash-0731"),
            "deepseek-v4-flash"
        );
        assert_eq!(normalize_model("z-ai/glm-5.2"), "glm-5-2");
        assert_eq!(
            normalize_model("grok-4.20-multi-agent-xhigh"),
            "grok-4-20-multi-agent"
        );
    }

    #[test]
    fn the_same_model_is_priced_identically_under_every_agent() {
        // 同一 Opus 5 会以裸名、路由前缀名、点号写法出现在不同 harness 的日志里。
        for model in [
            "claude-opus-5",
            "anthropic/claude-opus-5-ps-aws-dst",
            "claude-opus-5-2026-07-24",
        ] {
            for agent in [UsageAgent::Claude, UsageAgent::Codex, UsageAgent::Dsh] {
                let (pricing, source) = pricing_of(model, agent);
                assert_eq!(source, PricingSource::Listed, "{model}");
                assert_eq!(pricing.input, 5.0, "{model}");
                assert_eq!(pricing.output, 25.0, "{model}");
                assert_eq!(pricing.cache_write, 6.25, "{model}");
                assert_eq!(pricing.cached_input, 0.5, "{model}");
            }
        }
    }

    #[test]
    fn longer_families_win_over_their_own_prefixes() {
        assert_eq!(
            pricing_of("claude-opus-4-8", UsageAgent::Claude).0.input,
            5.0
        );
        assert_eq!(
            pricing_of("claude-opus-4-1", UsageAgent::Claude).0.input,
            15.0
        );
        assert_eq!(
            pricing_of("claude-sonnet-4-6", UsageAgent::Claude).0.input,
            3.0
        );
        assert_eq!(
            pricing_of("claude-sonnet-5", UsageAgent::Claude).0.input,
            2.0
        );
        assert_eq!(pricing_of("glm-5.2", UsageAgent::Codex).0.input, 1.4);
        assert_eq!(pricing_of("glm-5", UsageAgent::Codex).0.input, 1.0);
        assert_eq!(pricing_of("mimo-v2.5-pro", UsageAgent::Dsh).0.input, 0.435);
        assert_eq!(pricing_of("mimo-v2.5", UsageAgent::Claude).0.input, 0.14);
    }

    #[test]
    fn sonnet_5_keeps_its_two_dollar_rate_after_august_2026() {
        // $2/$10 已从"限时"转为长期价,9/1 不再上调。
        let (pricing, source) = pricing_for_request(&UsageRequest {
            timestamp: 0.0,
            date: date(2026, 12, 1),
            agent: UsageAgent::Claude,
            model: "claude-sonnet-5".to_owned(),
            input_tokens: 0,
            output_tokens: 0,
            cache_creation_tokens: 0,
            cache_read_tokens: 0,
        });
        assert_eq!(source, PricingSource::Listed);
        assert_eq!(pricing.input, 2.0);
        assert_eq!(pricing.output, 10.0);
    }

    #[test]
    fn deepseek_peak_hours_double_the_rate() {
        let at_utc_hour = |hour: u32| {
            chrono::NaiveDate::from_ymd_opt(2026, 8, 21)
                .and_then(|day| day.and_hms_opt(hour, 30, 0))
                .map(|naive| naive.and_utc().timestamp() as f64)
                .unwrap()
        };

        let off_peak = pricing_for_request(&UsageRequest {
            timestamp: at_utc_hour(0),
            date: date(2026, 8, 21),
            agent: UsageAgent::Dsh,
            model: "deepseek-v4-pro-0813".to_owned(),
            input_tokens: 0,
            output_tokens: 0,
            cache_creation_tokens: 0,
            cache_read_tokens: 0,
        });
        let peak = pricing_for_request(&UsageRequest {
            timestamp: at_utc_hour(2),
            date: date(2026, 8, 21),
            agent: UsageAgent::Dsh,
            model: "deepseek-v4-pro-0813".to_owned(),
            input_tokens: 0,
            output_tokens: 0,
            cache_creation_tokens: 0,
            cache_read_tokens: 0,
        });

        assert_eq!(off_peak.0.input, 0.66);
        assert_eq!(off_peak.0.output, 1.98);
        assert_eq!(peak.0.input, 1.32);
        assert_eq!(peak.0.output, 3.96);
    }

    #[test]
    fn zero_token_placeholder_requests_do_not_flag_estimated_pricing() {
        let requests = vec![UsageRequest {
            timestamp: 1.0,
            date: date(2026, 7, 15),
            agent: UsageAgent::Claude,
            model: "<synthetic>".to_owned(),
            input_tokens: 0,
            output_tokens: 0,
            cache_creation_tokens: 0,
            cache_read_tokens: 0,
        }];

        let (totals, _) = aggregate_requests(
            &requests,
            date(2026, 7, 15),
            date(2026, 7, 15),
            None,
            false,
            None,
        );

        assert_eq!(totals.request_count, 1);
        assert_eq!(totals.estimated_request_count, 0);
        assert_eq!(totals.total_cost, 0.0);
    }
}
