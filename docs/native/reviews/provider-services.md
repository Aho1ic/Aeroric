# P8-02 评审记录 — provider、proxy、local router 与凭据边界（2026-10-04 收口）

## 1. 范围与实现（六刀）

Input 全量上收 core（~28K 行）：`app_settings/` 15 文件 13,507 行原样上收
（刀二，提交 ba9147d，适配 3 类：storage/路径/subprocess 改指 core 同名；
codex_chat_proxy.py 981 行随迁 core/resources 单一来源）；`local_router/`
10.4K 行（刀一，含 cache/stream/transforms/circuit breaker/usage 既有测试
原样保留）；`operations/`（刀三：agent_ops 纯状态注册表＋共享 wire 类型
`operations::types`）；`providers/`（刀三：storage_oauth 纯逻辑——PKCE、
authorize URL、state 常量时间比较、token 兑换、内置凭据拒绝）；platform
登录 shell 面（login_shell_unix/windows 202+289 行）与 `detect_node` 探测
随刀二/三上收。宿主留 Tauri 命令壳（commands.rs/proxy_test/agent_ops/
agent_tools/node_runtime/secrets/storage_oauth）。

Swift 面：`AeroricUI/Settings/{SettingsStore,SettingsCapability}`（P5-05
已立）＋刀五 `ProviderServiceViews.swift` 五视图（ProxySettingsView scheme
守卫拒 ftp://；ProviderSettingsView catalog 与已安装 Agent 支持列表区分；
LocalRouterSettingsView 安全默认 127.0.0.1:15721＋非回环拒空 token；
AgentOperationPanel 后端持有状态呈现 running 进度/取消、终态不显示取消；
ConfigImportExportSheet 凭据导出默认脱敏＋二次确认才含明文）。

刀六（本轮，收口接线）：宿主 `agent_ops.rs` 转薄壳——注册表/类型/判定
只有 core `operations` 一份，宿主消费 `operations::{begin,finish_snapshot,
report_progress_snapshot,snapshot_for,snapshots,cancellation_flag}`；
`agent_tools.rs` 五个共享类型改 re-export core `operations::types::*`（含
新增上收的 `AgentInstallProgress`），`AgentUpgradeResult` 在 finish 接线
边界逐字段换型（宿主/ core schema 两侧共存副本，serde 形状逐字一致；
runtime_recovery 两侧本就同源 core dsh_host::state）。**2 条 agent topic
双发接线**（详见 §5）。

## 2. 验证实测

- core：lib **1540/0**（operations 9＋providers 19（实测 19，先写 21 被
  门禁当场逮住改 19）＋settings_services 7）；clippy `--all-targets
  --all-features -D warnings` 0；fmt 干净。
- 宿主：lib **1329/0**（无代理、`--test-threads=1`；刀六删 8 条随业务入
  core 的重复单测，1337−8）；clippy `-D warnings` 0；fmt 干净。
- 门禁：`native:check provider-services` **4 case/51 用例**（settings_
  services 7＋operations 9＋providers 19＋ProviderSettingsTests 9，floors
  双写）；`native:check tauri-adapter` **12 case/1390 用例**（含
  src-tauri-lib-tests 新地板 **2011→1329**——2011 自 P7-06 起过时：545
  条随 sync/rag/storage 入 core、129 条随 P8-02 刀二 app_settings 入 core
  时漏更，本轮一次性对齐实测）；`native:check skeleton` **24 case/188
  用例**；Kit 全包 0 failed（金样 mapped/pending 计数随刀六 5/34 同批）；
  App `pnpm native:build` OK（六步链＋验签 intact）；smoke rc 0、doctor
  18/18、contracts 12 份逐字节、inventory consistent、digest verified、
  freeze-check noop、typecheck 干净。

## 3. 门禁断点修复（三条既有红灯，均 HEAD 实测复现后根修）

