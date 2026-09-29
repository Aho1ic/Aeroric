//! 会话 Markdown 导出：实现已于 2026-09-30 迁至 core（P3-04 第五片，补记二百四十五）
//! `aeroric_core::sessions::export`。本文件只剩两件事：
//! ① 按原可见性 re-export，`session.rs` 的 `use export::…` 与 `crate::report` 的
//! `crate::session::…` 路径一字不动；
//! ② 宿主闸门包装——`validate_session_path_for`（依赖宿主 `AgentFamily` 与允许根）
//! 留在这一侧，`AgentFamily → ExportSource` 的映射也在这里做一次（唯一真相关系
//! 同第二片的 home 注入，模块头注释写了理由）。

use super::validate_session_path_for;

#[cfg(test)]
pub(crate) use aeroric_core::sessions::export::write_export_markdown;
pub use aeroric_core::sessions::export::ExportTaskMeta;
pub(crate) use aeroric_core::sessions::export::{
    export_session_markdown, format_timestamp_ms, sanitize_md_inline, validate_export_output_path,
    ExportSource,
};

/// 族别判定：`Claude | Codex` 仍交给 core 内的内容嗅探分支（原 `_ if is_codex_format(…)`
/// 的形状逐字保真），`Dsh`／`Omp` 走各自的显式解析器。
fn export_source(family: crate::app_settings::AgentFamily) -> ExportSource {
    use crate::app_settings::AgentFamily;
    match family {
        AgentFamily::Claude | AgentFamily::Codex => ExportSource::ClaudeOrCodex,
        AgentFamily::Dsh => ExportSource::Dsh,
        AgentFamily::Omp => ExportSource::Omp,
    }
}

pub(super) fn export_session_markdown_inner(
    session_path: &str,
    project_path: &str,
    family: crate::app_settings::AgentFamily,
    output_path: &str,
    meta: &ExportTaskMeta,
) -> Result<(), String> {
    let canonical = validate_session_path_for(session_path, project_path, family)?;
    export_session_markdown(&canonical, output_path, export_source(family), meta)
}
