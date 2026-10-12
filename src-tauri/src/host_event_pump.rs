//! P8-05 切片一百零四：桌面事件泵的 Tauri 适配层。引擎在 core
//! `events::pump`（翻译/游标/退避重连与传输解耦）；这里只有适配件：
//! [`HostPumpSource`]（把 [`HostClient`] 的 `events.since` 适配成引擎的
//! [`PumpSource`]，断线重连＝重新 connect + 握手）。窗口出口的产品装配在
//! `core_adapter::install_core_services`（窗口 emit）。

use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::host_client::HostClient;

pub use aeroric_core::events::pump::{run, EventSink, PumpSource};

/// 宿主 UDS 上的 `events.since`。重连＝对同一 data dir 重新 connect+握手；
/// 断线期间事件由宿主 EventLog 环（512 条）缓冲，重连后从同一游标续读。
pub(crate) struct HostPumpSource {
    client: Option<HostClient>,
    data_dir: PathBuf,
}

impl HostPumpSource {
    pub(crate) fn connected(client: HostClient, data_dir: &Path) -> Self {
        Self {
            client: Some(client),
            data_dir: data_dir.to_path_buf(),
        }
    }
}

impl PumpSource for HostPumpSource {
    async fn since(&mut self, after: &str) -> Result<Value, String> {
        let client = self
            .client
            .as_mut()
            .ok_or_else(|| "event pump has no live host connection".to_string())?;
        match client.events_since(after).await {
            Ok((aeroric_protocol::envelope::Frame::Success(reply), _)) => Ok(reply.result.clone()),
            Ok((frame, _)) => {
                self.client = None;
                Err(format!(
                    "events.since answered with a non-success frame: {frame:?}"
                ))
            }
            Err(error) => {
                // 断线后丢弃这条连接：重连由引擎调 reconnect()。
                self.client = None;
                Err(error.to_string())
            }
        }
    }

    async fn reconnect(&mut self) {
        match HostClient::connect(&self.data_dir, "event-pump").await {
            Ok(client) => self.client = Some(client),
            Err(error) => {
                eprintln!("[event-pump] reconnect failed ({error}); retrying with backoff");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host_client::scripted::{handshake_script, ScriptedHost};
    use aeroric_core::events::pump::{window_channel, POLL_INTERVAL};
    use aeroric_protocol::envelope::Success;
    use std::sync::{Arc, Mutex};

    #[derive(Default)]
    struct Recorder {
        events: Mutex<Vec<(String, Value)>>,
        skips: Mutex<Vec<String>>,
    }

    impl EventSink for Recorder {
        fn emit(&self, channel: &str, payload: &Value) {
            self.events
                .lock()
                .expect("recorder")
                .push((channel.to_string(), payload.clone()));
        }

        fn skipped(&self, event: &str) {
            self.skips.lock().expect("recorder").push(event.to_string());
        }
    }

    fn event(name: &str, data: Value) -> Value {
        serde_json::json!({ "event": name, "data": data })
    }

    #[test]
    fn v1_topics_translate_and_plain_names_pass_through() {
        assert_eq!(
            window_channel("dbx.transfer_progress").as_deref(),
            Some("dbx-transfer-progress")
        );
        assert_eq!(
            window_channel("notebook.rag_index_progress").as_deref(),
            Some("notebook-rag-index-progress")
        );
        assert_eq!(
            window_channel("dbx-production-confirm-requested").as_deref(),
            Some("dbx-production-confirm-requested"),
            "the confirm request is already a legacy channel name"
        );
        assert_eq!(
            window_channel("task-status").as_deref(),
            Some("task-status")
        );
        assert_eq!(window_channel("unknown.future.topic"), None);
    }

    #[test]
    fn the_translation_table_matches_the_authority_rows() {
        // 权威表 docs/native/event-topics.json：泵表行必须能在其中找到
        // （topic 名与唯一 legacyChannel 逐字一致），防止两表漂移。
        let authority = include_str!("../../docs/native/event-topics.json");
        let table: serde_json::Value = serde_json::from_str(authority).expect("parse authority");
        for (topic, channel) in aeroric_core::events::pump::TOPIC_TRANSLATIONS {
            let row = table["topics"]
                .as_array()
                .expect("topics array")
                .iter()
                .find(|row| row["topic"] == *topic)
                .unwrap_or_else(|| panic!("authority table lacks {topic}"));
            assert_eq!(row["status"], "mapped", "{topic}");
            assert!(
                row["legacyChannels"]
                    .as_array()
                    .expect("channels")
                    .iter()
                    .any(|c| c == channel),
                "{topic} → {channel} not in the authority row"
            );
        }
    }

    #[tokio::test]
    async fn pump_polls_from_a_snapshot_cursor_and_advances_it() {
        // 脚本：since("0") 只给游标（宿主语义），since("2") 补发两条，之后空。
        let seen = Arc::new(Mutex::new(Vec::<Value>::new()));
        let seen_for_script = Arc::clone(&seen);
        let handshake = handshake_script("pump-token");
        let host = ScriptedHost::start(
            "pump",
            Arc::new(move |method, params| {
                if method != "events.since" {
                    return handshake(method, params);
                }
                seen_for_script.lock().expect("seen").push(params.clone());
                // The pump carries the cursor as a string (the wire shape of
                // `events_since`); parse it back for the scripted branches.
                let after = params["after"]
                    .as_str()
                    .and_then(|raw| raw.parse::<u64>().ok())
                    .unwrap_or(0);
                let (events, latest, reset): (Value, u64, bool) = match after {
                    0 => (serde_json::json!([]), 2, false),
                    2 => (
                        serde_json::json!([
                            event(
                                "dbx.transfer_progress",
                                serde_json::json!({"transferId": "t"})
                            ),
                            event("task-status", serde_json::json!({})),
                        ]),
                        5,
                        false,
                    ),
                    _ => (serde_json::json!([]), 5, false),
                };
                Some(aeroric_protocol::envelope::Frame::Success(Success {
                    v: 1,
                    id: "pump".to_string(),
                    ok: true,
                    result: serde_json::json!({
                        "latestSeq": latest, "reset": reset, "events": events
                    }),
                    extra: Default::default(),
                }))
            }),
            Some("pump-token"),
        )
        .await;
        let client = HostClient::connect(&host.data_dir, "event-pump")
            .await
            .unwrap();
        let sink = Arc::new(Recorder::default());
        let recorder = Arc::clone(&sink);
        let source = HostPumpSource::connected(client, &host.data_dir);
        let task = tokio::spawn(async move { run(source, recorder).await });
        // 泵 500ms 一轮；给三轮的时间后停掉并断言。
        tokio::time::sleep(std::time::Duration::from_millis(1900)).await;
        task.abort();
        let delivered = sink.events.lock().expect("recorder");
        assert!(
            delivered
                .iter()
                .any(|(channel, _)| channel == "dbx-transfer-progress"),
            "transfer progress must be delivered: {delivered:?}"
        );
        assert!(
            delivered
                .iter()
                .any(|(channel, _)| channel == "task-status"),
            "plain names must pass through: {delivered:?}"
        );
        let asked = seen.lock().expect("seen");
        assert_eq!(asked[0]["after"], "0", "first poll is the fresh snapshot");
        assert!(
            asked.iter().any(|params| params["after"] == "5"),
            "cursor must advance to the batch cursor: {asked:?}"
        );
        let _ = POLL_INTERVAL;
    }
}
