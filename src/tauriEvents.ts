/**
 * Tauri 后端 ↔ 前端 ↔ mobile 三方共享的事件名单一来源。
 *
 * Rust 侧 emit 用同一批字符串(src-tauri/src/*.rs 的 `app.emit(...)`),mobile
 * 的 push 通道也复用 `task-status` 等名字。消费端 import 常量即可保证前端内部
 * 不漂移。
 *
 * 注意接线测试 `src/test/app-event-wiring.test.tsx` **并不** import 本模块:
 * 它自己维护一份 `EXPECTED_EVENTS` 字面量并断言集合相等。所以改这里的值会让
 * 那个测试失败(保护是有的),但保护来自两份清单的重复而非单一来源 —— 改名时
 * 必须同时改测试里的字面量,而 Rust 的 emit 侧没有任何静态检查兜底,只能靠
 * 运行期失配暴露。
 *
 * 已有的两处常量(AGENT_OPERATION_EVENT、APP_SETTINGS_CHANGED_EVENT)保留
 * 在原位,避免大面积 import 改动;新事件一律加在这里。
 */

// ── 任务生命周期 ──────────────────────────────────────────────────────────────
export const TASK_STATUS_EVENT = "task-status";
export const TASK_SESSION_EVENT = "task-session";
export const REMOTE_TERMINAL_RESIZED_EVENT = "remote-terminal-resized";
export const REMOTE_TASK_REQUEST_EVENT = "remote-task-request";

// ── DSH 审批/提问 ────────────────────────────────────────────────────────────
export const DSH_APPROVAL_REQUESTED_EVENT = "dsh-approval-requested";
export const DSH_QUESTION_REQUESTED_EVENT = "dsh-question-requested";
export const DSH_APPROVAL_RESOLVED_EVENT = "dsh-approval-resolved";
export const DSH_QUESTION_RESOLVED_EVENT = "dsh-question-resolved";

// ── DSH host 状态失效通道(桥接到 DOM 的 dsh-host-refresh)────────────────────
export const DSH_HOST_SESSION_ADDED_EVENT = "dsh-host-session-added";
export const DSH_HOST_SESSION_REMOVED_EVENT = "dsh-host-session-removed";
export const DSH_HOST_SESSION_STATUS_EVENT = "dsh-host-session-status";
export const DSH_HOST_WORKSPACE_CHANGED_EVENT = "dsh-host-workspace-changed";
export const DSH_HOST_WORKSPACE_ORDER_CHANGED_EVENT = "dsh-host-workspace-order-changed";
export const DSH_HOST_WORKSPACE_REMOVED_EVENT = "dsh-host-workspace-removed";
export const DSH_HOST_ARCHIVED_SESSIONS_CHANGED_EVENT = "dsh-host-archived-sessions-changed";
export const DSH_HOST_AGENT_ERROR_EVENT = "dsh-host-agent-error";

// ── 应用生命周期 ──────────────────────────────────────────────────────────────
export const APP_EXIT_REQUESTED_EVENT = "app-exit-requested";
export const APP_RESTART_REQUESTED_EVENT = "app-restart-requested";
