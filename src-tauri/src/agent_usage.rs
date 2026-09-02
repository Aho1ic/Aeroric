//! Agent 配置的使用频次账本。
//!
//! 为什么需要它:首页选配置的列表原来恒为「内置在前 + 档案声明序」,常用的配置可能
//! 排在最底下。要按「最近用得多的排前面」排序,就需要一份持久的使用记录 —— 应用设置
//! 里没有这个维度,任务列表里也只有当前 agent 而没有历次选择的计数。
//!
//! 存储形态是**按本地日期分桶**而不是存原始时间戳:
//! - 7 天窗口的口径是「日历日」(今天 + 前 6 天),分桶后求和即可,不必扫时间戳;
//! - 条数有上界(裁剪窗口内每个 agent 最多几十个桶),不会随使用次数无界增长。
//!
//! `total` 是终身计数,**永不裁剪**。它承担「7 天内都是 0 时按历史总次数排序」这条
//! 兜底规则 —— 如果总次数也从桶里求和,裁剪一发生这条规则就失真了。

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::sync::OnceLock;

use chrono::{Duration, Local, NaiveDate};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use crate::storage::atomic_write;

/// 当前 Unix epoch 毫秒。
///
/// 本地留一份而不是引用别处的共享助手:这个模块只需要这一个语义,不想为它拖一条
/// 跨模块依赖。仓库里若已有统一的时间戳助手,把这里换过去是安全的 —— 语义一致。
fn now_ms_i64() -> i64 {
    Local::now().timestamp_millis()
}

/// 排序主键的窗口宽度(日历日,含今天)。
const USAGE_WINDOW_DAYS: i64 = 7;

/// 日桶保留多久。比窗口宽得多,给「改窗口宽度」留余量,也让近期趋势可回溯;
/// 裁掉的只是桶,`total` 不受影响。
const DAILY_BUCKET_RETENTION_DAYS: i64 = 60;

static AGENT_USAGE_STORE_MUTEX: OnceLock<Mutex<()>> = OnceLock::new();

fn agent_usage_store_mutex() -> &'static Mutex<()> {
    AGENT_USAGE_STORE_MUTEX.get_or_init(|| Mutex::new(()))
}

fn store_path() -> Result<PathBuf, String> {
    Ok(crate::storage::aeroric_dir()?.join("agent-usage.json"))
}

