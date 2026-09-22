# ADR-001 — 原生边界：最低系统、材质、组件规则与 OMP 历史读取

- 状态：**accepted（含一项显式搁置）**
- 日期：2026-09-21
- 关联：`MIGRATION_PLAN.md` Task P1-02 Action 3/4/5、INV-02、INV-06、INV-11、§2.4
- 决策证据来源：用户在本会话中的两次明确选择（见 §4、§5 的"用户决策"行）；`docs/native/environment.md`；`docs/native/command-map.json`；`docs/native/feature-matrix.json`

## 1. 背景与不可破坏前提

Aeroric 现有 macOS 客户端是 Tauri（WKWebView + React）。原生迁移后 macOS 上会同时存在三个可能想写同一数据目录的入口：Tauri、SwiftUI、`aeroric-cli`。因此本 ADR 的"边界"包含两层：**技术栈边界**（谁实现什么）与**数据写入边界**（谁是唯一 owner）。后者由 INV-02/INV-03 约束，本 ADR 不重复定义，只补前者的落点。

机械核对结果（P1-02）：注册命令 **660** 条，其中 `domain_rpc` 579、`platform_specific` 62、`host_local_os_ui` 19、`internal` 0（初版把 `tests::*` 误判为 internal，已改判为 F08 run/test 能力，见 `command-map.json` meta.revisions）。这 660 条是本 ADR 全部范围的锚点。

## 2. 决策：最低部署系统 = macOS 14.0

- `macos/project.yml` 的部署目标与 `macos/AeroricKit/Package.swift` 的 `platforms` 一律写死 **macOS 14.0**。
- 理由：SwiftUI `@Observable` 宏与 Observation 框架、`GlassEffect`/`Material` 相关可用性矩阵、以及 Swift 严格并发检查的实际可用面，都要求 14 作为下限；13 及以下会迫使 store 退回到 `ObservableObject`，与 P4-02 的 actor IPC 设计冲突。
- 后果与义务：任何 macOS 15/26 才有的 API 必须以 `if #available` 门控，且 **P10-02 必须在最低支持系统与当前开发系统（本机 27.0）分别实测**，不得用单机当前 OS 代替 availability 验证（§P10-02 Action 2 原文要求）。
- 本机构建证据：Swift 6.4 / target `arm64-apple-macosx27.0.0`（见 `docs/native/environment.md` §1.1）。

## 3. 决策：材质基座 = `NSVisualEffectView` / SwiftUI `Material`

| 层 | 采用 | 明确不采用 |
|---|---|---|
| 窗口背后 | `NSVisualEffectView`，`material = .sidebar / .underWindowBackground`，`blendingMode = .behindWindow`，`state = .followsWindowActiveState` | 普通透明色冒充材质（W1） |
| 内容面板 | `.thinMaterial` / `.ultraThinMaterial` | 给每行文字叠一层 blur |
| 终端 / 编辑器 | 足够不透明的 text background | 半透明底 + 前景字（可读性与 a11y 双失败） |
| macOS 26+ `glassEffect` | **仅** availability-gated 增强，且只放可交互控件 | 当作基线；当作普通视图的默认底 |

附加义务：`reduced transparency` / `increased contrast` / `reduced motion` 三分支必须真实改变表现（透明关闭后背景不透明且文字可读），且 glass/classic 属**外观偏好**，不得与 `interactionMode`（native/advanced）复用同一枚开关（§2.4）。颜色一律从 semantic role 取值，不得把某张截图的 RGB 写遍 View。

## 4. 决策：相邻互斥控件的原生等价规则

**约束来源**：INV-11 要求 React 侧继续复用 `src/components/ui/AnimatedSelection.tsx`；SwiftUI 无法 import TSX，因此该约束在原生侧必须有一条**等价规则**而不是空缺。

采纳规则（原生）：

