//! DSH API 调用的编解码面（P3-05 第二片起步）。
//!
//! 本片只收 `decode_result_value`——HTTP RPC 信封的**纯**解码：稳定 envelope
//! （`result.ok`／`result.error.message`／`result.value`）与两条在网错误文案
//! （`DSH API {method} response has no result`／`… rejected the request: {message}`）
//! 逐字迁移自宿主 `src-tauri/src/dsh_protocol.rs`（补记二百五十一），`value`
//! 缺省成 `Null` 的旧默认同样保真。真正的 HTTP 客户端（reqwest／鉴权／重试）
//! 仍留宿主 `dsh_webui/api_client.rs`——它是 I/O，按 sessions 的先后次序逐步上收。

use serde_json::Value;

/// Decode the stable DSH HTTP RPC result envelope while preserving its legacy
/// defaults and error wording.
pub fn decode_result_value(body: Value, method: &str) -> Result<Value, String> {
    let result = body
        .get("result")
        .ok_or_else(|| format!("DSH API {method} response has no result"))?;
    if result.get("ok").and_then(Value::as_bool) != Some(true) {
        let message = result
            .get("error")
            .and_then(|error| error.get("message"))
            .and_then(Value::as_str)
            .unwrap_or("unknown DSH API error");
        return Err(format!("DSH API {method} rejected the request: {message}"));
    }
    Ok(result.get("value").cloned().unwrap_or(Value::Null))
}