/// 单个配置的账目。
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
struct AgentUsageRecord {
    /// 终身使用次数。裁剪日桶不动它。
    #[serde(default)]
    total: u64,
    /// 最后一次使用的 epoch 毫秒。0 表示从未使用。
    #[serde(default)]
    last_used_at: i64,
    /// 本地日期(`YYYY-MM-DD`)-> 当天次数。
    #[serde(default)]
    daily: BTreeMap<String, u64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct AgentUsageStore {
    #[serde(default)]
    version: u32,
    #[serde(default)]
    agents: BTreeMap<String, AgentUsageRecord>,
}

/// 前端拿到的那份快照:每个配置只暴露排序需要的三个数。
#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentUsageStats {
    /// 近 `window_days` 个日历日内的次数(排序主键)。
    pub recent_count: u64,
    /// 终身次数(主键全为 0 时的兜底)。
    pub total_count: u64,
    /// 最后一次使用的 epoch 毫秒;0 表示从未使用(次数相同时的第二排序键)。
    pub last_used_at: i64,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentUsageSnapshot {
    pub agents: BTreeMap<String, AgentUsageStats>,
    /// 让前端知道主键的口径,不必和后端各存一份常量。
    pub window_days: i64,
    /// 快照的计算时刻(epoch 毫秒)。
    pub computed_at: i64,
}

fn load_store() -> AgentUsageStore {
    let Ok(path) = store_path() else {
        return AgentUsageStore::default();
    };
    // 读不到(首次运行)或解不开(手工改坏)都退到空账本:排序退化成原目录序,
    // 比让首页选择器报错强得多。
    match fs::read_to_string(&path) {
        Ok(data) => serde_json::from_str(&data).unwrap_or_default(),
        Err(_) => AgentUsageStore::default(),
    }
}

fn save_store(store: &AgentUsageStore) -> Result<(), String> {
    let path = store_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let json = serde_json::to_string_pretty(store).map_err(|error| error.to_string())?;
    atomic_write(&path, &json)
}

fn today_local() -> NaiveDate {
    Local::now().date_naive()
}

fn date_key(date: NaiveDate) -> String {
    date.format("%Y-%m-%d").to_string()
}

/// 窗口内最早那一天。`window_days = 7` 时是「今天减 6 天」,即今天连同前 6 天共 7 天。
fn window_start(today: NaiveDate, window_days: i64) -> NaiveDate {
    today - Duration::days(window_days.max(1) - 1)
}

/// 日桶里落在 `[start, today]` 之间的次数之和。
///
/// 解析不出日期的 key 一律跳过而不是当成今天 —— 把脏数据算进主键会让排序莫名其妙。
fn count_in_window(record: &AgentUsageRecord, today: NaiveDate, window_days: i64) -> u64 {
    let start = window_start(today, window_days);
    record
        .daily
        .iter()
        .filter_map(|(key, count)| {
            let date = key.parse::<NaiveDate>().ok()?;
            (date >= start && date <= today).then_some(*count)
        })
        .sum()
}

/// 裁掉过老的日桶。只动 `daily`,`total` 与 `last_used_at` 保持原样。
fn prune_daily(record: &mut AgentUsageRecord, today: NaiveDate) {
    let cutoff = today - Duration::days(DAILY_BUCKET_RETENTION_DAYS);
    record.daily.retain(|key, _| {
        key.parse::<NaiveDate>()
            .map(|date| date >= cutoff)
            .unwrap_or(false)
    });
}

fn snapshot_from(store: &AgentUsageStore, today: NaiveDate) -> AgentUsageSnapshot {
    AgentUsageSnapshot {
        agents: store
            .agents
            .iter()
            .map(|(agent, record)| {
                (
                    agent.clone(),
                    AgentUsageStats {
                        recent_count: count_in_window(record, today, USAGE_WINDOW_DAYS),
                        total_count: record.total,
                        last_used_at: record.last_used_at,
                    },
                )
            })
            .collect(),
        window_days: USAGE_WINDOW_DAYS,
        computed_at: now_ms_i64(),
    }
}

/// 记一次使用,并返回记完之后的整份快照。
///
/// 返回整份而不是只返回这一个 agent:前端要拿它直接替换内存里的排序依据,
/// 只回一条的话调用方还得自己合并,容易和并发的另一次记录打架。
#[tauri::command]
pub fn record_agent_config_usage(agent: String) -> Result<AgentUsageSnapshot, String> {
    let agent = agent.trim().to_string();
    if agent.is_empty() {
        return Err("agent id is empty".to_string());
    }
    let today = today_local();
    let _guard = agent_usage_store_mutex().lock();
    let mut store = load_store();
    store.version = 1;
    let record = store.agents.entry(agent).or_default();
    record.total = record.total.saturating_add(1);
    record.last_used_at = now_ms_i64();
    let bucket = record.daily.entry(date_key(today)).or_insert(0);
    *bucket = bucket.saturating_add(1);
    prune_daily(record, today);
    save_store(&store)?;
    Ok(snapshot_from(&store, today))
}

/// 读一份当前快照。窗口按**读取时**的本地日期算,所以跨过零点后重新读一次就会滑动。
#[tauri::command]
pub fn load_agent_usage_snapshot() -> Result<AgentUsageSnapshot, String> {
    let _guard = agent_usage_store_mutex().lock();
    let store = load_store();
    Ok(snapshot_from(&store, today_local()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(year: i32, month: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(year, month, day).expect("valid date")
    }

    fn record_with_days(days: &[(&str, u64)]) -> AgentUsageRecord {
        AgentUsageRecord {
            total: days.iter().map(|(_, count)| *count).sum(),
            last_used_at: 1_700_000_000_000,
            daily: days
                .iter()
                .map(|(key, count)| ((*key).to_string(), *count))
                .collect(),
        }
    }

    /// 窗口是 7 个日历日:今天与「今天减 6 天」都在内,「今天减 7 天」在外。
    /// 这条边界是排序主键的定义,写歪一天不会有任何报错,只会静默排错。
    #[test]
    fn window_covers_today_and_six_days_back() {
        let today = date(2026, 9, 2);
        let record = record_with_days(&[
            ("2026-09-02", 1), // 今天
            ("2026-08-27", 2), // 今天 - 6,在内
            ("2026-08-26", 4), // 今天 - 7,在外
        ]);

        assert_eq!(window_start(today, 7), date(2026, 8, 27));
        assert_eq!(count_in_window(&record, today, 7), 3);
    }

    /// 未来日期的桶(用户改过系统时钟)不该算进窗口。
    #[test]
    fn window_excludes_future_buckets() {
        let today = date(2026, 9, 2);
        let record = record_with_days(&[("2026-09-03", 5), ("2026-09-02", 1)]);

        assert_eq!(count_in_window(&record, today, 7), 1);
    }

    #[test]
    fn window_skips_unparseable_bucket_keys() {
        let today = date(2026, 9, 2);
        let record = record_with_days(&[("not-a-date", 9), ("2026-09-02", 2)]);

        assert_eq!(count_in_window(&record, today, 7), 2);
    }

    /// 裁剪只动日桶。`total` 是「7 天内全是 0 时」的排序依据,被裁掉就等于这条规则失效。
    #[test]
    fn prune_drops_old_buckets_but_keeps_total() {
        let today = date(2026, 9, 2);
        let mut record = record_with_days(&[("2026-09-02", 1), ("2026-01-01", 7)]);
        let total_before = record.total;

        prune_daily(&mut record, today);

        assert_eq!(record.total, total_before);
        assert_eq!(record.daily.keys().collect::<Vec<_>>(), vec!["2026-09-02"]);
    }

    /// 同一天记多次要累加到同一个桶里,而不是各开一个。
    #[test]
    fn snapshot_sums_same_day_bucket() {
        let today = date(2026, 9, 2);
        let mut store = AgentUsageStore::default();
        store.agents.insert(
            "claude".to_string(),
            record_with_days(&[("2026-09-02", 3), ("2026-08-30", 2)]),
        );

        let snapshot = snapshot_from(&store, today);
        let stats = snapshot.agents.get("claude").expect("claude stats");

        assert_eq!(stats.recent_count, 5);
        assert_eq!(stats.total_count, 5);
        assert_eq!(snapshot.window_days, 7);
    }

    /// 窗口外还有记录时,`recent_count` 归零但 `total_count` 仍在 —— 这正是兜底规则要的形状。
    #[test]
    fn snapshot_reports_zero_recent_with_nonzero_total() {
        let today = date(2026, 9, 2);
        let mut store = AgentUsageStore::default();
        store
            .agents
            .insert("codex".to_string(), record_with_days(&[("2026-06-01", 4)]));

        let stats = snapshot_from(&store, today)
            .agents
            .get("codex")
            .cloned()
            .expect("codex stats");

        assert_eq!(stats.recent_count, 0);
        assert_eq!(stats.total_count, 4);
        assert!(stats.last_used_at > 0);
    }

    /// 没记录过的配置不出现在快照里,前端按「缺失即全零」处理。
    #[test]
    fn snapshot_omits_unrecorded_agents() {
        let snapshot = snapshot_from(&AgentUsageStore::default(), date(2026, 9, 2));

        assert!(snapshot.agents.is_empty());
    }

    /// 空 id 不建账:否则一个空 key 会一直挂在账本里，谁都对不上。
    #[test]
    fn record_rejects_blank_agent_id() {
        assert!(record_agent_config_usage("   ".to_string()).is_err());
    }
}
