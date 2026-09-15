/** SFTP 文件操作。endpoint 由调用方经 toTauriSftpEndpoint 组装。 */
export const SFTP_COMMANDS = {
  readDir: "sftp_read_dir",
  readTextFile: "sftp_read_text_file",
  readImagePreview: "sftp_read_image_preview",
  readDirectorySummary: "sftp_read_directory_summary",
  createDirectory: "sftp_create_directory",
  deletePaths: "sftp_delete_paths",
  renamePath: "sftp_rename_path",
  copyPaths: "sftp_copy_paths",
  movePaths: "sftp_move_paths",
} as const;

/** SSH 主机密钥信任。 */
export const SSH_HOST_KEY_COMMANDS = {
  trust: "trust_ssh_host_key",
} as const;

/** Shell 会话生命周期（local shell / kill）。 */
export const SHELL_SESSION_COMMANDS = {
  sendInput: "send_input",
  resize: "resize_pty",
  kill: "kill_shell",
} as const;

/** SSH / WSL 终端会话。 */
export const SSH_SHELL_COMMANDS = {
  kill: "kill_ssh_shell",
} as const;

export const WSL_SHELL_COMMANDS = {
  open: "open_wsl_shell",
  kill: "kill_wsl_shell",
  validateProjectPath: "validate_wsl_project_path",
} as const;

/** DSH 会话辅助。 */
export const DSH_SESSION_COMMANDS = {
  respondRemoteEvent: "respond_dsh_remote_event",
  openHostPath: "open_dsh_host_path",
  updateSessionQueue: "update_dsh_session_queue",
} as const;

/** Docker 面板动作。 */
export const DOCKER_COMMANDS = {
  containerAction: "docker_container_action",
  deleteImage: "docker_delete_image",
  tagImage: "docker_tag_image",
} as const;

/** 项目/任务落盘（App 权威源）。 */
export const PERSISTENCE_COMMANDS = {
  saveProjects: "save_projects",
  saveProjectTasks: "save_project_tasks",
} as const;

/** 系统文件管理器 / 路径打开。 */
export const OS_INTEGRATION_COMMANDS = {
  openInSystemFileManager: "open_in_system_file_manager",
} as const;
