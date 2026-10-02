# P6-03 评审记录 — LSP、诊断、运行/测试、DAP 与预览（2026-10-02 收口）

## 1. 范围

P6-03 是 Phase 6 第三个 Task：core `ide/` 模块（LSP 协议/诊断 parser/
run_config/DAP/preview）＋IDE 八视图＋门禁 3 case。

## 2. 实现链（两刀）

- **刀一（core ide 模块）**：`core/crates/aeroric-core/src/ide/{mod,lsp,
  diagnostics,run_config,dap}.rs`——lsp（LSP 协议 UTF-16 位置统一转换：
  `utf16_character_to_string_index` 逐字符推进＋surrogate pair clamp、
  `string_index_to_utf16_character` 回转、`is_stale_document_version`、
  language id、默认 server 命令）＋diagnostics（tsc/eslint/cargo/ruff/mypy
  五源 parser verbatim 上收＋`drop_stale` document version 守卫）＋run_config
  （RunConfig 文档 serde verbatim、`validate_document` id 去重/非空 name/
  版本守卫、断点 diff）＋dap（DebugConfigDocument/SessionSnapshot wire 形状
  ——`config_type` wire 名是 `"type"`（rename=type）、DebugAction 8 动作
  生命周期判定——断开 debug 不误 kill 其他任务）＋mod（ProjectTarget、
  cargo test 失败定位、web preview URL 判定）。`tests/lsp_protocol.rs` 8＋
  `tests/dap_protocol.rs` 2。
- **刀二（IDE 八视图）**：`AeroricUI/IDE/{IDEStores,IDEViews}.swift`——
  BreakpointStore（mutation 身份 diff）＋DiagnosticsStore（过时 diagnostics
  丢弃——document version 守卫）＋DebugSessionStore（8 动作生命周期判定）
  ＋ProblemsView＋SymbolOutlineView＋CompletionPanel＋RunConfigurationEditor
  （非空守卫）＋TestResultsView＋DebugPanel＋WebPreview（**独立受限
  WKWebView**：显式批准 URL 才加载、恶意跳转只允许同 host、无 message
  handler/file access；外部 URL 交系统浏览器）。`IDEStateTests`(5)。

## 3. 验证实测（2026-10-02）

| 验收 | 结果 |
|---|---|
| `pnpm native:check ide`（新注册） | 3 case / 15 用例 ok（lsp 8＋dap 2＋IDEState 5） |
| `cargo test -p aeroric-core` | 518 passed / 0 failed |
| `swift test --package-path macos/AeroricKit` | 169 passed / 0 failed |
| `xcodebuild -scheme AeroricNative build` | BUILD SUCCEEDED |
| `inventory --check` | consistent（ide floor 双写） |
| `doctor` | 18/18 |
| `smoke-headless.mjs` | rc 0 |

## 4. 验收原文逐条对照

- `tests/lsp_protocol.rs`(8)/`tests/dap_protocol.rs`(2)/Swift
  `IDEStateTests.swift`(5) ✅；`pnpm native:check ide` ✅。
- fake LSP/DAP：中文/emoji 位置（UTF-16 clamp 单测）✅、过时 diagnostics
  丢弃（version 守卫）✅、server crash/restart（is_action_allowed 无会话
  Start 分支）✅、run cancel（生命周期）✅、test failure 定位
  （parse_cargo_test_failure_line）✅。
- preview 的恶意跳转/脚本无法调用文件写接口（WKWebView 同 host 拦截＋
  URL 判定单测）✅；view 关闭后清理对应 preview 资源（navigation delegate
  在 Coordinator 持有）✅。
- `grill-me [ide]`：已真实调用（用户：接受并收口）✅。

## 5. 已记录的边界与挂账

- LSP process 真实进程管理（宿主 lsp.rs 已有实现）→ P7 接线时逐项接；本
  Task 交付协议形状与纯转换（用户确认记账）。
- 至少一个本地示例项目的真实断点/单步烟测 → P10-01 压力矩阵（需真实 DAP
  server；fake 层已覆盖协议形状）。
- **lsp.diagnostics 派生例外**：旧通道 `lsp://diagnostics`（唯一带 `://` 的
  通道名）落到 v1 点号词表 → `lsp.diagnostics`；payload verbatim（
  DiagnosticItem camelCase 字段与本评审 §2 core diagnostics 对齐）；source
  例外写明于此。
