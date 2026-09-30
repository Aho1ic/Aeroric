//! `/api/remote.mux` 的双向客户端 —— P3-05 方法群片(补记二百六十五 §3)已
//! verbatim 上收至 core `agents::dsh_host::remote_mux`(991 行:连接、worker、
//! 帧编解码与 `$events` 解析;方法群的 `list_workspaces` 等直接依赖
//! `DshRemoteMux`,orphan 规则下随 transport 整体迁移)。本文件只剩 re-export
//! 壳,保持 `dsh_webui.rs:38` 与 `commands.rs:675-676` 的 `remote_mux::…` 路径
//! 不变。D1(grill [dsh]，补记二百六十四)批准 core 持网络出口后,WS 传输与
//! HTTP transport 同批入 core。

pub(crate) use aeroric_core::agents::dsh_host::remote_mux::{
    parse_remote_event_item, remote_event_result_args, DshMuxStream, DshRemoteMux, RemoteEventItem,
    REMOTE_EVENT_RESULT_ENDPOINT, REMOTE_EVENT_STREAM_ENDPOINT,
};
