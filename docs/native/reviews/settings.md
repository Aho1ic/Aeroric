# P5-05 评审记录 — 设置完整映射（2026-10-02 收口）＋ Phase 5 Gate

## 1. 范围

P5-05 是 Phase 5 最后一个 Task：设置完整映射而不是只做模式开关。G11 兑现
（19 panel 全映射），Phase 5 Gate（PTY/native/unsupported 三模式测试）同批
收口。

## 2. 实现链（两刀）

- **刀一（模型＋panel 映射＋Secret 策略＋capability）**：
  `AeroricUI/Settings/{SettingsStore,SettingsCapability}.swift`——
  SettingsStore（G11 19 panel 逐条映射：panelRegistry key 集与 React 对齐、
  wsl 保留 `platforms:["windows"]` 唯一门控、permissions 全平台可见；
  `mapsAllReactPanels` 守卫；Secret 策略：masked 尾 3 位、**短串全打码**（尾
  3 位可见会泄露整个 secret）、单次写入纯透传不持有、`isUIOnly` 分界（
  general/theme/fonts/shortcuts＝UI-only，proxy/mcp 等＝core 配置）；
  `validateSave` 保存守卫不污染旧配置）＋SettingsCapability（四族别名、
  OMP thinking levels 无 ultra——不发 OMP 不支持的 effort、升级三渠道需
  显式确认、离线模型目录提示）。`SettingsStoreTests`(6)/`SettingsCapabilityTests`(5)。
- **刀二（七视图集）**：`AeroricUI/Settings/SettingsViews.swift`——
  GeneralSettingsView（UI-only @AppStorage＋shared 经 onSharedSave 提交）＋
  AppearanceSettingsView＋AgentSettingsView＋CustomAgentEditor（family 显式
  声明可选）＋ModelSettingsView（capability 门控 effort）＋ShortcutSettingsView＋
  AgentUpdateView（三渠道＋显式确认＋进度）＋SettingsSearchIndex。

## 3. 验证实测（2026-10-02）

| 验收 | 结果 |
|---|---|
| `pnpm native:check settings-ui`（新注册） | 1 case / 11 用例 ok（min 11） |
| `swift test --package-path macos/AeroricKit` | 153 passed / 0 failed |
| `xcodebuild -scheme AeroricNative build` | BUILD SUCCEEDED |
| `inventory --check` | consistent（settings-ui-unit floor 双写） |
| `doctor` | 18/18 |
| `smoke-headless.mjs` | rc 0 |
| 受影响面 10 模块门禁 | tasks-ui 10／sessions-ui 20／settings-ui 11／mode-switch 9／native-terminal 12／shell 92／tasks 151／storage 488／protocol 98／swift-client 74 全绿 |

## 4. 验收原文逐条对照

- `SettingsStoreTests`/`SettingsCapabilityTests` ✅；`pnpm native:check
  settings`（settings-ui 注册）✅；`inventory --check` 全部原 panel 有映射
  （G11：`mapsAllReactPanels` 19 条逐条）✅。
- 无效值保存失败不污染旧配置：`validateSave`（proxy 无 scheme 拒绝）✅；
  关闭设置重开值正确：@AppStorage（UI-only）＋shared 经调用方提交 ✅；
  离线模型目录有来源/更新时间提示而非虚假在线结果：`modelCatalogHint` ✅。
- `grill-me [settings]`：已真实调用（用户：接受并收口）✅。
- Phase 5 Gate（PTY/native/unsupported 三模式测试）：
  - PTY：`native-terminal` 3 case/12 用例（plan_replay/租约真链路）✅
  - native：sessions-ui 20＋tasks-ui 10＋settings-ui 11 ✅
  - unsupported：`ModeSessionIntegrationTests`（非 DSH 保持 requiresAdvanced）
    ＋`structuredCapabilitiesVerified` 非 DSH false ✅

## 5. 已记录的边界与挂账

- shared business 设置（proxy/mcp 等）的具体 core 写入实现 → P6+ 各 panel
  落地时逐项接；本 Task 交付的是映射分界与守卫（onSharedSave 提交点）。
- Secret 单次写入的宿主写入路径 → 各 panel 落地时接（store 不持有明文）。
