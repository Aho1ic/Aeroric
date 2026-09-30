// DSH HTTP RPC 信封的解码已随 P3-05 第二片迁入 core
// `aeroric_core::agents::dsh_host::client::decode_result_value`（补记二百五十一）；
// re-export 保持 `crate::dsh_protocol::…` 路径与原可见性不变（唯一消费点
// `dsh_webui/api_client.rs:68`），错误文案与缺省 `Null` 语义逐字随迁。
// api_client 上收 core 后，宿主 lib 侧唯一消费点消失；re-export 只服务本文件
// cfg(test) 的解码测试（原址保真，与 protocol_inventory 同一口径）。
#[cfg(test)]
pub(crate) use aeroric_core::agents::dsh_host::client::decode_result_value;

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    // 以下两条是宿主既有测试，经 re-export 原样跑在 core 实现上
    // （「旧客户端零回归」的机器形状：一条不少、文案逐字）。core 侧金样在
    // `core/crates/aeroric-core/tests/dsh_host.rs`。
    #[test]
    fn decodes_success_value_and_defaults_missing_value_to_null() {
        assert_eq!(
            decode_result_value(
                json!({ "result": { "ok": true, "value": { "id": 7 } } }),
                "test"
            )
            .unwrap(),
            json!({ "id": 7 })
        );
        assert_eq!(
            decode_result_value(json!({ "result": { "ok": true } }), "test").unwrap(),
            Value::Null
        );
    }

    #[test]
    fn preserves_missing_result_and_rejection_errors() {
        assert_eq!(
            decode_result_value(json!({}), "session.list").unwrap_err(),
            "DSH API session.list response has no result"
        );
        assert_eq!(
            decode_result_value(
                json!({ "result": { "ok": false, "error": { "message": "denied" } } }),
                "session.list",
            )
            .unwrap_err(),
            "DSH API session.list rejected the request: denied"
        );
    }
}
