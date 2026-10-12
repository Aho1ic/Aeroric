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

## 6. 后续事实更正（2026-10-08，P8-05 slice29）

本记录§2的run_config “serde verbatim”并不精确：当时core漏了debug type
的lowercase标注（Node/Python，而旧Tauri是node/python）；native
`validate_document`也不等价于旧本地IO的路径/command/breakpoint校验。
本片以真实serde红灯复现并修正前者，保留后者为独立native policy，再上收
真实legacy文档读写，未删除原测试或重写历史用户验收。两条冻结父RPC的
ADR-006 target拒绝也经真host先红后绿补齐，**不代表run/SSH/DAP已迁完**。
边界与证据见`docs/native/extraction/run-config.md`和主计划切片二十九。

## 7. 后续事实更正（2026-10-09，P8-05 slice31）

DAP定义的core `DebugConfigType`也曾漏lowercase（Node/Python而非旧wire的
node/python）；原两条self-roundtrip测试未检出。现以直接wire期望先红后绿
纠正；native `validate_document`仍独立保留，真实legacy定义路径/attach/
breakpoint校验及文档IO才在本片上收。原DAP29测试保留，live canonical
resolution/remote/process不视为已经迁移。详见
`docs/native/extraction/debug-config.md`及主计划切片三十一的实际验证记录；
不改写此前用户验收，也不声称新的独立评审或真实调试器验证。

## 8. grill-me [ide/slice110](2026-10-12,P8-05 切片一百一十**

**工具来源与调用入口**:`grill-me` skill(`~/.claude/skills/grill-me/SKILL.md`,revision
同仓库既有记录)委托 `grilling` skill(`~/.claude/skills/grilling/SKILL.md`);本次以
`ask` 一次性提交 frontier 四问(§4.2 的模块追问口径:数据/任务 owner、反例、边界、
回滚、旧客户端回归)。

**首轮(4 问,用户逐条作答)**:

1. **远端孪生边界**:`remote_lsp_*` 21 条留 Tauri 只共用 core 引擎,宿主对远端 target
   逐字拒绝(切片94 先例)——长期正确,还是宿主该经注入的 SSH executor 也接管远端 LSP?
   **用户答**:超出了本问范围——「彻底舍弃 windows 和 Linux 客户端,只做 macOS 和 iOS 端,
   前端只使用 SwiftUI,删除 wsl 等 windows 独占的功能,买了 Apple developer,将当前项目
   转型成纯 macOS 原生项目,后面要上架 Apple store」。⇒ **这不是 slice110 的边界选择,
   而是一次项目转型声明**;与本模块其余问题无关,单独按 ADR 流程处理(见
   `docs/native/adr/039-distribution-pivot-macos-ios.md`,状态 **proposed**,不与已冻结的
   INV-10 / §6.1 冲突判定混写)。
2. **diagnostics topic 处置**:**用户答「维持现状」**——宿主 append legacy 字面量
   `lsp://diagnostics`,事件泵按设计丢弃带点号名,v1 topic `lsp.diagnostics` **保持
   `pending: P6-03`**,不写没有发射点的 mapped。与切片110 实施一致,无需改动。
3. **会话 teardown 形态**:**用户答「fire-and-forget spawn(现状)」**——注册表簿记同步、
   RPC 立即应答;回归用例以会话表(而非进程存在性)为判据。与实施一致。
4. **假语言服务器作为证据是否充分**:**用户答「够——会话面证据以宿主投影的 DTO 形状为准」**
   ——本机无 `typescript-language-server`、窄表只认 ts/js;断言对象是宿主交付给客户端的强类型
   字段。真 server 端到端归 P10-01。**本评审如实记下它证明与不证明什么**:证明「帧协议/握手/
   超时/响应投影/错误码/拒绝路径」;不证明真 server 的 capability 协商、workspace folder、
   增量语义。

**未达成交叉理解的部分**:Q1 未回答原问题,故「宿主是否最终接管远端 LSP」在 G43 关闭前仍是
**悬置决策**,不得记作已定。Q2–Q4 已定且与实施一致,无返工。