- **core-cargo-fmt**：`--all` 形态自 P7-04（dbx-core 路径依赖入 core）被
  拖进仓外 dbx workspace（vendored wry 本机缺失）→ `Failed to find
  targets` 环境性假红（HEAD 实测 rc 1）；且 `--manifest-path` 对 virtual
  workspace manifest 从仓根解析 targets 本身即 cargo fmt 缺陷。改经
  `sh -c 'cd core && cargo fmt --check'` 在 core/ 内执行 CI 同义检查——
  自 P7-04 起该 case 首次真实生效。
- **core-no-tauri-deps**：`objc2 v` 在 trash v5.2.6（P6-01 刀一与宿主同
  版本入 core）拉入的 objc2/objc2-foundation 上假红（cargo tree -i objc2
  唯一进入路径＝trash）。禁用清单精化为 GUI 族点名（objc2-web-kit/
  app-kit/uikit），tauri/wry 本体仍由 'tauri v'/'tauri-'/'wry v' 正面拦截。
- **inv02-core-source-scan**：9 处**注释**命中禁用字面量（P7-06 rag 六处
  ＋刀三 providers/operations 三处）——扫描器不区分注释与代码。逐条改写
  注释措辞（`#[tauri::command]`→Tauri 命令宏、AppHandle→Tauri 句柄），
  零代码改动，门禁 rg 复跑 0 命中。
- **extraction-docs-current**（P7-05 遗留）：database-notebook.md 的表在
  56 文件搬入 core 后从未重生成（committed 75 vs 生成 19）。`--write`
  重建 19 行/168 wrappers，register 四标量按生成值刷新（782→110、
  650→249），文档头补 2026-10-04 更正注记。

## 4. 验收原文对照

- 新建 `tests/settings_services.rs`（7 用例：codex 目标三件套/claude 排除
  codex_like/空 base_url 不路由/默认 upstream 常量稳定/同 fixture 两次
  构造路由结果逐条相同=迁移前后对账面/路由表进 RouterAgentRuntime 不变）
  ＋`ProviderSettingsTests.swift`（9 例）✅；`pnpm native:check
  provider-services` 全绿 ✅。
- fake HTTP server 面：settings_services 以同 fixture 双构造路由结果逐条
  相同承担"迁移前后相同协议 fixture"对账；local_router 既有 cache/stream/
  circuit breaker/usage 测试原样保留在 core（SSE 分片/401/429/5xx/取消/
  proxy 开关/secret redaction 由其覆盖）；不向真实 provider 发账单（无真
  网络出口，全部 localhost fake/fixture）✅。
- **调用 `grill-me [provider-services]`**：已真实调用（grill-me→grilling
  链，用户答复见 §6）✅。
- base URL 校验/代理排除规则/自定义 agent policy 不在 Swift 复制（core
  `RouterOutboundProxy`/`RouterRuntimeConfig::validate` 提交时拒绝并回显，
  Swift 只回显不复制校验）✅；router listener 非回环拒空 token、无公开
  HTTP 端口 ✅；凭据导出 includeSecrets 默认 false＋二次确认＋0600 提示 ✅。

## 5. 2 条 v1 event topic 接线与 G01 出口单一决定（具名移交项兑现）

- **接线形状**（P7-06 sync sink 双发同款）：宿主 emit wrapper 一次回调发
  两条——旧通道照发（`agent-operation-changed`/`agent-tool-install-progress`
  字面量发射，扫描器按 src-tauri 字面值找发射点）＋v1 topic 走 core 常量
  （`operations::AGENT_OPERATION_TOPIC`/`types::TOOL_INSTALL_PROGRESS_TOPIC`，
  记作 dynamic emit site、freeze 逐字记录）。payload 逐字同一份快照。
- **权威副本**：操作注册表只有 core `operations` 一份，晚挂载前端经
  `get_agent_operations` 对账。