1. 相邻互斥的导航、tab、视图模式选择器、分段控件 **必须**使用系统选中态：`Picker(..., selection:)` + `.pickerStyle(.segmented)`、`List(selection:)`、`Toggle`、`Menu`。
2. **禁止**手绘相邻项的 active 背景、禁止自绘下划线做"选中态动画"。
3. 独立开关、复选框、折叠按钮、树展开器不属于"相邻互斥目标"，不在本规则范围内（与 React 侧同一豁免口径）。
4. 统一封装放在 `AeroricUI/Design/NativeSelection.swift`（P4-03），它只包系统控件的样式/键盘/a11y，**不新绘活动底纹**。
5. React 原规则保持不动，不因本 ADR 放宽。

**用户决策**：批准"到 P1-04 时把上述原生规则段追加进 `AGENTS.md`，不删除 React 的 `AnimatedSelection.tsx` 原规则"。因此：
- 本 ADR 只记录决策；`AGENTS.md` 的实际修改由 **P1-04 Action 7** 执行，且前提是 Trellis 已 start 且实施范围获批。
- 在 `AGENTS.md` 更新前，不得把 SwiftUI UI 实现标记为"已解锁"（P1-04 Acceptance 原文）。

## 5. 决策：OMP 历史白名单差异 → **默认禁用该分支**

实测差异（`src-tauri/src/session_omp.rs`）：

| 放行根 | 位置 | 与 AGENTS / INV-06 的关系 |
|---|---|---|
| `~/.aeroric/agent-homes/omp/sessions` | :114-116 | 合规 |
| `~/.aeroric/agent-homes/*/sessions`（**全部**托管档案，注释称 Phase 7 再按 agent 校验） | :120-128 | 托管目录内，但相对"按 agent 校验"是放宽态；须保留为待收口项 |
| `~/.omp/agent/sessions` | :130 | **与"Never read or write the user's own `~/.omp`"冲突**（只读，无写入） |
| `~/.omp/profiles/*/agent/sessions`，通过 `fs::read_dir(~/.omp/profiles)` 发现 profile 名 | :131 → :146-156 | **同样读取 `~/.omp`**，且目录枚举本身即信息读取 |

`MIGRATION_PLAN.md` 要求"登记差异，默认不新增扫描/读写；要求明确批准'显式选择的只读历史导入'是否保留。未决时该分支禁用并保留代码，不擅自删旧用户数据。"

**用户决策**：**默认禁用该分支，待单独审定。** 因此：

1. 迁移后的核心在 `AgentCapabilities` / host capability 中把"外部（非托管）OMP 历史读取"置 **`false`**，代码与既有行为**保留在 Tauri 客户端中不删**（不得因迁移而破坏现有用户可见行为，INV-10）。
2. 新 headless core 的 OMP 会话发现根**只允许** `~/.aeroric/agent-homes/**`。
3. 解禁的唯一路径：单独一次显式审定，且必须同时满足——只读、仅导入**用户显式选中**的会话文件、不以 `readdir ~/.omp/profiles` 作为发现手段（改为要求用户给出确切路径）、不写入、不进入日志与快照。审定前该分支在原生侧不可达。
4. 本决策不改变 §1.5 的既有事实：Tauri 侧现状行为保持不变，"当前原始行为差异不被顺便修复"（P1-02 Acceptance）。
5. 附带义务：`agent-homes/*` 全量放行（:120-128）与注释承诺的"按 agent 校验"不一致，作为待收口项登记在 `feature-matrix.json` 的 G 序列中一并处理，不得在迁移中静默固化。

## 6. 边界归属规则（660 条命令如何分）

| category | 归属 | 是否可被 host 执行 | 原生侧呈现 |
|---|---|---|---|
| `domain_rpc`（579） | `aeroric-core` service | **是**（须有 schema + capability 登记） | 经 `AeroricClient` RPC |
| `platform_specific`（62） | `aeroric-core` 的 `cfg` 分支 | 仅当 host 实际支持 | macOS 显示"不支持 + 原因"，**不得伪装本地**（F12） |
| `host_local_os_ui`（19） | 客户端（Tauri/SwiftUI/CLI 各自适配层） | 否 | 原生实现；CLI 返回 `INTERACTIVE_UI_REQUIRED` |
| `internal`（0） | 不导出 | 否 | — |

