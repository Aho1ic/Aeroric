//! P8-05 切片一百零四：事件泵引擎——把宿主 EventLog 的事件翻译成前端窗口
//! 通道并投递。与具体传输解耦：调用方提供 [`PumpSource`]（宿主 UDS 的
//! `events.since`，或测试脚本）与 [`EventSink`]（窗口 emit，或收集器）。
//!
//! 背景：`events.since` 在桌面侧没有生产消费方时，宿主记录的生产确认请求、
//! 传输进度、rag 进度在 External 模式全部断路（确认回路 fail-closed 到 300s
//! 超时拒绝）。引擎只转发宿主 **实际记录** 的事件；无点号名透传，点号 v1
//! topic 按权威表翻译（点号是非法 Tauri 窗口事件名——这正是泵存在的理由）。

use std::time::Duration;

use serde_json::Value;

/// v1 topic → 前端窗口通道。权威表 `docs/native/event-topics.json` 的
/// `legacyChannels`；宿主 EventLog 里记录 v1 topic（103 的传输进度、99 的
/// rag 进度），前端监听 legacy 通道（React `databaseApi.ts` / rag 面同款）。
pub const TOPIC_TRANSLATIONS: &[(&str, &str)] = &[
    ("dbx.transfer_progress", "dbx-transfer-progress"),
    ("notebook.rag_index_progress", "notebook-rag-index-progress"),
];

/// 泵轮询间隔：进度/确认事件对延迟不敏感（确认预算 300s），500ms 足够。
pub const POLL_INTERVAL: Duration = Duration::from_millis(500);
/// 退避重连：1s 起步逐次翻倍，30s 封顶。断线期间事件由宿主 EventLog 环
/// （512 条）缓冲，重连后从**同一游标**续读。
pub const RECONNECT_MIN: Duration = Duration::from_secs(1);
pub const RECONNECT_MAX: Duration = Duration::from_secs(30);

/// 事件来源：宿主 UDS 的 `events.since`（Tauri 侧适配 [`crate::host_client::
/// HostClient`]），或测试脚本。成功返回应答的 `result` 对象（含
/// `latestSeq`/`reset`/`events`），失败返回可读的传输错误。重连语义也属于
/// 来源——断线后引擎要求「since(cursor) 从同一游标续读」（UDS 断线期间事件
/// 由宿主 EventLog 环缓冲）。
pub trait PumpSource: Send {
    fn since(
        &mut self,
        after: &str,
    ) -> impl std::future::Future<Output = Result<Value, String>> + Send;
    fn reconnect(&mut self) -> impl std::future::Future<Output = ()> + Send;
}

/// 泵把事件送到哪里。产品实现是窗口 emit（legacy 通道名）；测试用收集器。
pub trait EventSink: Send + Sync {
    fn emit(&self, channel: &str, payload: &Value);
    /// 无法投递的事件计数（未知点号名、环挤穿重同步等）。诊断用，不影响转发。
    fn skipped(&self, event: &str);
}

/// 一次泵循环的产出（诊断/测试断言用）。
#[derive(Debug, Default, PartialEq, Eq)]
pub struct PumpCycle {
    pub delivered: usize,
    pub skipped: usize,
}

/// 单条事件的窗口通道判决。
///
/// - 表内 v1 topic → 翻译后的 legacy 通道；
/// - 不含点号的名字 → 原样透传（97 的 `dbx-production-confirm-requested`、
///   任务状态族——宿主记录什么名字窗口就收什么名字）；
/// - 含点号的未知名 → `None`：翻译不了，而 Tauri 又拒绝点号窗口事件名，
///   转发等于静默丢弃——不如计数留下痕迹。
pub fn window_channel(event: &str) -> Option<String> {
    if let Some((_, channel)) = TOPIC_TRANSLATIONS.iter().find(|(topic, _)| *topic == event) {
        return Some((*channel).to_string());
    }
    if event.contains('.') {
        return None;
    }
    Some(event.to_string())
}

fn deliver(sink: &dyn EventSink, batch: &Value) -> PumpCycle {
    let mut cycle = PumpCycle::default();
    let Some(events) = batch["events"].as_array() else {
        return cycle;
    };
    for event in events {
        let Some(name) = event["event"].as_str() else {
            continue;
        };
        let Some(channel) = window_channel(name) else {
            cycle.skipped += 1;
            sink.skipped(name);
            continue;
        };
        sink.emit(&channel, &event["data"]);
        cycle.delivered += 1;
    }
    cycle
}

async fn poll_once<S: PumpSource>(
    source: &mut S,
    after: &str,
    sink: &dyn EventSink,
) -> Result<(String, PumpCycle, bool), String> {
    let result = source.since(after).await?;
    let cycle = deliver(sink, &result);
    let cursor = result["latestSeq"]
        .as_u64()
        .map(|seq| seq.to_string())
        .or_else(|| result["cursor"].as_str().map(str::to_string))
        .ok_or_else(|| "events.since reply has no cursor".to_string())?;
    let reset = result["reset"].as_bool().unwrap_or(false);
    Ok((cursor, cycle, reset))
}

/// 泵主循环：连上宿主后一直跑，直到调用方 drop 掉返回的 future（任务被
/// abort 即停）。
///
/// `reset == true`（游标落在被挤穿的区间）时历史已不可补发：重同步到最新
/// 游标并计数，不假装补齐。传输错误退避重连后从**同一游标**续读。
pub async fn run<S: PumpSource>(mut source: S, sink: std::sync::Arc<dyn EventSink>) {
    let mut cursor = String::from("0");
    let mut backoff = RECONNECT_MIN;
    loop {
        match poll_once(&mut source, &cursor, sink.as_ref()).await {
            Ok((next, _cycle, reset)) => {
                backoff = RECONNECT_MIN;
                if reset {
                    // 环已挤穿游标：跳到最新，丢掉的区间计数留下痕迹。
                    sink.skipped("events-reset");
                }
                cursor = next;
                tokio::time::sleep(POLL_INTERVAL).await;
            }
            Err(error) => {
                eprintln!("[event-pump] poll failed ({error}); reconnecting");
                tokio::time::sleep(backoff).await;
                backoff = std::cmp::min(backoff * 2, RECONNECT_MAX);
                source.reconnect().await;
            }
        }
    }
}
