# P8-01 评审记录 — Skill Hub、MCP、hook 与插件配置（2026-10-03 收口）

## 1. 范围与实现

core `extensions/{mod,skills,mcp,hooks,plugins,trust}.rs`：skill 名/安装落点
守卫（verbatim）、MCP server 校验（空 command/NUL/env 键）＋env secret 掩码、
hook `.js/.ts` 资源守卫（omp loader 静默丢弃的教训固化）、插件目录守卫＋
语义版本比较、信任判定（写入范围越界一律拒绝——确认不洗白）、原子安装
（失败不留 .tmp）。`AeroricUI/Extensions/{ExtensionStore,ExtensionViews}.swift`
六视图（SkillHubView/SkillDetail/MCPSettingsView/HookStatusView/
PluginManagerView/ExtensionTrustSheet）。宿主 skills/mcp/hooks/dsh_plugins
命令层未动（网络/钥匙串/State 半边留宿主）。

## 2. 验证实测

- `pnpm native:check extensions`：2 case/12 用例（extensions.rs 7＋
  ExtensionStoreTests 5；floors 双写）。
- core 全量 1567/0（+7）、host 1466/0、Kit 213/0、App 构建验签 OK、
  smoke rc 0、doctor 18/18、contracts 逐字节、inventory consistent。
- core/host clippy `-D warnings`、fmt 干净。

## 3. 验收原文对照

- 恶意路径（`..`/分隔符/NUL）、压缩包守卫（skills 名即 zip 安装名守卫；
  zip bomb 防护沿用 notebook/import guards）、无效 MCP command/env secret
  mask（`mask_env` 凭据键 → ••••；空值不掩）✅。
- 安装失败不留下半配置（父路径占用注入 → 零 .tmp 残留）✅；禁用扩展不删
  用户原始目录（宿主命令层语义未动；core 侧无删除入口）✅。
- 扫描≠执行、安装前来源/revision/写入范围/权限表单（ExtensionTrustSheet）、
  运行中 Agent 下次启动才生效提示（pendingRestartNotice，不显示"已生效"）✅。
- `grill-me [extensions]`：已真实调用（用户：接受并收口）✅。

## 4. 挂账

- 事件扫描器按 src-tauri 字面值找发射点：sync 常量定义已随 P7-06 入 core，
  宿主 emit 恢复字面量 `"notebook-sync-updated"`（`SYNC_UPDATED_TOPIC` 走
  常量，作为 dynamic emit site 记录）。
- dsh_plugins/hooks 的网络与钥匙串半边、marketplace 抓取留宿主命令层；
  纯守卫/判定面已上收。minimax enable/commit/rollback 的版本语义由
  `plugins::compare_versions` 支撑。
