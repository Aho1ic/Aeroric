/**
 * Agent 设置面板专用命令。密码/密钥不走这些命令的返回值。
 */
export const AGENT_SETTINGS_COMMANDS = {
  writeConfigFile: "write_agent_config_file",
  exportConfigBundle: "export_agent_config_bundle",
  renameCustomProfile: "rename_custom_agent_profile",
  deleteCustomProfile: "delete_custom_agent_profile",
  updateCustomAccess: "update_custom_agent_access",
  updateDshReasoningEffort: "update_dsh_reasoning_effort",
  updateOmpThinkingLevel: "update_omp_thinking_level",
} as const;

/** Skill Hub 安装/卸载。 */
export const SKILL_HUB_COMMANDS = {
  clear: "clear_skill_hub",
  uninstall: "uninstall_skill",
} as const;

/** 系统权限面板。 */
export const PERMISSIONS_COMMANDS = {
  openSystemSettings: "open_system_permission_settings",
  restartForPermissions: "restart_app_for_permissions",
} as const;

/** Hooks / Local Router 状态。 */
export const HOOKS_COMMANDS = {
  uninstall: "uninstall_hooks",
} as const;

export const LOCAL_ROUTER_STATUS_COMMANDS = {
  getStatus: "get_local_router_status",
} as const;

/** 手机远程设备管理。 */
export const REMOTE_ACCESS_COMMANDS = {
  revokeDevice: "remote_revoke_device",
} as const;

/** WSL 设置与配置写盘。 */
export const WSL_SETTINGS_COMMANDS = {
  saveSettings: "save_wsl_settings",
  writeConfigFile: "write_wsl_config_file",
  writeAgentConfig: "write_wsl_agent_config",
  restart: "restart_wsl",
} as const;

/** DSH 插件 / preset 面板。 */
export const DSH_PLUGIN_COMMANDS = {
  openConfigFile: "open_dsh_config_file",
  openAgentPresetDocument: "open_dsh_agent_preset_document",
  copyAgentPreset: "copy_dsh_agent_preset",
  removeAgentPreset: "remove_dsh_agent_preset",
} as const;
