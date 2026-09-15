/** PTY / 终端 I/O（local 任务）。 */
export const TERMINAL_COMMANDS = {
  sendInput: "send_input",
  resize: "resize_pty",
  killShell: "kill_shell",
} as const;

/** 通知中心。 */
export const NOTIFICATION_COMMANDS = {
  markRead: "mark_notification_read",
  markAllRead: "mark_all_notifications_read",
} as const;

/** Agent 版本探测/升级取消。 */
export const AGENT_VERSION_COMMANDS = {
  cancelOperation: "cancel_agent_operation",
} as const;

/** 用量统计索引。 */
export const USAGE_COMMANDS = {
  refreshIndex: "refresh_usage_statistics_index",
} as const;