补充硬约束（本 ADR 立此为准）：

- `aeroric-core` 不得出现 `tauri::` / `AppHandle` / `tauri::State`；由 P3-01 的 `rg` 门禁机械证明。
- SwiftUI **不**写 Agent 启动参数、**不**解析原始会话 JSONL、**不**直接改共享业务数据（INV-02）。
- 一个数据目录只有一个业务写入者（INV-03）；`AlreadyInUse` 必须显示锁持有者与安全退出方式，**不得**自动 kill PID。
- UI 模式切换只是呈现方式变化，不得 kill/resume/relaunch Agent，不得改变 sessionId / 权限 / 模型 / cwd / PTY generation（INV-04）。
- 不从终端 ANSI 文本猜工具调用或审批（INV-12）；未知内容保留为带来源的 opaque 项。
- 不得把 `remote-protocol` 当作全量业务协议库：它是 **relay 撮合控制面**，业务契约在 `packages/remote-contracts` 与 `src-tauri/src/remote`。本地 IPC 协议版本与 relay 版本号**互不借用**。

## 7. 未决与阻塞

| 项 | 状态 | 阻塞什么 |
|---|---|---|
| OMP 外部历史导入解禁 | **deferred（默认禁用）** | 仅阻塞"非托管 OMP 历史在原生视图可见"这一子能力；不阻塞 P2/P3 主线 |
| `AGENTS.md` 原生规则段落 | **已写入**（SwiftUI 选择态 + OMP 历史边界两段）。该文件按用户决定继续留在 gitignore 内，所以这两条规则目前只存在于本机；可跟踪的规范落点是 `.trellis/spec/aeroric-macos/frontend/index.md` 与本 ADR | 不阻塞；P1-04 的 SwiftUI UI 实现标注已解锁 |
| `grill-me` 真实调用 | **已调用并完成第一轮 frontier 追问**：技能根、固定 revision 与 sha256、实际调用与问答结论见 `docs/native/reviews/skeleton.md` › Actual invocation。本行长期停留在「本会话不可调用」，因为那个结论写于用户批准把技能拷入全局根之前，之后没人回写——同一份文档里的其它「阻塞」字样也可能因此过期，引用前先核 | 不再阻塞；P1-05 Gate 只剩 Q1／Q5／Q6／Q7 待用户答复 |
| Q7（DSH 能力形态） | **已按实测重新定标**：待定的是 66 个 typed wrapper 中 41 个零证据者在 core 的暴露方式，以及谁承担与上游的版本偏移（能力清单钉住 `c291e796…` / `0.1.5-rc.2` / protocol v2；live 名字 198，另 48 项已退役） | 不阻塞文件级清单（`docs/native/extraction/dsh-webui.md` 已交付并进门禁），阻塞 P3-05 的落点细化 |

## 8. 后果

- 正面：范围以 660 条命令为可核对全集，分类可被 `inventory.mjs` 独立重算；材质与控件规则有明确禁令而非"看起来像"；OMP 的权限差异从"文档与实现不一致"变成"有主的显式搁置"。
- 代价：最低 14.0 排除了 13.x 用户；`host_local_os_ui` 与 CLI 的 `INTERACTIVE_UI_REQUIRED` 语义意味着少数能力在纯 CLI 下必须走"命令行应答"而不是静默降级；原生侧暂时看不到用户手动用系统 omp 跑出的历史。
- 后续可能推翻本 ADR 的证据：若 P5-01 证明 SwiftTerm 无法覆盖必选终端能力，终端呈现方案需重开 ADR，**不得**降级为 `TextEditor` 伪造原生 TUI（§5.4）。
