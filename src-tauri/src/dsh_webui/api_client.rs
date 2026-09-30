//! `DshApiClient` —— P3-05 方法群片(补记二百六十五 §3)已 verbatim 上收至 core
//! `agents::dsh_host::api`(struct、`new`、`call`、`bounded_utf8_prefix` 与 61 个
//! RPC 方法;orphan 规则决定 impl 与 struct 不可跨 crate 分片,故整体迁移)。
//! 本文件只剩 re-export 壳,保持 `dsh_webui.rs:34` 的 import 路径与全部宿主
//! 调用点(`api.call(...)`、`DshApiClient::new(...)`、错误摘要函数)不变。

pub(crate) use aeroric_core::agents::dsh_host::api::{
    bounded_utf8_prefix, DshApiClient, DSH_HTTP_ERROR_SNIPPET_BYTES,
};