- **G01 单一决定（修正案一遗留"不得再传一次"）**：**有订阅者，不删
  emit**——两条 v1 topic 原生事件面有消费者（bridge verbatim 转发），旧
  通道照发（React 兼容面；进度另有 agent-operation-changed（in-use）承载
  同源快照）。dsh-host-remote-event/stream-error 两条仍归 P3-05 的 DSH
  host 面（发射点 dsh_webui.rs 不在 P8-02 Input 内，无权触碰）。落盘：
  feature-matrix.json G01 evidence（verified_on 2026-10-04）＋ADR-007
  修正案三＋event-topics.json 两行 mapped（source 带 § 锚点）。
- 账目变化：event-topics mapped 3→5、pending 36→34；contracts --write 报
  2 changed（mapped 计数＋notebook-sync-updated emitter 修复 0→1——旧冻
  结态漏记 P8-01 恢复的字面量发射，本轮冻结如实记录）；Swift 金样计数
  同批 5/34。

## 6. grill-me [provider-services]（§4.2 门禁，真实调用）

入口 `Skill(grilling)`，五轴自查＋frontier 经 AskUserQuestion 提交（2026-10-04），
四问全按推荐项获答：

- **Q-P1（G01 出口单一决定）**：**接受「有订阅者，不删 emit」**——两条 agent
  topic 已 mapped＋宿主双发接线（旧通道照发＋v1 topic 原生事件面，core
  operations 权威注册表）；dsh-host-remote-event/stream-error 留 P3-05 的
  DSH host 面。落盘：feature-matrix G01 evidence＋ADR-007 修正案三＋
  event-topics 两行 mapped。
- **Q-P2（四条门禁断点修复）**：**接受全部四处**（core-cargo-fmt 在 core/
  内执行、objc2 禁用清单精化、INV-02 注释改写、extraction register 刷新）。
- **Q-P3（宿主测试地板）**：**接受对齐 1329**（2011 自 P7-06 起过时）。
- **Q-P4（收口裁定）**：**接受并收口**——P8-02 accepted，进 P8-03。

**G31 三件套（当场记）**：`git rev-parse HEAD`＝`45574feeedb77e751ba6213eeadb5c4c2680dc6a`
（45574fe，2026-10-03）；`git status --porcelain`＝19 条目（刀六代码＋本轮
注定落盘的台账/文档，按既定纪律随收口提交入库）；`git diff | git hash-object
--stdin`＝`8367afe2db5abe0f22b4caa1bd83cfd5a3f4893e`。

**五轴自查结论**：①唯一 owner——两条 topic 的发射/接线收敛到宿主两个 emit
wrapper，注册表收敛到 core `operations` 一份（宿主薄壳）；②反例——同
fixture 双构造路由逐条相同（settings_services ⑤⑥）、迟到旧操作事件不覆盖
新操作（operations::tests 第 9 条负例）、providers 19/21 计数被门禁当场逮住
（minCases 防注水反例）；③边界——凭据导出默认脱敏＋二次确认、router 非回
环拒空 token、OAuth 内置凭据拒绝（桌面产物永不内嵌 secret 回归在册）、
Swift 不复制 core 校验；④证据——本文 §2 全部当日实测，无引用旧读数；
⑤悬置——无产品决策悬置（G01 已获答闭合；dsh 两条归属明确在案）。

## 7. 挂账

- `AgentUpgradeResult`/`AgentUpgradeChannel` 宿主 schema 与 core settings
  schema 两侧共存（刀四"接线前两侧共存"口径），换型 shim 在 agent_ops
  finish 边界；两侧归并属 P8-05 cutover 数据对账面。
- core `providers` 的网络半边（真实 OAuth 回调服务器/token 兑换 HTTP）
  留宿主 storage_oauth.rs；core 只收纯逻辑（21→19 用例面）。
- windows winget/node_version 一键安装命令面留宿主 node_runtime.rs
  （detect_node/windows_node_directories 已上收 core platform）。
- core-cargo-fmt 若 CI 侧同样受 virtual-manifest 限制，需在 P10-04（原生
  CI）复核 CI 实际执行的形态；本机以 core/ 内执行为准。
