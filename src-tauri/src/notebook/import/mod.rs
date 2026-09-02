//! 从第三方笔记应用导入到随手记。
//!
//! 移植自 Markio `src-tauri/src/import/`,但**报告与落点两层重写**,理由见
//! `docs/notebook-markio-integration-plan.md` 的「P9.0 开工前的事实核对」:
//!
//! - 报告([`report`]):Markio 那份是两个计数 + 封顶 50 条的自由文本,过不了这一节
//!   自己的准入条件(四类逐项列出)。这里是「状态 + 问题清单」两层。
//! - 落点([`landing`]):Markio 逐字节复制到 `<workspace>/imports/<provider>/` 就完事。
//!   随手记的 vault 是**被索引的**,文件名还是 wikilink 的目标,所以落点要过一层。
//!
//! 护栏([`guards`])基本照搬 —— zip-bomb 上限、Windows 保留设备名那几条,Markio 的
//! 理由是对的。

#[cfg(test)]
mod apple_tests;
#[cfg(test)]
mod dir_tests;
#[cfg(test)]
mod enex_tests;
#[cfg(test)]
mod zip_tests;

pub mod apple_notes;
pub mod bear;
pub mod evernote;
pub mod guards;
pub mod landing;
pub mod logseq;
pub mod manifest;
pub mod notion;
pub mod obsidian;
pub mod report;
pub mod roam;
pub mod run;
pub mod walk;
pub mod zip_common;
pub mod zip_src;

pub use report::ImportReport;

// 只 re-export 命令层签名真正需要的那一个类型。P9e 的七个 `notebook_import_*` 都返回
// `ImportReport`,所以它现在有确定的使用者;`ItemStatus` / `SkipReason` / `ItemIssue`
// 只出现在报告**内部**(serde 顺着 `ImportReport` 一路序列化下去),命令层不具名引用
// 它们,所以不跟着铺一层 —— 那会换来一条 `unused_imports`,而这个 crate 同样不留警告。
