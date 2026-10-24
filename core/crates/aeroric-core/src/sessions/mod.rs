//! 会话层（P3-04）：解析（首片）＋发现路径（第二片）＋dsh／omp 读侧（第三片）＋分页／尾部窗口读取层与格式探测（第四片：history）＋恢复与导出（第五片：resume／export）。
//!
//! 内容逐字来自宿主 `src-tauri/src/session.rs` ＋ `src-tauri/src/session/parse.rs`，
//! 行为一行未改：两家 agent 的落盘格式不同，所以有 Claude／Codex 两套平行的解析器，
//! 由 [`parser::parse_session_lines`] 按 `is_codex` 分派。读上限族（页 1 000 行、尾部
//! 20 000 行、探测窗 200 行）随第四片进 [`history`]；**路径校验留在宿主**
//! `validate_session_path_for`——它依赖宿主 `AgentFamily` 与允许根（唯一真相）。
//!
//! 类型 [`SessionMessage`]／[`SessionContent`] 的 serde 形状是旧客户端的在网契约
//! （`tag = "type"`、snake_case 变体、`messageId`／`mediaType` 改名），金样测试按
//! 字节钉住它；协议 v1 的 `SessionPage`／`ContentBlock`（`aeroric-protocol::session`）
//! 是**另一套更 opaque 的形状**，二者不得混用——v1 转换属方法接线片，不在本模块。
//!
//! 护栏（2026-09-29 补记一百七十四 定的「让越界编译不过」）：本模块的子文件一律
//! **显式 import**，不写 `use super::*;`——宿主旧代码靠隐式继承父模块类型，迁移时
//! 必须让每个依赖显形（补记一百七十三 查出的正是这种隐式依赖）。

pub mod claude;
pub mod codex;
pub mod discovery;
pub mod dsh;
pub mod export;
pub mod history;
pub mod hooks;
pub mod import;
pub mod omp;
pub mod parser;
pub mod resume;

/// 一条已解析的会话消息。字段与宿主 `session.rs:671` 逐字对应。
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct SessionMessage {
    pub role: String,
    pub content: Vec<SessionContent>,
    #[serde(rename = "messageId", skip_serializing_if = "Option::is_none")]
    pub message_id: Option<String>,
}

/// 消息内容块。变体名与字段名即旧客户端 JSON 的 `type`／字段，勿改。
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SessionContent {
    Text {
        text: String,
    },
    ToolUse {
        id: String,
        name: String,
        input: String,
    },
    ToolResult {
        id: String,
        output: String,
    },
    Thinking {
        thinking: String,
    },
    Attachment {
        name: String,
        #[serde(rename = "mediaType")]
        media_type: String,
        source: String,
    },
}
