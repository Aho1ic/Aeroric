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
