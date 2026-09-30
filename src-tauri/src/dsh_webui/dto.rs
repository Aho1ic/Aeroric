//! DSH Web 的传输结构 —— P3-05 方法群片(补记二百六十五 §3)已 verbatim 上收至
//! core `agents::dsh_host::dto`;本文件只剩 re-export 壳,保持 `dsh_webui.rs:51`
//! 的 `pub use dto::*` 通配链与宿主调用点路径不变。`supported_dsh_reasoning_effort`
//! 在 core 已 pub(宿主 `dsh_webui.rs` 测试模块直接导入);`default_true` 与
//! `DshSettings*` 是 pub(crate),只剩方法群(也已入 core)在用,宿主侧无需名字。

// 名单＝宿主 lib 与 cfg(test) 测试模块的全部消费面：部分类型（如
// `DshSessionModels`/`DshModelGroup`/`DshModelInfo`/`DshReasoningEffort`/
// `DshModelCatalogFailure`）只在测试里构造（`supported_dsh_reasoning_effort`
// 的目录判定测试），lib 构建下 unused 属预期，故 allow。
#[allow(unused_imports)]
pub use aeroric_core::agents::dsh_host::dto::{
    supported_dsh_reasoning_effort, DshCredentialView, DshDirectoryEntry, DshDirectoryListing,
    DshDiscoveredModel, DshGlobalModels, DshGoal, DshHostInfo, DshModelCatalogFailure,
    DshModelGroup, DshModelInfo, DshModelReasoning, DshModelSelection, DshPresetInfo,
    DshPresetList, DshPresetReadResult, DshProtocolCapabilities, DshProviderInfo,
    DshReasoningEffort, DshRemoteEvent, DshSessionHistory, DshSessionModels, DshSessionSummary,
    DshSkillEntry, DshStreamFrames, DshSubagentSummary, DshWorkspace, DshWorkspaceList,
};
