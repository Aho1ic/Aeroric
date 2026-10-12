//! Slice 104: the desktop event pump closes the External production-confirm
//! loop end to end against the real host — execute (pending) → host EventLog
//! → pump engine → window-shaped sink → respond → execute returns. Before the
//! pump, this loop was fail-closed (300s refusal): events.since had no desktop
//! consumer.

use std::sync::Arc;
use std::time::Duration;

use aeroric_core::database::dbx_connections::default_connection_config;
use aeroric_core::database::dbx_types::DbxDatabaseType;
use aeroric_core::events::pump::{run, EventSink, PumpSource};
use aeroric_protocol::envelope::Frame;
use serde_json::{json, Value};
use tokio::io::BufReader;
use tokio::net::unix::{OwnedReadHalf, OwnedWriteHalf};

mod host_harness;
use host_harness::Harness;

fn success(reply: Frame) -> Value {
    match reply {
        Frame::Success(reply) => reply.result,
        other => panic!("expected success, got {other:?}"),
    }
}

async fn authenticated(host: &Harness) -> (BufReader<OwnedReadHalf>, OwnedWriteHalf) {
    let (mut reader, mut writer) = host.connect().await;
    success(Harness::handshake(&mut reader, &mut writer, &host.token, "pump").await);
    (reader, writer)
}

async fn call(
    reader: &mut BufReader<OwnedReadHalf>,
    writer: &mut OwnedWriteHalf,
    method: &str,
    params: Value,
) -> Frame {
    Harness::call(reader, writer, &Harness::request("pump", method, params)).await
}

/// The pump source over a live UDS connection: real events.since, real wire.
struct UdsEventSource {
    reader: BufReader<OwnedReadHalf>,
    writer: OwnedWriteHalf,
}

impl PumpSource for UdsEventSource {
    async fn since(&mut self, after: &str) -> Result<Value, String> {
        let result = success(
            call(
                &mut self.reader,
                &mut self.writer,
                "events.since",
                json!({"after": after}),
            )
            .await,
        );
        eprintln!(
            "[pump-src] since({after}) -> {}",
            serde_json::to_string(&result).unwrap()
        );
        Ok(result)
    }

    async fn reconnect(&mut self) {}
}

#[derive(Default)]
struct Recorder {
    events: std::sync::Mutex<Vec<(String, Value)>>,
    skips: std::sync::Mutex<Vec<String>>,
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

#[tokio::test(flavor = "multi_thread")]
async fn production_confirmation_closes_through_the_pump() {
    let host = Harness::start("pumpconfirm").await;
    let (mut reader, mut writer) = authenticated(&host).await;
    let path = host.home.join("pump-confirm.sqlite");
    rusqlite::Connection::open(&path)
        .unwrap()
        .execute_batch("CREATE TABLE items (id INTEGER PRIMARY KEY, name TEXT NOT NULL);")
        .unwrap();
    let mut config = default_connection_config(
        "pump-db",
        "Pump Database",
        DbxDatabaseType::Sqlite,
        path.to_string_lossy().into_owned(),
        0,
        None,
        false,
    );
    config.is_production = true;
    success(
        call(
            &mut reader,
            &mut writer,
            "database.dbx_save_connection",
            json!({"connection":{
                "id":"pump-db", "name":"Pump Database", "dbType":"sqlite",
                "readOnly":false, "createdAt":1, "dbx":config
            }}),
        )
        .await,
    );
    let sql_path = host.home.join("pump.sql");
    let sql = "INSERT INTO items VALUES (1, 'pumped');";
    std::fs::write(&sql_path, format!("\u{feff}{sql}")).unwrap();

    // The pump engine over its own live UDS connection, delivering to the
    // window-shaped sink (legacy channels).
    let (pump_reader, pump_writer) = authenticated(&host).await;
    let sink = Arc::new(Recorder::default());
    let recorder = Arc::clone(&sink);
    let source = UdsEventSource {
        reader: pump_reader,
        writer: pump_writer,
    };
    let pump = tokio::spawn(async move { run(source, recorder).await });

    // Execute on a second connection: pending until the confirmation arrives.
    let (mut execute_reader, mut execute_writer) = authenticated(&host).await;
    let params = json!({"request":{"connectionId":"pump-db", "path":sql_path}});
    let pending = tokio::spawn(async move {
        call(
            &mut execute_reader,
            &mut execute_writer,
            "database.dbx_execute_sql_file",
            params,
        )
        .await
    });

    // The pump (not a direct events.since poll) must deliver the confirm
    // request to the window-shaped sink.
    let confirm = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let found = {
                let events = sink.events.lock().expect("recorder");
                events
                    .iter()
                    .find(|(channel, _)| channel == "dbx-production-confirm-requested")
                    .map(|(_, payload)| payload.clone())
            };
            if let Some(payload) = found {
                return payload;
            }
            assert!(
                !pending.is_finished(),
                "SQL execution must wait for the confirmation"
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("the pump must deliver the confirmation request promptly");
    assert_eq!(confirm["sql"], sql, "{confirm}");
    assert_eq!(confirm["connection"], "Pump Database");
    assert_eq!(confirm["databases"], json!([]));
    let request_id = confirm["requestId"]
        .as_str()
        .expect("requestId")
        .to_string();

    // Approve over the pump-independent connection: the pending execute
    // completes and the row lands.
    success(
        call(
            &mut reader,
            &mut writer,
            "database.respond_dbx_production_confirmation",
            json!({"requestId": request_id, "approved": true}),
        )
        .await,
    );
    let reply = pending.await.unwrap();
    let rows = success(reply);
    assert_eq!(rows.as_array().unwrap().len(), 1);
    pump.abort();
}
