// ── Session metrics ───────────────────────────────────────────────────────────

use chrono::Timelike;
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use serde_json::Value;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
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
static METRICS_CACHE: Lazy<Mutex<HashMap<String, (SystemTime, SessionMetrics)>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

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

fn parse_claude_metrics(content: &str) -> SessionMetrics {
    let mut input_tokens: u64 = 0;
    let mut output_tokens: u64 = 0;
    let mut cache_creation: u64 = 0;
    let mut cache_read: u64 = 0;
    let mut tool_calls: u64 = 0;
    let mut last_context: u64 = 0;
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
        context_window: 0, // Claude session 不带窗口大小
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
        if !path.exists() {
            return Err(format!("Session file not found: {}", session_path));
        }
        Ok(parse_session_metrics_cached(path))
    })
    .await
    .map_err(|e| format!("read_session_metrics join error: {}", e))?
}

// ── Local usage statistics ──────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum UsageAgent {
    Codex,
    Claude,
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
    pub(crate) priced_request_count: u64,
    pub(crate) unpriced_request_count: u64,
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

#[derive(Clone, Copy)]
struct ModelPricing {
    input: f64,
    cached_input: f64,
    cache_write: f64,
    output: f64,
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

fn pricing_for_request(request: &UsageRequest) -> Option<ModelPricing> {
    let model = request.model.to_ascii_lowercase();

    if request.agent == UsageAgent::Codex {
        let pricing = if model.starts_with("gpt-5.6-sol") {
            (5.0, 0.5, 6.25, 30.0)
        } else if model.starts_with("gpt-5.6-terra") {
            (2.5, 0.25, 3.125, 15.0)
        } else if model.starts_with("gpt-5.6-luna") {
            (1.0, 0.1, 1.25, 6.0)
        } else if model.starts_with("gpt-5.5-pro") || model.starts_with("gpt-5.4-pro") {
            (30.0, 30.0, 0.0, 180.0)
        } else if model.starts_with("gpt-5.5") {
            (5.0, 0.5, 0.0, 30.0)
        } else if model.starts_with("gpt-5.4-mini") {
            (0.75, 0.075, 0.0, 4.5)
        } else if model.starts_with("gpt-5.4-nano") {
            (0.2, 0.02, 0.0, 1.25)
        } else if model.starts_with("gpt-5.4") {
            (2.5, 0.25, 0.0, 15.0)
        } else if model.starts_with("gpt-5.3-codex") {
            (1.75, 0.175, 0.0, 14.0)
        } else {
            return None;
        };
        return Some(ModelPricing {
            input: pricing.0,
            cached_input: pricing.1,
            cache_write: pricing.2,
            output: pricing.3,
        });
    }

    let pricing = if model.contains("opus-4-8")
        || model.contains("opus-4.8")
        || model.contains("opus-4-7")
        || model.contains("opus-4.7")
        || model.contains("opus-4-6")
        || model.contains("opus-4.6")
        || model.contains("opus-4-5")
        || model.contains("opus-4.5")
    {
        (5.0, 0.5, 6.25, 25.0)
    } else if model.contains("sonnet-5") {
        let introductory_end = chrono::NaiveDate::from_ymd_opt(2026, 8, 31).unwrap();
        if request.date <= introductory_end {
            (2.0, 0.2, 2.5, 10.0)
        } else {
            (3.0, 0.3, 3.75, 15.0)
        }
    } else if model.contains("sonnet-4") {
        (3.0, 0.3, 3.75, 15.0)
    } else if model.contains("haiku-4-5") || model.contains("haiku-4.5") {
        (1.0, 0.1, 1.25, 5.0)
    } else if model.contains("haiku-3-5") || model.contains("haiku-3.5") {
        (0.8, 0.08, 1.0, 4.0)
    } else {
        return None;
    };

    Some(ModelPricing {
        input: pricing.0,
        cached_input: pricing.1,
        cache_write: pricing.2,
        output: pricing.3,
    })
}

fn estimated_request_cost(request: &UsageRequest) -> Option<f64> {
    let pricing = pricing_for_request(request)?;
    Some(
        (request.input_tokens as f64 * pricing.input
            + request.cache_read_tokens as f64 * pricing.cached_input
            + request.cache_creation_tokens as f64 * pricing.cache_write
            + request.output_tokens as f64 * pricing.output)
            / 1_000_000.0,
    )
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
    if let Some(cost) = estimated_request_cost(request) {
        totals.total_cost += cost;
        totals.priced_request_count = totals.priced_request_count.saturating_add(1);
    } else {
        totals.unpriced_request_count = totals.unpriced_request_count.saturating_add(1);
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
) -> (UsageStatisticsTotals, Vec<UsageStatisticsDay>) {
    let mut totals = UsageStatisticsTotals::default();

    if hourly {
        let current_hour = chrono::Local::now().hour();
        let mut hours = BTreeMap::<u32, UsageStatisticsTotals>::new();
        for hour in 0..=current_hour {
            hours.insert(hour, UsageStatisticsTotals::default());
        }

        for request in requests {
            if request.date != from || request.date > to {
                continue;
            }
            if agent.is_some_and(|selected| selected != request.agent) {
                continue;
            }
            add_request(&mut totals, request);
            let hour = chrono::DateTime::from_timestamp(request.timestamp.trunc() as i64, 0)
                .map(|timestamp| timestamp.with_timezone(&chrono::Local).hour())
                .unwrap_or_default();
            if let Some(bucket) = hours.get_mut(&hour) {
                add_request(bucket, request);
            }
        }

        finalize_totals(&mut totals);
        let series = hours
            .into_iter()
            .map(|(hour, mut bucket)| {
                finalize_totals(&mut bucket);
                UsageStatisticsDay {
                    date: from.to_string(),
                    hour: Some(hour),
                    totals: bucket,
                }
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
        } else if file_type.is_file()
            && path.extension().and_then(|ext| ext.to_str()) == Some("jsonl")
        {
            files.insert(path.canonicalize().unwrap_or(path));
        }
    }
}

pub(crate) fn usage_roots() -> Vec<PathBuf> {
    let Some(home) = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
    else {
        return Vec::new();
    };
    let mut roots = HashSet::new();

    if let Ok(entries) = std::fs::read_dir(&home) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name == ".codex" || name.starts_with(".codex-") {
                roots.insert(entry.path().join("sessions"));
            }
            if name == ".claude" || name.starts_with(".claude-") {
                roots.insert(entry.path().join("projects"));
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
        _ => return Err("agent must be all, codex, or claude".to_owned()),
    };

    let to = chrono::Local::now().date_naive();
    let from = to - chrono::Duration::days(i64::from(range_days - 1));
    let requests = crate::usage_index::load_requests(from, to)?;

    let (totals, series) = aggregate_requests(&requests, from, to, selected_agent, range_days == 1);
    let (codex, _) = aggregate_requests(&requests, from, to, Some(UsageAgent::Codex), false);
    let (claude, _) = aggregate_requests(&requests, from, to, Some(UsageAgent::Claude), false);

    Ok(UsageStatistics {
        range_days,
        from: from.to_string(),
        to: to.to_string(),
        agent,
        updated_at: crate::usage_index::latest_updated_at()?,
        totals,
        series,
        breakdown: UsageStatisticsBreakdown { codex, claude },
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

        let (totals, series) =
            aggregate_requests(&requests, date(2026, 7, 14), date(2026, 7, 15), None, false);

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

        let (totals, series) = aggregate_requests(&requests, today, today, None, true);

        assert_eq!(totals.request_count, 1);
        assert_eq!(series.len(), now.hour() as usize + 1);
        assert_eq!(
            series.last().and_then(|bucket| bucket.hour),
            Some(now.hour())
        );
        assert_eq!(
            series.last().map(|bucket| bucket.totals.request_count),
            Some(1)
        );
    }

    #[test]
    fn unknown_models_are_reported_as_unpriced() {
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

        let (totals, _) =
            aggregate_requests(&requests, date(2026, 7, 15), date(2026, 7, 15), None, false);

        assert_eq!(totals.priced_request_count, 0);
        assert_eq!(totals.unpriced_request_count, 1);
        assert_eq!(totals.total_cost, 0.0);
    }
}
