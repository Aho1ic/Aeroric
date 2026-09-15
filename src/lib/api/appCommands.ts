/** 桌面窗口 / 退出闸门等 shell 级命令。 */
export const APP_SHELL_COMMANDS = {
  hideWindow: "hide_main_window",
  exitListenerReady: "app_exit_listener_ready",
} as const;

/** SSH 连接元数据（密码走独立通道，不经此命令）。 */
export const SSH_CONNECTION_COMMANDS = {
  save: "save_ssh_connections",
} as const;

/** 清理 / 自动清理设置。 */
export const CLEANUP_COMMANDS = {
  updateAutoCleanup: "update_auto_cleanup_settings",
  deleteTaskTerminalHistories: "delete_task_terminal_histories",
} as const;

/** 任务进程控制（与 TASK_MIRRORS.cancel 并存的本地补充）。 */
export const TASK_PROCESS_COMMANDS = {
  reset: "reset_task_process",
  cancelLocal: "cancel_task",
} as const;

/** DBX 生产确认闸。 */
export const DBX_COMMANDS = {
  respondProductionConfirmation: "respond_dbx_production_confirmation",
} as const;

/** 手机远程任务请求完成回执。 */
export const REMOTE_TASK_COMMANDS = {
  completeTaskRequest: "remote_complete_task_request",
} as const;

/** Local Router / agent 启动校验。 */
export const LOCAL_ROUTER_COMMANDS = {
  validateAgentLaunch: "validate_agent_launch",
  switchTarget: "switch_local_router_target",
} as const;
