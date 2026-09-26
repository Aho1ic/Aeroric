//! 运行时事件面。**唯一的任务事件词汇表仍是 `tasks::events`**——这里只再导出,
//! 不定义第二套通知类型;"另建一套不兼容生命周期"是计划 Action 1 明文禁止的。

pub use crate::tasks::events::{NullSink, TaskEventSink, TaskNotification};
