# Aeroric → SwiftUI 原生 macOS 客户端迁移执行计划

> 计划基准：2026-09-21；Aeroric 本地提交 `ca69a4b`，分支 `main`。
>
> 文档性质：**待评审的实施计划，不是已经完成的迁移报告**。本次只新增本文件，不创建 Swift 工程、不修改运行时代码、不安装工具、不提交或推送。
>
> 执行对象：一次只完成一个 Task 的实现 Agent。所有 Task 均有 **Input / Action / Acceptance Criteria**；前置验收失败不得进入依赖它的 Task。
>
> 路径规则：除 `../参考项目/...` 和下表明确的缩写外，文件路径均相对于 Aeroric 根目录；`新建` 文件当前不存在。测试名、脚本名、RPC 方法名中明确标为新建的内容，是本计划要求实现的接口，不是声称仓库已有的命令。

| 文中路径缩写 | 唯一展开规则 |
|---|---|
| `AeroricProtocol/`、`AeroricClient/`、`AeroricUI/` | 分别为 `macos/AeroricKit/Sources/` 下的同名目录 |
| `AeroricApp/`、`Config/` | 分别为 `macos/AeroricApp/`、`macos/Config/` |
| `AeroricProtocolTests/`、`AeroricClientTests/`、`AeroricUIUnitTests/` | 分别为 `macos/AeroricKit/Tests/` 下的同名 Swift Package 单元测试 target；**不启动 App** |
| `AeroricUITests/`，或显式标注“XCTest UI”的文件 | `macos/AeroricUITests/`；Xcode 的 `AeroricNativeUITests` target，使用 XCUIApplication 启动测试 App，**不由 swift test 运行** |
| 未带目录的 `*Tests.swift` | 协议 suite 放 `AeroricProtocolTests/`，IPC/client suite 放 `AeroricClientTests/`，其他 reducer/store/policy suite 放 `AeroricUIUnitTests/`；例外必须在任务中写全路径 |
| Rust `tests/*.rs` | 默认 `core/crates/aeroric-core/tests/`；`protocol_roundtrip` 属 `aeroric-protocol`，`ipc_auth/ipc_frames/event_replay/host_lifecycle/host_cutover` 属 `aeroric-host`，`cli_headless` 属 `aeroric-cli`；`tests/fixtures/` 随所属 crate；不得在仓库根误建 `tests/` |
| Action 中新建的 Rust `storage/`、`agents/`、`metrics/`、`files/`、`ide/`、`git/`、`connections/`、`platform/`、`infrastructure/`、`database/`、`notebook/`、`storage_backends/`、`extensions/`、`settings/`、`providers/`、`local_router/`、`operations/`、`remote/`、`maintenance/`、`adapters/` | 统一在 `core/crates/aeroric-core/src/` 下展开；Input 中的旧同名模块仍按下句从 `src-tauri/src/` 展开，不能误把新服务写回旧客户端 |

Input 中省略根前缀的既有 Rust 模块，如 `app_settings/schema.rs`，从 `src-tauri/src/` 展开；实施前在任务工件写出最终完整路径。大模块目录展开遵循 §5.1，不能把缩写当成另一个新工程。

## ▶ 执行进度标注（Agent 维护，暂停/继续以此为准）

**标注约定**：

- 状态取值 `pending` / `in_progress` / `blocked` / `accepted`，与 §5.3 模板一致。
- 每完成一个 Task 或其一个 Action 步骤，立即：① 更新下表对应行；② 在该 Task 小节标题下写入 `> **【执行状态】**` 引用块，记录实际命令、退出码、证据路径；③ 在此处更新"最后一次执行"摘要。
- 只有真实命令输出可写为"通过"；未运行的验收一律写"未运行"，不得用相邻成功命令代替。§0.3 的工具状态在环境变化后须整段重写。
- 任何 Phase Gate 未闭合不得进入依赖它的 Phase（§5.2）。

### 进度总表

| Task | 状态 | 阻塞项 | 最后更新 |
|---|---|---|---|
| P1-01 | **accepted**（Action 1–5 全部完成；Acceptance 四条逐条通过，含一处已记录的实现冲突） | 无遗留阻塞；grill-me 已装但本会话不可调用（见 P1-05 行） | 2026-09-21 |
| P1-02 | **accepted**（三份规划工件落盘；未写脚本、未改运行时代码） | 后续增量：G08/G10 已由工具闭环、新发现 G15，见 P1-05 补记 | 2026-09-21 |
| P1-03 | **accepted**（父+10 子任务建立，validate 双通过且无截断告警；仅 start Phase 1 子任务） | 新发现 D8 注入上限已处理 | 2026-09-21 |
| P1-04 | **accepted，但一条自述已被推翻**（11 项验收命令当时全绿；骨架可构建、check 入口统一。**其中「`--freeze` 可复现地重生成冻结清单并保留人工字段」当时并不成立，见 P1-05 第七次补记**） | 无（`AGENTS.md` 不被跟踪的口径问题转入 P1-05） | 2026-09-21 |
| P1-05 | **门禁待答复**（`grill-me` 已真实调用并完成 frontier 追问；Phase 1 内不依赖门禁的收尾**已全部做完**：忽略白名单修复、注入分片、`--freeze` 保人、空模块拒注册、INV-02 正控。门禁现为 20 case／92 用例） | Q1／Q5／Q6／Q7 待答；G16 定性归 P2-01 首步 | 2026-09-21 |
| P2-01 ~ P2-05 | pending | Phase 1 Gate | — |
| P3-01 ~ P3-07 | pending | Phase 2 Gate | — |
| P4-01 ~ P4-04 | pending | Phase 3 Gate | — |
| P5-01 ~ P5-05 | pending | Phase 4 Gate | — |
| Phase 6 | pending | Phase 5 Gate | — |
| Phase 7 | pending | Phase 6 Gate | — |
| Phase 8 | pending | Phase 7 Gate | — |
| Phase 9 | pending | Phase 8 Gate | — |
| Phase 10 | pending | Phase 9 Gate | — |

### 续跑指令（暂停后从这里继续）

**当前状态：Phase 1 的四个实现型 Task（P1-01～P1-04）已 accepted；P1-05 的交付文档、基线与回滚记录均已完成，`grill-me` 链路也已真实调用并完成第一轮 frontier 追问；第六～九轮（忽略白名单漏洞、空白卫生、注入分片；`--freeze` 会静默删人工字段 → `inventory-freeze-is-noop`；注册 0 条 case 的模块曾让门禁假绿 → `gate-runner-unit`；**INV-02 源码守卫曾被一个 `.gitignore` 完全绕过 → `--no-ignore` + 每个被扫路径一条正控探针**；第十轮（INV-09 签名基线此前无守卫 → 新增 `inv09-signing-baseline`；`check-patches` 空表恒绿 → 要求 ≥9 条且识别引号注册表）也已做完 —— 现在只剩等用户回答 Q1／Q5／Q6／Q7。**

⚠ **不需要重启会话**（本文件与 `tooling.md` 早前都写错过，已各自更正）：本 CLI 读取的技能根是 `~/.mirasim/skills`（`~/.claude/skills` 与 `~/.agents/skills` 都是指向它的符号链接），项目内的 `.claude/skills/` **完全不被读取**——反证是同目录里 frontmatter 合法的 `trellis-check` 同样不在注册表中。已按用户批准把固定 revision 的 `grill-me`/`grilling` 拷入该全局根（四个文件 sha256 与 `docs/native/tooling.md` §2 逐字一致，原有 6 个技能未受影响），注册表**当场热加载**，`Skill(grilling)` 随即成功。

**下一步按顺序：**

1. 用户回答 `docs/native/reviews/skeleton.md` 末尾的 **Q1／Q5／Q6／Q7**：RPC 命名（三方案实测零碰撞，660 名本就全局唯一）／Phase 1 是否提交到 main（不 push）／远程与 WSL 的 git 语义归属哪个 check 模块／DSH 取「42 条具名 core 方法」还是「一个 mux 网关服务」。
   （**Q2／Q3／Q4 已实施并复验，不需再答**：地板只升不降＋孤儿检测、INV-02 源码与 manifest 双守卫、日志写入前脱敏并有自测。Q3 的守卫在第九轮被证过一个可绕过路径，现已加正控，勿当作免检项。）
2. 按答案实施，重跑 `pnpm native:check skeleton`（当前 **20 cases / 92 用例**，实测 ~6 s），把复验结果与问题级别写回 `reviews/skeleton.md`，再把 P1-05 与 Phase 1 Gate 标 `accepted`。
3. 才开始 **P2-01**（`aeroric-protocol` 的 envelope/DTO/schema/fixtures + Rust/TS/Swift 三方 decoder 共用**同一份** fixtures）。Phase 2 的产物、输入与验收见本文件 §Phase 2。
4. **Phase 1 内不依赖门禁的收尾已全部做完**，包括 G16 的定性：93 条不可达命令已全部归入四个桶并落成 `docs/native/reachability-triage.md`（A 34 条 `prefixCommand` 运行时拼接、B 4 条 `tests::*` 整簇未接线、C 42 条 `dsh_webui::*` 被 `/api/remote.mux` 旁路、D 15 条逐项待判）。**下一步只剩回答待决问题。**
5. **待你决策的问题清单：Q1～Q6（`docs/native/reviews/skeleton.md` 末尾）+ Q7 = G17（DSH 能力形态。**第十二次补记按实测重新定标**：真正待定的是 66 个 typed wrapper 中 41 个零证据者如何在 core 暴露，而 `protocol_inventory.rs` 把上游 dsh `c291e796…` / `0.1.5-rc.2` / protocol v2 钉住，所以选择题其实是「谁承担与上游的版本偏移」；证据在 `docs/native/extraction/dsh-webui.md`。旧述：42 个 typed core 方法 vs 一个 mux/gateway core service + 钉死的 `protocol_inventory` 清单（实测 10 张表 246 项：62 本地 RPC + 87 remote 方法 + 19 事件 + 19 事件模式 + 11 流帧 + 48 项已退役；live 198））**。这些都属于"中低风险由谁接受"与产品形态选择，Agent 不能替你把风险归零。
6. 答复之后 P2-01 才可开工，且它的第一步就是把 93 条按 `reachability-triage.md` 的桶落实进 `methods.schema.json`（A 桶收敛成 target 参数化，C 桶按 Q7 的答案处理，D 桶逐条定性）——**不得把 81 条无人调用的名字原样提升进契约**。

**续跑时必须记住的硬约束（本轮实测踩出来的，不看就会重复踩）：**

- 每条 `task.py` / `get_context.py` 调用都要在同一行 `export TRELLIS_CONTEXT_ID="qoder-native-macos-20260921-aeroric"`。活跃任务 `.trellis/tasks/09-21-native-phase-1`（父任务 `09-21-native-macos` 保持 `planning`）。
- 需要"活跃指针 + 实现未授权"同时成立时沿用 D1：`start` 写指针后把 `task.json.status` 改回 `planning`。
- `command-map.json` 字段是 **snake_case**（`rust_path` / `definition_resolved` / `check_module` / `feature_ids`）；按 `rustPath` 读会得到 661 条假不一致。
- `cargo tree` 纯度断言只读 **stdout**；`swift test` 用例数只数 **`Test Case '…' passed`** 行；`node --test` 必须 `--test-reporter=tap` 才有 `# pass N`。三条均已固化进 `checks.json`。
- `pnpm test` 单次全量变红不足以判定回归：本机同一套代码实测 3 失败 → 369 全绿；先定向复跑再全量复跑。
- **静态抽取的三条教训**（写任何新扫描器都会再踩）：符号表必须**文件内优先**（`PROGRESS_EVENT` 在两文件取值不同）；跨行嵌套泛型必须用**深度计号的调用点扫描**而不是正则；`check()` 里冻结工件按 `repoRoot` 读、源码文件必须按 `sourceRoot` 找，否则临时树负向测试会全量误报。
- 不得动 `../dbx`（38 个脏文件属用户）、不得动五个参考仓库、不得改 `src-tauri/tauri.conf.json` 的 ad-hoc 基线（INV-09）。
- Phase 1 产出**仍未提交**（用户当前决定，见 Q5），回滚点即 `ca69a4b` + 可整块丢弃的新增目录；提交需按 INV-01 先出文件清单再确认。**已有一次实证**：`checks.json` 因对未跟踪文件误用 `git checkout` 兜底而被截断成 0 字节、靠重建恢复（详见 `docs/native/rollback.md` › Self-inflicted loss）。新增硬规则：**破坏性验证一律在临时副本上做（照 `inventory.test.mjs` 的 `tempTree()`）；未跟踪文件改前先 `cp x x.bak`，绝不用重定向 / `/dev/null` / 管道兜底恢复；恢复后必须逐条重跑受影响门禁**，因为当时门禁仍然是绿的（配置已载入进程内存），绿灯不能证明文件完好。
-

Q
2
／
Q
3
／
Q
4

已
提
前
落
地
（
只
增
加
守
卫
，
不
预
设

Q
1
／
Q
5
／
Q
6
／
Q
7

的
答
案
）
：
I
N
V
-
0
2

现
在
有
真
实
的
源
码
与

m
a
n
i
f
e
s
t

守
卫
（
原
先
只
扫

`
c
a
r
g
o

t
r
e
e
`

的
断
言
在
骨
架
期
近
乎
恒
真
）
、
`
m
i
n
C
a
s
e
s
`

有
不
可
孤
儿
化
的
地
板
（
已
实
测
：
偷
改

1
2
→
1

立
即
失
败
）
、
日
志
写
盘
前
脱
敏
并
有

8

条

f
i
x
t
u
r
e

自
测
充
当
门
禁

c
a
s
e
（
首
跑
即
抓
出
我
自
己
的

2

处
泄
漏
）
。
*
*
Q
1
、
Q
5
、
Q
6
、
Q
7

仍
完
全
未
动
*
*
：
R
P
C

命
名
未
冻
结
、
未
提
交
、
g
i
t

归
属
与

D
S
H

形
态
仍
待
你
定
。

### 最后一次执行（2026-09-21，P1-01 Action 1 只读环境采集）

**已验证事实（本会话真实命令输出）**：

| 命令 | 退出码 | 结果 |
|---|---|---|
| `sw_vers` | 0 | macOS 27.0 / Build 26A28 |
| `uname -m` | 0 | `arm64` |
| `xcode-select -p` | 0 | `/Applications/Xcode.app/Contents/Developer` |
| `xcodebuild -version` | 0 | `Xcode 27.0` / `27A266a` |
| `xcrun swift --version` | **69** | Xcode license 未接受 |
| `xcrun --sdk macosx --show-sdk-path` | **69** | 同上，SDK 路径不可得 |
| `python3 --version` | **69** | `/usr/bin/python3` 是 Xcode shim，同样被许可阻塞；机器上无 Homebrew/其它 `python3` |
| `cc --version` | **69** | `/usr/bin/cc` 被阻塞 |
| `rustc --version` / `cargo --version` | 0 | 1.93.0 / 1.93.0，来自 `~/.cargo/bin`，非 Xcode |
| `node --version` / `pnpm --version` | 0 | v25.9.0 / 10.33.0 |
| `trellis --version` | 0 | **0.6.15**（§0.3 原记 0.6.16，环境已变化） |
| `trellis platforms` | 0 | 仅 `Claude Code (claude-code) — .claude` |
| `command -v xcodegen` | 1 | 未安装 |
| `git -C ../dbx rev-parse HEAD` | 0 | `7904231…` == `scripts/dbx-ref.txt` pin，**一致** |
| `git -C ../dbx status --short` | 0 | 38 个脏文件（含 `crates/dbx-core/*`、`vendor/wry/*`）；按 Action 3 不自动 checkout/reset |
| `cargo build`（/tmp 一次性 bin 探针） | 非 0 | 链接阶段失败于同一 Xcode 许可错误 |
| `cargo check`（同上探针，纯 bin 不链接） | 0 | 通过 |

**由此得到的硬结论（比 §0.3 更严格）**：Xcode 许可不只阻塞 Swift/Xcode 路线，它同时阻塞 `/usr/bin/cc` 与系统 `python3`，因此 **Rust 链接（`cargo build`/`cargo test`）与全部 `.trellis/scripts/*.py` 均不可运行**。当前可运行的只有：不触发链接的 `cargo check` 子集、Node/pnpm、以及不依赖 Xcode 的只读检索。

**尚未运行的验收**：P1-01 全部 Acceptance Criteria（其前置要求 `python3 --version` 与 `xcrun swift --version` 真实成功）、`pnpm install --frozen-lockfile --ignore-scripts`、`pnpm lint/typecheck/test/build` 基线、任何 cargo/swift/xcodebuild 构建。`docs/native/environment.md` 与 `$TASK_DIR/research/environment.md` 未创建：按 Action 5 的顺序，须先获得"创建 Trellis 任务并进入 planning"的同意，且 python3 目前不可用。

**解除阻塞所需的人工动作（Agent 不得代做，见 Action 2 与 §5.4）**：

1. 用户在自有终端执行 `sudo xcodebuild -license`（或 `sudo xcodebuild -license accept`）并接受 Xcode 与 Apple SDKs 许可；随后 `sudo xcode-select --switch /Applications/Xcode.app/Contents/Developer` 已由当前 `xcode-select -p` 满足，无需变更。
2. 同意安装 XcodeGen（建议 `brew install xcodegen` 并固定版本），供 P1-04/P4-01 的 `pnpm native:generate` 使用。
3. 同意 P1-01 Action 5 的 Trellis `task.py create "Native macOS migration" --slug native-macos`，以产生 planning 任务指针；未同意则本计划停留在只读诊断，不进入 P1-02。
4. 明确是否安装/启用 `grill-me`（§4.1，P1-05 门禁）；不安装则模块 Gate 一律标 `BLOCKED: grill-me unavailable`。
5. DBX 已处于 pin 且工作区脏，Action 3 要求不自动 reset；需确认保持现状即可，不需要 `prepare-dbx.sh` 改动 `../dbx`。

## 0. 先纠正迁移起点，冻结不可破坏的约束

### 0.1 实际起点与交付目标

Aeroric **已经是 Tauri 2 + React 19 + Rust 桌面应用**，不是一个只有 CLI 的仓库。现有外部 Agent CLI 由 Rust PTY / DSH 运行时启动；项目、任务编排和部分持久化时序仍在 React 中。因此，仅把 `#[tauri::command]` 去掉，或在 Swift 中启动旧二进制，**不能得到完整的无头 Aeroric**。

最终交付三个可独立验收的入口：

1. **Rust 核心与无头 CLI**：新命令 `aeroric-cli`，无需窗口、WebView、React 或 Tauri 事件循环，能够管理项目、执行/恢复/取消任务、读取会话、操作已迁移的工具服务。
2. **SwiftUI 原生 macOS App**：独立的 `AeroricNative.app`；SwiftUI + 必要的 AppKit 桥接，不把整个 React 页面嵌入 WebView 冒充原生 GUI。
3. **继续工作的 Tauri 跨平台客户端**：迁移期间的回退界面、Windows/Linux 正式客户端、行为对照基线。不得先删旧客户端再追补功能。

“全面吸收参考项目”指：把关键能力登记到有执行任务和验收的能力矩阵中；**不是合并五个仓库、复制它们的整个模型执行器，也不是声称已阅读每一行代码**。已有外部 Agent 负责的推理循环、OAuth 登录、沙箱和插件执行，优先通过能力适配保留，不在 Swift 内重新实现一套。

### 0.2 不可变条件

| 编号 | 实施约束 |
|---|---|
| INV-01 | 只在 `main` 工作；不创建 feature/fix/worktree 分支。提交前按 `.trellis/workflow.md` 获得确认；本计划不授权 push、打 tag 或发布。 |
| INV-02 | Rust 核心不得依赖 `tauri`、窗口句柄、React、Swift；SwiftUI 不写 Agent 启动参数、不解析原始会话 JSONL、不直接改共享业务数据。 |
| INV-03 | 一个数据目录只能有一个业务写入者。SwiftUI、Tauri、CLI 不能各自启动一个写同一目录的运行时。 |
| INV-04 | UI 模式切换只是呈现方式变化，不 kill/resume/relaunch Agent，不改变会话 ID、权限、模型、工作目录和 PTY generation。 |
| INV-05 | OMP 交互运行保持原始 PTY 字节流；不得添加 `--mode`，不得恢复已经移除的 `omp_rpc.rs` 交互方案。一次性模型探测的既有例外不能扩展到交互启动。 |
| INV-06 | OMP 托管启动使用 `~/.aeroric/agent-homes/omp`；`setup_env` 之后清除 `PI_PROFILE` / `OMP_PROFILE`。不新增对用户 `~/.omp` 的自动扫描、配置写入或导入。现存历史读取白名单与 AGENTS 文字有差异，按 P1-02 单独审定，不能借迁移静默扩大权限。 |
| INV-07 | OMP 参数在 POSIX `--` 前，首条 prompt 在其后；hook 保持 `.js` / `.ts`；`agent_end.willContinue=true` 不发终止状态；懒会话发现不新增超时。 |
| INV-08 | 当前四个协议族 Claude / Codex / DSH / OMP 保留；内置档案别名、自定义 Agent、`sessionAgent` 与真实 `sessionFamily` 均不能折叠成单一布尔值。 |
| INV-09 | 不改 `src-tauri/tauri.conf.json` 的本地 ad-hoc 基线：`signingIdentity: "-"`、`hardenedRuntime: false`。未来 Developer ID / notarization 是另一条显式签名路径。 |
| INV-10 | 保留已有 JSON 数据、凭据保护、MCP/hook、远程配对、E2EE、WSL 和 DBX；迁移不得以“macOS 不用”为由删除跨平台后端。 |
| INV-11 | 现有 React UI 修改仍经 pnpm/Vite/TypeScript/项目组件体系；相邻互斥控件继续复用 `src/components/ui/AnimatedSelection.tsx`。SwiftUI 无法直接 import TSX；原生目标的规则范围必须在 P1-02 明确后再动 UI。 |
| INV-12 | 不把终端 ANSI 文本猜成工具调用、审批协议或结构化答案。只有明确可靠的协议/日志投影才进入原生会话视图。未知内容保留为有来源的 opaque 项。 |

### 0.3 本次工具状态，不得写成“验收已通过”

- `trellis --help`、`trellis --version`、`trellis platforms` 已实际调用。CLI 报告 `0.6.16`；同时提示项目模板可更新。这不授权自动执行 `trellis update`。
- `python3 ./.trellis/scripts/get_context.py --mode packages` 和 `task.py current --source` 实际返回 **Xcode license 未接受**。`xcodebuild -version` 能显示 `Xcode 27.0 / 27A266a`，但**打印版本不证明编译工具可用**。
- `swift`、`cargo`、`pnpm` 的命令路径存在；没有据此宣称构建成功。
- `xcodegen` 未在 PATH 中发现。`grill-me` 可执行文件及本次检查的技能目录内同名技能均未发现。
- `grill-me` 有公开的 **Agent skill** 入口；本次复核的候选入口转交 `grilling`，用于追问设计、确认决策，不是天然存在的压力测试可执行文件。安装时须同时核对依赖技能及固定 revision；不能杜撰 `grill-me review --stress` 之类参数，也不能用一次普通自审伪装成工具调用。见 §4、W6。
- 本次没有运行 Swift、Xcode 或 Rust 工程构建，没有调用尚未安装的 `grill-me`。

## 1. 渐进检视记录与功能追踪

### 1.1 检视办法及证据边界

已按“浅目录 → 包清单/入口 → 定向符号检索 → 核心文件局部读取 → 跨模块追踪”的方式检视五个本地参考目录。未把任一参考仓库全量读入上下文。实现时按以下命令重新定位，**一次只跟一个功能**：

```bash
cd "/Users/macbook/Downloads/同步空间/LYX/Aeroric"
find ../ZCode/packages/services/src -maxdepth 1 -type d
rg -n '目标符号' ../ZCode/packages/services/src/目标模块
sed -n '起始行,结束行p' ../ZCode/packages/services/src/目标模块/已定位文件
```

上面后两行是检索模板，不可把中文占位符原样当可执行命令。每次研究落盘：仓库 HEAD、文件、符号、行为、采用点、未验证项。记录参考仓库是否有工作区修改；HEAD 只是定位基准，不代表工作区内容必然与提交一致。不修改参考仓库，不运行它们的安装脚本。

| 来源 / 本地 HEAD | 已检视的关键入口 | 采用与不采用 |
|---|---|---|
| Aeroric `ca69a4b` | `src-tauri/src/lib.rs`、`pty.rs`、`event_watcher.rs`、`session.rs`、`session_omp.rs`、`storage.rs`、`app_settings/schema.rs`；`src/state/app/`、`src/lib/api/`、`src/components/TerminalView.tsx` | 保留真实行为；尤其将 React 的任务编排迁入核心，而不只抽 Tauri 宏。 |
| ZCode `872ad96` | `DESIGN.md`；`packages/rpc/src/channelServer.ts`；`packages/shared/src/execution-state.ts`；`packages/ui/src/App.tsx`、`app-shell/useAppChromeState.ts`；services/desktop 的分层目录 | 采用宿主/服务/客户端边界、工作区身份、权限与 plan 状态分离、窗口局部状态；不移植 Electron 主进程。 |
| opencode `0e3dfd1769` | `packages/core/src/pty/protocol.ts`、`state.ts`、`session.ts`；`packages/tui/src/app.tsx`；app/server/session 目录 | 采用 transport-free service、输出 cursor、有界 replay、session list/history/prompt/interrupt/compact/revert 能力模型；不把其具体 wire 格式套到现有 Aeroric 远程协议。 |
| MiMo-Code `1592084b` | `README.md` 的 Core Features；`packages/app`、`packages/opencode`、`packages/desktop/src/{main,preload,renderer}` 的定向检视 | 与 opencode 的共同来源只计算一次。额外吸收 memory/checkpoint、task tree、goal、subagent、compose/workflow 等能力，不复制内部私有状态。 |
| minimax-code `73a2581` | `packages/agent-core/src/protocol/agent-event.ts`、`agent-message.ts`；`packages/local-runtime-v2/src/runtime.ts`；`packages/local-runtime/src/api/hosted-agent-session.ts`；TUI ACP 控制状态 | 采用带 ID 的流式消息/工具生命周期、hosted snapshot、分阶段启动与迁移锁、能力驱动控制。公开源码不包含其桌面客户端，不能声称参考了它的 GUI 实现。 |
| primuse `4d07ac42` | `Primuse/App/{PrimuseApp,AppServices,MacMenuBarController}.swift`；`Primuse/Views/Mac/{MacContentView,MacSidebar,PMTitleBar,MacSettingsView}.swift`；`Theme/PrimuseTheme.swift`；`project.yml`；`Docs/MacDesignAudit.md` | 采用标题栏/侧栏/详情/底栏布局、语义 token、真实材质、独立设置窗口、视觉 QA；不复制音乐业务、App Groups、商店签名、窗口修复代码和私有视图层级技巧。 |

补充证据：Aeroric 的 `remote-protocol/src/lib.rs` 是 **relay 撮合控制面**，不是整个业务 RPC 类型库；业务契约还在 `packages/remote-contracts` 和 `src-tauri/src/remote`。本地 Swift IPC 不能随意改 relay 协议版本。

### 1.2 功能覆盖矩阵：不得以“以后迁移”关闭条目

标记含义：**N** 原生界面；**C** 核心/CLI；**A** 外部 Agent 能力适配；**K** 保留跨平台实现。每一行必须在 `docs/native/feature-matrix.json` 中有唯一 ID、来源、owner、Task ID、正反例测试和最终状态。

| ID | 必须覆盖的能力 | 现有/参考依据 | 目标 | 执行位置 |
|---|---|---|---|---|
| F01 | 项目、分组、固定、排序、隐藏、头像、最近访问 | Aeroric storage/projectMutations | C+N | P2-03、P5-03 |
| F02 | 任务创建、todo、排队、失败原因、归档、收藏、重命名、完成/取消 | taskLifecycle/taskMutations/taskStatus | C+N | P2-04、P5-03 |
| F03 | 四族与自定义 Agent、模型/effort/权限、升级、配置导入导出 | app_settings、agent_tools、agent_ops | C+N | P3-02、P5-05 |
| F04 | 原始 PTY、输入、resize、IME、回放、后台输出、generation | pty、TerminalView、opencode PTY | C+N | P3-03、P5-01 |
| F05 | 会话发现/分页/导出、恢复/接管/跨族 handoff、用量 | session*、taskSession、usage* | C+N | P3-04、P5-04 |
| F06 | DSH 原生消息、tool、审批、问题、附件、slash、queue、goal、jobs、subagent、trajectory | dsh_webui、dshProtocol、dshSessionFeatures | C+N | P3-05、P5-04 |
| F07 | 文件树、搜索替换、编辑、Markdown、图片、保存冲突、local history | fs/search/formatter/local_history | C+N | P6-01、P6-02 |
| F08 | LSP、诊断、symbol、completion、run/test、DAP、预览 | lsp/dap/run_config、ProjectPage | C+N | P6-03 |
| F09 | Git status/diff/stage/commit/push/history/blame/stash/conflict | git、git_path_guard | C+N | P6-04 |
| F10 | worktree 创建/隔离/统计/合并/丢弃 | worktreeOps、git | C+N | P6-05 |
| F11 | SSH、主机 key、SFTP、远程文件/Git/终端、连接恢复 | ssh/sftp/remote_* | C+N | P7-01 |
| F12 | WSL 配置、项目、Agent、文件、Git、终端 | wsl* | C+K | P7-02；macOS 显示不支持，不伪装本地 |
| F13 | Docker、端口、Conda 环境 | docker/ports/conda | C+N | P7-03 |
| F14 | DBX SQL/Redis/Mongo、表格、结构、导入导出、生产确认、取消查询 | database、同级 dbx | C+N | P7-04 |
| F15 | notebook/quick notes、富文本/Markdown、附件、模板、标签/双链、任务、回收站、导入导出 | notebook 全部子模块 | C+N | P7-05 |
| F16 | notebook RAG、embedding secret、同步/对象存储/OAuth | notebook/rag、notebook/sync、storage_backend* | C+N | P7-06 |
| F17 | Skill Hub、MCP、hooks、插件、provider、proxy、local router | skills/mcp/hooks/local_router/dsh_plugins | C+N+A | P8-01、P8-02 |
| F18 | 手机/远程控制、配对、E2EE、replay、审批/终端租约 | remote、mobile、remote-contracts | C+N+K | P8-03 |
| F19 | 通知、统计、清理、周报、权限、更新和 Release 页面 | notification/analytics/report/permissions | C+N | P8-04 |
| F20 | capability-driven plan/default、compact/fork/revert/share/export | ZCode/opencode/minimax session/control | A+N | P9-01 |
| F21 | memory/checkpoint、task tree、goal/stop、后台 subagents | MiMo + 既有 DSH | A+N | P9-02 |
| F22 | compose、确定性 workflows、bounded retry、Dream/Distill | MiMo | A+N | P9-03 |
| F23 | ACP、OAuth、插件/连接器、browser/多模态/voice | minimax + MiMo | A+N | P9-04 |
| F24 | 设置中 TUI/高级 ↔ 纯原生 UI，无损切换 | 用户要求 | N | P5-02 |
| F25 | glass/classic、a11y、菜单、窗口、快捷键、多窗口 | primuse + macOS | N | P4-03、P4-04、P10-02 |
| F26 | 不带 GUI 的启动/操作/退出与复连 | 用户要求 + 四个 CLI 参考架构 | C | P3-06、P3-07、P8-05 |

P1-02 必须再从命令注册表和页面注册表机械核对遗漏。上述表是功能组，不代替逐命令清单；增加条目必须补 owner 与测试，不能在表中填写笼统的“其他功能”。

## 2. 目标结构、数据流与明确技术决策

### 2.1 新目录与依赖方向

```text
Aeroric/
├── src/                              # 保留 React，沿用当前工具链
├── src-tauri/                        # 保留独立 Cargo 工程/target/Cargo.lock
│   └── src/{core_adapter,host_client}.rs
├── core/                             # 新增独立 Rust workspace，不改根为 workspace
│   ├── Cargo.toml                    # workspace + patches + workspace.dependencies
│   ├── Cargo.lock
│   └── crates/
│       ├── aeroric-protocol/         # serde DTO、稳定错误、能力与事件
│       ├── aeroric-core/             # 业务服务、持久化、PTY、外部 Agent 适配
│       ├── aeroric-host/             # 宿主启动、UDS server、认证、分发
│       └── aeroric-cli/              # aeroric-cli 二进制，serve/run/task/... 命令
├── macos/
│   ├── project.yml                   # XcodeGen 源，生成 AeroricNative.xcodeproj
│   ├── Config/{Debug,ReleaseLocal}.xcconfig
│   ├── AeroricApp/{Info.plist,Resources/,Sources/}
│   ├── AeroricUITests/
│   └── AeroricKit/
│       ├── Package.swift
│       ├── Package.resolved           # 首次引入外部 package 后生成，不伪造空锁文件
│       ├── Sources/{AeroricProtocol,AeroricClient,AeroricUI}/
│       └── Tests/{AeroricProtocolTests,AeroricClientTests,AeroricUIUnitTests}/
├── contracts/native/v1/              # JSON Schema、黄金样本；不重用 relay 版本号
├── scripts/native/                  # pnpm 调用的可复现脚本
└── docs/native/                      # 决策、矩阵、评审证据；不存真实凭据/会话
```

依赖顺序：

```text
aeroric-protocol ← aeroric-core ← aeroric-host ← aeroric-cli
        ↑               ↑
 Rust/Swift/TS fixtures  └── Tauri 的临时 embedded adapter
        ↑
Swift AeroricProtocol ← AeroricClient ← AeroricUI ← AeroricApp
                              │
                              └── 认证 UDS → host → core
```

- `aeroric-host` 是库；`aeroric-cli serve` 是宿主进程入口。不增加一个名字不明的第二 daemon。
- `aeroric-core` 不通过 Swift callback 获取业务状态；只在窗口/系统 UI 交互时发有超时的 platform request，CLI 返回 `INTERACTIVE_UI_REQUIRED` 或提供明确命令行应答方式。
- 不引入 Rust↔Swift FFI 作为第一阶段主通道：先避免 ABI、内存所有权、Swift unwind 与 Rust panic 边界。后续性能数据证明 IPC 不足才开新 ADR。
- Rust `.await` 期间不得持有 `parking_lot` 锁。保留 `TaskManager` 的 generation 防陈旧退出逻辑和实测锁获取顺序；注释与实现有出入时先测试、再修正文档。

### 2.2 宿主与写入权的迁移阶段

| 阶段 | Tauri | SwiftUI/新 CLI | 数据策略 |
|---|---|---|---|
| P1–P2 | 原实现 | 尚无生产入口 | 只读盘点、复制到临时目录的脱敏 fixture |
| P3–P7 | 使用逐步抽出的 embedded core；保留旧包装入口 | 独立 host，仅允许测试专用 `--data-dir` | 禁止两个运行时写同一目录；未接入全服务前不切用户数据 |
| P8-05 后 | macOS 自带同版本 host helper，通过 `host_client.rs` 连接；不依赖安装 Native Preview；Windows/Linux 可继续 embedded | 连接/复用同一个 host | host 持跨进程排他锁；所有业务 mutation 由 host 执行 |
| P10 验收后 | 仍可作为回退 UI | 原生客户端可候选为 macOS 默认入口 | 切 UI 不回滚格式、不丢任务；是否替换正式产品须另行批准 |

不要在 P3 让旧 Tauri 和新 daemon 同时碰 `~/.aeroric`，再期待最后解决冲突。`AlreadyInUse` 必须明确显示锁持有者与安全退出方式，不能自动杀 PID。

### 2.3 本地协议 v1：字段与传输约束

采用 **Unix domain socket + UTF-8 JSONL** 作为 macOS 本地通道。高频 PTY 内容在 JSON 中以 base64 封装原始字节；不要在 UTF-8 尚未拼齐时损坏输出。远程/mobile 保留原有二进制 terminal framing，经过显式适配。

```json
{"v":1,"id":"req-uuid","method":"task.start","idempotencyKey":"op-uuid","params":{"projectId":"p","taskId":"t","expectedRevision":"7"}}
{"v":1,"id":"req-uuid","ok":true,"result":{"taskId":"t","generation":"2","revision":"8"}}
{"v":1,"id":"req-uuid","ok":false,"error":{"code":"REVISION_CONFLICT","message":"State changed","retryable":false}}
{"v":1,"event":"task.updated","epoch":"host-uuid","seq":"18","data":{"taskId":"t","generation":"2","revision":"8"}}
{"v":1,"event":"terminal.output","epoch":"host-uuid","seq":"19","data":{"taskId":"t","generation":"2","offset":"0","nextOffset":"3","bytesBase64":"YWJj"}}
```

这些是 envelope 示例，不是完整 Task DTO。协议 schema 中必须逐方法固定必填字段：

- `v` 是 **本地协议版本**；握手协商版本交集、server build、数据格式、功能列表。不能因为版本等于 1 就推断所有 capability 都有。
- ID 用字符串；`seq`、`revision`、`generation`、字节 offset 用十进制字符串，避免 JS 超过安全整数范围。时间统一 epoch milliseconds；可选字段与显式 `null` 分开规定。
- socket 所在私有目录 `0700`，socket / token / lock 文件 `0600`；验证 owner、拒绝 symlink、拒绝已有不可信文件。校验系统返回的 peer UID。
- canonical data-dir hash 参与 socket 身份；macOS socket 路径受长度限制。优先使用经过 owner 校验的短临时私有目录，创建前按 `sockaddr_un.sun_path` 容量检查；不能把很长的中文仓库路径直接拼入 socket 名。
- 每个 host epoch 生成 256-bit token，首帧在 5 秒内认证；token 只通过私有文件/继承管道传递，**不得进 argv、日志、URL 或子 Agent 环境**。同 UID 恶意进程不属于可由 token 完全隔离的威胁边界，需如实说明。
- JSONL 一帧最大 1 MiB；PTY 原始块最大 64 KiB；JSON base64 后仍须守总帧上限。附件/大文件走分页/专用流，不扩大总帧无限传。
- control queue 与 terminal queue 分开，建议初始上限分别 256 条、4 MiB/连接；慢消费者先失去 terminal 订阅并得到 `STREAM_GAP`，控制事件不可静默丢弃，必要时断开要求重同步。
- `seq` 由一个有序发布点分配，snapshot 返回 `(epoch, highWatermark)`；订阅注册与 snapshot waterline 必须构成原子边界，防止两次 RPC 中间漏事件。
- 默认状态 replay 4096 条且总计不超过 8 MiB；超窗返回 `SNAPSHOT_REQUIRED`。host 重启换 epoch，旧 seq 不沿用。PTY 每条流独立 offset/generation，不能与业务 seq 混淆。
- 原始 PTY 回放不等于完整终端屏幕 snapshot。缓冲截断时发送显式 reset + truncated 标记；不能从任意转义序列中间接着喂，并承诺“屏幕完全恢复”。P5-01 验证终端状态重建；达不到时明确“历史已截断”，保持实时交互可用。
- `requestId` 用于路由应答；mutation 的 `idempotencyKey` 用于重试去重。未知是否已经执行的 `task.start` 不得自动再启动；必须查询 task/generation 后对账。
- `cancel request` 只取消 RPC 等待；`task.cancel` 才取消 Agent。socket 断开不代表任务结束。
- 错误码至少包含 `INVALID_ARGUMENT`、`UNAUTHENTICATED`、`UNSUPPORTED_CAPABILITY`、`REVISION_CONFLICT`、`NOT_FOUND`、`BUSY`、`STREAM_GAP`、`SNAPSHOT_REQUIRED`、`HOST_RESTARTED`、`INTERACTIVE_UI_REQUIRED`、`IO_ERROR`。

### 2.4 模式不是 Agent 协议

定义 `InteractionMode = native | advanced`：

| 能力 | 纯原生 UI (`native`) | TUI/高级命令行 (`advanced`) |
|---|---|---|
| 通用项目/任务/文件/Git/设置 | SwiftUI | 同一 SwiftUI 外壳，可展开终端 |
| DSH 有已验证结构化能力的会话 | 原生消息、composer、审批、问题、目标与工具卡片 | 保留既有 DSH terminal projection，并标注不是 OMP 原生 TUI |
| Claude/Codex 的当前 PTY 会话 | 原生任务操作、经已有 parser 得到的历史；不假装具备结构化实时审批 | 原样运行并显示上游 TUI |
| OMP / 任意 PTY 自定义 Agent | 原生任务元数据；若已有受支持历史投影则只读显示；交互缺口明确提示 | 原样显示上游 PTY；从不加 `--mode` |
| 上游需要终端确认 | 显示“此 Agent 的当前交互需要高级模式”和用户触发的切换按钮；不自动批准、不注入猜测按键 | 焦点交给真实终端 |
| ACP / 其他新增结构化 adapter | 仅能力协商及安全测试通过后开放 composer | 可以另外选择原始 CLI 入口，但不能热替换当前会话协议 |

**不可满足的组合必须显式说明**：既要求任意 CLI 的全部交互都用原生控件、又禁止其结构化协议/终端，就无法可靠实现。本计划保留所有 CLI 功能的方式是高级模式；纯原生模式不偷偷嵌入终端，不承诺所有第三方 TUI 都能被无损转成原生聊天。

偏好持久化：

- 全局 `interactionMode` 是新窗口默认值；已有导入用户默认 `advanced` 保持原行为，新安装默认 `native`。
- 每窗口实际模式、selected project/task、输入草稿、滚动锚点归 `WorkspaceStore`；业务会话不随窗口消亡。
- 设置开关 OFF → `native`，ON → `advanced`；这是独立 `Toggle`，不是两份会互相竞争的布尔值。
- 切换必须暂缓至 IME composition 完成，保存草稿；终端 renderer 可保持实例而隐藏，但解绑输入租约和窗口焦点。任务输出继续在 host 消费。

## 3. 执行与验收规约

### 3.1 所有 Task 的通用验收

每个 Task 的 Acceptance Criteria 自动包含：

1. 只修改该 Task 列出的文件；发现新依赖先补到 Task PRD，不顺手大改。
2. 编写/移动代码前读相应 `.trellis/spec/`；不把参考仓库约定覆盖本仓约定。
3. 先写行为测试再完成实现；测试不能只验证文件存在、字符串包含或 mock 自己返回自己。
4. 所有命令从仓库根运行；脚本用带空格/中文的绝对路径测试，不依赖开发者机器目录。
5. 通过 `git diff --check`，检查 `git status --short`，没有凭据、真实用户会话、编译产物或参考仓库变更；新增未跟踪文本文件另用 `git diff --no-index --check /dev/null <文件>` 检查，不能漏验。**判据修正（P1-02 实测）**：该命令对任何非空文件**恒返回退出码 1 且输出为空**，因此"无空白错误"的判据是**输出字节数为 0**，不得拿退出码当门禁；照字面用退出码判会让每个 Task 都误报失败。另注意本仓 `.gitignore` 默认忽略 `docs/*`、`.claude`、`.trellis/`、`AGENTS.md`，`git status --short` 看不见它们并不代表文件不存在——本计划工件所在目录的放行规则见 P1-02 执行状态。
6. 记录：命令、退出码、用例数、未运行项、环境阻塞。测试 runner 返回 0 但执行 0 个用例不算通过。
7. 测试只用隔离临时目录及 fake agent，不能修改真实 `~/.aeroric`、`~/.omp`、`~/.ssh`、Keychain、远端数据或生产数据库。
8. 每新增一个 core 服务，同时更新 `command-map`、本地协议和 CLI 可达性；未提供专用子命令的已注册方法，经 P3-07 的受 schema/capability 约束的 `rpc` 子命令调用。不得只给 Swift View 接线而让对应能力仍需打开 GUI。

### 3.2 统一脚本接口（在 P1-04 实现前不可运行）

`package.json` 增加以下 script，所有原生前端工作仍通过项目入口执行：

| 命令 | 必须执行的真实动作 |
|---|---|
| `pnpm native:doctor` | 只读环境/许可/工具/DBX pin 检查，返回非零及具体缺失项；不得自动安装或接受许可 |
| `pnpm native:generate` | 验证 `project.yml`、调用已锁定的 XcodeGen，生成项目；不手改 pbxproj |
| `pnpm native:build` | Rust host build → 复制到 app 预构建位置 → Xcode build → 显式内部到外部 ad-hoc 签名 → 验签 |
| `pnpm native:dev` | 完成上述 Debug build 后 `open` 指定 app；不启动另一套隐藏前端 |
| `pnpm native:check <module>` | 按 `checks.json` 跑明确 test target / Swift suite，确认用例数非零；未知 module、缺测试、skip 必选项均失败 |
| `pnpm native:check all` | 矩阵中全部必选模块、旧 Tauri/TS 回归、契约、平台兼容、签名检查 |
| `pnpm native:inventory --check` | 命令/页面/功能矩阵一致性检查；新增入口未映射则失败 |
| `pnpm native:contracts --check` | Rust 导出的 schema 与已提交 schema/fixtures、Swift/TS decoder roundtrip 无漂移 |
| `pnpm native:ui-test` | 已生成 Xcode 项目的 `AeroricNativeUITests`，真实 app + fake host + 隔离数据 |

验收命令清单存 `scripts/native/checks.json`；每个 Task 在引入 suite 时同步增加其模块映射、必选 case、最少 test count，不等到 Phase 10 才登记。runner 使用 argv 数组而不是字符串 shell 拼接、设置超时、保留真实退出码、收集 stdout/stderr 到忽略目录 `.artifacts/native/`。不得实现“总是 exit 0”的占位 check。`all` 是最终完整集合，首次必过门禁在 P10-05；早期阶段只运行其已实现模块，不能把 `all` 偷改为“当前碰巧存在的测试”。

测试命名约定：Rust `core/crates/<crate>/tests/<suite>.rs` 对应 `cargo test --manifest-path core/Cargo.toml -p <crate> --test <suite> --locked`。Swift 单元 suites 在 `macos/AeroricKit/Tests/`，由 `swift test --package-path macos/AeroricKit --filter <Suite>` 执行；XCTest UI suites 只经 `pnpm native:ui-test` 调用 `xcodebuild test`。尖括号是替换模板，`checks.json` 必须保存展开后的真实 argv。P1 创建 Cargo 锁文件后，后续 Rust 命令默认 `--locked`；确需依赖变更时单独记录并审查锁文件 diff。

## 4. `grill-me` 强制 Review 门禁

### 4.1 工具准备不能伪造

P1-05 先识别用户选用的 `grill-me` 分发形式：

- **Skill 形式**：读取真实 `SKILL.md` 和来源 revision，按当前 Agent 平台已公布的技能入口调用 `grill-me`。若该 revision 是转交 `grilling` 的入口，必须同时确认被调用技能存在，记录完整调用链；不能只安装别名文件就算准备完成。支持 slash 的平台可用 `/grill-me`；这是一条 **Agent 指令，不是 shell 命令**。
- **确有本地 CLI 形式**：先执行 `grill-me --help`，仅使用帮助中存在的参数；把实际 argv 记入 `docs/native/tooling.md`。
- 当前未提供工具时：可继续写计划/测试，但模块 Gate 标记 **BLOCKED: grill-me unavailable**，不宣布模块通过，不自建一个同名脚本冒充。
- 安装/启用来源经用户确认，不执行未知来源的 `curl | sh`。公开同名 skill 仅是候选，不能冒认一定是用户指定的那一个。

### 4.2 每个核心模块验收时实际执行的 Review 指令

下文“执行 `grill-me [模块名]`”统一表示通过已验证技能/CLI 入口发送以下具体输入，**每个模块一次独立记录，不是最终统一跑一次**：

```text
grill-me
审查模块：<module-id>
设计：MIGRATION_PLAN.md 对应 Task + docs/native/ 对应 ADR
代码：<明确的 changed files，不传整个仓库>
验证：<已执行命令、退出码、用例数、反例和故障注入结果>
请沿调用链逐项追问：
1. 数据/任务/PTY 的唯一 owner 是谁？退出、断线、重入、并发顺序是什么？
2. 哪个反例可以推翻当前实现？是否由真实测试覆盖？
3. 权限、路径、token、日志、审批重放和不可信数据的边界在哪里？
4. 旧 Tauri/CLI、四个 Agent 家族、存储/远程协议会怎样回归？
5. 回滚是否会丢数据、重复启动任务或更改用户权限？
按已固定技能版本的提问轮次执行；可由源码/测试回答的先查证，产品决策交用户，
不得把未回答的问题写成已通过。
```

由执行 Agent（不是假定 tool 内建功能）保存 `docs/native/reviews/<module-id>.md`：tool 来源/revision、实际调用入口、审查 commit/diff hash、问答、证据、问题级别、修复后复验结果。

**Gate 通过条件**：实际调用证据存在；必选测试通过；无未解决的崩溃/数据丢失/安全越界/功能阻断问题；中低风险明确由谁接受。`grill-me` 不替代压力测试，压力测试是以下各 Task 指定的 cargo/Swift/XCTest/脚本动作。

## Phase 1 — 环境、Trellis 依赖树与工程骨架

**入口**：只读基线已理解；不要求用户已有 Developer 账号。

**出口**：环境可用、任务依赖明确、空骨架可构建、工具 Review 路径真实存在。未过出口不抽运行时。

### Task P1-01 — 环境证据与人工前置条件

> **【执行状态】accepted @ 2026-09-21** — Action 1～5 全部执行完毕，逐条验收通过。要点：
>
> - **Action 2（许可）**：首轮实测 `xcrun swift` / `xcrun --sdk macosx --show-sdk-path` / `python3` / `cc` **全部退出码 69**；且一次性临时目录里的 `cargo build` 在链接阶段报同一错误，证明阻塞面覆盖 Rust 链接与全部 Trellis python 脚本。Agent 未代接受许可、未用 sudo；上报后由用户自行 `sudo xcodebuild -license` 解除，复测全部退出 0（Swift 6.4 / `arm64-apple-macosx27.0.0`）。
> - **Action 3（DBX）**：`git -C ../dbx rev-parse HEAD` == `scripts/dbx-ref.txt` pin，**无漂移**；工作区 38 个脏文件（`crates/dbx-core/*`、`vendor/wry/*`）**未做任何 checkout/reset**；`patches/dbx-security.patch` 经 `apply --reverse --check` 确认已应用，故 `prepare-dbx.sh` 幂等首分支成立、未运行该脚本。
> - **Action 4（缺失工具）**：经用户授权安装 XcodeGen `2.46.0`（`/opt/homebrew/bin/xcodegen`）。未升级任何既有 Node/Rust/pnpm。
> - **Action 5（Trellis）**：经用户授权执行 `task.py create "Native macOS migration" --slug native-macos` → 真实返回 **`.trellis/tasks/09-21-native-macos`**（`TASK_DIR`）。`current --source` → `Source: session:qoder-native-macos-20260921-aeroric`；`get_context.py --mode packages` 真实成功（现有 3 包）。落盘 `docs/native/environment.md` 与 `$TASK_DIR/research/environment.md` 并互相链接。
> - **`pnpm install --frozen-lockfile --ignore-scripts`** 退出 0（`Already up to date`，4 workspace 项目）。
>
> **两处必须被下游知道的实测偏离**：
>
> 1. **P1-01 原文两条验收在 Trellis 0.6.15 下互斥**：`current --source` 有指针只能靠 `task.py start`（`task.py:134` 是唯一调用 `set_active_task` 处），而 `start` 无条件 `planning → in_progress`（`task.py:141-145`；degraded 分支 `task.py:124-131` 同样翻）。按 §2.1"先测试、再修正文档"处理：`start` 写指针后把 `task.json.status` 显式改回 `planning`，并记入 `docs/native/environment.md` §1.5.2。后续需要"活跃指针 + 实现未授权"同时成立时沿用此法。
> 2. **`TRELLIS_CONTEXT_ID` 不跨 Bash 调用存活**：Agent 每次 shell 独立，之后所有 `task.py` / `get_context.py` 调用必须在同一条命令内重新 `export TRELLIS_CONTEXT_ID="qoder-native-macos-20260921-aeroric"`，否则落回 degraded、指针读不回来。
>
> **§0.3 已过期**：其中 `trellis 0.6.16` 实测为 **0.6.15**；"Xcode 许可未接受"已解除；"`grill-me` 未发现"已改为"按固定 revision `c55ee460…` 安装完成，但本会话技能注册表未热加载、无法真实调用"（详见 `docs/native/tooling.md` §4）。
>
> **本 Task 未运行**：`pnpm lint/typecheck/test/build`、`cargo check --manifest-path src-tauri/Cargo.toml --locked`（属 P1-05 基线）、任何本项目 swift/xcodebuild 构建。

- **Input**：`README.md` Development、`package.json`、`src-tauri/Cargo.toml`、`scripts/dbx-ref.txt`、`.trellis/workflow.md`、§0.3。
- **Action**：
  1. 先只读查询 `sw_vers`、`uname -m`、`xcode-select -p`、`xcodebuild -version`、`xcrun swift --version`、`rustc --version`、`cargo --version`、`node --version`、`pnpm --version`、`python3 --version`、`trellis --version`、`command -v xcodegen`，暂存真实输出；许可阻塞期间不开始代码实现。
  2. 遇到许可阻塞，停止依赖 Xcode 的命令；让用户在其终端运行 `sudo xcodebuild -license` 阅读并接受。Agent 不替用户接受协议、不使用 sudo 自动绕过。
  3. DBX 若不存在，先按 README 获取用户确认再准备；若已存在且 dirty，不自动 checkout/reset。核对 pin 与 `scripts/prepare-dbx.sh` 的 idempotent 行为。
  4. XcodeGen 等缺失工具经确认安装后记录版本；不为本任务升级现有 Node/Rust/pnpm。
  5. 环境允许 Trellis 脚本运行后，**先取得创建任务并进入 planning 的同意**，再显式执行以下命令。若脚本提示缺少 session identity，按提示设置本会话唯一 `TRELLIS_CONTEXT_ID` 再查询/创建，不能等到 P1-03 才处理而使此步验收无有效指针。若已有本迁移任务则核对并复用，不能每次继续都新建：
     ```bash
     trellis --help
     trellis --version
     trellis platforms
     python3 ./.trellis/scripts/task.py --help
     python3 ./.trellis/scripts/task.py current --source
     python3 ./.trellis/scripts/task.py create "Native macOS migration" --slug native-macos
     ```
     使用 create 返回的真实路径设置 `TASK_DIR`，例如 `export TASK_DIR=".trellis/tasks/09-21-native-macos"`；例子不是固定路径。之后才新建 `docs/native/environment.md`、`$TASK_DIR/research/environment.md` 记录证据并互相链接。未同意建任务时只交付只读诊断，不进入 P1-02。
- **Acceptance Criteria**：
  - `python3 --version`、`python3 ./.trellis/scripts/get_context.py --mode packages`、`xcrun swift --version` 真实成功；许可错误不得以版本打印成功掩盖。
  - `git -C ../dbx rev-parse HEAD` 与 pin 差异已说明；`pnpm install --frozen-lockfile --ignore-scripts` 在确认后成功或有明确阻塞。
  - 环境报告区分已验证/未运行；没有为解决问题关闭 SIP/Gatekeeper、改全局权限或要求 Apple Team。
  - `python3 ./.trellis/scripts/task.py current --source` 指向本次真实任务；任务仍处于 planning，尚未授权脚本/应用代码实现。

### Task P1-02 — 冻结逐命令清单、数据格式与规则范围

> **【执行状态】accepted @ 2026-09-21** — 三份规划工件已落盘：`docs/native/feature-matrix.json`（26 个 F 组 + 13 条 G 遗漏项）、`docs/native/command-map.json`（660 条命令逐条带 `registered_at` / `definition_candidates` / `category` / `core_service` / `rpc_method` 提案 / `swift_scene` / `check_module` / `test_id` / `feature_ids`）、`docs/native/adr/001-native-boundary.md`。**未写任何脚本、未改任何运行时代码**（Action 1 的"只做规划工件"约束成立；`inventory.mjs` 仍归 P1-04）。
>
> 机械核对（独立复算，非引用二手结论）：解析 `src-tauri/src/lib.rs:487-1162` 的 `generate_handler![...]` → **660 条**（656 条模块限定 + 4 条 crate-root：`hide_main_window` / `exit_app_after_task_flush` / `restart_app_after_task_flush` / `app_exit_listener_ready`），与 `src-tauri/src/command_registration_tests.rs` 的双向集合相等断言一致；顶层模块 **52** 个 + crate root；settings panel **19** 个（仅 `wsl` 带 `platforms:["windows"]` 门控，`AppSettingsDialog.tsx:174` 是唯一消费者）。
>
> **三条必须让下游知道的新发现（不属于原 §1.2 二十六个能力组，已作为 G 条目登记）**：
>
> 1. **正则命中数 ≠ 命令数**：`rg -n '#\[tauri::command\]' src-tauri/src` 得 **677**，真实命令 **660**，差 **17** 处全部出现在注释/文档/守卫测试自身的字符串里（`secrets.rs:9,245,288,410,420,456,465`、`command_registration_tests.rs:1,54,143,168`、`notebook/rag/{mod,commands,embed,index}.rs`、`app_settings/mod.rs:63`）。这正是本节 Acceptance 警告的"把正则匹配数冒充完整语义解析"。**P1-04 的 `inventory.mjs` 必须做 attribute→fn 配对而不是计数**，并预期 677/660 这个差值。
> 2. **§3.1 通用验收第 5 条按字面执行必然失败**：`git diff --no-index --check /dev/null <文件>` 对任何非空文件**恒返回退出码 1 且输出为空**（实测 `docs/native/command-map.json`：`exit=1`、`output-bytes=0`）。正确的判据是**输出为空即无空白错误**，不能用退出码。已在本文件标注，避免后续每个 Task 都误判为门禁失败。
> 3. **`docs/*`、`.claude`、`.trellis/`、`AGENTS.md` 都被 `.gitignore` 忽略**（`.gitignore:29,44,56` + `AGENTS.md` 条目），本计划的矩阵/ADR/评审证据原本会**只存在于单机**、无法作为"已提交清单"被 `native:inventory --check` 复核。已按该文件自身的既有约定（`!docs/remote-public-access.md` 旁的注释："新增被引用的文档时一并放行"）追加 `docs/native/` 白名单，并做负向验证：嵌套的 `docs/native/tmp-probe/scratch.log` 仍被 `docs/native/**` 忽略。`.claude`（grill-me 落盘处）与 `.trellis`（任务树）**保持忽略不变**，属该仓既有约定，其内容 provenance 改由 `docs/native/tooling.md` 承载；`AGENTS.md` 本身不被跟踪，P1-04 对它的修改无法进 diff，需在 P1-04 单独确认口径。
>
> **分类自纠（已写入 `command-map.json` meta.revisions）**：初版把 `tests::discover_tests/run_tests/remote_discover_tests/remote_run_tests` 判为 `internal`（误读为测试夹具），实为 **F08 运行/测试**产品能力，已改判；`git::{create,merge,remove}_task_worktree` 与 `worktree_diff_stats` 追加 **F10** 并拆出独立 `worktree` check 模块。最终分布：`domain_rpc 579 / platform_specific 62 / host_local_os_ui 19 / internal 0`，22 个 check 模块。
>
> **F21/F22/F23/F24 对应命令数为 0** 是预期结果（外部高级能力 + 新增模式开关，本仓无对应命令），已在矩阵 meta 里显式标注"不得当作迁移遗漏，也不得据此声称已支持"。
>
> **用户决策证据（ADR-001 §4、§5）**：① 批准 P1-04 向 `AGENTS.md` **追加**原生控件规则段（SwiftUI 用 `Picker(.segmented)` / `List(selection:)`，禁手绘邻接 active 背景；React 的 `AnimatedSelection.tsx` 原规则不动）；② OMP 非托管历史读取（`session_omp.rs:130` 的 `~/.omp/agent/sessions`、`:131→:146-156` 的 `readdir ~/.omp/profiles`）**默认禁用、代码保留、待单独审定**，新 core 的 OMP 发现根只允许 `~/.aeroric/agent-homes/**`。附带把"`agent-homes/*` 全量放行 vs 注释承诺按 agent 校验"的不一致登记为 **G13**。
>
> **本 Task 未运行**：任何构建/测试命令（P1-02 不交付可执行物）；`grill-me` 未对本模块调用（工具在本会话不可用，见 P1-05）。

- **Input**：P1-01 已获同意并创建的 planning task；`AGENTS.md`；`src-tauri/src/lib.rs`、`command_registration_tests.rs`；`src/lib/api/`；`src/state/app/`；`src/components/app-settings/panelRegistry.tsx`；§1。
- **Action**：
  1. **本 Task 只做规划工件，不编写业务或检查脚本**。新建 `docs/native/feature-matrix.json`、`docs/native/command-map.json`、`docs/native/adr/001-native-boundary.md`。用 `rg`/局部读取收集每个 Tauri command、事件、后台服务、settings panel 的源文件/符号；分类为 domain RPC / host-local OS UI / platform-specific / 内部实现。`inventory.mjs` 的实现与负向测试放 P1-04。
  2. 每个 domain 命令指定目标 Rust service、RPC 方法、Swift scene、测试 ID；禁止把 `remote-protocol` 错列为全量业务协议。
  3. 在 ADR 记录本计划的最小 macOS 版本选择为 **14.0**；基础材质用 `NSVisualEffectView`/`Material`；macOS 26+ 的 `glassEffect` 只做 availability-gated 增强。
  4. 在 ADR 显式提出原生组件规则：React 保留 `AnimatedSelection.tsx`；SwiftUI 采用平台 `Picker(.segmented)`、`List(selection:)` 等系统选中态，禁止手绘邻接 active 背景。此处只记录决策；用户批准范围且 Trellis start 后，才由 P1-04 修改 `AGENTS.md` 的相关段落，不删除 React 原规则。
  5. 登记 OMP `session_omp.rs` 的既有用户历史白名单与 AGENTS 禁止读写文字的差异，默认不新增扫描/读写；要求明确批准“显式选择的只读历史导入”是否保留。未决时该分支禁用并保留代码，不擅自删旧用户数据。
- **Acceptance Criteria**：
  - `rg -n 'generate_handler!|#\[tauri::command\]' src-tauri/src` 和 `rg -n 'id:|Panel' src/components/app-settings/panelRegistry.tsx` 的结果逐项对照清单；记录总数与人工核对项，不把正则匹配数冒充完整语义解析。每个条目有实际源文件，尚不运行不存在的 inventory 脚本。
  - ADR 包含 UI 规则与 OMP 历史规则的用户决策证据；未决项阻塞对应实现。
  - 当前原始行为差异不被“顺便修复”；源文件路径均经本地验证。

### Task P1-03 — 显式调用 Trellis，建立父子任务及依赖

> **【执行状态】accepted @ 2026-09-21**
>
> - Action 1：复用 P1-01 的 `TASK_DIR=.trellis/tasks/09-21-native-macos`，**未重复 create 父任务**；`trellis --version` → 0.6.15、`task.py --help/current/list` 实跑。未运行 `trellis init`，未运行 `trellis update`。
> - Action 2：`task.json` 与真实路径核对一致（父 `status: planning`、`children` 10 条、日期前缀 `09-21-` 由脚本产生）。
> - Action 3：新建 `prd.md`（R1～R13 + 各 Phase 验收勾选）、`design.md`（目录/依赖方向/写入权阶段/协议 v1/模式模型/无 FFI 决策）、`implement.md`（顺序、每 Task 通用规则、任务索引表、大模块展开、**Deviation log D1–D8**）、`research/reference-map.md`、`research/dependencies.md`。implement 显式链接本计划并记录版本，避免两份计划漂移。
> - Action 4：`task.py create "Phase N — …" --slug native-phase-N --parent "$TASK_DIR" --no-start` × 10 全部成功；每个 Phase 子任务的 `implement.md` 都写明本 Phase 的 Task ID、前置 Gate、依赖边与阻塞登记（Trellis 父子树只表分组，**依赖以 `research/dependencies.md` 为准**）。
> - Action 5：`.trellis/config.yaml` 的四包合并方案（`aeroric-core/aeroric-macos/aeroric-desktop/aeroric-web` + 四个 spec index）写在 Phase 1 子任务 `implement.md` 中，实际修改归 P1-04；本 Task 未改工程配置。
> - Action 6：`add-context` 真实条目父任务 implement 7 条 / check 4 条，10 个子任务各 implement 5 条 / check 3 条；`validate` 父任务与 Phase 1 子任务均 `✓ All validations passed` 且**无截断告警**；`list-context` 显示真实条目。**仅 start Phase 1 子任务**（`09-21-native-phase-1` → `in_progress`），其余 9 个保持 `planning`，未提前宣布全树可执行。
> - Action 7：未覆盖其他会话指针（本会话 key `qoder-native-macos-20260921-aeroric`）；未运行任何可能自动提交的 `archive` / `add_session` 流程。
>
> **新发现 D8（必须让下游知道）**：`validate` 报告 `MIGRATION_PLAN.md`（165 330 B）与 `docs/native/command-map.json`（399 592 B）超过 `.trellis/config.yaml` 的 `context_injection.max_file_bytes = 32768`，注入时会被静默截断；另有 `max_artifact_bytes = 65536` 与 `max_total_bytes = 131072` 两级上限。处理：从注入清单中移除这两个超限项，改为新增 **`docs/native/command-map.summary.json`（16 861 B，只含 meta + 模块分类表 + revisions/known_ambiguities）** 作为可注入摘要，660 条命令全集仍以 `docs/native/command-map.json` 为唯一真相、按需 `Read` 或用 P1-04 的 `inventory.mjs` 读取。**不得把摘要当成逐命令清单**；本 Task 未调高项目配置上限（改配置属 P1-04）。
>
> 另记：`task.py add-context` 的 JSONL 行 schema 是 `{"file":…,"reason":…}`（不是 `path`），且新任务自带一行 `_example` 占位、填入真实条目后须删除——`validate` 会把占位行也计入条目数。

- **Input**：`.trellis/workflow.md`、`.trellis/config.yaml`、P1-02 矩阵、本文件。
- **Action**：
  1. 复用 P1-01 经同意创建的 `TASK_DIR`，本计划交付不等于用户批准启动迁移。显式核对工具与任务状态，不重复 create 父任务：
     ```bash
     trellis --help
     trellis --version
     trellis platforms
     python3 ./.trellis/scripts/task.py --help
     python3 ./.trellis/scripts/task.py current --source
     python3 ./.trellis/scripts/task.py list
     ```
  2. 核对 `TASK_DIR/task.json` 与真实路径；日期前缀由脚本产生，禁止假定重跑仍是示例路径。
  3. 新建 `$TASK_DIR/prd.md`（需求与验收）、`design.md`（边界/决策）、`implement.md`（顺序/门禁）、`research/reference-map.md`、`research/dependencies.md`。在 implement 中链接本计划，记录版本，避免两份计划各自漂移。
  4. 用 `task.py create "...标题..." --slug native-phase-N --parent "$TASK_DIR"` 按 Phase 创建子任务，子任务工件列出 Task ID 与前置 Phase。**Trellis 的父子树不是依赖执行器**；依赖边必须写在 artifacts 中。
  5. 在 Phase 1 子任务 `implement.md` 列明 `.trellis/config.yaml` 新包与对应 spec 的合并方案，实际修改移到 P1-04；不在实施批准前修改工程配置。
  6. 以 `task.py add-context "$TASK_DIR" implement ... ...` / `check ... ...` 加入现有跨层、协议规范和研究；各至少一条真实项。复杂子任务各自完成三份 artifacts 并得到实施批准后，才 `task.py start <真实子任务路径>`；当前仅 start Phase 1 子任务，后续阶段按顺序分别评审/start，不提前宣布全树可执行。
  7. 没有 session identity 时，按脚本提示设置唯一 `TRELLIS_CONTEXT_ID` 再 start；不覆盖其他会话指针。已有 `.trellis` 不运行 `trellis init`，不盲目 update。
- **Acceptance Criteria**：
  - `python3 ./.trellis/scripts/task.py validate "$TASK_DIR"` 成功；`python3 ./.trellis/scripts/task.py list-context "$TASK_DIR"` 显示真实条目；Phase 1 子任务也通过同样检查且在获准后为 `in_progress`。新包在 P1-04 配置后再验，不能提前要求存在。
  - `research/dependencies.md` 至少有 P1→P2→P3→P4→P5、P5→P6→P7→P8→P9→P10 的依赖和 Gate；不能只画树而不注明阻塞。
  - 用户未批准实施时保持 planning；未获提交批准不运行可能自动提交的 archive/add_session 流程。

### Task P1-04 — 创建可构建骨架和统一检查入口

> **【执行状态】accepted @ 2026-09-21** — 全部 Acceptance 命令实测通过，逐条如下。
>
> | 验收命令 | 结果 |
> |---|---|
> | `cargo check --manifest-path core/Cargo.toml`（首次，生成锁） | 通过，产出 `core/Cargo.lock`（7497 B） |
> | `cargo check --manifest-path core/Cargo.toml --locked` | 通过 |
> | `swift build --package-path macos/AeroricKit` | `Build complete! (20.04秒)` |
> | `node scripts/native/check-patches.mjs` | 通过：9 条 patch 在 src-tauri 与 core 间 path spec 与 git rev **完全一致** |
> | `cargo tree --manifest-path core/Cargo.toml --locked` 无 tauri/webview | 通过（**stdout 上 0 命中**） |
> | `pnpm native:check skeleton` | `7 case(s) passed, 24 test cases executed`（Rust 5 + Swift 12 + node:test 7） |
> | `pnpm native:check nonexistent` | 退出码 **2**，明确拒绝把未登记模块当成功 |
> | `pnpm native:inventory --check` | `registered=660 mapped=660 features=26 gaps=13 → consistent`，退出码 0 |
> | `node --test scripts/native/inventory.test.mjs` | 7/7 通过，含 **4 条负向**（删映射/造映射/未知 check 模块/未知 Fxx 必须失败）与 1 条"grep 计数会高估"回归 |
> | `python3 ./.trellis/scripts/get_context.py --mode packages` | 7 包齐现：原 3 包 + `aeroric-core/macos/desktop/web`，`default_package` 仍为 `aeroric-mobile` |
> | `cargo fmt --all --check` / `cargo clippy --all-targets -- -D warnings`（core） | 均退出 0 |
> | `git diff --check` / `git status --short` | 干净；变更仅 `.gitignore`、`package.json`、新增 `core/`、`macos/`、`scripts/native/`、`docs/native/`；无凭据/真实会话/编译产物/参考仓库改动 |
>
> **交付物**：`core/`（workspace + `aeroric-protocol`/`aeroric-core`/`aeroric-host`/`aeroric-cli` 四 crate，CLI `main.rs` **故意 `exit(2)` 报未就绪**）、`core/crates/aeroric-protocol/tests/skeleton.rs`、`macos/AeroricKit/`（3 library + 3 test target，真实 JSONL 编解码/分包/多字节 UTF-8/依赖方向 smoke）、`scripts/native/{doctor,check,inventory,inventory.test,check-patches}.mjs` + `{generate,build,dev,ui-test,contracts}.mjs`（未就绪者 `exit(78)`，绝无假绿）、`checks.json`、`package.json` 的 8 条 `native:*`、`.trellis/spec/{aeroric-core,aeroric-macos,aeroric-desktop,aeroric-web}/…/index.md`。
>
> **必须让下游知道的实测细节**：
>
> 1. **`swift test` 用例数只能用 `Test Case '…' (passed|failed)` 行**（本机 12 条）。`Executed N tests` 在 SwiftPM 6.4 下**每个 bundle 重复输出**，求和会把 12 读成 36；`--filter` 亦不保证有该行。`check.mjs` 因此以逐用例行为主、`Executed` 上界为兜底。
> 2. **`cargo tree` 的纯度断言只能读 stdout**：`[patch]` 未使用告警走 stderr，其文本含 `…/src-tauri/vendor/…`，全量输出 grep 会在完全干净树上必然失败（实测 stderr 命中 3 次、stdout 命中 0 次）。已作为 `kind:"forbid"` 的 `stream:"stdout"` 固化进 `checks.json`。
> 3. **虚拟 workspace 不允许顶层 `[lints]`**：`core/Cargo.toml` 用 `[workspace.lints.clippy]`，四个成员各自 `[lints] workspace = true`。
> 4. **骨架期 9 条 patch 全部 `not used in the crate graph`** 是预期：`portable-pty`/DBX forks 直到 P3-02（PTY）与 P7-04（database）才进图。patch 的**路径与 rev 正确性**由 `check-patches.mjs` 保证，不等于已被解析验证；两者不得混为一谈。
> 5. **零用例检测真实生效**：首轮 `inventory-unit` 因 node 默认 spec reporter 无 `# pass` 行而被判 `FAIL (runner exited 0 but executed 0 cases)`，遂固定 `--test-reporter=tap`。这验证了 §3.1 第 6 条不是纸面规则。
> 6. `inventory.mjs` 读 `command-map.json` 时键名是 **`rust_path`（snake_case）**，首轮因误用 `rustPath` 产生 661 条假不一致——**P2-01 之后任何新读该文件的工具都必须用 snake_case 键**，字段名即契约。
> 7. **`AGENTS.md` 已按 P1-02 批准追加两节**（`### Native SwiftUI selection rule`、`### Native OMP session history boundary`），React 的 `AnimatedSelection.tsx` 原文一字未动。注意 **`AGENTS.md` 本身被 `.gitignore` 忽略**，该修改不进 diff、不随仓库分发，只对本机后续 Agent 生效——这是 D4 的遗留口径问题，**留待 P1-05/Phase 1 Gate 时由用户裁决**（是否解除忽略）。
> 8. 新增计划外工件：无。`scripts/native/checks.json` 的 `plannedModules` 只是把 §3.2 与 Phase 2–10 的模块名提前登记为"存在但尚未实现"，`check.mjs` 对它们一律非零，避免 `all` 被偷换成"当前碰巧存在的测试"（§3.2 原文风险）。
>
> **【补记 @ 2026-09-21，同会话】首版 P1-04 验收有一项没做到位，现已补齐——记录以免后来者误以为本来就覆盖了。** Action 7 要求的是"**注册命令 / 事件 / 面板**逐项映射"，首版 `inventory.mjs` 只覆盖了命令。现补为三面对账，并新增 `--freeze`（可复现地重生成冻结清单，且**合并时保留人工已写的 disposition，不被抽取覆盖**——**这句当时不成立，也当时无法被 `--check` 发现；2026-09-21 第七次补记记录了实测破坏、修法与新增的 `inventory-freeze-is-noop` 门禁**）：
>
> - **事件 38 条**：逐条含 `emitters[]` / `listeners[]` / `rust_consts[]` / `ts_consts[]` / `orphan` / `disposition` / `gap_id`，另有 **29 处动态 emit 站点原样登记**（`AgentInstallStage::*` 实为进度回调、不是 Tauri 事件；`dsh_webui.rs:3007` 的 10 路帧映射、`lib.rs:198` 的 `action.event_name()`、`pty.rs:504` 的 `OutputSink::Event` 全部归入"动态"而非猜测归属）。抽取结果与 §1.1 的独立人工检视**完全一致**（38 emitted / 35 listened / 3 条无消费者），两条互不依赖的路径互相印证。
> - **面板 19 条**：key + `via`（18 个字面量 + 1 个 `ALL_AGENT_CONFIGS_NAV_KEY` 常量）+ registry 位置 + `target_scene` + `task_ids`。
> - **失败面**：新事件未冻结、冻结事件已消失、源路径失效、孤立事件未定性、`gap_id` 不存在、面板缺项或多项、面板无原生落点、动态 emit 数漂移——逐项报错。`inventory-unit` 由 7 例增至 **12 例**（新增 5 条负向），骨架门禁由 24 用例增至 **29 用例**。
>
> 抽取过程中真实踩到并修掉的三个坑（后续任何静态扫描器都会再踩一遍，值得留档）：
>
> 1. **全局符号表会串味**：`PROGRESS_EVENT` 在 `report.rs:28` 与 `notebook/rag/commands.rs:35` 取值不同，按全局 map 解析会静默选中"最后扫到的那个"，把 `weekly-report-progress` 错记成 `notebook-rag-index-progress`。现按**文件内优先、跨文件仅在唯一时解析、歧义显式记为 unresolved**。
> 2. **逐行正则丢订阅**：`listen<{\n …questions: Array<{…}>…}>(` 这种跨行 + 嵌套尖括号的泛型参数，任何单行正则都抓不到；先试"整段跳过非括号字符"的宽松写法反而把监听数从 34 打到 26 并引入 `appSettings.localRouter.statusRunningAt` 这类假事件名。最终改为**带深度计号的调用点扫描器**，且 `;` / `}` 只在深度归零时才终止泛型（内联对象类型里合法）。
> 3. **`--check` 必须在注入上限内**：`command-map.json` 已从 400 KB 增长，摘要 `command-map.summary.json` 19.5 KB 仍在 32768 字节以内；摘要额外加入 `event_dispositions` / `panel_targets` 两个只读视图，并显式声明**摘要不得代替清单行**。

- **Input**：P1-02 ADR、P1-03 已批准实施的子任务；现有 Cargo patches/DBX 路径、pnpm scripts、`.gitignore`。
- **Action**：
  1. 新建 `core/Cargo.toml`、四 crate 的 `Cargo.toml` 与 `src/lib.rs`，CLI 增加 `src/main.rs`。先只提供版本/空 service 接口，不复制业务。
  2. 保留 `src-tauri`、`remote-protocol`、`remote-relay` 的独立 workspace/锁文件；不把它们全部吸入根 workspace，避免现有 release `src-tauri/target/...` 路径改变。
  3. `core/Cargo.toml` 是其依赖解析的 patch owner。按现有 `src-tauri/Cargo.toml` 将 portable-pty/ssfmt/notify-types 和 DBX forks 的 revision 与路径准确映射；不要让下游 crate 的 patch 被 Cargo 忽略。添加 `scripts/native/check-patches.mjs` 比较两边 source/revision。
  4. 新建 `macos/AeroricKit/Package.swift`，三个 library target 和文首约定的三个单元测试 target；各自创建 `Sources/<target>/PackageSmoke.swift`、`Tests/<target>Tests/PackageSmokeTests.swift`（UI 使用 `AeroricUIUnitTests` 目录），加入真实编解码/依赖方向 smoke test。最低 macOS 14，固定 Swift tools version；尚无外部依赖时不要求或伪造 `Package.resolved`，在 P5-01 首次解析外部依赖时生成并纳入版本控制。
  5. 新建 `scripts/native/{doctor,generate,build,dev,ui-test,check,contracts,inventory}.mjs`、`checks.json`，修改 `package.json` 加 §3.2 命令。此时尚未实现的 build/UI-test/contract 动作必须非零报未就绪，不能假绿；P4-01 必须接通最小生成/构建/签名/UI-test 流水线，不能等到 P10 才能运行 App。
  6. 修改 `.gitignore` 忽略 `.artifacts/native/`、`core/target/`、Swift `.build/`、DerivedData 和生成的 Xcode 工程；保存 `project.yml` 和共享 scheme 定义作为生成来源。
  7. 实现 P1-02 规划的 inventory 检查：注册命令/事件/面板逐项映射、重复/缺失/失效源路径报错；新增 `scripts/native/inventory.test.mjs`，在临时复制的清单删一条映射，必须失败，不改真实清单制造假事故。仅在已有明确批准时更新 `AGENTS.md` 的原生规则范围。
  8. 修改 `.trellis/config.yaml` 的 packages，**合并**增加 `aeroric-core: { path: core }`、`aeroric-macos: { path: macos }`、`aeroric-desktop: { path: src-tauri }`、`aeroric-web: { path: . }`；保留原三包与 `default_package`。新建 `.trellis/spec/aeroric-core/backend/index.md`、`.trellis/spec/aeroric-macos/frontend/index.md`、`.trellis/spec/aeroric-desktop/backend/index.md`、`.trellis/spec/aeroric-web/frontend/index.md`；只写已批准约定，按项目规范使用英文。
- **Acceptance Criteria**：
  - `cargo check --manifest-path core/Cargo.toml` 首次生成审查后的锁；此后 `cargo check --manifest-path core/Cargo.toml --locked`、`swift build --package-path macos/AeroricKit` 成功。
  - `node scripts/native/check-patches.mjs` 成功；`cargo tree --manifest-path core/Cargo.toml --locked` 核心无 tauri/webview 依赖。
  - `pnpm native:check skeleton` 执行 Rust 与 Swift 非零数量测试；`pnpm native:check nonexistent` 必须非零。
  - `pnpm native:inventory --check`、`node --test scripts/native/inventory.test.mjs` 通过；`python3 ./.trellis/scripts/get_context.py --mode packages` 看到新增包。未获原生规则批准时不能把 SwiftUI UI 实现列为已解锁。

### Task P1-05 — 接通 Review 工具并固化基线

> **【执行状态】交付物已完成，Gate 按用户决定挂账（不宣布通过）@ 2026-09-21**
>
> **Action 1**：新建 `docs/native/tooling.md`（工具真实形态判定、固定 revision `c55ee460…`、MIT 许可、sha256、完整调用链 `grill-me → grilling`、本会话不可调用的实测证据）与 `docs/native/reviews/skeleton.md`（§4.2 输入、被审文件清单、已执行命令与用例数、五个追问项的本地证据、**未解决风险**、"实际调用"一节留空待填）。
> **Action 2**：新建 `docs/native/baseline.md`，记录 `pnpm lint`(0) / `pnpm typecheck`(0) / `pnpm build`(0) / `cargo check --manifest-path src-tauri/Cargo.toml --locked`(0，1m11s) / `pnpm test` 的**两次**全量运行，以及新骨架侧 10 条命令的退出码与用例数。原有失败**单列未回退断言**。
> **Action 3**：新建 `docs/native/rollback.md`：基线 `ca69a4b`、当前**零提交**、每个 Phase 的关闭旋钮、五条跨回滚不变的规则、P8-05 才执行的演练步骤，以及"P1–P4 产出可整块丢弃且对用户数据零影响"的判据。未复制、未恢复任何真实用户目录。
>
> **基线期间两项必须留档的实测**：
>
> 1. `pnpm test` 首轮 **3 个用例失败**（`notebook-panel-core.test.tsx` ×2、`notebook-panel-tags.test.tsx` ×1，后者耗时 33.2 s），把这 2 个文件单独重跑 **110/110 全绿**（CPU 时间 182 s > 墙钟 120 s），第二次全量重跑 **`Test Files 369 passed (369)`、退出码 0**。结论：这是 369 文件满并行下的**负载抖动**，不是代码缺陷、也不是迁移引入的回归（`git diff --stat` 只有 `.gitignore` 与 `package.json` 两处纯新增）。**纪律**：此后本机任何一次红色全量 `pnpm test` 都必须先复跑或定向复跑再判定为回归。
> 2. `core/` 与 `macos/` **完全不在现有 CI 覆盖内**（仓库根无 `Cargo.toml`，`checks.yml` 的 fmt/clippy/test/audit 全部按 `--manifest-path` 指定三个旧 workspace，eslint/prettier glob 只到 `src`/`packages`/`mobile`）。已登记为 **G14**；P7-02 与 P10-04 必须真正落地这两处 CI 扩展，否则 `core/` 只有本地门禁。
>
> **Gate 判定**：`pnpm native:check skeleton` 通过（7 cases / **33 用例**）；基线命令无被掩盖的失败；无崩溃/数据丢失/越界问题。**Acceptance 第 2 条已真实执行**：经用户批准把固定 revision 的 `grill-me`/`grilling` 拷入本 CLI 实际读取的技能根 `~/.mirasim/skills`（`~/.claude/skills` 的符号链接目标）后注册表当场热加载，`Skill(grilling)` 调用成功并完成第一轮 frontier 追问（详见 `docs/native/reviews/skeleton.md` › Actual invocation）。**但 Q1～Q6 六项决策仍待用户回答并实施复验，因此本 Task 不标 accepted，Phase 1 Gate 保持 open**——§4.2 明令"不得把未回答的问题写成已通过"。
>
> **本轮附带完成的、不依赖门禁的 Phase 1 收尾**（用户上一问已选择"继续做不依赖门禁的收尾"）：
>
> - **G10 关闭**：660 条命令全部按"模块子树内 fn 声明唯一"规则解析出 `definition_resolved`（**660/660，0 条待人工**）。规则必须用子树而非 `<mod>.rs`/`<mod>/mod.rs` 两点，因为 `app_settings/mod.rs` 以 glob `pub use` 重导出子文件里的命令（其 :63 注释正是讲这件事）；`skills::list_skills` 的歧义据此消解（`skills.rs:1190` 是命令，`dsh_webui.rs:2711` 只是 impl 方法）。`inventory --check` 现在会**回读该文件该行**断言它确实是该 fn 的声明，指错即失败。
> - **G08 关闭**：5 处动态命令名展开为封闭枚举（合计 83 个名字）——`prefixGitCommand`/`lspCommand` 按 `invokeFacade.ts:93-97` 的 local/`remote_`/`wsl_` 三态展开，另外三处面板的 `persist(command)` 与 `handleToggle` 各自只有两个字面量。`--check` 断言站点文件行仍存在、枚举非空、且"生成但未注册"的集合必须与 `known_unregistered` **逐字相等（两个方向漂移都失败）**。
> - **新发现 G15（中等，属既有缺陷而非迁移引入）**：`src/lib/api/git.ts:25-34` 声明了 `remote_git_checkout_branch`/`wsl_git_checkout_branch`/`remote_git_create_branch`/`wsl_git_create_branch` 三个 target 的镜像字段，但 `lib.rs` **从未注册这四个名字**；`prefixGitCommand` 因此能拼出不存在的命令。当前潜伏（`BranchBar.tsx:64,391` 只用 `.local`），P6-04 必须"注册命令"或"删字段并在 UI 显式禁用远程分支操作"二选一。
> - 门禁用例 24 → 29 → **33**；`inventory.test.mjs` 7 → 12 → **16 例**（负向 4 → 9），新增的负向包含"**悄悄缩小已披露缺口的集合也必须失败**"这一条，防止用删声明的方式掩盖问题。
>
> **【第二次补记 @ 2026-09-21，仍不依赖门禁】命令可达性对账（P1-02 要求的"从注册表机械核对遗漏"的另一半）**
>
> - **正面结论**：前端 **422 处** `invoke("字面量")` 调用点的命令名**全部命中 660 注册表，0 个未知**——即除 G15 之外，前端没有其它"调一个不存在的命令"的同类缺陷。该不变式已固化为 `--check` 断言（今后任何拼错/删漏立即失败）。
> - **新发现 G16（重要，影响 P2-01）**：**93/660** 条命令在活代码里查无任何调用方（`src/`、`mobile/`、`packages/` 无字面量，`src-tauri/src` 内除自身 `fn` 声明与 `lib.rs` 注册外无按行引用）；集中在 `dsh_webui` 42、`lsp` 的 `remote_` 变体 12、`remote_git` 10、`wsl_git` 10、`notebook` 3、`dsh_plugins` 3、`app_settings` 3。**其中 81 条是我上一版自行标注的"待提案 RPC 方法"**——照原样提升进 `methods.schema.json` 就是把死面冻结进三方 wire 契约。已登记 `command-map.json` 每条命令的 `reachability{frontend_literal,other_package_literal,rust_internal_refs,registered}` + `reachable`，`meta.reachability` 记录规则、被排除目录与分布。
> - `.worktrees/` 污染是这个核对里最容易犯的错：`review-fixes` 与 `arch-debt` 两个陈旧 worktree 里有整份旧代码，全仓 grep 会把死命令看起来"有人用"。扫描器现固定排除 `node_modules/.git/.worktrees/.build/.artifacts/target/dist/build`，并把实际排除项写进 `meta.reachability.excluded_dirs` 以便复核。**不得动这两个 worktree**（属用户在建工作）。
> - **两条实现教训（写任何门禁都会再犯）**：① **同一规则只能有一份实现**——我最初用一次性脚本冻结、再用 `inventory.mjs` 复算，两者对"引用计数"的定义不同（按出现次数 vs 按行、集合键 `${file}:${line.length}` 还会碰撞），复算先把一致率打到 741 条假漂移、并把 unreachable 从 93 误报成 4。现改为 `inventory.mjs --freeze-reachability` **与被 `--check` 复算的同一个 `buildReachIndex`**，冻结与校验不可能再各说各话。② **门禁必须快**：按命令逐个扫全仓是 O(名称数×文件数)，把 `native:check skeleton` 从约 1 分钟拖到 **超过 15 分钟仍未结束**（实际会被跳过，等于没有门禁）。改成单遍 token 索引后，`--check` **0.87 秒**、整条 `pnpm native:check skeleton` **7.5 秒 / 35 用例**。
> - 门禁规模：用例 33 → **35**；`inventory.test.mjs` 16 → **18 例**（负向 9 → 11）；`gaps` 15 → **16**；`command-map.summary.json` 20.9 KB → **29.98 KB，已逼近 32 768 字节注入上限**——Phase 2 再加内容前必须先把摘要拆成多份或改为按需读取，否则会重演 D8 的静默截断。
>
> **【第三次补记 @ 2026-09-21】G16 定性完成 + 新决策项 G17**（纯取证，不依赖 Q1～Q6 的答案，任一答案下都成立）
>
> 93 条不可达命令已全部解释完，落成 `docs/native/reachability-triage.md`，四个桶：
>
> | 桶 | 数量 | 结论 | 归属 |
> |---|---|---|---|
> | A：`remote_*`/`wsl_*` 且本地父命令**可达** | **34** | **不是死代码**：由 `src/lib/invokeFacade.ts:93-97` 的 `prefixCommand` 运行时拼接，镜像表只写了一半所以字面量扫不到 | P2-01 必须收敛成"**1 个方法 + target 参数**"，否则同一行为会在契约里出现 34 次 |
> | B：父命令**也不可达**的变体 | **2** | `tests::discover_tests / run_tests / remote_discover_tests / remote_run_tests` 整簇未接线（UI 走 `run_config::*`） | P6-03（F08 run/test） |
> | C：`dsh_webui::*` typed 命令零证据 | **41**（原写 42 系误记。`command-map.json` 实算：该模块 66 条命令中 25 条有任意证据、24 条 `reachable=true`、**41 条零证据**） | **结构性重复，不是腐烂**：React 的 DSH 层实际走 `/api/remote.mux` WebSocket 下行（`src/dshProtocol.ts:16-21`），这 42 条 typed 命令被旁路。权威能力清单是 `protocol_inventory.rs` 钉死的 **198 个 live 名字**（`RPC_METHODS` 62 + `REMOTE_METHODS` 87 + `REMOTE_EVENTS` 19 + `REMOTE_EVENT_MODES` 19 + 流帧 3/4/4；另有 10/10/28 项显式标为 RETIRED，不算能力面。第十二次补记更正：此前此处写「235 个上游 RPC 方法名」是从未经文件核实的误记）（commit `c291e796…`、`0.1.5-rc.2`、protocol v2） | **新决策项 G17** |
> | D：其余零证据 | **15** | 逐条列出；`dsh_plugins::{install,uninstall,toggle}_dsh_plugin`、`skills::get_marketplace_skill_details`、`notebook::notebook_{git_sync,sync_bind,sync_unbind}` 更像 **F16/F17 尚未接 UI 的应有能力**，不是死代码 | 各归 F 对应 Task |
>
> - **G17（新登记）**：P3-05 必须**一次性显式决定** DSH 能力的移植形态——42 个 typed core 方法，还是一个 gateway/mux 形态的 core service + 固定 inventory 清单。默认"42 条全搬"会把同一上游能力复制成两套本地契约，且 §5.1 的一文件一条展开会把这层重复**固化进 core**。这条不能靠沉默关闭。
> - 硬约束重申：93 条**一律不得凭本文件删除**（INV-10：不得因"macOS React 不调用"就删跨平台/mobile 相关后端面）。
> - 仍存疑且已如实标注：mobile 是否经 relay 用 `packages/remote-contracts` 里的方法名访问桶 C（P8-03 验）；`notification::install_release_update` 是否被 Rust 侧调用（行级启发式可能把自身声明误计为引用）；198 个 live 名字（尤其 62 条 `RPC_METHODS`）是否仍与当前 dsh-web 一致（参考仓库只读，未核）。
> - `gaps` 16 → **17**；`meta.counts.reachability_buckets` 已写入矩阵，`--check` 与 18 例测试保持全绿（整条门禁 7.5 秒）。
>
> **【第四次补记 @ 2026-09-21】Q2/Q3/Q4 提前落地（只增加守卫、不删除任何自由度，故不必等 Q1/Q5/Q6/Q7）+ 一次 Agent 自伤事故**
>
> 先说清楚**为什么这三项可以先做**：它们只是把 §3.1 第 6 条、§3.2"不得实现总是 exit 0 的占位 check"与 INV-02 变成机器强制，不预设任何产品形态，也不冻结 wire 契约命名（那才是 Q1 的真正内容，仍未做）。
>
> - **Q3（INV-02 真守卫）**：新增两个 `kind:"absent"` case —— 源码扫描 `tauri::|AppHandle|tauri::State|WebviewWindow` 与 core manifest 依赖黑名单。诚实说明：原先唯一的纯度断言扫 `cargo tree`，而骨架期图里根本没有可引入 tauri 的依赖，**那条绿灯近乎恒真**；现在 INV-02 从第一天起就有人守。`absent` 的语义是"rg 退出 1 才算通过，退出其他非零值=扫描器坏了、不得当成 absence"。
> - **Q2（地板）**：`runner.floors` + inventory 断言 `minCases == 地板`、无地板条目即失败、**地板出现孤儿条目即失败**（孤儿=整个测试被删或改名）。已实测有效：把 `swift-package-smoke` 的 `minCases` 从 12 偷偷改成 1，门禁立刻报 `minCases 1 != frozen floor 12`。同时如实写明能力边界：这是"降标准必须改两处、可评审"的摩擦力，**不是证明**——同时改两个数字仍靠 diff 评审。
> - **Q4（脱敏）**：日志写盘前按 6 类规则洗，并记录**未脱敏原文的 sha256**（可证明"洗的是什么"而不留下秘密）。并配 `--redact-selftest` 门禁 case。**它首跑就抓出我自己两个真泄漏**：kv 规则把 `Authorization: Bearer <token>` 里的单词 `Bearer` 当成值洗掉、token 原样留下；以及带引号的值（`api_key: "AKIA…"`）因字符类排除引号而**完全不洗**。修法是规则排序（scheme 类先跑）+ 允许带引号值 + 增加 Basic/AKIA/sk- 形态，现 8 个 fixture 全过。
> - 门禁规模：9 → **10 个 case**，用例数 33 → **35**（新增均为 gate/selftest 型，不新增测试数）。
>
> **【第五次补记 @ 2026-09-21】自查推翻了自己两条写进冻结工件的结论**
>
> 这轮不做新功能，专门核对"我之前断言过但没验证过的事实"。结果两条是错的，都已经在文档里就地更正（不静默改写）：
>
> 1. **Q1 的前提是假的。** 我在给用户的选项里写"剥离前缀**有撞名风险**"。实算 579 条提案：**三种方案碰撞数全为 0**（原样 0／剥前缀 0／去命名空间 0），因为**660 个命令名本身就是全局唯一的**（已验证；仓库自己的注册守卫也不允许重名）。所以"安全"从来不是区分点，区分点只有可追溯性与未来扩展。推荐仍是 (A)，但**理由换成诚实的那个**；`reviews/skeleton.md` 里明确标注了这条更正，避免用户因为一个不存在的风险而选 (A)。
> 2. **可达性扫描器有一个会致祸的洞。** 它**整块丢弃测试文件**。后果：`discover_tests`/`run_tests` 被我判成"整个 run/test 入口未接线"，而实际上 `src/test/test-explorer-panel.test.tsx:65,170` 明确断言 `invoke` 收到 `"run_tests"` —— **功能是接线的**，只是生产调用点不用裸字面量。**如果照我原来的分类执行"死代码清理"，就会把活功能从迁移范围里剔掉。** 修法：测试证据作为**弱信号** `test_only_literal` 保留（不再直接丢弃，也不单独判定可达），前缀派生关系记为 `prefix_derived_of` / `prefix_derived_parent_reachable`。
> 3. 顺带清掉一处无依据引用：`reachability-triage.md` 桶 D 曾写"React 用的是 `note_sync_*` 名字"——`grep` 结果 **0 命中**，根本不存在替代名，已改成"只是未接线"。
> 4. **更正后的不相交划分（和为 93）**：28 前缀派生 ／ 6 前缀派生+测试 ／ 6 仅测试证据 ／ **53 完全无证据**（`dsh_webui` 41）。带 `rpc_method` 提案的 81 条里 **40 条已有解释**，真正需要逐条判定的收敛到 **41 条**。`gaps` 计数不变（17），G16 标为 `triaged-rev2-awaiting-phase-2`。
> 5. 结构性事实值得留给 P2-01：**660 个命令名全局唯一** ⇒ "方法名 = 旧命令名"无需命名空间也不撞车；反过来说，日后改任何一个旧命令名都是**协议变更**而不是重构。
>
> 门禁仍 10 case／35 用例全绿，`--check` consistent，`git diff --check` 干净。Q1／Q5／Q6／Q7 依旧未动——**这四项要的是你的判断，不是我的验证**。
>
> ⚠ **事故记录（Agent 自伤，已恢复，如实留档）**：做地板规则的破坏性验证后，我用 `git checkout -- scripts/native/checks.json` 复原，但该文件**尚未被 git 跟踪**（`??` 状态），checkout 静默失败，而我 `||` 后面的兜底把 `/dev/null` 的内容写回文件 → **`checks.json` 被截断成 0 字节**。
> - 发现方式：紧接着做完整性检查时 `wc -c` 为 0、JSON 解析失败——不是靠"测试还是绿的"发现的（当时确实还是绿的，因为进程内已加载）。
> - 恢复：按已知设计重建该文件，并**逐条重跑 10 个 case + 18 个测试 + `--check`** 以确认行为与丢失前一致（35 用例、10 case、inventory consistent 全复现）。文件内已写明"reconstructed after truncation"。
> - **两条永久教训**（已并入下方纪律与 `rollback.md`）：① **未跟踪文件没有 git 兜底**，任何"恢复"必须显式 `cp` 备份再改，绝不能用重定向/`/dev/null`/管道兜底；做破坏性验证前先 `cp x x.bak`。② 这正是 **Q5 的实证**：Phase 1 全部产出现在仍只存在于这台机器的单一副本里，`docs/native/` 与 `scripts/native/` 一旦被误写就没有第二份。**建议现在就提交到 `main`（不 push）**，但这仍是你的决定，我没有替你做。
>
> **另**：用户同时决定 ① Phase 1 产出**暂不提交**（保持未提交状态，回滚点即 `ca69a4b` 工作树）；② `AGENTS.md` **维持被忽略、不再处理**，SwiftUI 原生规则的持久载体以已跟踪的 `.trellis/spec/aeroric-macos/frontend/index.md` 与 `docs/native/adr/001-native-boundary.md` 为准。D4 的口径问题据此关闭。
>
> **【第六次补记 @ 2026-09-21】门禁答复之外的最后一类风险：工件"存在但不被看见"。** 这轮不加功能，专查我自己写进 §3.1 第 5 条却从未真正执行过的两项，结果挖出一个正在发生的丢失。
>
> 1. **`.gitignore` 的白名单是按目录枚举写的，于是漏掉了 `extraction/`。** 实测：`docs/native/extraction/database-notebook.md`（**28 496 B**，§5.1 那份 75 文件的展开工件）被 `.gitignore:58 docs/native/**` 判为忽略，`git ls-files --others` 里根本不出现——也就是说它**只存在于这台机器**，永远无法提交，正是我在 `repo-measurement-traps` 里写下"被忽略≠不存在，白名单外的东西只有一份"的那个失效模式，这次落在我自己当天的产出上。修法不是再补一行枚举，而是**改成按扩展名 + 任意深度放行**：`!docs/native/**/`（先解除目录自身的忽略，git 才会进去）+ `!docs/native/**/*.md` + `!docs/native/**/*.json`。用一次真实探针验证两侧都成立：临时建 `docs/native/__probe/deeper/probe.md` → 出现在未跟踪列表；同目录 `probe.txt` → 仍被忽略。Phase 4+ 还会新建工件目录，不该再依赖手动枚举。
> 2. **空白卫生**（§3.1 第 5 条要求的判据=输出字节为 0，不是退出码）：`MIGRATION_PLAN.md` 若干"只有空格的空行"、四个 `core/crates/*/Cargo.toml` 的 EOF 多余空行、以及上面那份 extraction 文档的 EOF 空行，全部清掉；现在 **52 个未跟踪文本文件 0 问题**。顺带说明：extraction 那份之前扫不到，正是因为第 1 条的忽略漏洞——两个缺陷是同一根因的两面。
> 3. **注入上限这件事从"逼近"变成了"已解决"。** 上一轮记录的 `feature-matrix.json` 31 560 B / `command-map.summary.json` 30 942 B（96%／94% of 32 768）确实就是 D8 的重演前夜。新建 **`scripts/native/digest.mjs`**（`pnpm native:digest` 重切，`--check` 校验）把两份 canonical 摘要切成 **7 个分片 + `index.json`，最大片 16 769 B（51% 上限）**；canonical 文件保持原样继续当真相源，分片只是"切"不是"摘要"。关键在 `--check` 的四条断言：canonical 每个顶层键**必须恰好落在一篇**（漏键＝静默丢行，直接失败）、不得重复分配、分片合并后必须**逐值还原** canonical、以及每片不得超上限、分片记录的 `sourceSha256` 必须等于当前 canonical（改了 canonical 忘了重切＝stale 失败）。**21 份 Trellis 清单**（11 implement + 10 check + 父）改指向分片，11 个 native 任务 `validate` 全通过，`list-context` 实测分片确实被注入。
> 4. 新增 **8 条负例测试**（`digest.test.mjs`，全部在临时副本上做，绝不碰冻结清单）：漏键／重复分配／幽灵键／改 canonical 不重切／手改分片 payload／分片文件缺失／超上限。**两个 bug 是这套自校验当场抓出来的**：① 我最初用 `!==` 比较对象，于是刚写出的分片被自己判成不匹配；② `repoRoot` 用 `new URL(...).pathname` 求目录——本仓路径含中文（`同步空间`），pathname 是**百分号编码**的，一律改用 `fileURLToPath`。这两类错误如果留到 Phase 4 之后，代价完全不同。
> 5. 门禁由 10 case／35 用例增至 **12 case／43 用例**（新增 `digest-shards-current` gate 与 `digest-unit` test，后者按既有规则同时登记地板 `digest-unit@skeleton: 8`）；`unknown module → exit 2`、`native:doctor 18/18`、`inventory --check consistent`（660/660/26/17/38/19/660/5/422/93）、`git diff --check` 干净。**因为这轮真的动了四个 `core/crates/*/Cargo.toml`**（删 EOF 空行），`cargo fmt --manifest-path core/Cargo.toml --all --check` 与 `cargo clippy … -- -D warnings` 都**重新实跑并各自 exit 0**（后者用重定向拿真实退出码，不看管道尾巴），门禁内没有这两条不代表可以不验。
>
> 仍未动的还是那四项：**Q1／Q5／Q6／Q7**。第 1、3 条各自降低了"未提交状态"的损失概率（工件现在可提交、注入不再靠边缘余量），但**是否提交仍是你的决定**。
>
> **【第七次补记 @ 2026-09-21】"可复现的冻结清单"这条 P1-04 的自述是假的——`--check` 看不见它。**
>
> 趁门禁还没答复，去验一条我一直声称但从未实测的性质：`--freeze` 能不能把已冻结的 `command-map.json` **原样重生成**（"冻结"应有的意思）。结果是不能，而且损失方式是静默的：
>
> 1. **跑一次 `--freeze` 会删掉人工批注。** 实测：714 866 B → 713 046 B，**38 个事件上的 `note` 键全部消失**，其中 14 条是有内容的结论（如 `agent-tool-install-progress`："progress already reaches the UI via agent-operation-changed; P3-02 must either give it a consumer or drop the emit"、`dsh-host-remote-event`："P3-05 dsh_host must define who consumes raw host frames"）。根因：合并写的是 `{...抽取结果, disposition: 旧的 ?? 默认, gap_id: …, target: …}` ——**白名单式**只搬它认识的人工字段，其余一律被抽取结果覆盖掉。
> 2. 改成"只覆盖源派生键"后**仍然错**：`disposition`／`gap_id` 被从 `emitted-dynamically`／`redundant-channel`／`needs-native-consumer`、`G01` 统一重置成 `untriaged`／`null`。因为 `extractEvents` 自己在每个条目里塞了 `disposition: null, gap_id: null` 占位（`inventory.mjs:401`），所以"抽取结果"本身就带着会覆盖人工判定的键。
> 3. **最该记住的一点：这两次破坏发生时 `--check` 都照样报 consistent**（第 1 次尤甚——它比较的字段恰好都还在）。所以"P1-04 的 11 项验收全绿"对这条性质**没有任何覆盖力**。真正暴露它的只有"跑一遍再比字节"。
> 4. 修法是**反过来列黑名单**：`EVENT_SOURCE_KEYS`／`PANEL_SOURCE_KEYS` 声明哪些键由源码派生、允许覆盖，其余（包括**以后新加的**任何人工字段）一律保留；再把人工字段的清单打印出来（现在会显示 `events[disposition, gap_id, note, target] panels[maps_commands, target_scene, task_ids]`），使"保留了什么"变成可核对的输出而不是信念。修完实测：对真实工件跑 `--freeze` **sha256 逐字节不变**（91c99c56…），`--freeze-reachability` 本来就是 no-op（0.6 s，同样逐字节一致）。
> 5. 我写的第一版回归测试**是假绿**：它给 `--freeze` 喂的假 `live` 省略了真实抽取器会带的 `disposition: null` 占位，于是"note 保住了"的断言通过、而真工件照掉。补成与 `extractEvents` 实际输出同形之后，变异测试（把合并逻辑改回白名单版）显示 **3 条失败、2 条通过**——两条新测试确实能抓住这个 bug（"idempotent" 那条在变异体下也通过，它的价值是在**真工件**上量到的：旧代码连幂等都不满足）。**教训**：stub 比真实生产者简单，等于没测。
> 6. 这条性质现在是门禁的一部分：新增 case **`inventory-freeze-is-noop`**（`inventory.mjs --freeze-check`，只比较不写盘；漂移时 exit 1 并提示先看 diff）。`inventory-unit` 由 18 增至 **22**，按既有规则**同一次编辑**里把 `runner.floors["inventory-unit@skeleton"]` 一起改成 22。门禁现为 **13 case／47 用例**，`--check` consistent，`git diff --check` 干净。
> 7. 工件零永久损失：两次破坏都发生在**我先 `cp` 出备份之后**（`/tmp/command-map.PRE-freeze.json`，sha256 已核对），每次都按备份 `cp` 复原并复验哈希——这正是第五次补记里那条"未跟踪文件没有 git 兜底"纪律的第一次实战生效。
>
> **【第八次补记 @ 2026-09-21】门禁自己有个空洞：注册了 0 条 case 的模块会「绿」。**
>
> 延续第六／七轮的做法——不写新功能，专门去测"文档里声称过但没量过"的性质。这次量的是 `native:check` 本身，而 `checks.json` 的 `runner.rules` 早就白纸黑字写着 **"unknown module, missing suite, skipped required case and zero executed cases all fail"**。实测：
>
> 1. **`missing suite` 那一条并没有实现。** 在临时副本里给 config 加一个 `"protocol": {cases: []}`（就是 P2-01 下一步会注册的那个模块），`node scripts/native/check.mjs protocol` 输出 **`native:check protocol: 0 case(s) passed, 0 test cases executed`，exit 0**。也就是说：**一个什么都不查的门禁可以长期全绿**，而且它的形状恰好"看起来像通过了"——有名字、有输出、退出码 0。
> 2. `all` 有同一个洞且更隐蔽：它判断"是否就绪"的条件是 `planned.filter(m => !config.modules[m])`，**只看模块键存不存在**；一旦 `protocol` 被注册成空 cases，它就从"未就绪"名单里消失。更糟的是它当时的报错文字写的是 "registered modules have no cases yet"——**它在宣称检查的正是它没检查的那件事**。
> 3. 修法是把"什么才算一个就绪模块"抽成一条规则、一处实现：新增 `scripts/native/gate-config.mjs` 的 `moduleGateProblem(name, entry)`，要求 ① 已注册 ② `cases` 是**非空数组** ③ 每条 case 有 `name` 且 `argv` 是以字符串开头的非空数组。runner 与 `all` 都改成用它；`all` 的消息同时如实报出 `46/46 planned modules are not gate-ready`。顺带删掉循环里那段"argv 形状"检查——它现在被上游校验保证，留着只会让人误以为那里还有守卫。
> 4. 规则为什么放在单独文件而不是 `check.mjs` 里：`check.mjs` 是**顶层带副作用的脚本**（解析 argv → 读 config → 可能 `process.exit`），import 它就是在执行它，测试没法直接拿函数用。第一次尝试确实踩到这个（测试进程被 exit 2 掉），所以"抽成可导入的小模块"是必要的而不是洁癖。
> 5. 新增 **9 条测试**（`check-runner.test.mjs`）：5 条打规则本身、2 条打**已提交的 `checks.json`**（每个注册模块必须 gate-ready；`all` 今天仍必须拒绝）、2 条是**真子进程端到端**——把 runner 拷进临时树、注入 `"cases": []`、断言 exit 非 0 且报 "zero cases"（exit 码是 runner 自己的，不是我对它的复算）。变异验证：把 runner 里那次校验调用删掉，端到端那条**立刻失败**，说明它不是摆设。顺带确认既有行为没动：未知模块 exit 2、无参数 exit 2、`all` exit 3。
> 6. 注册为门禁 case **`gate-runner-unit`**（地板同一次编辑里写 9）。`pnpm native:check skeleton` 当时为 **14 case／56 用例**（第九、十轮之后依次是 65、78，各轮末尾都记当时的数，不回填）。`--check`、`--freeze-check`、`digest --check`、`doctor 18/18`、`git diff --check` 全部照常，`command-map.json` 哈希仍是 91c99c56…（本轮没有再碰冻结工件）。
>
> **为什么这类检查值得连着做**：第六轮（按目录枚举的白名单吞掉 28 KB 工件）、第七轮（`--freeze` 删人工字段而 `--check` 说 consistent）、第八轮（空模块绿）是**同一个失效模式的三个面**——守卫的判据与被守护的东西其实不对齐，而现有信号一律"看起来通过"。三次都不是靠跑现有门禁发现的，只有"量一条声称过但没量过的性质"才能发现。P2-01 起会不断注册新模块、新 case、新工件，这个模式只会更常触发，所以在进入 Phase 2 之前把它变成门禁（`gate-runner-unit` + `inventory-freeze-is-noop` + `digest-shards-current`）比多做一点功能更有价值。
>
> **【第九次补记 @ 2026-09-21】INV-02 的源码守卫曾被一个 ignore 文件完全绕过——这条是真的越界守卫失效。**
>
> 接着量下一条声称过没量过的性质：`inv02-core-source-scan` 报"无禁用匹配"，究竟是"扫过了且干净"，还是"根本没扫到"？实测（临时放入 `core/crates/aeroric-core/src/zz_probe.rs`，内含 `tauri::AppHandle`，同目录再放一行 `.gitignore` 把它忽略，跑完立刻删）：
>
> 1. **`rg` 默认尊重 `.gitignore`**，于是守卫 **exit 1、输出为空** → 门禁打印 `ok (no forbidden matches)`。**违规文件就在被扫目录里，INV-02 却报通过。** 任何人往 `core/**/src` 里塞一个 `.gitignore`（或命中 `core/target/` 之类的规则外溢）就能让这条架构不变量的守卫变成空守卫。
> 2. 同一条还覆盖另一个更常见的场景：**被扫目录变成空目录**时 `rg` 同样 exit 1 → 同样"通过"。（对照实验：把 runner 拷进一棵没有 `core/` 的临时树 → 现在报 `control probe … found no content (exit 2)`，不再假绿。）
> 3. 修法两层。① 两个 `absent` case 的 `rg` 一律加 `--no-ignore`，让忽略规则无法藏东西；② `absent` 现在**必须**声明 `controls`——**每个被扫路径一条**"必须能找到内容"的探针（`rg -q --no-ignore pub <path>`；manifest 用 `^\[`），探针不命中就 FAIL，判据从"没匹配"变成"确实读过且没匹配"。`forbid` 同理必须有 `mustContain`（`core-no-tauri-deps` 现在断言输出里出现四个 crate 名，空/截断的 `cargo tree` 不再等于纯净）。
> 4. 这条规则**在注册期就生效**：`moduleGateProblem` 直接拒绝"没有 controls 的 absent case"和"没有 mustContain 的 forbid case"。证据是它当场拒绝了我自己刚写的配置（`2 "absent" case(s) declare no usable controls`），逼着三个守卫全部补上正控。以后谁加新守卫，忘了正控就进不了门禁。
> 5. 修复是**实测**的，不是推理的：同一份攻击样本下，门禁从 `ok (no forbidden matches)` 变成 `FAIL (forbidden matches present)`，`native:check skeleton` 退出码从 0 变成 **1**；样本删除后回到 exit 0／14 case。探针文件与 `.gitignore` 均已删除，`core/crates/aeroric-core/src/` 只剩 `lib.rs`。
> 6. `gate-runner-unit` 由 9 增至 **18** 条（正控 helper 单测 + 5 条注册期断言 + 3 条**真子进程端到端**：语料没被读→门禁红、被读了→绿、`forbid` 输出为空→红）。变异验证：把 runner 里两处正控接线删掉，恰好这两条端到端用例失败。地板同一次编辑里 9→18。
> 7. 门禁现为 **14 case／65 用例**。顺带自查一处我自己的测量 sloppy：第一次跑"违规样本"时用 `| grep` 之后读 `$?`，量到的是 grep 的退出码而不是门禁的——重测后才拿到真实的 exit 1。这正是本补记批评的那类"信号看起来对"的错。
>
> **【第十次补记 @ 2026-09-21】INV-09 一直没有守卫；`check-patches` 也在空表上恒绿。**
>
> 1. **INV-09（macOS ad-hoc 签名基线不得被静默改掉）此前没有任何门禁 case**——`checks.json` 里 INV-02 有守卫、日志脱敏有自测，而 §5「不得改 `src-tauri/tauri.conf.json` 的 ad-hoc 基线」和 P10-03「同时检查原 baseline 未变」全靠人记得。P2–P9 会有大量 Task 碰配置，这正是最容易被顺手改掉的一条。新增 **`inv09-signing-baseline`**（`scripts/native/check-signing-baseline.mjs`）：断言 `bundle.macOS.signingIdentity === "-"` 且 `hardenedRuntime === false`，**缺键同样算失败**——Tauri 的默认值是 `hardenedRuntime: true`，所以"删掉一行"就是一次静默的行为变更，而值对比什么都看不出来。同时跨文件核对签名 CI 路径仍然现场生成 `tauri.macos-signed.json` 并用 `--config` 传进去（workflow 里 `jq -n '{bundle:{macOS:{hardenedRuntime:true}}}' > …`，`tauri.macos-signed.json` 本就不该被提交——我一开始以为 AGENTS.md 的这句话过期了，**实测后才收回**：它是构建期产物，文档没写错）。
> 2. **`check-patches` 是第九轮那类空洞的又一例**：把两侧 `[patch.*]` 表都删掉后它打印 `0 patch entries identical…` 并 **exit 0**（实测）。它的本意恰恰是"core 与出货客户端解析到不同源码时 cargo 不会告诉你"。现在要求两侧各解析出 ≥`--min-entries`（配置里写 9）条，**空表直接失败**，并且识别 `[patch."https://…"]` 引号注册表（旧正则只认 `[patch.crates-io]`，换注册表名就等于悄悄不比对）。真实仓仍是 9 条一致、exit 0。
> 3. 新增 **`invariants-unit`** 13 条（地板同一次编辑写 13）：INV-09 的翻转/删键/换身份/形状漂移/坏 JSON/workflow 断链共 7 条，patch  parity 的恒等/双侧空/只删一侧/rev 漂移/源类型漂移/路径目标不存在/引号注册表 6 条。变异验证（`if (false)` 掉那两条判据）**恰好** 2 条失败、11 条通过。
> 4. **两次方法论踩坑，如实记**：① 第一次变异体把 `if (...) push(...)` 整行换成注释，留下悬空 `else` → 文件语法错，`node --test` 报"0 pass 1 fail"，我差点把"文件级失败"当成"变异生效"；变异必须是**语义**替换（`if (false)`），并且要先 `node --check`。② 把测试文件拷进临时树会让**读真实仓库文件的用例失真**（它的 `repoRoot` 来自自身位置），于是一条本来通过的正控被记成失败——补上 `src-tauri/tauri.conf.json` 与 workflow 两个输入后才拿到干净结果。**变异测试要在能复现被测环境的最小输入集上做**，否则结论双向都不可信。
> 5. 门禁现为 **16 case／78 用例**，`--check`／`--freeze-check`／`digest --check`／`doctor 18/18`／`git diff --check` 全部照常。
> **【第十一次补记 @ 2026-09-21】§5.1 的第三块展开交付了；顺手把它自己依赖的生成器变成可复现的，结果发现本文档的两列数字一直是错的。**
>
> 1. **先解掉阻塞事实。** §5.1 有意没展开 `remote/` 的理由是"mobile 是否经 relay 而非 Tauri invoke 访问命令"未决。这是可测的：`grep -rl "@tauri-apps" mobile --exclude-dir=node_modules` → **0 处引用**，mobile 的传输层是 `mobile/src/transport/{e2ee,orca-e2ee-v2,pairing-offer,remote-connection}.ts`。问题关闭，`remote/` 不再需要任何用户决策（`dsh_webui/` 仍等 Q7/G17）。
> 2. **交付 `docs/native/extraction/remote.md`**：22 行、9 977 行代码、**9 个 `#[tauri::command]` wrapper 全部在 `remote/mod.rs`**（其余 21 个文件是 RPC/传输/handler 的 service 面），聚合风险：10 个文件带 `AppHandle`/`State`、6 个 emit、10 个 spawn、9 个碰凭据、1 个 `include_str!`、`cfg(windows)`/`cfg(unix)` 各 1。手写备注里钉住真正的排序约束：`tasks_rpc.rs:483` 至今 `app.emit("remote-task-request")` 并等 React 应答（`src/App.tsx:498`、`src/test/app-event-wiring.test.tsx:124`、`src/test/app-remote-task-request.test.tsx`），所以"domain 执行不依赖 React listener"这条**要到 P8-03 才能宣称**；未验证的两项（`remote_*` 在 mobile 之外的调用者、配对设备的 per-method capability 名单）明写留给 P8-03，不假装知道。
> 3. **把"生成器会中止"这句声称变成真的。** `database-notebook.md` 当初声称"生成器与 command-map.json 交叉校验，不一致就中止不写"——但那段代码是一次性的、已不存在，**这句话根本无法复核**。新增 `scripts/native/extraction.mjs`（`--write` 生成、`--check` 比对、wrapper 总数与 command-map.json 不一致就拒绝写出），命令定义直接复用 `inventory.parseCommandDefinitions`，避免同一件事出现两套计数实现。
> 4. **重导立刻抓出本文档自己的历史错误**：`testOnlyItemsExcluded` 记 914，实测这些文件里的 `#[test]`/`#[tokio::test…]` 属性只有 **782** 个（`connection_secrets.rs` 18→10、`notebook/sync/engine.rs` 64→40，逐文件与 `grep -c` 对得上）；`pubSymbolsTotal` 记 586，而列 0 的 `pub` 项（含旧版漏掉的 `pub(crate)` 与各 `mod.rs` 面）实为 **650**。已在文档里就地更正并说明原因，不静默改写。同时修掉我新代码的两个错：把 `impl` 里的 `pub fn` 当顶层导出（虚增约 50%）、`#[cfg(test)]` 与 `mod tests {` 分行时识别不出测试块（`legacy_sqlite.rs` 一度报 24 个 moved symbols 而 `#[test]` 数恰好 14 对上，说明块识别失败）。
> 5. 现在三处都在门禁里：`extraction-docs-current`（database+notebook 表必须等于重导结果）、`extraction-docs-current-remote`、`extraction-unit` 9 条（地板 9 同一次编辑）。**首次跑就拦住我的测试夹具自己写错**（`slice(0,3)` 其实没删掉那一行，"缺行必须失败"的断言遂失败）与**空模块只抛 ENOENT 而不是可读错误**，两处都已改。
> 6. 门禁 **19 case／87 用例**，其余检查照旧全绿；`command-map.json` 哈希未变。
> 7. **顺手补上 P1-03 自己的漏项**：这两份 §5.1 展开此前**没有被任何子任务的 context 清单引用**，未来做 P7-04/P8-03 的会话根本不会看到它们。现已接上，但对 28 533 B 的 `database-notebook.md`（= 32 768 上限的 87 %）**故意不注入**——注入就会重演 D8 的静默截断，改为在该子任务 `implement.md` 里写明"按需 Read 对应行"；只有 9 232 B 的 `remote.md` 进 P8-03 的清单。复查：全部被注入文件中无一超上限，最大 16 769 B（51 %）。
> **【第十二次补记 @ 2026-09-21】第 4 块 §5.1 展开交付了；延后它的理由被自己的测量推翻，而且推翻的东西正是 Q7 的输入。**
>
> 1. **我此前写进本文档的"66 个文件"是错的。** 实测：`dsh_webui` 是 **9 个文件**（同名 `dsh_webui.rs` 4 280 行 + `dsh_webui/` 8 个文件，合计 9 417 行），**66 是命令 wrapper 数**（兄弟文件 14 + `commands.rs` 52），恰好等于 `command-map.json` 里 `dsh_webui::*` 的 66 条——我把命令数当成了文件数，并用它当作"现在展开必然返工"的理由。文件级展开与 Q7 无关，所以那份展开已交付：`docs/native/extraction/dsh-webui.md`（9 行、66 wrapper，`extraction-docs-current-dsh-webui` 持续校验）。
> 2. **为它给生成器补了"模块 = 目录 + 兄弟 .rs"的走法**：只走目录会漏掉 `dsh_webui.rs` 里的 14 个 wrapper，得 52 对 66 —— 交叉校验当场就会拒绝写出。这个自我防护在本模块第一次起作用（此前 `dsh_webui` 根本没被展开过，所以这条路径没被踩过）。
> 3. **顺带纠正了另外三条同样从未回源的计数**，因为它们都在给用户的决策输入里：
>    - 桶 C 零证据命令是 **41** 条，不是 42（实算：66 条中 25 条有任意证据、24 条 `reachable=true`）。
>    - `protocol_inventory.rs` 不是"235 个上游 RPC 方法名"。它是**钉死的 10 张表、246 项**：`RPC_METHODS` 62、`REMOTE_METHODS` 87、`REMOTE_EVENTS` 19、`REMOTE_EVENT_MODES` 19、流帧 3/4/4，另有 **48 项显式 RETIRED**（10/10/28）不该当能力面 ——**live 198**。"235"这个数字在文件里对不上任何东西。
>    - 上述四处旧文字（本文 3 处 + `reachability-triage.md` 1 处）已就地更正并注明撤回。
> 4. **Q7 因此需要重新定标。** `remote_mux.rs`（990 行）**没有任何 `#[tauri::command]`**，也不按方法名表分发：它的公开面是客户端 `connect/is_alive/open/next/remote_mux_url/parse_remote_event_item/remote_event_result_args`，即"通过 mux URL 与 dsh 服务端通信"。所以选项不是"42 个方法 vs 1 个网关"，而是：66 个 typed wrapper 中 41 个零证据者要不要在 core 具名暴露，**以及谁承担与上游的版本偏移** —— 清单钉住 `SOURCE_COMMIT c291e796…`、包版本 `0.1.5-rc.2`、`PROTOCOL_VERSION 2`。这条重述连同证据已写进 `dsh-webui.md`，并在 §6 的待决策清单里替换了旧措辞。**给你的问题因此更清楚了，而不是更多了。**
> 5. 仍未验证、明确留给 P3-05 而不是假装知道：62 条 `RPC_METHODS` 是否全部对得上当前上游；41 条零证据命令是否被 mobile/relay 触达；`terminal_render.rs`（1 232 行、22 个顶层项）是纯 projection 格式化还是携带必须随宿主迁移的状态。
> 6. 门禁 **20 case／87 用例**；P3-05 所在 Phase 3 的 context 清单已加入 `dsh-webui.md`（8 359 B，安全低于上限）。
>
> **【第十三次补记 @ 2026-09-21】660 条命令的 `category` 此前是不可核对的手工字段；ADR 里还留着两条早已过期的"阻塞"。**
>
> 1. **发现。** 审计对象是 P2–P9 的路由依据本身。`command-map.json` 的 `meta.method` 写着分类来自"**52 模块级人工分类表** → 命令名级 override"——但那张表**没有任何地方存过**：既不在工件里也不在脚本里，是一次会话里手工应用的。于是 579／62／19 这三个决定"谁能进 host mutation allow-list、谁在 macOS 必须显示不支持原因"（本 ADR 的硬约束）的数字，**没有任何可追溯来源**。
> 2. **实算是可完全重建的，且元数据里的数字自己就是错的。** 按 `rust_path` 首段聚合：**56 个模块**（不是 52），只有 4 个模块内部混用类别，对应**恰好 4 条命令级例外**——`fs::open_in_system_file_manager`、`dsh_plugins::open_dsh_config_file`、`dsh_webui::open_dsh_agent_preset_document`、`app_settings::get_system_fonts`，四条都是真的 OS/客户端动作（交给系统打开文件、枚举系统字体），分类本身站得住；问题只在"没人能验证它是站得住的"。
> 3. **落盘 + 变成门禁。** `command-map.json` 新增 `meta.category_resolution`：规则、56 条模块默认、4 条例外（每条带 `why` 与定义位置 `file:line`）、混合模块明细；`meta.method` 的 52 就地更正。`inventory --check` 现在推导 `category = override ?? moduleDefault[模块]` 并拒绝：与推导不符、新模块没有默认值（**新模块不得静默继承 `domain_rpc`**）、例外没有理由、例外/默认值已失效（命令改了或模块没了）。5 条新负例（`inventory-unit` 22→27，地板同一次编辑）。**这批测试断言的是各自分支输出的原文消息**而不是仅仅 `ok === false`，所以"通过"就证明分支真的执行过——避免又造一批只会一起变绿的空壳断言。
> 4. **顺手在别的工件里清掉两条过期"阻塞"。** ADR §7 长期写着"`grill-me` 本会话不可调用 → 阻塞 P1-05 Gate 进而阻塞 Phase 1 出口"，但第一轮追问其实早已真实完成（第五次补记前后）；也写着"`AGENTS.md` 原生规则段落已批准、延后执行"，而这两段其实已经写入。两处都改成实测状态，并各留一句"此前为何没回写"，因为**同一份文档里其它没被复核的"阻塞"字样可能同样过期**——`meta.category_resolution` 就是这条规律的第三个实例。
> 5. **一个我自己造出来的反例被自己拦住了：** 给 canonical `command-map.summary.json` 加 `category_resolution` 后，`digest.mjs --check` 立刻拒绝：**key "category_resolution" is in no shard — sharding would silently drop it**。这正是第七／八轮那条"分片不得成为丢行的借口"的设计目的第一次在真实改动上开火（此前只在单测里开火）。按规则把它并入 `command-map-head` 分片后重切，7 片仍全部可无损还原 canonical，最大 16 769 B（51 % 上限）。
> 6. 门禁 **20 case／92 用例**（`inventory-unit` 27 条），`--check`／`--freeze-check`／`digest --check`／`doctor 18/18`／`git diff --check` 全绿。本轮 `command-map.json` 的哈希**有意变了**（新增 `meta.category_resolution`、更正 `method`），备份 `/tmp/command-map.PRE-cat.json` 可逐字节回退，`--freeze`／`--freeze-reachability` 对新内容仍是 no-op（`extraction-docs-current*` 与 `inventory-freeze-is-noop` 均通过）。

- **Input**：§4、工具探测结果、`scripts/native/checks.json`、现有 `.github/workflows/checks.yml`。
- **Action**：
  1. 新建 `docs/native/tooling.md`、`docs/native/reviews/skeleton.md`、`docs/native/baseline.md`，按 §4 确认并启用真实 `grill-me`；不把技能当 shell 二进制。
  2. 在 baseline 记录现有 `pnpm lint`、`pnpm typecheck`、`pnpm test`、`pnpm build` 与 `cargo check --manifest-path src-tauri/Cargo.toml --locked` 的结果。原有失败单列，不能降低断言以让迁移通过。
  3. 新建 `docs/native/rollback.md`，记录基线 commit、数据备份/恢复演练路径、每阶段开关，不执行真实用户目录复制或回滚。
- **Acceptance Criteria**：
  - `pnpm native:check skeleton` 成功；基线命令失败时标为阻塞并定位，不声称全绿。
  - **显式调用 `grill-me [skeleton]`**，审查单写入实际证据；缺工具则 Phase 1 Gate 不通过。
  - P1 artifacts 完整且获实施范围确认；此时仍未迁移用户数据。

## Phase 2 — 契约、存储与无 UI 的任务状态机

**入口**：Phase 1 Gate 通过。

**出口**：无 Tauri 的核心能在临时目录操作项目/任务；Rust、TS、Swift 对同一 fixtures 一致；旧 GUI 行为仍保持。

### Task P2-01 — 固化本地 DTO、capability、错误和事件

- **Input**：`src/types.ts`、`src-tauri/src/storage.rs`、`app_settings/schema.rs`、`event_names.rs`；`packages/remote-contracts/src/{index,shared}.ts`；§2.3。
- **Action**：
  1. 新建 `core/crates/aeroric-protocol/src/{envelope,project,task,session,capability,error,event,terminal}.rs`，定义 `ProjectTarget`、`TaskSnapshot`、`SessionOwner`、`AgentCapabilities`、`RpcError`、`EventEnvelope`；从 `lib.rs` 导出。
  2. Agent ID 与 family 分开；TaskStatus 保留 `todo/pending/running/input_required/detached/interrupted/done/failed/cancelled`。未知上游消息作为 `OpaqueContent`；未知 mutating method 拒绝。
  3. 新建 `contracts/native/v1/{envelope,methods,events}.schema.json`、`contracts/native/v1/fixtures/{success,error,unknown,terminal,session,legacy-task}.json`；Rust 的 DTO/schema 导出是源，fixtures 是跨语言 oracle。新增方法必须同步 schema，不手写三份互不检查的契约。
  4. 新建 `macos/AeroricKit/Sources/AeroricProtocol/{Envelope,Models,Capabilities,RPCError,JSONValue}.swift`；为未知字段/unknown event 提供容错，不用 `Any` 穿透 View。
  5. 新建 `src/lib/nativeHostProtocol.ts` 作为未来 Tauri host 接入的 TS decoder，不取代 remote-contracts 的现有 v2/v3。
  6. 新建 `core/crates/aeroric-protocol/src/bin/export-schema.rs`，接通 P1 的 `scripts/native/contracts.mjs`：在临时目录生成 schema、与提交版本比较并调用三方 decoder 测试；`--check` 不覆盖仓库文件，漂移必须非零退出。
- **Acceptance Criteria**：
  - 新建 Rust `tests/protocol_roundtrip.rs`、Swift `ProtocolGoldenTests.swift`、`src/test/native-host-protocol.test.ts`；覆盖缺字段、未知 enum、超长帧、64-bit cursor、非法 base64。
  - `pnpm native:contracts --check`、`pnpm native:check protocol` 全部通过，且三方读取**同一** fixtures。
  - **调用 `grill-me [protocol]`**，重点审查本地/远程版本边界、secret 投影、枚举兼容。

### Task P2-02 — 路径、依赖注入与单写者存储

- **Input**：`src-tauri/src/storage.rs`、`app_settings/load_save.rs`、`path_guard.rs`、`git_path_guard.rs`、`secrets.rs`；现有数据文件格式。
- **Action**：
  1. 新建 `core/crates/aeroric-core/src/{paths,context,clock,storage_lock}.rs`、`storage/{mod,projects,tasks,settings,atomic,compat}.rs`，实现 `AppPaths`、`CoreContext`、可替换 `Clock`、跨进程 `DataDirectoryLease`。
  2. 默认路径仍与现有 Aeroric 一致；`--data-dir` 只改变 Aeroric 管理的数据，不默默重定向外部用户 home。测试使用 fake settings/secret/hook provider，所有 I/O 都显式注入。
  3. 原子写保留私有权限、临时文件、rename、失败清理；对需要持久成功保证的操作 flush/fsync 并注明平台限制。锁不只用内存 Mutex；不凭 PID 文件存在就判断锁活着。
  4. v1 继续兼容既有 JSON 文件布局，不趁机整体改 SQLite。增加 runtime metadata 独立文件记录 schema/epoch/revision；保存未知扩展字段，保留备份。
  5. 新建 `tests/storage_compat.rs`、`tests/storage_lock.rs`；在 `src-tauri/Cargo.toml` 添加到独立 core workspace 的 path dependency，修改原 `src-tauri/src/storage.rs` 已抽纯读写函数为薄适配，OS 特有行为留 adapter。完整实例注入/写入权移交在 P2-05 原子切换，不能提前禁用旧保存路径导致 GUI 丢写，也不能并行启用两套 writer。
- **Acceptance Criteria**：
  - `pnpm native:check storage`：旧 fixture 读写不丢 session family、worktree、归档等字段；损坏 JSON 返回可恢复错误，不写空文件覆盖。
  - 两个**独立进程**竞争相同目录只允许一个写入；不同目录可并存；symlink、权限错误、原子写中断、磁盘写失败有反例。
  - 同一 fixture 用旧 DTO 和新 reader 往返通过；真实用户目录未变更。
  - **调用 `grill-me [storage]`**，锁/原子写/恢复未解决则不能进入状态机迁移。

### Task P2-03 — 将项目管理变成核心命令

- **Input**：`src/state/app/projectMutations.ts`、`src/projectTarget.ts`、`projectGroups.ts`、`projectOrder.ts`、`projectPersistence.ts`、`storage::Project`。
- **Action**：
  1. 新建 `core/crates/aeroric-core/src/projects/{mod,service,mutations}.rs`，实现 `ProjectService` 的 list/open/rename/pin/hide/reorder/group/remove。
  2. 以 `ProjectTarget` 保留 local/ssh/wsl 身份；路径比较与 projectId 稳定性保留，不把 `ssh://` 当本地文件路径。
  3. removal 只移除管理记录，删除磁盘目录是另一个明确危险操作；有运行任务时拒绝或要求显式取消。
  4. 新建 `contracts/native/v1/fixtures/projects.json` 和 `tests/projects.rs`，为 React 原纯函数与 Rust 新函数提供同输入同结果。
- **Acceptance Criteria**：
  - `pnpm native:check projects`：项目重复打开不重复创建；中文/空格路径、大小写、丢失远程连接、group rename/order 均有确定结果。
  - 业务操作不调用任何 UI 回调；未跑窗口也能列出保存的项目。
  - **调用 `grill-me [projects]`**；通过前不把 React 写入责任移交。

### Task P2-04 — 迁移 React 的任务生命周期与状态持久化

- **Input**：`src/state/app/{taskLifecycle,taskLaunch,taskStatus,taskMutations,taskSwitch,taskDeleteDone,worktreeOps}.ts`；`src/{taskSession,sessionHandoff,taskCleanup,taskCompletion,taskFlush}.ts`；`src-tauri/src/remote/tasks_rpc.rs`。
- **Action**：
  1. 新建 `core/crates/aeroric-core/src/tasks/{mod,service,reducer,launch_plan,session_owner,handoff,cleanup,operations,executor}.rs`。`TaskService` 成为 create/update/start/resume/cancel/complete/archive/delete 的唯一编排者；在 `executor.rs` 先定义最小 `TaskExecutor`/`TaskRuntimeObserver` 端口，使用 typed launch/resume/cancel 请求和带 generation 的结果/事件。这里不依赖尚未创建的 P3 runtime；用 fake executor 验状态机，P2-05 注入 legacy executor，P3 再替换其实现。
  2. 将“先持久化再启动”“配置切换失败回滚”“首次输入门控”“worktreeDiscarded 禁止 resume”“completedAt 的终态规则”“DSH 的继续策略”写成无 UI 状态机。
  3. `task.start` 先持久化 pending 和 operation key，再创建子进程、再发布 generation；崩溃夹在步骤中时下次启动对账为 interrupted/可确认状态，不能无条件重新 spawn。
  4. UI 通知、选中哪个任务、Toast 留客户端；业务错误成为 `RpcError`。新的核心服务接口不得依赖 React `remote-task-request` listener；旧远程 wire 的调用切换在 P8-03 完成，不能在此先删掉仍被使用的兼容入口。
  5. 新建 `contracts/native/v1/fixtures/task-transitions.json`、`tests/task_lifecycle.rs`、`tests/task_idempotency.rs`；fake executor 记录副作用调用、可注入失败与迟到事件，复用 TS 既有测试的真实场景而非只比较状态名。
- **Acceptance Criteria**：
  - `pnpm native:check tasks`：完整状态转换表、同 ID 重试只 spawn 一次、旧 generation exit 不覆盖新 generation、取消/手动完成/迟到 hook 竞态。
  - fake runtime 没有 GUI listener 时 start/resume/cancel 全部可执行；持久化失败时不得启动进程。
  - **调用 `grill-me [tasks]`**；特别追问 React 尚有哪一个业务决策未移走，清单必须为零或明确阻塞项。

### Task P2-05 — 建立 Tauri embedded adapter，保持旧客户端可回归

- **Input**：P2-01～04；`src-tauri/src/{lib,storage,event_names}.rs`；`src/lib/api/invoke.ts`、`src/state/app/useAppTauriEvents.ts`。
- **Action**：
  1. 新建 `src-tauri/src/core_adapter.rs`：`CoreGateway`、`TauriEventSink`、legacy DTO 转换；在 `lib.rs` managed state 中注入 core service。新建 `src-tauri/src/legacy_task_executor.rs` 实现 P2-04 端口，按 local/SSH/WSL target 和 Agent family 分发到既有 PTY/DSH/SSH/WSL 的低层启动、恢复、停止和清理 body；必要时只在原 `pty.rs` / `dsh_webui/` / `ssh.rs` / `wsl.rs` 提取内部函数，暂不迁整个运行时。
  2. 保留既有 command 名与可兼容参数；将项目/任务 mutations 分阶段改为薄 wrapper。每个 legacy save 接口在移交后不得用旧全量 snapshot 覆盖新 revision。
  3. 修改 `src/state/app/{projectsStore,tasksStore,taskLifecycle,taskStatus}.ts`：React 接受 authoritative snapshot，仅保留 UI 派生态；乐观更新有 revision 对账及失败回滚，不再重复写业务状态。
  4. 固定调用方向为 `legacy command → TaskService → LegacyTaskExecutor → low-level launch body`，不得再从 executor 调 command wrapper/TaskService 形成递归；launch/exit/hook 通过 observer 回同一 reducer，不复活 React reducer。尚未迁出的 worktree/清理副作用由临时端口调用原实现，不许无声 no-op。
  5. 新建 `src/test/native-core-adapter.test.ts`、`src-tauri/src/core_adapter_tests.rs`，保留现有 command-registration 约束；记录 temporary adapter 仅存在于 Tauri crate，不让 core 依赖它。
- **Acceptance Criteria**：
  - `pnpm typecheck`、`pnpm test`、`cargo test --manifest-path src-tauri/Cargo.toml --locked --lib`；`pnpm native:check tauri-adapter`。
  - 旧 GUI 经真实 legacy executor 创建/恢复/完成一项 fake 任务，只发生一次存储写入序列和一次 spawn；没有双事件/事件回环/递归命令调用；四族暂未抽取的启动路径仍可达。
  - **调用 `grill-me [tauri-adapter]`**；Phase 2 Gate 通过后才开始抽 PTY。

## Phase 3 — CLI 核心解耦、原始 PTY 与宿主

**入口**：核心 DTO/存储/状态机完成，P2 的 spawn 测试可用 fake executor。

**出口**：不启动 GUI 的 CLI 真正运行 fake agent 并完成生命周期；四族命令生成与旧行为等价；本地 IPC 可认证和复连。

### Task P3-01 — 提取运行时端口，替代 AppHandle 与全局查找

- **Input**：`src-tauri/src/lib.rs::TaskManager`、`pty.rs`、`event_watcher.rs`、`subprocess.rs`、`platform/`、`clock.rs`；P2 `CoreContext`。
- **Action**：
  1. 新建 `core/crates/aeroric-core/src/runtime/{mod,manager,ports,events,shutdown}.rs`，定义 `Runtime`、`TaskRegistry`、`EventSink`、`ProcessSpawner`、`PlatformServices`，复用 P2 的 executor/observer 端口，不另建一套不兼容生命周期。
  2. 将 `TaskManager` 中 PTY maps/session maps/generation 容器归属迁到 `TaskRegistry`，legacy executor 暂时引用同一 registry；每个 task 的清理 idempotent。不得让两份全局 map 同时成为真相源；P3-03/05 只替换本地 PTY/DSH 分支，SSH/WSL 分支仍通过 Tauri 临时 adapter 接同一 registry/generation/observer，直到 P7-01/02 分别验收后才能删除整个临时 adapter。
  3. 所有后台 watcher 接收可取消 token 与 `Arc<CoreContext>`；结束时 join/回收，不能因 Swift view deinit 杀任务。每个 await 前检查锁作用域。
  4. 新建 `src-tauri/src/platform_adapter.rs` 提供文件管理器、系统 dialog 等 UI 端口；Tauri emitter 只把 core 事件转换给旧前端。
  5. `lib.rs` setup 改成按阶段启动核心服务，失败服务写 diagnostics 与 capability=false；关键存储/锁失败不启动写服务。
- **Acceptance Criteria**：
  - 新建 `tests/runtime_lifecycle.rs`；`pnpm native:check runtime`：启动两次不重复 watcher、shutdown 两次安全、退出监控不能清新 generation。
  - `rg -n 'tauri::|AppHandle|tauri::State' core/crates/aeroric-core/src` 无命中；`cargo check --manifest-path core/Cargo.toml --locked` 与 `cargo check --manifest-path src-tauri/Cargo.toml --locked` 均通过。
  - **调用 `grill-me [runtime]`**，追问锁序、任务/窗口寿命、初始化降级，记录实际实现而非只检查 trait 名称。

### Task P3-02 — 原样迁移四族命令构造与托管配置

- **Input**：`pty.rs::{build_*_cmd,add_*_launch_args,setup_omp_env}`；`app_settings/{launch_spec,agent_env,agent_scripts,builtin,custom_agents}.rs`；`agent_family_maps.rs`、`dsh_home.rs`、`omp_home.rs`、`hooks.rs`、`mcp.rs`。
- **Action**：
  1. 新建 `core/crates/aeroric-core/src/agents/{mod,launch,environment,capabilities,claude,codex,dsh,omp,custom}.rs`；把 pure command planning 与实际 spawn 分开为 `LaunchPlan { program, args, cwd, env, unset_env }`。
  2. 托管 home/config/hook 安装逻辑移动到 `agents/managed_home/` 对应文件；版本 marker、凭据权限、MCP 每次重生成语义保留。使用 `include_str!` 的 hook 文件必须随代码移动并修正相对路径。
  3. 新建 `agents/managed_home/aeroric-hook.mjs`、`aeroric-omp-hook.js` 的单一资源来源；删除旧来源前更新全部引用及测试，不维护两个会漂移的拷贝。
  4. 严禁以 shell 拼接取代 argv；真实用户 shell PATH 探测可缓存，但环境过滤在 spawn 前最后执行。远程 shell 必须沿用现有审查过的 quoting 方法。
  5. 新建 `tests/agent_launch.rs`、`tests/omp_invariants.rs`；将 4 家族×3 权限档×prompt 空/非空×resume/new×custom/home/env 的 case 表保存为 fixtures。
- **Acceptance Criteria**：
  - `pnpm native:check agents` 比较迁移前后 argv/env/cwd，差异必须在 approved ADR 中逐项说明。
  - OMP 7 档 thinking 正确；兼容输入 ultra 最多归一到 max，不向上游发送 ultra；存在 `PI_PROFILE`/`OMP_PROFILE` 时子进程环境仍指向 managed home。
  - OMP 带 prompt 的交互 argv 不含 `--mode`，flags 在 `--` 前；hook 扩展名和 willContinue 反例测试通过。
  - **调用 `grill-me [agents]`**；未经用户确认不运行安装/升级命令或写真实 Agent 配置。

### Task P3-03 — 提取 PTY 字节流、租约、回放与退出

- **Input**：`src-tauri/src/pty.rs` 的 reader/batcher/run/resume/send_input/resize/finalize；`remote/{terminal_hub,terminal_streams,terminal_frames}.rs`；`src/terminalRingBuffer.ts`；opencode `pty/protocol.ts`。
- **Action**：
  1. 新建 `core/crates/aeroric-core/src/terminal/{mod,pty,stream,replay,input,lease,history}.rs`，定义 `TerminalService`；reader 首先发布原始 bytes，legacy Tauri sink 才增量解码成现有字符串。
  2. `TerminalIdentity = (taskId, generation)`；offset 用原始字节计数。startup pending resize 沿用，不允许隐藏 view 的 0×0 resize 覆盖前台尺寸。
  3. 单流一个输入/resize 控制租约，多个只读订阅者。native/legacy/mobile/CLI 切控制者需要明确 acquire/release，旧租约写入返回错误；不能两个窗口同时改 PTY 大小。
  4. 有界 replay 与 history compaction 共享一个 owner；向缓慢客户端发送 gap，不阻塞 PTY reader、不让任务因窗口隐藏卡死。首订阅 snapshot 与 live 的边界用锁/单线程序列化保证。
  5. 新建 `tests/{pty_bytes,terminal_replay,terminal_lease,pty_exit}.rs` 和 `tests/fixtures/fake-agent.mjs`；fake agent 可以输出任意 bytes、等待输入、报告 size、fork 子进程并以指定 code 退出。
  6. 新建 `core/crates/aeroric-core/src/terminal/executor.rs`，实现 `CorePtyExecutor` 并只替换 Tauri 临时 executor 的**本地 PTY**分支；本地 DSH 分支保留至 P3-05，SSH/WSL 执行分支保留至 P7。不保留两份同 target 的启动 body，不改变 TaskService 的唯一状态 owner。
- **Acceptance Criteria**：
  - `pnpm native:check terminal-core`：跨帧中文/emoji/ANSI、非法 UTF-8、NUL、100 MiB 连续输出，端到端原始 bytes hash 相等（对未截断区间）；有界内存不随总输出线性增长。
  - cancel 杀正确 process group 并 reap；SIGTERM/非零退出/EOF/正常退出/迟到 hook 均只产生一次结算，不能统一“有 session 就 done”覆盖现有家族差异。
  - 断线重订阅无重复/漏读；缓冲过期得到明确 gap/reset；20 次 resize 最终尺寸正确，隐藏终端无写权限。
  - **调用 `grill-me [terminal-core]`**；压力结果与 CPU/RSS 在证据中，不以“看着流畅”代替。

### Task P3-04 — 会话发现、解析、恢复与指标脱离窗口

- **Input**：`session.rs`、`session/{parse,approval,export}.rs`、`session_dsh.rs`、`session_omp.rs`、`event_watcher.rs`、`usage.rs`、`usage_index.rs`、`analytics.rs`；`src/taskSession.ts`。
- **Action**：
  1. 新建 `core/crates/aeroric-core/src/sessions/{mod,discovery,registry,parser,claude,codex,dsh,omp,history,resume,export}.rs`，迁移对应行为，保留既有路径校验与读取上限。
  2. 懒发现 watcher 属于 task generation，停止条件是对应任务结束/已绑定，不是 UI 的超时；OMP 不设会话创建 deadline。
  3. 将 hook watcher 改成无 `AppHandle` 的 `HookEventReader`，保留 partial JSONL、截断、轮转、重复 SessionStart、claimed path 排他性。
  4. 新建 `metrics/{mod,usage,index,analytics}.rs`，只对权威 usage record 累计；terminal 文字不作为计费依据，重放不能重复计数。
  5. `SessionPage` 返回稳定 message/tool ID、cursor、source family、unknown content；UI 不读会话文件；跨族 handoff 生成新会话且记录来源，不冒充 native resume。
- **Acceptance Criteria**：
  - 新建 `tests/{session_discovery,session_parsers,session_resume,hook_reader,usage_index}.rs`；`pnpm native:check sessions` 覆盖四族与 custom home、部分 JSONL、损坏记录、长文件分页、路径越界、DSH 压缩 transcript。
  - OMP 等待超过旧 DSH deadline 仍可绑定；同一文件不能给两个 task；退出后 late watcher 不改新会话。
  - **调用 `grill-me [sessions]`**；会话 owner 不是当前 UI 下拉框中的 agent，而是保存的真实 session owner。

### Task P3-05 — 将 DSH 结构化宿主变成核心服务

- **Input**：`src-tauri/src/dsh_webui.rs`、`dsh_webui/{startup,commands,dto,api_client,remote_mux,protocol_inventory,terminal_render}.rs`、`dsh_protocol.rs`、`dsh_plugins.rs`；`src/dshProtocol.ts`、`dshSessionFeatures.ts`。
- **Action**：
  1. 新建 `core/crates/aeroric-core/src/agents/dsh_host/{mod,startup,client,dispatcher,state,approvals,questions,attachments,projection}.rs`，逐文件提取现有 DSH host 所有权、API 调用与状态 reducer。
  2. 结构化事件保留原 message/tool/request ID 与 causal order；tool started/update/finished 对同一 ID 合并。DSH terminal projection 只是适配输出，不能借用到 OMP。
  3. 审批绑定 sessionId/task generation/requestId/client identity；过期、重复、其他 task 的应答拒绝；网络重放不能再次批准。
  4. 把 DSH command inventory 每条映射为 RPC/capability：history/search/fork/archive/preset/queue/goal/jobs/questions/attachments/log export 等不能只迁 prompt。
  5. 新建 `tests/{dsh_host,dsh_approvals,dsh_reconnect}.rs`；提供本地 fake DSH server，不调用真实付费模型。
  6. 新建 `core/crates/aeroric-core/src/agents/dsh_host/executor.rs`，接到同一 `TaskExecutor`/observer；Tauri 改用 core 的本地 PTY/DSH 实现后，只删临时 executor 的已替换本地分支，**不删整个 `src-tauri/src/legacy_task_executor.rs`**。SSH/WSL 分支维持旧客户端功能到 P7-01/02；回归覆盖 target×family×resume/cancel，不只测本机四族。
- **Acceptance Criteria**：
  - `pnpm native:check dsh`、`pnpm native:inventory --check`；fake server 中多 turn、问题阻塞、拒绝审批、附件失败、webui 重启恢复均有断言。
  - 无 GUI 的 core 集成测试可以经 typed service 方法回答审批；无回答时保持等待，不能因没有窗口自动同意。实际 CLI 子命令在 P3-07 接通并验收，此处不调用尚未实现的 CLI。
  - **调用 `grill-me [dsh]`**，重点复核 existing DSH resume/continue 差异与权限路由。

### Task P3-06 — 实现认证 UDS 宿主与事件重同步

- **Input**：§2.2–2.3；P3-01～05；`remote/event_log.rs`、`remote/terminal_frames.rs` 仅作行为参考。
- **Action**：
  1. 修改骨架已有的 `core/crates/aeroric-host/src/lib.rs`，新建同目录 `{startup,uds,auth,codec,dispatcher,subscriptions,diagnostics,shutdown}.rs`；逐步启动：lock → data compat → runtime → listener → ready。
  2. 新建 `core/crates/aeroric-core/src/events/{mod,log,snapshot}.rs`；实现序号分配/发布的单一原子边界。不得照搬“AtomicU64 自增后再另取队列锁”而未验证并发发布次序。
  3. 配置允许的 methods/capabilities；只注册已完成服务。认证前只接受 handshake；跨 data-dir token 不可用；不监听 TCP `0.0.0.0`。
  4. supervisor metadata 包含 PID、启动时间、epoch、build/protocol/data-dir hash；验证现存 listener 的握手，不凭进程名杀死其他版本。
  5. readiness 通过父进程管道或已认证 RPC 返回，stdout/stderr 日志脱敏。host 退出取消 watcher、flush storage、根据退出策略处理子进程。
- **Acceptance Criteria**：
  - 新建 `tests/{ipc_auth,ipc_frames,event_replay,host_lifecycle}.rs`；`pnpm native:check host`。
  - 错 token/错 UID/旧 epoch/超长 JSONL/partial frame/未知方法被正确拒绝；并发 100 个 mutation 的 seq 顺序与 snapshot 一致。
  - slow client 不阻塞其他客户端；重连去重、snapshot watermark 缺口、host crash、socket 残留、路径过长、二次启动均有测试。
  - **调用 `grill-me [host]`**；任何 unauthenticated mutation 都阻塞整个后续阶段。

### Task P3-07 — 创建真正无头的 `aeroric-cli`

- **Input**：P3-06 host、P2 command contracts；`src-tauri/src/main.rs` 作为“旧桌面入口”参照。
- **Action**：
  1. 修改骨架已有的 `core/crates/aeroric-cli/src/main.rs`，新建同目录 `{args,client,output,attach}.rs`，实现确定的命令树：
     ```text
     aeroric-cli --data-dir PATH doctor --json
     aeroric-cli --data-dir PATH serve
     aeroric-cli --data-dir PATH project list|open
     aeroric-cli --data-dir PATH run --project PATH --agent ID --prompt-file PATH --json
     aeroric-cli --data-dir PATH task list|get|cancel|complete|resume
     aeroric-cli --data-dir PATH task input TASK_ID --text-file PATH
     aeroric-cli --data-dir PATH session list|history|export
     aeroric-cli --data-dir PATH approval list|respond
     aeroric-cli --data-dir PATH rpc METHOD --params-file PATH --json
     aeroric-cli --data-dir PATH events --after CURSOR
     aeroric-cli --data-dir PATH attach TASK_ID
     ```
     `|` 是语法说明，实际子命令分别定义；`--help` 必须列真实参数。
  2. `run` 连接/按策略启动 host，默认等待本轮进入 settled/terminal；交互请求以结构化状态返回，不冒充完成。`--json` 的 stdout 只放 JSONL，诊断走 stderr。
  3. `attach` 转发真实 PTY bytes，使用 termios raw mode RAII 恢复；SIGWINCH 转 resize，退出 detach 不 cancel。非 TTY 时拒绝 attach 并提示 `events/session history`。
  4. 明确 exit code：0 操作成功（不代表 task done）；2 参数错误；3 连接/认证失败；4 task failed；5 需要人工交互；130 明确取消。`--help` 写明等待语义。
  5. 新建 `scripts/native/smoke-headless.mjs` 创建 temp data-dir/fake project/fake agent，运行 serve、run、input、cancel、resume、history，再退出；finally 清理自己创建的进程和目录。
  6. `rpc` 只调用 host 已注册并通过 schema/capability 校验的方法，不等于任意代码执行或绕过确认；mutations 使用同一 idempotency/revision 和危险操作确认契约。P6–P9 新服务至少补一条 CLI 正例及拒绝例。P3 独立 host 仅声明已迁完的本地执行能力；SSH/WSL 在 P7 接通前返回明确 unsupported，不借用 GUI 私有端口；旧 Tauri 仍经临时 target adapter 保持原有支持。
- **Acceptance Criteria**：
  - 新建 `tests/cli_headless.rs`；`pnpm native:check cli` 实际启动 CLI 子进程，不启动 Tauri/NSApplication；`cargo tree --manifest-path core/Cargo.toml --locked -p aeroric-cli` 不含 Tauri。
  - `cargo run --manifest-path core/Cargo.toml -p aeroric-cli --locked -- --help` 与以上接口一致；headless smoke 输出 task 的确定最终状态且无孤儿 fake agent。
  - attach 的 Ctrl-C/EOF/异常路径恢复本地 terminal settings；readiness/日志不会污染 JSON stdout。
  - 使用真实 CLI 的 `approval list/respond` 对 fake DSH 完成拒绝、批准、重复/过期请求测试；`task input` 对 PTY 要求有效控制租约，对结构化 Agent 走声明能力，不向不支持的协议猜测注入按键。
  - **调用 `grill-me [cli]`**；至此 Phase 3 Gate 才能声称“有纯无头入口”。

## Phase 4 — SwiftUI 工程、IPC 客户端与 primuse 风格基础框架

**入口**：真实本地协议、fake host 和 core CLI 可验；原生 UI 规则范围已经批准。

**出口**：原生 app 可独立 Debug 构建、连接 host、展示权威 snapshot，glass 与降级外观都可用。

### Task P4-01 — XcodeGen 工程和本地无账号构建

- **Input**：P1 原生包；primuse `project.yml` 仅参考生成结构；§0 签名约束。
- **Action**：
  1. 新建 `macos/project.yml`、`Config/{Debug,ReleaseLocal}.xcconfig`、`AeroricApp/Info.plist`、`AeroricApp/Sources/{AeroricApp,AppDelegate,AppServices,BootstrapView}.swift`、`AeroricApp/Resources/Assets.xcassets/Contents.json`、`macos/AeroricUITests/AppLaunchTests.swift`；application/test target 均有真实可编译源文件，不用空 target 蒙混工程生成。
  2. 创建 `AeroricNative` application target、`AeroricNativeUITests` target、共享 scheme；本地包依赖 `AeroricKit`，不重复编译相同 Swift 源到两个 target。
  3. 预发布 bundle identifier 采用 `com.aeroric.native.dev`，display name `Aeroric Native Preview`；与旧 `com.aeroric.desktop` 区分，TCC 授权不得假定共享。正式 ID 合并由 P10-04 决策。
  4. `DEVELOPMENT_TEAM` 留空、`CODE_SIGN_STYLE=Manual`、`CODE_SIGN_IDENTITY=-`、`ENABLE_HARDENED_RUNTIME=NO`、`ENABLE_APP_SANDBOX=NO`。不加入 CloudKit/App Groups/push/商店 provisioning。
  5. Debug 编译可 `CODE_SIGNING_ALLOWED=NO`，但可运行 bundle 最后由 build script 明确签名；不把链接器的临时签名当完整 app 签名。**本 Task 即接通** `scripts/native/{generate,build,dev,ui-test}.mjs`，新建 `scripts/native/{sign-local,verify-bundle}.sh`：构建本架构 helper→复制进 bundle→生成/构建 app→先 helper/嵌套代码后 app ad-hoc 签名→验签。P10-03 是完整性/故障/发布配置强化，不是首次提供可运行构建。
  6. `AppServices` 先只管理明确注入的 preview 配置与 bootstrap state，`BootstrapView` 显示环境/构建标识；不要提前引用 P4-02 才实现的 `HostSupervisor`/`HostConnection`。P4-02 再注入唯一 supervisor/client；`AppDelegate` 先定义 Dock reopen、Cmd+Q、最后窗口关闭策略，P4-04 接多窗口。
  7. `native:ui-test` 用 Xcode 的本地 ad-hoc runner 签名，不请求 provisioning；`AppLaunchTests` 只验冷启动、bootstrap 标识、关闭，不提前要求业务界面。app 与 test runner 均注入隔离 data-dir，签名/启动命令失败必须使脚本非零退出。
- **Acceptance Criteria**：
  - `pnpm native:generate`；`xcodebuild -list -project macos/AeroricNative.xcodeproj` 包含 app/test scheme。
  - `xcodebuild -project macos/AeroricNative.xcodeproj -scheme AeroricNative -configuration Debug -destination 'platform=macOS' -derivedDataPath .artifacts/native/DerivedData CODE_SIGNING_ALLOWED=NO build` 成功。
  - `pnpm native:build`、`pnpm native:ui-test` 通过；`pnpm native:dev` 启动已 ad-hoc 签名的 bootstrap app，最小签名检查成功，app 不访问生产数据。此处只证明最小构建/本地签名/启动，不提前声称完整业务或 P10 全量签名审计完成。
  - **调用 `grill-me [native-bootstrap]`**，审查本地无账号构建、helper 复制/签名顺序、测试 runner 与正式 App 的配置隔离；此门禁不替代 P10-03 的完整签名审计。

### Task P4-02 — Swift actor IPC、宿主监督和状态 Store

- **Input**：`AeroricProtocol` DTO、P3 host 测试、§2.3。
- **Action**：
  1. 新建 `macos/AeroricKit/Sources/AeroricClient/{HostConnection,JSONLCodec,RequestRegistry,EventReducer,HostSupervisor,ReconnectPolicy}.swift`；`HostConnection` 用 actor，阻塞 Unix socket I/O 放专用队列而非 MainActor。
  2. 请求以 UUID 映射 continuation，完成/超时/cancel/disconnect 都只能 resolve 一次；断线后不重发未知结果的 mutation。认证读取私有 token，不向 UI 暴露。
  3. `HostSupervisor` 只从固定 bundle `Contents/MacOS/aeroric-cli` 启动，设置 cwd/env 白名单；验证 ready/版本，不从当前目录/PATH 执行同名恶意文件。
  4. 新建 `AeroricUI/State/{ApplicationStore,WorkspaceStore,ConnectionStore}.swift`：`@MainActor @Observable` store 消费 typed events；socket decode/大文件解析不在主线程；每窗口只有 selection/draft/layout 状态。
  5. UI 显示 connecting/ready/degraded/reconnecting/incompatible；明确 reconnect 不是 restart task，提供只读 snapshot 保留旧屏幕而非清空项目。
  6. 修改 `macos/AeroricApp/Sources/{AppServices,BootstrapView}.swift`，把 P4-01 bootstrap state 接到已实现的唯一 supervisor/client 和 ConnectionStore；移除临时注入实现，仍只使用 preview data-dir。
- **Acceptance Criteria**：
  - 新建 `AeroricClientTests/{HostConnectionTests,EventReducerTests,SupervisorTests}.swift`；`pnpm native:check swift-client`。
  - 1-byte 分片、多个帧粘包、迟到 response、重复 response、request cancel、epoch 变化、replay gap、unknown event 都不 crash。
  - Thread Sanitizer/Swift 严格并发检查通过；断网/杀 fake host 时 UI 响应且不重复启动 task。
  - **调用 `grill-me [swift-client]`**，审查 actor reentrancy、continuation 泄漏、host ownership。

### Task P4-03 — 创建玻璃材质和窗口外观组件

- **Input**：primuse `Theme/PrimuseTheme.swift::{NSVisualEffectBackdrop,PMWindowChromeConfigurator}`、`PMTitleBar.swift`；`packages/design-system/src/tokens.ts`、`scripts/generate-design-tokens.mjs`；P1 ADR。
- **Action**：
  1. 新建 `AeroricUI/Design/{ThemeTokens,NativeMaterial,GlassPanel,WindowAppearance,NativeSelection}.swift`。颜色从 semantic role 取值，不把某张截图的 RGB 写遍 View。
  2. `NativeMaterial: NSViewRepresentable` 创建 `NSVisualEffectView`，material=.sidebar/.underWindowBackground，blendingMode=.behindWindow，state=.followsWindowActiveState；`updateNSView` 更新所有可变属性。
  3. `GlassPanel` 内容区使用 `.thinMaterial`/`.ultraThinMaterial`；终端和编辑器用足够不透明的 text background，不对每一行文字叠一层 blur。`glassEffect` 仅放可交互控件并检查 OS availability。
  4. `WindowAppearance` 在所属 NSWindow ready 后设置透明标题栏和背景，保留标准红黄绿按钮；不全局 swizzle、不查私有子视图、不照抄 primuse 的持续修复循环。
  5. `NativeSelection` 只封装系统 `Picker` / `List(selection:)` 的统一样式、键盘与 accessibility，不新绘活动底纹；React 任何相应改动仍用 AnimatedSelection。
  6. 增加 reduced transparency / increased contrast / reduced motion 分支；动态系统色，glass/classic 是外观偏好，不能与 interactionMode 混为一枚开关。
- **Acceptance Criteria**：
  - 新建 `AeroricUIUnitTests/ThemePolicyTests.swift`；`pnpm native:check native-design`。
  - 实机将窗后放彩色窗口，激活/失活时材质确有变化；Reduce Transparency 后背景不透明且文字可读；深浅色/高对比无固定白字白底。
  - traffic lights、拖动空白标题区、双击标题栏、全屏、缩放、恢复尺寸正常；text selection 不拖走窗口。
  - **调用 `grill-me [native-design]`**，审查 native/React 组件约束和 a11y，不以截图相似掩盖非原生实现。

### Task P4-04 — 主窗口、设置窗口、菜单与导航

- **Input**：primuse `MacContentView.swift`、`MacSettingsView.swift`、`MacMenuBarController.swift`；Aeroric `ProjectPage.tsx`、`AppSettingsDialog.tsx`、`shortcuts.ts`。
- **Action**：
  1. 新建 `AeroricUI/Shell/{WorkspaceRootView,ProjectSidebar,WorkspaceDetail,TitleBar,StatusBar,CommandPalette}.swift`，布局为 titlebar + sidebar + detail + statusbar；sidebar 宽度初始 220pt、范围 180–360pt，主窗口最小 1000×640pt，可压缩次要面板。
  2. 项目导航用系统 `List(selection:)` 放在真实材质上；采用 primuse 的间距/层次，但不为照抄 ScrollView 手工活动背景违反本项目约束。
  3. 新建 `AeroricUI/Settings/{SettingsWindowController,SettingsRootView,SettingsSection}.swift`；用一个可复用 `NSWindowController` + `NSHostingController` 打开独立设置窗口，Cmd+, 复用同一窗口，不每次 new。
  4. 新建 `AeroricApp/Sources/AppCommands.swift`，声明 New Task/Open Project/Settings/Close Window/Show Main Window 等菜单；focused workspace 决定命令目标，防多窗口操作错项目。
  5. Loading/empty/error/disconnected 分开；内容加载前不误显示“无项目”；取消打开文件不创建空项目。每个控件给稳定 accessibility identifier。
- **Acceptance Criteria**：
  - `pnpm native:check shell` + `pnpm native:ui-test`：Cmd+,、Cmd+W、Dock reopen、两个窗口不同 project selection、sidebar keyboard selection 均通过。
  - App 中无 React 根页面/远程网页承载主导航；切换窗口不创建第二个数据写入者。
  - **调用 `grill-me [shell]`**；Phase 4 Gate 只在键盘与窗口生命周期验收后通过。

## Phase 5 — 终端、纯原生会话与设置模式切换

**入口**：Swift client/store/shell 稳定；四族 capabilities 可查询。

**出口**：原生任务操作完整；模式切换不改变进程；不支持结构化的 Agent 有明确且可用的高级入口。

### Task P5-01 — 用原生终端 renderer 承接同一 PTY

- **Input**：P3 TerminalService、现有 `TerminalView.tsx` / terminal tests；SwiftTerm 官方 `TerminalView` 文档与锁定 revision。
- **Action**：
  1. 在 `macos/AeroricKit/Package.swift` 为 `AeroricUI` 增加经评估并固定的 SwiftTerm 依赖，提交 `Package.resolved`；先做能力 spike，验证上游实际 API，不按记忆伪造 delegate 签名。
  2. 新建 `AeroricUI/Terminal/{NativeTerminalView,TerminalCoordinator,TerminalStore,TerminalInputPolicy}.swift`；`NSViewRepresentable` 只创建 renderer，**不用会自行 spawn 的 LocalProcessTerminalView 再开一个进程**。
  3. coordinator 将输出 bytes 喂 renderer，将 delegate 的输入/resize 发 host；保留 CJK/emoji、bracketed paste、alternate screen、鼠标协议、键盘组合、IME、字体缩放。
  4. terminal instance 以 taskId+generation 管理，切模式只 detach UI input lease；同 task 返回时使用 cursor 补流，不重新启动 shell。
  5. 重连及截断时执行 renderer reset、明确 truncated 提示，按 host replay 从安全边界重建；测试不支持的 escape/image protocol 时保留现有 Tauri 终端回退入口，并阻塞“完全终端等价”验收。
  6. 默认不自动执行 OSC 52 clipboard 写入，不自动打开终端发出的 URL；用户触发复制/打开前校验 scheme 并确认。
- **Acceptance Criteria**：
  - 新建 `TerminalCoordinatorTests.swift`、XCTest UI `TerminalInteractionTests.swift`；`pnpm native:check native-terminal`。
  - 同一个 fake-agent 输出 fixture 同时喂 xterm 与原生 renderer，对比屏幕/光标/alternate-screen；手测 OMP 与 `vim`/`less`/交互选择器（只读临时文件）。
  - 输入中文时 Enter 先完成 composition，不误提交；测试 harness 对 renderer 做 100 次 attach/detach/隐藏/重显，无额外子进程、writer、subscription 或 delegate 泄漏。真实设置 Toggle 的切换验收在 P5-02，不在此要求尚未创建的控制器。
  - **调用 `grill-me [native-terminal]`**；SwiftTerm 未满足必选终端能力时先解决或记录阻塞，不直接降级成 TextEditor。

### Task P5-02 — 设置中的原生模式按钮与无损切换

- **Input**：§2.4；`app_settings/schema.rs` 的迁移后实现、`src/settings.ts`、`src/components/app-settings/GeneralPanel.tsx`、`AnimatedSelection.tsx`；P4 stores。
- **Action**：
  1. 在 core settings schema 增加带迁移默认值的 `interactionMode`；新增 `AeroricUI/Settings/InteractionSettingsView.swift` 和 `AeroricUI/State/InteractionModeController.swift`。
  2. 用原生 `Toggle("TUI / 高级命令行模式", isOn: ...)` 的显式 `Binding` 映射 enum；OFF 标签“纯原生 UI”；文案注明不改变底层 Agent。保存失败回滚 UI，显示原因。
  3. 新建 `AeroricUI/Workspace/SessionPresentation.swift`，由 enum + capabilities 决定 renderer，允许注入明确类型的 native presentation factory；unsupported 时显示状态卡与“切换到高级模式”按钮，不自动修改全局偏好。P5-04 之前仅显示原生任务元数据/明确的未接通状态，不引用不存在的 ComposerView，也不能把占位视图标为 DSH composer 已完成。
  4. 切换顺序：暂缓 IME → 保存窗口草稿/滚动锚 → 释放旧输入租约 → 挂新 presentation → 获取必要 lease → 恢复 focus。全程不调用 start/resume/cancel/reset_task_process。
  5. React 若暴露对应设置，修改现有 GeneralPanel；独立 switch 可沿用旧 switch 组件，相邻互斥选项必须 `AnimatedSelectionGroup`。不是另建脱离 Vite 的设置页。
- **Acceptance Criteria**：
  - 新建 `ModeSwitchTests.swift`、`src/test/interaction-mode.test.tsx`、core `tests/settings_mode.rs`；`pnpm native:check mode-switch`。
  - fake host 记录 100 次切换后 `spawnCount=1`、generation 不变、sessionId 不变、原始输出顺序不变；draft 与等待审批不丢。
  - 此处验模式 controller、renderer 生命周期与保存策略；OMP native 模式没有伪造 composer/approval，advanced 真终端可继续同一任务。DSH 真 composer/approval 的切换集成验收移到 P5-04，并作为 Phase 5 必过出口，不用占位组件冒充通过。
  - **调用 `grill-me [mode-switch]`**；模式切换与任务协议转换的差别必须在 UI 文案及测试中都成立。

### Task P5-03 — 项目、任务列表与原生任务编辑器

- **Input**：F01/F02、P2 services、`src/components/RunningView.tsx`、`src/state/app/{taskMutations,projectMutations}.ts`。
- **Action**：
  1. 新建 `AeroricUI/Projects/{ProjectListView,OpenProjectSheet,ProjectGroupEditor}.swift`、`AeroricUI/Tasks/{TaskListView,TaskRow,TaskDraftView,TaskDetailHeader,TaskActions}.swift`。
  2. 项目/任务列表绑定 authoritative stores；status/time/error/agent 信息来自 snapshot；archive/star/filter 在 host 与窗口局部过滤间明确区分。
  3. draft 包含 agent/model/effort/permission/prompt/attachments/worktree；host capability 决定选项，创建时提交 revision 和 idempotency key。
  4. 删除/取消/丢弃区分，运行中删除必须确认；批量操作逐项返回成功/失败，不能全局 Toast 一句话丢掉局部失败。
  5. 原生 `NSOpenPanel` 通过 platform UI adapter 选择路径；不得把客户端路径“选过”当 server 路径校验替代。
- **Acceptance Criteria**：
  - 新建 `TaskStoreTests.swift`、XCTest UI `TaskWorkflowTests.swift`；`pnpm native:check tasks-ui`。
  - 创建→pending→running→input_required→done；cancel/failed/interrupted/resume；archive/unarchive/star/rename/filter，已有任务重启后仍可见。
  - 多窗口 revision conflict 自动刷新后提示用户重试，不覆盖其他窗口变更。
  - **调用 `grill-me [tasks-ui]`**。

### Task P5-04 — 原生会话消息、审批、问题、轨迹与用量

- **Input**：P3 session/DSH services；`src/dsh{Protocol,ToolViews,TrajectoryFormat,TrajectoryLedger,SessionLogExport}.ts`；`src/components/RunningView.tsx`。
- **Action**：
  1. 新建 `AeroricUI/Sessions/{SessionView,MessageRow,ContentBlock,ToolCallRow,SessionPaginator,SessionActions,UsagePanel}.swift`；Markdown 渲染显示内容不得执行 HTML/脚本，点击 URL 校验 scheme。
  2. 新建 `AeroricUI/Approvals/{ApprovalSheet,QuestionSheet,PendingRequestStore}.swift`，每个 request 按 ID 去重，明确 approve/deny/cancel；拒绝后状态由 host 确认，不能本地抹掉就算处理。
  3. 新建 `AeroricUI/DSH/{ComposerView,AttachmentPicker,SlashCommandPicker,QueuePanel,GoalPanel,SubagentPanel,TrajectoryView}.swift`；模型/effort 控制只显示 capability 允许的值。
  4. session history 用稳定 ID diff 更新，分页不跳动；stream delta 可批量合并，不每字重排全部消息。保留未知 tool/content 项及 source ID。
  5. 支持 session export/search/resume/adopt/handoff；跨协议动作显示新会话提示，确保旧历史仍能打开。
  6. 把真实 DSH composer/审批/问题视图接入 P5-02 `SessionPresentation`，新增 `macos/AeroricUITests/ModeSessionIntegrationTests.swift`；移除对应未接通状态但保留其他 Agent 的 unsupported 状态。
- **Acceptance Criteria**：
  - 新建 `SessionReducerTests.swift`、`ApprovalRoutingTests.swift`、`MessagePaginationTests.swift`；`pnpm native:check sessions-ui`。
  - 10,000 条 fixture history 分页可滚动；重复 tool_end 不重复 row/usage；未知事件不使整个会话解码失败。
  - 两个任务同时等待审批不会串题；模式切换/断线/过期/host restart 不自动批准；附件失败可撤销重试。
  - `pnpm native:check mode-switch`、`pnpm native:check sessions-ui`、`pnpm native:ui-test` 联合通过：DSH composer 真实发送/收消息，等待审批/带草稿时切换 100 次不丢数据、不重启任务；OMP 仍只通过原始 PTY 进行其原生交互。
  - **调用 `grill-me [sessions-ui]`**。

### Task P5-05 — 设置完整映射而不是只做模式开关

- **Input**：现有 `app-settings/panelRegistry.tsx` 所有 panel、`app_settings/schema.rs`、`modelOptions.ts`、`agent_tools.rs`、`agent_ops.rs`。
- **Action**：
  1. 新建 `AeroricUI/Settings/{GeneralSettingsView,AppearanceSettingsView,AgentSettingsView,CustomAgentEditor,ModelSettingsView,ShortcutSettingsView,AgentUpdateView,SettingsSearchIndex}.swift`。
  2. 每项设置映射到 core schema/command；分 UI-only（窗口尺寸等）与 shared business（agent、proxy、MCP）；不要用 `@AppStorage` 另存一份共享业务配置。
  3. Secret 只显示 `configured`/masked 状态，替换写入传一次后清内存；不预填原 key，不进日志、快照、剪贴板或崩溃报告。
  4. 升级 agent 走现有 operation 服务；显示计划的安装渠道、影响任务、进度/取消/恢复；用户确认后执行，不因打开设置自动 upgrade。
  5. 所有四族记录、内置别名、自定义档案和用量列表一致；原生设置不发 OMP 不支持的 effort。
- **Acceptance Criteria**：
  - 新建 `SettingsStoreTests.swift`、`SettingsCapabilityTests.swift`；`pnpm native:check settings`，`pnpm native:inventory --check` 检查所有原 panel 有映射。
  - 无效路径/model/effort/secret 保存失败不污染旧配置；关闭设置重开值正确；离线模型目录有来源/更新时间提示而非虚假在线结果。
  - **调用 `grill-me [settings]`**；Phase 5 Gate 必须同时包含 PTY、native、unsupported 三种模式测试。

## Phase 6 — 原生工作区：文件、编辑器、IDE 与 Git

**入口**：任务/会话原生工作区已可用。

**出口**：现有开发工具有原生等价入口；行为仍由同一 Rust 服务提供，而非 Swift 另写 shell 命令。

### Task P6-01 — 文件树、读取、搜索替换与路径安全

- **Input**：`src-tauri/src/{fs,search,path_guard,posix_fs}.rs`；`src/lib/api/{fs,search}.ts`；已有文件/search 测试。
- **Action**：
  1. 新建 `core/crates/aeroric-core/src/files/{mod,service,read,write,search,path_policy,watch}.rs`；逐项移动 read/list/create/rename/copy/trash/search/replace 的业务主体，旧 command 只做 adapter。
  2. 读写请求带 projectId + relative path + expected file version；server 端校验根目录、symlink、路径组件和操作上限，不依赖客户端过滤 `..`。
  3. 对写入、重命名、删除使用现有安全 path guard/目录句柄策略，避免 canonicalize 与后续 open 之间的 symlink 替换竞态。现有允许的显式外部文件访问保持独立授权路径。
  4. 新建 `AeroricUI/Files/{FileBrowserView,FileTreeStore,FileRow,SearchPanel,ReplacePreviewSheet,FileOperationSheet}.swift`；树懒加载，搜索结果支持 cursor；替换先 preview，再提交匹配版本的改动。
  5. 文件 watcher coalesce/overflow → rescan，不能每个 event 重读整仓；服务持有取消 token，不用 Swift Timer 高频轮询。
- **Acceptance Criteria**：
  - 新建 Rust `tests/files.rs`、`tests/search_replace.rs`、Swift `FileTreeTests.swift`；`pnpm native:check files`。
  - temp project 中 symlink escape、`../`、Unicode、空格、hidden、权限拒绝、大目录、watcher overflow、写时文件被外部修改均覆盖。
  - 替换预览后外部修改文件时拒绝覆盖；删除进废纸篓/既有策略，不默认永久删除。
  - **调用 `grill-me [files]`**，重点检查 TOCTOU、搜索结果路径与 mutation 身份。

### Task P6-02 — 原生文本/Markdown 编辑、预览和本地历史

- **Input**：`src-tauri/src/{formatter,local_history}.rs`；现有 CodeMirror、Markdown、图片预览相关组件；F07 矩阵。
- **Action**：
  1. 新建 core `files/{format,local_history,preview}.rs`；保留 formatter 命令发现、超时、取消与路径校验。
  2. 新建 `AeroricUI/Editor/{NativeTextEditor,EditorCoordinator,DocumentStore,EditorTabs,SaveConflictSheet,MarkdownPreview,ImagePreview,LocalHistoryPanel}.swift`。
  3. 编辑用 `NSTextView`/TextKit 桥接，维护一个 document model、UndoManager、dirty/revision/selection；实现行号、find/replace、tab/indent、编码/换行、语言标识、快捷键与 CJK IME。不得用一个基本 TextEditor 就宣称等价 CodeMirror。
  4. 渲染 Markdown 优先原生 attributed content；表格/代码块/公式/图表等复杂内容按 feature-matrix 单独验收。如必须局部 WKWebView，内容由本地已审查 renderer 生成，CSP、无远程脚本、无任意 file access、无广泛 message handler；**不得承载应用导航、会话 composer 或通用业务 UI**。
  5. source/read mode 用系统 Picker；保留未保存文档，关 tab/quit 明确 save/discard/cancel。autosave 版本冲突不可静默覆盖。
  6. 大文件/二进制/未知编码显示有原因的只读模式与显式外部打开选项；不能无限载入并卡主线程。
- **Acceptance Criteria**：
  - 新建 Rust `tests/file_history.rs`、Swift `DocumentStoreTests.swift`、XCTest UI `EditorTests.swift`；`pnpm native:check editor`。
  - UTF-8 BOM、CRLF、emoji UTF-16 selection、外部修改、Undo/Redo、重复 save、退出取消不丢改动；格式化失败原文不变。
  - Markdown 恶意 HTML、`javascript:` link、本地文件探测不能执行或泄露；local history restore 有预览/确认且可撤回。
  - **调用 `grill-me [editor]`**；必选编辑功能缺失时保持迁移未完成，不以 Tauri 回退掩盖原生缺口。

### Task P6-03 — LSP、诊断、运行/测试、DAP 与预览

- **Input**：`src-tauri/src/{lsp,dap,run_config,diagnostics,formatter}.rs`、`lsp/`、`dap/`；`src/lib/api/lsp.ts`、`ProjectPage.tsx` 中 run/debug 接线。
- **Action**：
  1. 新建 core `ide/{mod,lsp,dap,run_config,diagnostics,test_runner,preview}.rs`；保留协议 parser 子模块，使用 projectTarget 而非仅本地 path。
  2. LSP process 按 workspace/language 生命周期管理；request cancellation、document version、didOpen/change/close、offset encoding 在核心统一转换，不让 Swift 各组件自行换算。
  3. 新建 `AeroricUI/IDE/{ProblemsView,SymbolOutline,CompletionPanel,RunConfigurationEditor,TestResultsView,DebugPanel,BreakpointStore,WebPreview}.swift`。
  4. Debug 控件覆盖 start/attach/pause/continue/step/stop、stack/scopes/variables/breakpoints，未知 adapter 状态可诊断；断开 debug 不误 kill 其他任务。
  5. web preview 只允许显式批准的本地/项目 URL，独立受限 WKWebView；不向页面注入 host token/任意 command bridge，外部 URL 交系统浏览器。
- **Acceptance Criteria**：
  - 新建 `tests/{lsp_protocol,dap_protocol,run_config}.rs`、Swift `IDEStateTests.swift`；`pnpm native:check ide`。
  - fake LSP/DAP 验证中文/emoji位置、过时 diagnostics 丢弃、server crash/restart、run cancel、test failure定位；至少一个本地示例项目完成真实断点/单步烟测（不联网）。
  - preview 的恶意跳转/脚本无法调用文件写接口；view 关闭后清理对应 preview 资源。
  - **调用 `grill-me [ide]`**。

### Task P6-04 — Git 全流程及危险操作确认

- **Input**：`src-tauri/src/{git,git_path_guard}.rs`、`git/transport.rs`；`src/lib/api/git.ts`；Git 相关 frontend fixtures。
- **Action**：
  1. 新建 core `git/{mod,service,status,diff,index,commit,history,refs,stash,conflicts,transport}.rs`；按现有函数模块化移动，不在 Swift 直接调用 `git`。
  2. 参数使用 argv 与 `--`，校验 pathspec/revision/options；相同 repo mutating 操作串行化；read timeout 不留下持锁进程。
  3. 新建 `AeroricUI/Git/{GitPanel,ChangeList,DiffView,StageActions,CommitSheet,HistoryView,BranchGraph,BlameView,StashView,ConflictResolver}.swift`。
  4. stage/unstage、分块操作、commit、push、fetch/pull、stash、branch、conflict 的已有能力逐项映射；push/丢弃/冲突覆盖需独立确认，不预设授权。
  5. diff/binary/rename/submodule/巨大文件显示有界结果，原始 diff 可下载到用户指定位置，不截断后执行错误的 partial patch。
- **Acceptance Criteria**：
  - 新建 `tests/git_service.rs`、`GitStoreTests.swift`；`pnpm native:check git`。
  - 使用临时 repo + 本地 bare remote，验证 untracked/rename/冲突/空提交/失败 push；不触碰 Aeroric 真实暂存区。
  - 参数以 `-` 开头、换行、空格文件名不成为选项注入；取消 commit 后 index 不被额外改变。
  - **调用 `grill-me [git]`**。

### Task P6-05 — worktree 与任务生命周期联合迁移

- **Input**：`src/state/app/worktreeOps.ts`、`src/lib/api/worktree.ts`、`src/taskSession.ts`、Rust git worktree 函数、AGENTS main-only。
- **Action**：
  1. 新建 core `git/{worktree,worktree_policy}.rs`，定义 `WorktreeService`；任务关联 path/branch/baseBranch/discarded/diffstats 仍写 Task snapshot。
  2. 新建 `AeroricUI/Git/{WorktreeCreateSheet,WorktreeTaskBadge,WorktreeMergeSheet,WorktreeDiscardSheet}.swift`。
  3. **开发 Aeroric 本身禁止新增分支**；产品能力仍可给用户其他仓库创建 worktree，但必须显式选择并尊重目标项目的 branch policy。测试只在临时仓库验证。
  4. merge/discard 前检查 running task、dirty/untracked、目标 branch、worktree 是否属于该 task；移除需要确认；失败返回逐步骤恢复信息。
  5. 已 discarded 的 task 禁用 resume/merge/discard，但历史与路径审计保留；不能在按“继续”时重新创建同名目录覆盖用户文件。
- **Acceptance Criteria**：
  - 新建 `tests/worktree_lifecycle.rs`、`WorktreePolicyTests.swift`；`pnpm native:check worktree`。
  - tmp repo 完成 create→task→diff→merge；冲突/dirty/discard/reopen 有反例；真实 Aeroric 仍是 main 且没有新分支。
  - **调用 `grill-me [worktree]`**；Phase 6 Gate 包含 F07–F10 每个子能力，不能只有 git status。

## Phase 7 — 连接、基础设施、数据库与笔记

**入口**：本地项目/文件/Git/编辑器完成，core 不依赖窗口。

**出口**：完整保留原有生产力模块；macOS 不支持的平台特性明确降级，跨平台实现继续编译。

### Task P7-01 — SSH/SFTP 与远程工作区

- **Input**：`src-tauri/src/{ssh,ssh_hostkey,ssh_proxy,sftp,sftp_windows,remote_fs,remote_git,remote_os}.rs`；现有 SSH/SFTP 面板。
- **Action**：
  1. 新建 core `connections/{mod,ssh,host_key,proxy,sftp,remote_files,remote_git,remote_os}.rs`，提取无 Tauri 业务；保留 credential owner 与连接配置格式。
  2. 新建 `AeroricUI/Connections/{ConnectionList,SSHConnectionEditor,HostKeyPrompt,SFTPBrowser,TransferQueue,RemoteWorkspaceBanner}.swift`。
  3. 首连/主机 key 变化分别提示，拒绝不可自动重试为信任；密码/私钥 passphrase 不进 snapshot/log；不复制私钥到新 native 配置目录。
  4. 远程任务/terminal 仍由 core 走现有安全 launch mapping；remote cwd 与 local cwd 分开显示，防止 native 文件动作误落本机。
  5. 上传/下载进度、取消、恢复、覆盖确认与权限错误逐项实现；断线后命令若可能已经执行，先查询状态，不自动重放 rm/mv/任务创建。
  6. 新建 `core/crates/aeroric-core/src/connections/ssh_executor.rs`，提取 SSH 任务 start/resume/input/resize/cancel 的低层入口，实现同一 TaskExecutor/observer，继续使用唯一 task registry/generation。Tauri 与 headless host 接入并验收后，仅删除 `legacy_task_executor.rs` 的 SSH 分支；WSL 分支留到 P7-02，不能恢复 React 编排或另开任务 map。
- **Acceptance Criteria**：
  - 新建 `tests/{ssh_host_keys,sftp_transfer,remote_workspace}.rs`、`ConnectionStoreTests.swift`；`pnpm native:check connections`。
  - 默认用 fake SSH/SFTP；显式 opt-in 本地测试 sshd 才跑真连接，不连接用户生产服务器；hostkey changed 必须失败等待确认。
  - 非 ASCII/空格/前导 dash 远程路径不注入命令；cancel transfer 清临时文件而非删除原目标。
  - fake SSH 任务在 Tauri 与独立 CLI 均完成 start→input→resume/cancel；迟到 exit 不覆盖新 generation；没有 GUI 仍可执行。原 WSL executor 在此步仍可达，不被删除 SSH 分支连带移除。
  - **调用 `grill-me [connections]`**。

### Task P7-02 — WSL 与跨平台运行时保留

- **Input**：`src-tauri/src/{wsl,wsl_fs,wsl_git}.rs`、`platform/`、`src/projectTarget.ts`、`WslPanel.tsx`。
- **Action**：
  1. 新建 core `platform/{mod,unix,windows}.rs`、`connections/{wsl,wsl_files,wsl_git}.rs`，用 `cfg` 隔离实现，不删除 Windows 分支。
  2. 新建 `AeroricUI/Connections/UnsupportedTargetView.swift`；在 macOS 打开 WSL 项目时保持元数据、显示“需要 Windows 主机”，禁止当作本地 path。
  3. core capability 在 macOS 为 `wsl=false`；未来远程 Windows host 可以声明 WSL，但不得由 client OS 直接推断 host 能力。
  4. 修改 `.github/workflows/checks.yml` 增加 core 的 Windows/Linux 检查，保持原 Tauri matrix 与 source patches。
  5. 新建 `core/crates/aeroric-core/src/connections/wsl_executor.rs`，把 WSL 任务启动/恢复/输入/resize/取消接到同一 executor/observer，保留 Windows 专用实现。Windows Tauri 与 CLI 回归通过，且 P7-01 SSH 已替换后，才删除 `src-tauri/src/legacy_task_executor.rs` 和其注入接线；确认 local/SSH/WSL×受支持 family 均没有剩余 legacy 分支或旁路 writer。
- **Acceptance Criteria**：
  - 新建 `tests/platform_capabilities.rs`；`pnpm native:check platforms`。
  - Windows CI 运行 WSL 路由/quoting 单元测试（无 WSL 环境的真集成明确 skip，不能代替必选支持机器 smoke）；Linux/macOS 核心编译。
  - macOS UI 无法点击到 wsl.exe 启动；旧 WSL JSON 读写不丢 distribution/linuxPath。
  - Windows 支持机器上验证 WSL 的 start/resume/cancel、退出/重连/generation 竞态；core 的所有支持 target 已可无 GUI 运行，删除临时 adapter 不使旧 Tauri 失去任何已支持路由。
  - **调用 `grill-me [platforms]`**。

### Task P7-03 — Docker、端口与 Conda

- **Input**：`src-tauri/src/{docker,ports,conda}.rs`；现有 Docker/Ports/Conda 控件。
- **Action**：
  1. 新建 core `infrastructure/{mod,docker,ports,conda}.rs`；长任务返回 operationId，支持取消/状态查询。
  2. 新建 `AeroricUI/Infrastructure/{DockerPanel,ContainerDetail,ImageList,PortInspector,CondaEnvironmentPicker}.swift`。
  3. container start/stop/restart/remove、image inspect/remove、logs、port owner/stop 显式区分只读/破坏操作；无 Docker daemon 时显示不可用，不弹 sudo。
  4. Conda 选择传递经校验的环境路径，spawn 复用 Agent environment policy，不能污染全局 shell rc。
- **Acceptance Criteria**：
  - 新建 `tests/infrastructure.rs`、`InfrastructureStoreTests.swift`；`pnpm native:check infrastructure`。
  - fake CLI 覆盖 unavailable/permission denied/输出异常/取消；端口释放前确认 PID 启动身份，避免 PID 重用误杀。
  - 可选真 Docker smoke 只创建带随机 test label 的临时资源，finally 清理自己的资源。
  - **调用 `grill-me [infrastructure]`**。

### Task P7-04 — DBX 数据库能力与原生数据网格

- **Input**：`src-tauri/src/database/` 的 connections/query/grid/schema/import_export/redis/mongo/transfer/legacy_sqlite/connection_secrets/dbx_state；`scripts/prepare-dbx.sh`、Cargo patches。
- **Action**：
  1. 在 core 新建 `database/` 对应同名模块，逐个搬移业务；原 command 同名保留为 adapter。DBX 初始化不是 AppHandle 的职责；降级内存库要有显式 diagnostics，不能悄悄当持久连接库。
  2. 新建 `AeroricUI/Database/{DatabaseSidebar,ConnectionEditor,SchemaBrowser,SQLQueryEditor,ResultGrid,StructureEditor,ImportExportSheet,RedisBrowser,MongoBrowser,ProductionConfirmationSheet}.swift`。
  3. SQL 执行沿用 production assessment/confirmation；一次批准绑定 operationId、目标与 SQL hash，不对后续所有 SQL 放行。
  4. 结果分页/虚拟化；列类型/NULL/binary/大值明确，编辑使用原 DBX 条件保护，不拼 Swift SQL；query cancel/close result session 释放资源。
  5. 连接 secret 走既有专用服务；debug log 不输出 connection string/password；导入导出使用用户选定路径、预览与写入确认。
- **Acceptance Criteria**：
  - 新建 `tests/{database_query,database_grid,database_confirmation}.rs`、`DatabaseStoreTests.swift`；`pnpm native:check database`。
  - 必选用临时 SQLite 执行真实读写/事务/取消/导入导出；其他驱动用 DBX 契约测试＋显式测试容器，不接真实数据库。
  - 两个窗口确认不能串 SQL；百万行结果只请求有界页，内存不加载全表；close tab 后 result session 释放。
  - **调用 `grill-me [database]`**，并复核提取后的 root patch owner；不能因编译过就忽略 fork 解析差异。

### Task P7-05 — notebook/quick notes 原生读写与内容安全

- **Input**：`src-tauri/src/notebook/` 的 state/fs_ops/attachments/fields/links/mentions/tags/tag_rename/tasks/trash/snapshots/user_templates/export/import/vault_*；现有 notebook 组件及测试。
- **Action**：
  1. 新建 core `notebook/` 并保留对应业务文件结构；提取的纯 Rust parser/tests 原样移动，去除 command 宏和 UI state 查找，不重写已经稳定的导入格式。
  2. 新建 `AeroricUI/Notebook/{NotebookSidebar,NoteDocumentView,NoteMetadataEditor,BacklinksPanel,TagBrowser,NoteTaskInbox,TemplatePicker,AttachmentView,TrashView,ImportExportWizard}.swift`。
  3. 与文件编辑器复用 document/Markdown 基础组件；noteId/path/attachment identity 保留，不依赖 display title 当主键。
  4. 支持现有导入格式集合；zip 防 traversal/bomb、HTML sanitize、附件白名单、冲突重命名/中断报告沿用，不能调用系统打开未受信任脚本。
  5. note delete/restore/template/task checkbox 等调用 core command；多窗口 expectedRevision 冲突提示，不静默覆盖笔记。
- **Acceptance Criteria**：
  - 新建 `tests/notebook_contract.rs`、`NotebookStoreTests.swift`；`pnpm native:check notebook`，同时运行搬移后 import/archive 的既有测试。
  - Markdown/富文本往返、图片附件、重名标签、反向链接、任务复选框、trash restore、导入失败部分报告全部有样本。
  - ZIP `../`、symlink、超大解压不写出测试 vault；旧笔记与模板内容 hash 未被无关格式化改变。
  - **调用 `grill-me [notebook]`**。

### Task P7-06 — 笔记同步、RAG 与存储后端

- **Input**：`notebook/{sync,rag}/`；`storage_backend*.rs`、`storage_conn.rs`、`storage_oauth.rs`、`secrets.rs`、`app_settings/schema.rs` embedding 设置。
- **Action**：
  1. 新建 core `notebook/{sync,rag}/` 对应文件、`storage_backends/` 的 adapter；保留 SQLite vec extension 加载与 secrets 的平台后端，不为了签名启用不必要的 hardened runtime。
  2. 新建 `AeroricUI/Notebook/{SyncStatusPanel,SyncConflictSheet,RAGSearchView,EmbeddingSettingsView,StorageConnectionEditor}.swift`。
  3. index job 与 UI 生命周期分离；重建/取消可观察、成本与外网发送内容明确；默认不因打开笔记自动把全文发给 embedding provider。
  4. 同步保留 manifest/diff/conflict 规则与调度；OAuth 由既有凭据 owner 完成，native UI 不另起 refresh loop。
  5. Keychain 错误可诊断但不得把 key 降级为明文；不支持持久钥匙串的平台保留现有明确限制说明。
- **Acceptance Criteria**：
  - 新建 `tests/{notebook_sync,rag_pipeline,storage_backends}.rs`、`SyncStoreTests.swift`；`pnpm native:check notebook-services`。
  - fake storage/embedding 验证断线、冲突、quota/401、取消、重复 index 不重复计费；tmp SQLite 能实际加载 extension 并做向量查询。
  - UI 切模式/关窗后 scheduler 不重复创建，凭据不会出现在事件快照。
  - **调用 `grill-me [notebook-services]`**；Phase 7 Gate 需要 F11–F16 全部映射闭合。

## Phase 8 — 扩展管理、远程访问、运维与生产数据接入

**入口**：所有旧业务 service 已抽出；新 host 仍只用隔离数据。

**出口**：无 GUI 时远程与后台功能也工作；macOS Tauri/SwiftUI/CLI 连接同一 host，可以在备份与同意后接入用户数据。

### Task P8-01 — Skill Hub、MCP、hook 与插件配置

- **Input**：`src-tauri/src/{skills,mcp,hooks,dsh_plugins}.rs`、`src/components/app-settings/{SkillsPanel,McpPanel,HooksPanel,DshPluginsPanel}.tsx`；minimax 插件 enable/commit/rollback 参考。
- **Action**：
  1. 新建 core `extensions/{mod,skills,mcp,hooks,plugins,trust}.rs`，把目录查询、配置同步、启停/安装拆成只读与 mutation；复用已有实现和托管 home。
  2. 新建 `AeroricUI/Extensions/{SkillHubView,SkillDetail,MCPSettingsView,HookStatusView,PluginManagerView,ExtensionTrustSheet}.swift`。
  3. 扫描文件不等于执行扩展；安装前显示来源/revision、写入范围、所需权限；第三方脚本运行必须用户确认，不在 Swift app 内直接加载不受信任动态插件。
  4. MCP/hook 配置校验成功再原子替换；失败回滚；运行中 Agent 需下次启动才能生效时明确显示，不能显示“已生效”。
  5. OMP hook `.js` 资源、DSH plugin 版本与四族 config 路径各自测试；不强行所有 Agent 共用一个上游配置格式。
- **Acceptance Criteria**：
  - 新建 `tests/extensions.rs`、`ExtensionStoreTests.swift`；`pnpm native:check extensions`。
  - 恶意路径/压缩包/无效 MCP command/env secret mask 覆盖；安装失败不留下半配置；禁用扩展不会删除用户原始目录。
  - **调用 `grill-me [extensions]`**。

### Task P8-02 — provider、proxy、local router 与凭据边界

- **Input**：`src-tauri/src/app_settings/` 全部配置/模型/版本模块、`local_router/`、`local_router_commands.rs`、`agent_ops.rs`、`agent_tools/`、`node_runtime.rs`。
- **Action**：
  1. 新建 core `settings/`、`providers/`、`local_router/`、`operations/` 的对应模块；保留 local router cache/stream/transforms/circuit breaker/usage 的既有测试。
  2. 新建 `AeroricUI/Settings/{ProviderSettingsView,ProxySettingsView,LocalRouterSettingsView,AgentOperationPanel,ConfigImportExportSheet}.swift`。
  3. provider catalog 与已安装 Agent 支持列表区分；base URL 校验、代理排除/启用规则和自定义 agent policy 不在 Swift 复制一套。
  4. 凭据导出默认脱敏；明确“包含 secret”需二次确认及私有文件权限；禁止放入 feature matrix/review 证据。
  5. router listener 地址/认证沿用安全默认，不为 native 通信开启公开 HTTP 端口；OAuth 回调/代理错误有结构化诊断。
- **Acceptance Criteria**：
  - 新建 `tests/settings_services.rs`、`ProviderSettingsTests.swift`；`pnpm native:check provider-services`，迁移前后相同协议 fixture 的路由结果相同。
  - fake HTTP server 验证 SSE 分片、401/429/5xx、取消、circuit breaker、proxy 开关与 secret redaction；不向真实 provider 发送测试账单。
  - **调用 `grill-me [provider-services]`**。

### Task P8-03 — 远程/mobile 彻底脱离 React 任务请求中转

- **Input**：`src-tauri/src/remote/` 全部模块；`packages/remote-contracts`；`remote-protocol`、`remote-relay`；`src/state/app/useRemoteTaskRequests.ts`、mobile 现有协议测试。
- **Action**：
  1. 新建 core `remote/` 对应 auth/crypto/rpc/server/event_log/terminal/session_push 等模块；将 `tasks_rpc` / `files_rpc` 直接接 core service，不再 emit 到 React 后等待应答。
  2. `remote-protocol` 保持 relay v1，业务 RPC v2 fallback/v3 envelopes 继续，capabilities 只公布有安全 dispatcher 的方法；本地 host methods 不自动全开放到 remote。
  3. 同一 terminal lease owner/审批 registry 服务本地与远端；多设备抢控制权明确确认，失效设备不能继续输入。
  4. 新建 `AeroricUI/Remote/{RemoteAccessSettingsView,PairingView,DeviceListView,RemoteAuditView}.swift`，配对/撤销/relay status 等复用现有 core；二维码和 token 不进屏幕录制测试日志。
  5. 更新现有 remote golden fixtures 仅在确需新增字段时；字段可选、对旧 peer 行为一致。`mobile/` 只改契约必要部分，经其 pnpm 测试。
- **Acceptance Criteria**：
  - 新建 `tests/remote_headless.rs`，迁移原 `auth_stress_tests` / `lan_roundtrip_tests`；`pnpm native:check remote`。
  - 关闭全部 GUI、只跑 `aeroric-cli serve` 时，fake 配对客户端可 list/create/cancel task；v2/v3、Orca 兼容、重连、权限拒绝、加密帧反例都通过。
  - `pnpm --dir mobile typecheck`、`pnpm --dir mobile test`；`cargo test --manifest-path remote-protocol/Cargo.toml --locked` 与 relay tests 通过。
  - **调用 `grill-me [remote]`**；headless 仍依赖 `remote-task-request` 的实现不准通过。

### Task P8-04 — 统计、通知、清理、周报、权限与更新

- **Input**：`src-tauri/src/{analytics,agent_usage,usage_index,report,notification,startup_diagnostics}.rs`、`permissions/`；`src/taskCleanup.ts`、`weeklyReport.ts`、Release 页面。
- **Action**：
  1. 新建 core `maintenance/{mod,cleanup,reports,notifications,updates,diagnostics}.rs`；把 timer/policy 移出 React，保留用户 opt-in 与时区/频率。
  2. 新建 `AeroricUI/Operations/{UsageDashboard,NotificationCenterView,CleanupSettingsView,WeeklyReportView,PermissionSettingsView,ReleaseView,DiagnosticsView}.swift`。
  3. 任务后台通知由 host 发抽象 event；需要 NSUserNotification/打开系统设置/请求 TCC 的动作由拥有当前 app identity 的 **GUI platform adapter** 执行，不能认为 daemon 的授权等于 app 的授权。
  4. native permission report 使用实际 `com.aeroric.native.dev` 身份，保留 unstableSignature / notBundled 解释；Re-authorize 前说明 reset 的服务、bundle ID、后续重新请求动作，让用户确认。
  5. 更新准备/下载/校验与实际替换分开；本计划实施期间 preview app 不覆盖旧 Aeroric。清理只处理满足策略的任务，不删运行中/等待输入任务或无归属目录。
- **Acceptance Criteria**：
  - 新建 `tests/maintenance.rs`、`PermissionPresentationTests.swift`、`MaintenanceStoreTests.swift`；`pnpm native:check maintenance`。
  - fake clock 验证 cleanup/report 时区、重启幂等、失败重试；无需真实通知权限即可测 UI 状态，真实 TCC 单独手测记录。
  - ad-hoc 重签后界面不虚假显示永久授权；没有在后台静默 `tccutil reset`、提权或改系统权限。
  - **调用 `grill-me [maintenance]`**。

### Task P8-05 — 切换同一宿主与生产数据迁移演练

- **Input**：P1 command-map；P2–P8 所有已验收服务；`src-tauri/src/{core_adapter,lib}.rs`、`src/lib/api/`；`src-tauri/tauri.conf.json`、`.github/workflows/release-desktop.yml`；P4 本地签名脚本；`docs/native/rollback.md`；W7。
- **Action**：
  1. 新建 `src-tauri/src/host_client.rs`、`host_event_bridge.rs`、`host_supervisor.rs`，实现 macOS external gateway；所有 domain commands 均转 host，窗口/dialog/clipboard/permission UI 留本地。Windows/Linux embedded 路径保留。
  2. 在 `core_adapter.rs` 中明确 `Embedded | External` 互斥选择；同进程不双启动 watcher、remote server、notebook scheduler、local router。失败不能偷偷落到另一个 writer。
  3. `src/state/app` 删除已迁移的权威 mutation/timer 接线，但保留 UI 操作入口；`get_active_task_ids`/snapshot 从 host 来，而不是空的旧 TaskManager。
  4. 新建 `scripts/native/migration-rehearsal.mjs`、`docs/native/migration-rehearsal.md`：在 synthetic/用户批准的脱敏复制数据上做 old reader→new host→old UI/client 兼容演练、异常写入恢复和锁竞争。
  5. 用户同意后才关旧写入进程、备份真实目录、校验备份、启动 host；备份含配置/任务/notes DB 及 WAL/必要附件，以停写/DB backup API 保证一致性，不直接 cp 正在写的 DB。
  6. macOS Tauri、SwiftUI、CLI 各连接同一 host；产品升级不在运行任务中自动替换 host binary。版本不兼容显示需协调重启，不能 kill 并重复 spawn。
  7. **启用 external 的前置条件是 Tauri 自带可启动 helper，不允许依赖 Native Preview 已安装。** 新建 `src-tauri/tauri.macos.conf.json`，用 macOS 专属 `bundle.externalBin` 声明 `binaries/aeroric-cli`；新建 `scripts/native/prepare-tauri-host.mjs`，按明确 target triple 构建并复制为 `src-tauri/binaries/aeroric-cli-<target-triple>`，检查架构/build/protocol 与主程序一致。该生成目录加入 `.gitignore`；不提交机器二进制。
  8. 修改 `package.json` 增加 `native:prepare-tauri-host`，在 `src-tauri/tauri.conf.json` 的 beforeDev/beforeBuild hook 接到原 pnpm 动作之前；不替换 React/Vite 构建，也不改 macOS ad-hoc/hardenedRuntime 基线。脚本读取显式 CI target 或已验证的 Tauri target 环境，不把 runner CPU 当目标 CPU；Windows/Linux 未启用 external 时明确不生成此 helper。修改 `.github/workflows/release-desktop.yml` 的 macOS 构建准备步骤传入矩阵 target，仍保留八产物、两种签名路径及原安装包命名；这些是可构建性修改，不是发布授权。
  9. `TauriHostSupervisor` 从自己的可信 bundle `Contents/MacOS/aeroric-cli` 启动，不搜 PATH、不寻找 Native Preview 的目录；开发模式只用已校验的 target-specific 生成路径。先尝试认证连接，再按锁/ready 协议竞争启动；竞争失败者连接胜者，不成为第二 writer。CLI 和 Swift 使用同一仲裁规则；发现不兼容存活 host 返回需协调升级，不偷偷回退 embedded。
  10. 验证 Tauri bundler 对 helper 的嵌套签名/资源封印；本地保持 ad-hoc，已有 Developer ID CI 则沿原有身份和 hardenedRuntime overlay 签名，**不得把发布 helper 强行重签为 ad-hoc**。打包检查复用 P4 的验签逻辑，但按实际构建身份断言。
- **Acceptance Criteria**：
  - 新建 `tests/host_cutover.rs`、`src-tauri/src/host_supervisor_tests.rs`、TS `src/test/host-owner.test.ts`、`scripts/native/tauri-host-smoke.mjs`；`pnpm native:check cutover` 同时运行 core 与 Tauri 端测例。
  - 三客户端同时更改不同任务不丢更新；同 revision 冲突被拒；同 idempotency key 不重复 spawn；关闭 SwiftUI 后 CLI 和 mobile 仍可操作。
  - `pnpm native:prepare-tauri-host`、`pnpm tauri build --bundles app`、`node scripts/native/tauri-host-smoke.mjs`：在隔离 data-dir、repo 外 cwd、无存活 host 且**只装 Tauri、未装 Native Preview**的场景冷启动成功。helper 缺失/错架构/版本不兼容清楚报错，不启动旁路 writer；签名有效。
  - Tauri/native/CLI 三方同时冷启动，最终只有一个锁持有者、同一 epoch 和一套 watcher；失败竞争者被回收，没有多余 daemon。两种 macOS CPU 架构的打包证据分别记录。
  - 命令映射覆盖率 100%；无 `AppHandle` / React 回调成为 domain 执行必要条件；未支持的平台能力有明确 reason。
  - 只用测试数据成功演练回退到 Tauri，未恢复旧进程的陈旧全量内存 snapshot；不得用 `git reset --hard` 或覆盖用户新数据当“回滚”。
  - **调用 `grill-me [cutover]`**；通过后才允许 Native Preview 指向生产数据目录。

## Phase 9 — 吸收参考项目的高级能力，不另造 Agent 内核

**入口**：旧 Aeroric 的完整业务迁移 Gate 已闭合。

**出口**：高级能力有实际 adapter、原生 UI 与反例测试；不能把 README 中的功能词当成已支持接口。

### 9.1 补充定向检视结果

> **【核验 @ 2026-09-21】** 下表是**计划早前评审继承下来的结论**，本次迁移并未亲自复核过——而 §1.1 明确要求每次研究落盘"仓库 HEAD、文件、符号、行为、采用点、**未验证项**"并记录工作区是否脏。该要求此前未满足，现已补齐，逐条判定与 `file:line` 证据在 **`docs/native/reference-provenance.md` §7**（跟踪副本；`.trellis/…/research/reference-map.md` 只在本机存在）。
>
> 五个参考仓此刻**全部干净**、HEAD 与本表一致（`opencode` 在 **`dev`** 分支，已如实记录而非默认 main）。表内 16 个被引文件均存在。核验结果分三档：
>
> - **已验证 4 条**，可立即作为设计约束使用：minimax ACP `promptCapabilities` 的 image/audio/embeddedContext **均为 false**（`agent.ts:447-450`）；sandbox **fail closed**（`local-sandbox-service.ts:339,381,385,393` 与 `backend/selector.ts:29,37,52` 全部 `throw SandboxError('SANDBOX_UNAVAILABLE'…)`）；tools lease 确实带 audience/scopes/generation（`lease-broker.ts:136-137,239-240`）；OAuth 确有跨进程锁与 generation（`auth-core.ts:11,154,166,130-145`）；另 plugin-system 的 prepare/commit/rollback 亦证实（`:88,91,94,96`）。
> - **部分验证 2 条**，P9-02 实施前必须自己再确认，不得当既成事实引用：MiMo goal 只看到 `Map<string, Goal>` 内存 store（`goal.ts:113,119`），**"仅内存"未证实**（`:163` 提到 persisted-parts）；checkpoint 只看到 writer 与 watermark（`checkpoint.ts:468,453,204,546`），**"成功才推进水位"那一行未找到**。
> - **其余各行未复核**，因此不能作为已坐实的约束使用；按 §1.1 一次一个文件在 Phase 9 现场重derive。
>
> 直接影响：P9-04 的 `tests/sandbox_fail_closed.rs` 与"ACP 不支持音频时拒绝而非丢附件"两条验收，现在建立在读过的上游代码上而不是转述上；P9-02 的两条前提改为待确认项，避免把未证实的上游语义写进 native UI 承诺。参考仓仍严格只读，未写入、未运行其任何安装脚本。

以下是前述浅目录检视后进一步跟到的具体文件。只读检查，不代表它们的内部 API 可直接被外部客户端调用：

| 参考实现文件 | 确认的边界；实施时不能错认 |
|---|---|
| `../MiMo-Code/packages/opencode/src/session/checkpoint.ts::tryStartCheckpointWriter` | 上下文 checkpoint 有独立 writer，成功才推进水位；不是 Git 文件回滚点。 |
| `../MiMo-Code/packages/opencode/src/memory/service.ts::{reconcile,search}` | 有 scope 和索引策略，不能默认读取另一 Agent 的私人记忆。 |
| `../MiMo-Code/packages/opencode/src/task/registry.ts::{create,start,done}` | T1/T1.1 层级属于外部 session，不等于 Aeroric 顶层 task。 |
| `../MiMo-Code/packages/opencode/src/actor/spawn.ts` | 子 Agent 并行不自动等于独立 worktree/文件隔离；共享目录需要冲突政策。 |
| `../MiMo-Code/packages/opencode/src/session/goal.ts::{set,evaluate}` | 目标状态有内存所有者；不能因此承诺上游跨进程重启恢复。 |
| `../MiMo-Code/packages/opencode/src/workflow/{runtime.ts,builtin/compose.js}` | 工作流可能提交、合并、移除 worktree；journal replay 不保证所有副作用 exactly-once。 |
| `../MiMo-Code/packages/opencode/src/session/auto-dream.ts` | 自动 memory 整理需显式启用；prompt 写“只读”不能替代执行沙箱。 |
| `../MiMo-Code/packages/opencode/src/cli/cmd/tui/util/voice.ts` | 参考录音依赖外部工具；ASR payload/provider 有专属约束，不能只套任意 OpenAI-compatible URL。 |
| `../minimax-code/packages/tui/src/acp/agent.ts::createTuiAcpAgent` | adapter 明确协商 session 能力；被检视版本没有宣告 image/audio/embeddedContext，不能 native UI 强行传。 |
| `../minimax-code/packages/local-runtime-v2/src/service/plugin-system/plugin-system.ts` | plugin enable/import 有 prepare/commit/rollback，不只是 toggling 一个布尔值。 |
| `../minimax-code/packages/mcode-tools-host/src/{integration,lease-broker}.ts` | 专用 tools host 与 credential lease，有 audience/scopes/generation，不是 Aeroric 可任意复用的通用代理服务。 |
| `../minimax-code/packages/local-runtime-v2/src/service/sandbox/{local-sandbox-service.ts,backend/selector.ts}` | 隔离策略不能执行时失败，不静默降级成无沙箱。 |
| `../minimax-code/packages/oauth-core/src/auth-core.ts` | OAuth refresh 存在跨进程所有权和 generation；原生 UI 不另建竞争的刷新逻辑。 |

### Task P9-01 — 通用外部能力适配、plan/compact/fork/revert/share

- **Input**：opencode `core/src/session.ts`；ZCode `execution-state.ts`；minimax hosted session/control；P2 capabilities 和 P3 adapters。
- **Action**：
  1. 新建 core `adapters/{mod,registry,capability_probe,session_controls,structured_events}.rs` 和 `docs/native/adapter-contract.md`，统一 `ExternalAgentAdapter`：probe/start/attach/history/submit/interrupt/control。
  2. `AgentCapabilities` 分开 `nativePrompt`、`terminalInput`、`permissionRespond`、`questionsRespond`、`planMode`、`compact`、`fork`、`revert`、`share`、`usage`、`attachments`；每项含支持范围/限制与探测版本。
  3. 新建 `AeroricUI/Sessions/{SessionControlBar,PlanModeControl,ContextMeter,CompactSheet,ForkSessionSheet,RevertPreviewSheet,ShareExportSheet}.swift`。
  4. permissionMode 与 planEnabled 分字段并在请求入队时固定；不得队列消费时按当前 UI 设置重新解释。revert 若影响工作区，先 diff preview/确认；session revert 与 git revert 分开。
  5. 分享默认本地脱敏导出；公开分享/上传需要明确 endpoint 和用户确认，不因点复制链接隐式上传私有会话。
  6. 对尚无对外稳定接口的参考能力，明确 `unsupported` 与原因；在矩阵列为 blocked/pending，不实现抓 TUI 文本的伪 adapter。需要上游变化则新开独立任务，不修改参考仓库完成本任务。
- **Acceptance Criteria**：
  - 新建 `tests/adapter_contract.rs`、`SessionCapabilityTests.swift`；`pnpm native:check session-controls`。
  - 具备能力的 fake adapter 完成 request→external response→snapshot→UI 闭环；缺 capability 的控件隐藏/禁用，server 同样拒绝调用。
  - “支持”必须提供已验证的安装版本与端到端结果；mock 通过只能证明 Aeroric 适配代码，不能证明真实上游可用。
  - **调用 `grill-me [session-controls]`**；F20 未全部有可用实现或用户明确接受的边界时不能宣称“全面迁移完成”。

### Task P9-02 — memory/checkpoint、树形任务、goal 与 subagents

- **Input**：§9.1 前五行；现有 DSH goal/jobs/subagent；`src/sessionHandoff.ts`；P9 adapter contract。
- **Action**：
  1. 新建 core `adapters/{memory,checkpoint,task_tree,goal,subagents}.rs`；模型字段包含 externalSessionId/externalItemId/source/revision/persistenceKind。
  2. 新建 `AeroricUI/AgentTools/{MemoryBrowser,CheckpointPanel,AgentTaskTree,GoalEditor,SubagentTree,BackgroundJobsView}.swift`。
  3. checkpoint 展示创建状态/覆盖的 message cursor/来源；失败不能清历史。memory 搜索只在显式授权 scope；“纳入另一 Agent 记忆”需用户选择，不自动扫描。
  4. 外部 task tree 节点键 = adapter+session+nodeId，不能只用 T1；保持 done/abandoned 不被旧 start 事件复活。
  5. goal 显示上游持久化能力；仅内存目标在重启后标记未恢复，由用户确认重设，不静默自动续跑。
  6. subagent 展示父子关系、cwd、隔离级别、状态/成本/取消；取消父任务是否级联按上游 capability 明示；Aeroric 不自行重复 spawn 上游已创建的子代理。
- **Acceptance Criteria**：
  - 新建 `tests/agent_work_graph.rs`、`AgentWorkGraphTests.swift`；`pnpm native:check agent-work-graph`。
  - 两 session 均含 T1 时不串状态；out-of-order/duplicate updates 不复活终态；checkpoint writer failure 与 goal 重启反例通过。
  - 并发子代理共享 cwd 时显示警告；不能声称“默认隔离安全”；每个已 advertised 的真实 adapter 有一次受控 E2E。
  - **调用 `grill-me [agent-work-graph]`**。

### Task P9-03 — compose/workflow、重试边界与 Dream/Distill

- **Input**：§9.1 workflow/auto-dream；Trellis main-only 与本仓 worktree policy；P2 idempotency。
- **Action**：
  1. 新建 core `adapters/{workflow,workflow_journal,memory_maintenance}.rs`，以外部工作流为执行 owner；不要把其脚本直接 `eval` 在 Swift 或未经隔离的 host 上。
  2. 新建 `AeroricUI/AgentTools/{WorkflowRunView,WorkflowStageRow,WorkflowResumeSheet,MemoryMaintenanceSettingsView,GeneratedAssetReview}.swift`。
  3. UI 展示阶段 DAG、尝试次数、输入/输出引用、阻塞原因；bounded retry 写上限，副作用步骤（commit/push/delete/worktree）不自动重试，结果不明先 reconcile。
  4. 恢复记录 script hash/adapter version/input hash；不匹配禁止继续旧 journal，允许明确新 run；“继续”不等于从第一步重跑。
  5. 用于 Aeroric 仓库时禁止 workflow 自动建 feature branch/push；纯设计参考不搬入 MiMo compose 的默认提交合并政策。
  6. Dream/Distill 默认关闭，显示将读取/写入范围及调用成本；生成 skill/workflow/artifact 先放 staging，用户 review 后启用，不能自动执行生成脚本。
- **Acceptance Criteria**：
  - 新建 `tests/workflow_safety.rs`、`WorkflowStoreTests.swift`；`pnpm native:check workflows`。
  - fake workflow 在每个阶段 crash/resume，读步骤可重放、mutation 不重复；脚本变更使旧 journal 无效；重试达到上限为 blocked。
  - main-only policy 反例拦截 branch/push；Dream “只读提示词”不被记录为沙箱证明。
  - **调用 `grill-me [workflows]`**。

### Task P9-04 — ACP、OAuth、sandbox、browser/多模态与 voice

- **Input**：§9.1 minimax ACP/OAuth/tools host/sandbox、MiMo voice；P8 extensions/providers；Apple 录音权限说明需在实现前核对。
- **Action**：
  1. 新建 core `adapters/{acp,oauth_status,sandbox_status,tool_capabilities,media,voice}.rs`、`contracts/native/v1/fixtures/external-capabilities.json`。
  2. ACP adapter 单独版本协商/事件正规化；限制 cwd 与 extra dirs，image/audio 若上游不支持则拒绝；不能把 ACP 选项注入 OMP。
  3. 新建 `AeroricUI/AgentTools/{AuthenticationView,SandboxStatusView,ToolActivityView,MediaAttachmentView,VoiceInputButton,VoicePermissionSheet}.swift`。
  4. OAuth UI 只引导系统浏览器/device code 与显示状态；access/refresh token 仍由既有外部 owner 管，不能导出给页面 JavaScript。tools lease 只拿最小 scope，不能把上游专用 lease broker 冒充通用实现。
  5. sandbox status 区分 requested/effective/backend/unsupported；启用却未准备好必须 fail closed；UI toggle 成功不等于实际工具执行已隔离。
  6. browser/computer/media 工具由外部 Agent 执行；native 展示工具请求、权限、进度/结果，不默默开启系统 Accessibility/Screen Recording。
  7. voice 为显式可选能力：AppKit/AVFoundation 录音放 platform adapter，按需申请 microphone；core 转录 provider adapter 有受支持 payload 契约、取消、大小限制、privacy disclosure；transcript 先进入草稿，用户提交才发 Agent。
  8. 若引入新的 built-in family，必须单独批准，并扩展 `AgentFamily`、BuiltInAgentType、AGENT_OPTIONS/groupAgentOptions、AgentToolId、设置页、usage 列表与所有 exhaustive matches。本 Task 默认只提供 capability adapter/custom 配置，不偷加第五族。
- **Acceptance Criteria**：
  - 新建 `tests/{acp_contract,sandbox_fail_closed,oauth_owner,media_limits}.rs`、Swift `ExternalCapabilityTests.swift`、`VoiceDraftTests.swift`；`pnpm native:check external-protocols`。
  - ACP 不支持音频时拒绝而非丢附件；过期 OAuth generation 不覆盖新登录；无有效 sandbox backend 不运行工具；mic denied/取消/零音频/网络失败不发空 prompt。
  - voice/媒体集成必须在用户选择 provider 并批准网络调用后单独验证；此前矩阵状态是“adapter tested，真实集成未验证”，不能写完成。
  - **调用 `grill-me [external-protocols]`**；Phase 9 未兑现项逐项挂阻塞任务，不从总功能清单删除。

## Phase 10 — 全量 Review、故障注入、本地签名与交付

**入口**：所有必选功能完成或有用户逐项批准且明确标注的支持边界。

**出口**：本地无账号构建/签名/运行可复现、无头与 GUI 等价、可回退；发布仍需另外授权。

### Task P10-01 — 端到端矩阵、压力与故障注入

- **Input**：全部 feature-matrix/command-map/reviews、旧前端/后端测试、P1 baseline。
- **Action**：
  1. 新建 `scripts/native/{stress,parity,fault-injection}.mjs`、`docs/native/acceptance-matrix.md`；将每个 Fxx 展开为“CLI / Tauri / native / unsupported reason / 测试证据”。
  2. 新建 core `tests/end_to_end.rs`、`macos/AeroricUITests/{HeadlessParityTests,MultiWindowTests,RecoveryTests}.swift`；fake host 启动参数只允许测试 scheme，Release 禁用测试 bypass。
  3. 故障注入点：persist 前/后、spawn 前/后、session attach 前/后、snapshot/subscription 边界、approve response 丢失、history compaction、host SIGTERM/SIGKILL、客户端突然退出、锁/磁盘写失败。
  4. 压测固定数据量与机器：20 个 fake tasks、每个 5 MiB 输出、3 客户端、10 次重连、100 次模式切换、10,000 条消息、100,000 文件索引样本。记录 p50/p95、CPU、RSS、FD/thread/child 数。
  5. 初始验收预算：稳定热切换 p95 <150ms、100次切换后的 renderer/subscriber/子进程数回到基线；PTY replay 单连接队列≤4 MiB；业务 replay≤8 MiB；离线打开 shell 不等待模型网络。预算若需调整必须有基准数据及评审，不静默改阈值。
  6. 在 `checks.json` 审计各 Task 已登记的必选 case 名与最少 test count，新增本 Task 的 `system-parity` 映射；拒绝 blanket skip、仅字符串 grep 证明行为、mock 调用不验证后果。P10-02/03/04 新增的 a11y、完整签名和发布检查还未实现，本步不提前要求最终 `all` 成功。
- **Acceptance Criteria**：
  - `pnpm native:check system-parity`、`pnpm native:ui-test`、`node scripts/native/stress.mjs`、`node scripts/native/parity.mjs`、`node scripts/native/fault-injection.mjs` 通过；只验本 Task 与 P1–P9 已落地的测试。完整 `pnpm native:check all` 放 P10-05，不能提前删除后续必选项或将 skip 算通过。
  - `pnpm lint`、`pnpm format:check`、`pnpm typecheck`、`pnpm test`、`pnpm build`；core/src-tauri/remote crate 的 fmt、clippy、tests 按 CI 同等参数通过。
  - 确认 status/revision/原始字节/保存数据相同，而非只比较两端 UI 标签；任何数据丢失、重复副作用、unauthorized call 为 P0，不允许带病进入交付。
  - **调用 `grill-me [system-parity]`**，不能用此总审代替之前每模块 gate。

### Task P10-02 — 原生视觉、可访问性与交互 QA

- **Input**：primuse `Docs/MacDesignAudit.md`、P4 material/selection、P5 terminal/mode、P10 矩阵。
- **Action**：
  1. 新建 `docs/native/visual-qa.md`、`macos/AeroricUITests/{AccessibilityTests,AppearanceTests,KeyboardTests}.swift`；所有截图只用 synthetic data。
  2. 执行 light/dark/system × glass/classic × reduce-transparency on/off × active/inactive；最低支持 macOS 与当前开发系统分别测，availability 分支不能仅靠单机当前 OS 验证。
  3. 检查 VoiceOver label/value/selected/disabled、tab 顺序、焦点恢复、键盘菜单、Cmd+W/Cmd+Q、ESC、全屏/多显示器；相邻选项不能只用颜色表状态。
  4. 在 1000×640、常用 1440×900、Retina/非 Retina 尺寸验证；长中文/英文/错误文案不遮按钮，终端/编辑器文字可选且可读。
  5. 记录真实 blur 和滚动/窗口性能，解决每 row 一层 NSVisualEffectView、透明层重叠、失活窗口持续动画等问题；不用截图 PNG 覆盖 UI 作“修复”。
- **Acceptance Criteria**：
  - `pnpm native:check accessibility`、`pnpm native:ui-test`；人工 VoiceOver 与材料测试写明 OS/动作/结果。
  - 所有 destructive 操作能被键盘取消；模式 Toggle 可被读出，disabled capability 有可访问说明。
  - **调用 `grill-me [native-qa]`**；视觉更像 primuse 不是豁免 a11y/系统行为的理由。

### Task P10-03 — 本地 ad-hoc 打包、验签与 TCC 说明

- **Input**：`AGENTS.md` signing 说明、`src-tauri/tauri.conf.json`、P4 Xcode config 与已能运行的 build/sign/UI-test 脚本、P8 Tauri helper 冷启动证据、core host、官方 codesign 文档。
- **Action**：
  1. **强化而非首次实现** P4 已接通的 `scripts/native/build.mjs`、`scripts/native/{sign-local,verify-bundle}.sh`，新建 `docs/native/local-build.md` 与打包故障测例；加入 ReleaseLocal、clean build、资源篡改、架构不符和嵌套库签名覆盖。shell 均 `set -euo pipefail`，路径带引号，失败立即停止。
  2. build 串行执行 `cargo build --manifest-path core/Cargo.toml -p aeroric-cli --locked`（ReleaseLocal 加 `--release`）、Swift package resolve、XcodeGen、`xcodebuild`；把**本架构**的 host 复制到 app `Contents/MacOS/aeroric-cli`，不要沿用上次构建的旧二进制。
  3. artifact manifest 记录 CPU arch、Rust/Swift/Xcode 版本、core/app version、protocol/schema version、host hash；用 `file`/`lipo -archs` 验证 app 与 host 架构兼容。
  4. 从内到外显式签名嵌入的 Mach-O helper / dylib / framework，再签 app。使用 `--sign -`、`--timestamp=none`；**不以 `codesign --deep --sign` 代替正确嵌套顺序**，不加 hardened runtime/notarization/Apple Team。
  5. `verify-bundle.sh` 校验 host 不是指向仓库产物的 symlink，Info.plist/bundle version/签名封印存在，签名后不再改资源。framework 若有 nested code，按实际 bundle 列表分别签而非盲签所有文件。
  6. 测试 runner 构建与 app 分开：UI tests 可用本地 ad-hoc identity；不要因为 Xcode test runner 需要签名就在 release app 全局关闭验签。不使用 `-allowProvisioningUpdates` 请求在线账户。
  7. 新建 `scripts/native/bundle-tests.mjs` 并登记 `native:check signing-local`；在临时 bundle 副本上篡改资源/替换 helper，验证验签和 hash 检查确实失败，不破坏源产物。Tauri 本地包与 Native Preview 都有“单独安装可冷启动”的证据。
- **Acceptance Criteria**：
  - 以下是**生成 app 后**的真实验签命令；`APP` 路径由 build script 固定输出，示例使用 Debug：
    ```bash
    APP=".artifacts/native/DerivedData/Build/Products/Debug/AeroricNative.app"
    test -x "$APP/Contents/MacOS/aeroric-cli"
    codesign --force --sign - --timestamp=none "$APP/Contents/MacOS/aeroric-cli"
    # 若存在其他嵌套 dylib/framework，先按 sign-local.sh 清单完成签名再签 app。
    codesign --force --sign - --timestamp=none "$APP"
    codesign --verify --deep --strict --verbose=2 "$APP"
    codesign -dv --verbose=4 "$APP" 2>&1
    codesign -d -r- "$APP" 2>&1
    plutil -lint "$APP/Contents/Info.plist"
    shasum -a 256 "$APP/Contents/MacOS/aeroric-cli"
    open "$APP"
    ```
  - 显示 `Signature=adhoc`，资源/Info.plist 验证成功；flags 不含 runtime，TeamIdentifier 不依赖付费 Team；用户未登录 Developer 账号也可从**本地产物**正常启动。
  - 从 repo 外部工作目录启动 app 也能找到 helper/resources；使用独立临时 data-dir 完成 CLI/GUI task smoke。
  - `spctl --assess` 可作为诊断，**不是 ad-hoc 本地构建的通过条件**；不声称 ad-hoc 可公证或可无警告公网分发，不要求用户关闭 Gatekeeper。
  - `pnpm native:check signing-local` 执行正常包与故障包测例；重新运行 P8 的 Tauri-only 冷启动 smoke，不将 Native Preview 的 helper 恰好存在当作 Tauri 能独立交付。
  - 文档解释 ad-hoc 的 cdhash 变化可能要求重授 TCC；用户主动 Re-authorize，preview bundle ID 不误 reset `com.aeroric.desktop`。CLI/daemon 与 app 的权限归属分别手测。
  - **调用 `grill-me [signing-local]`**；同时检查原 `tauri.conf.json` baseline 未变。

### Task P10-04 — 兼容发布流程与安装产物检查

- **Input**：`.github/workflows/{checks,release-desktop}.yml`、`scripts/{check-version.mjs,create-macos-dmg.sh}`、P10-03、AGENTS Release assets。
- **Action**：
  1. 新建 `.github/workflows/native-macos-checks.yml`，仅做 compile/test/ad-hoc artifact；复用已锁定的 actions/security 政策，不用不明第三方 action。审计 P8 对现有 release workflow 的 helper 准备步骤，保留旧八产物矩阵、名字、签名分支；“保留流程”不等于省略 external 所必需的打包依赖。
  2. 修改 `scripts/check-version.mjs` 增加 core CLI 与 native Info/build version 一致性；版本来源仍统一，不手改五处产生分叉。
  3. 新建 `docs/native/release-decision.md`：preview 独立名字/ID/DMG；是否把 native 作为 macOS 正式包、何时更换 bundle ID/保留旧 TCC 记录，由用户在验收后决定，不能随迁移偷偷替换。
  4. 如用户另行授权发布，才修改 `release-desktop.yml` 添加明确 native artifacts；默认不冒占原 Tauri DMG 名称。正式 release 仍保留以下八个约定安装包及 sha256：
     ```text
     Aeroric-X.Y.Z-1.x86_64.rpm
     Aeroric_X.Y.Z_aarch64.dmg
     Aeroric_X.Y.Z_amd64.deb
     Aeroric_X.Y.Z_arm64-setup.exe
     Aeroric_X.Y.Z_arm64_en-US.msi
     Aeroric_X.Y.Z_x64-setup.exe
     Aeroric_X.Y.Z_x64.dmg
     Aeroric_X.Y.Z_x64_en-US.msi
     ```
  5. 新建 `scripts/native/verify-release-assets.mjs`：接收明确 repo/tag，读取预期列表与 `SHA256SUMS.txt`；对**从 Release 重新下载的每个 installer**计算 hash 后对比，而非只核对本地产物。下载到独立目录，不能覆盖现有用户文件。
  6. 新建 `scripts/native/release-contract.test.mjs`，本地 fixture 验八包命名、缺包、重复架构、hash 不符必须失败，登记 `native:check release-contract`。本地契约测试属于最终 `all`；真实上传/在线 Release 检查仅在另获发布授权时成为额外必过门禁，不让普通本地构建替用户自动发布。
- **Acceptance Criteria**：
  - `pnpm check:version`、`pnpm native:check release-contract`、native CI 两架构构建检查；不能把仅 Apple Silicon 本地成功当 Intel 已验证。
  - 不发布时：本地 artifact hash 清单、release-decision、CI 检查完成即可；不虚构网上 Release 检查结果。
  - 发布时：`node scripts/native/verify-release-assets.mjs --repo <用户确认的owner/repo> --tag vX.Y.Z` 实际通过、八包齐全、逐包 hash 对上；源代码 zip/tar 不能代替安装包。占位值先替换，不能盲跑。
  - **调用 `grill-me [release]`**；未获授权不 push main、不打 tag、不上传 Release。

### Task P10-05 — 最终逐模块追问、回滚演练与交接

- **Input**：`docs/native/reviews/`、acceptance/visual/release reports、Trellis 子任务、全部 blocker。
- **Action**：
  1. 新建 `docs/native/final-audit.md`、`docs/native/operator-runbook.md`，完成 §6 交付勾选；此时 P10-01～04 的必选测试均已创建，才运行首次最终全量门禁，不存在依赖后续 Task 的测试缺口。
  2. 最后 **显式调用 `grill-me [final-integration]`**：按 core/state/PTY/host/Swift/feature/cutover/signing 逐条读模块 review，追问证据之间是否矛盾；不能只对 README 提问。
  3. 在隔离数据完成一次 native→Tauri UI 回退、host 重启恢复与 backup restore；真实用户回滚前另行确认。回滚 UI 不复活已取消任务，不回退运行时数据格式到不兼容版本。
  4. 更新 `.trellis/spec/` 的实际新约定（spec 文档按项目要求用英文），子任务 implement 记录全部命令与未完成范围；不把不存在的习惯写成已有规范。
  5. 按 `.trellis/workflow.md` 提出 main 上的明确提交文件列表，用户确认后才 commit；archive/journal 的自动提交行为先核对配置；本 Task 不隐式 push。
- **Acceptance Criteria**：
  - `pnpm native:check all` 和 `git diff --check` 在**最终修改之后**重新通过；不能引用修复前旧日志。
  - 每个 core module 有自己的 `grill-me` 调用证据，open P0/P1 为零；未通过集成/真实平台测试明确写 blocked，不能靠删除矩阵行使总数达标。
  - CLI 无 GUI、原生 UI 无整页 WebView、TUI 同进程无损切换、本地无 Apple 账号签名、旧客户端/数据兼容全部有证据。
  - final-audit 给出“完成 / 阻塞 / 明确批准的支持边界”，并与 feature-matrix、Trellis status 一致；仅有 plan 或代码编译成功不得标整体完成。

## 5. 附录 A：机械抽取规则与单步执行顺序

### 5.1 大模块不能在一次 Task 内全仓替换

> **【已提前完成的规划工件 @ 2026-09-21】** `database/`（14 文件）与 `notebook/`（61 文件）已按本节要求展开成**一文件一条**，落在被跟踪的 `docs/native/extraction/database-notebook.md`（75 行、168 个 command wrapper、586 个 `pub` 符号、914 项测试专用条目单列、以及 `AppHandle`/`include_str!`/`cfg(windows)`/spawn/凭据 的聚合风险登记表）。生成器把行数与 `command-map.json` **交叉校验，不一致就中止不写文件**（该性质在第十一次补记前只是一句声称：当时生成代码是一次性的、不可复现；现为 `scripts/native/extraction.mjs`，并由两条 `extraction-docs-current*` 门禁 case 持续校验。重导同时更正了本文档 `testOnlyItemsExcluded` 914→782、`pubSymbolsTotal` 586→650 两列历史错误）。这条约束在本轮真实拦下三次错（173→含注释与断言字符串的虚高、111→测试块括号深度没退回、168→只算整行属性才对）。
> 已解除并完成：`remote/` 于第十一次补记展开为 `docs/native/extraction/remote.md`（22 行、9 wrapper，阻塞事实实测为"mobile 0 处 `@tauri-apps` 引用"）。仍**有意尚未**展开的一块：`dsh_webui/` 等 **Q7/G17** 的能力形态决定——若结论是 mux 网关，66 个文件的落点布局会整体不同，现在展开必然返工。二者已在 Phase 7 子任务 `implement.md` 记名为待办，不算遗漏。

上面涉及 `notebook/`、`database/`、`remote/` 等完整目录的 Task，实施 Agent 必须先在该子任务 `implement.md` 展开为**一文件一条**，不得一次性搬完整目录后再碰运气修编译。展开规则固定：

1. 从 `docs/native/command-map.json` 找到该服务的入口。
2. `find src-tauri/src/<模块> -type f` 只列本模块；`rg -n 'tauri::|AppHandle|State<|emit\\('` 定位依赖，不读取其他模块全文。
3. 每条填写：`source file → destination file → moved symbols → retained wrapper → test target`。对应目录默认保留相对文件名，例如：
   - `src-tauri/src/database/query.rs` → `core/crates/aeroric-core/src/database/query.rs`；command 宏 wrapper 留原路径，业务 body 只保留一份。
   - `src-tauri/src/notebook/import/evernote.rs` → `core/crates/aeroric-core/src/notebook/import/evernote.rs`；纯 parser 原测试一并移动。
   - `src-tauri/src/remote/crypto.rs` → `core/crates/aeroric-core/src/remote/crypto.rs`；v2/v3/relay wire 不随文件移动改版。
4. 先抽 pure DTO/helper，再抽 service，再接 wrapper，再删重复 body。每一小步 `cargo check` core+src-tauri；该模块全部条目通过后，执行其 Acceptance Criteria 与 `grill-me`。
5. `include_str!`、SQL migrations、测试 fixture 相对路径、cfg(windows)、vendor patch、Cargo feature、command-registration test 都加入该文件的检查项。
6. 遇到旧函数业务状态藏在 React 的情况，返回 P2-04 补核心行为，不把 React 页再放进 native WebView“解决”。

这个展开过程是为了单步执行，不是授权 Agent 自己改架构。涉及新的协议、存储格式、权限或 UI 技术选型，先补 ADR 并重新评审。

### 5.2 顺序与可并行边界

```text
P1-01 只读环境 → 获创建同意后 Trellis create（planning）
  → P1-02 清单/规则规划（不实现脚本）
  → P1-03 父子工件/依赖/实施 review/start
  → P1-04 骨架 → P1-05 基线/工具 Gate
  → P2-01 协议 → P2-02 存储 → P2-03 项目 → P2-04 任务 → P2-05 Tauri adapter
  → P3-01 runtime → P3-02 agent launch → P3-03 PTY → P3-04 sessions
  → P3-05 DSH → P3-06 host → P3-07 CLI
  → P4-01 工程及最小 build/sign/UI-test → P4-02 client → P4-03 design → P4-04 shell
  → P5-01 terminal → P5-02 mode → P5-03 tasks → P5-04 sessions → P5-05 settings
  → Phase 6 → Phase 7 → Phase 8（含 Tauri 自带 helper 冷启动）→ Phase 9
  → P10-01 集成/压力 → P10-02 a11y → P10-03 签名强化 → P10-04 发布契约
  → P10-05 最终 all + 交接
```

默认串行保证低级 Agent 不越级。只有主 Agent 明确划分互不重叠文件写入权时才并行（例如仅文档与独立测试 fixture）；schema、Task reducer、Cargo.lock、project.yml、AppServices 不允许多 Agent 同时编辑。reviewer 只读，不能自行派生更多子代理。没有工具不得假装已 dispatch。

### 5.3 单个 Task 的完成记录模板

```markdown
## <Task ID>
Status: pending | in_progress | blocked | accepted
Input:
- 实际读取的文件、符号与前置 Gate
Action:
- 实际修改路径与行为，不写“优化架构”
Acceptance Criteria:
- 命令 / 退出码 / 测试数量 / 证据路径
- GUI 手测：OS、步骤、预期、实际
Review:
- grill-me 模块/工具来源/调用入口/问答与修复复验
Compatibility:
- 旧 UI / CLI / 数据 / remote / 平台检查结果
Rollback:
- 关闭哪个新开关、使用哪个兼容客户端；如何处理新写入数据
Unresolved:
- 问题、owner、所阻塞的 Task；没有就写 none
```

### 5.4 故障分类与停止条件

| 现象 | 处理方式 | 禁止行为 |
|---|---|---|
| Xcode license / SDK 未就绪 | 用户完成环境准备，重跑 doctor | 擅自接受许可、谎称 Swift 编译成功 |
| grill-me 未安装/未响应 | 标记 review blocked，提供真实缺失信息 | 造同名脚本、自审后填“工具通过” |
| core 中仍有 Tauri/React 业务依赖 | 回到对应抽取 Task 解决 owner/port | 在 CLI 后台起隐藏 GUI |
| 两个 writer / task 重复启动 | 停止 cutover，保留证据，修锁/幂等 | 自动 kill 未确认 PID、删除 lock 文件强行继续 |
| native renderer 不兼容必要终端协议 | 能力 spike 阻塞，选型复评或维护明确回退 | ANSI→HTML/TextEditor 伪造原生 TUI |
| upstream 私有功能没有公开控制接口 | capability=false，登记阻塞/支持边界 | 抓屏推测状态、向 TUI 注入猜测批准键 |
| 数据 migration/回滚可能丢新写入 | 停写并请求用户批准恢复方案 | 自动覆盖真实目录、`git reset --hard` |
| 真实账户/网络/麦克风等尚未授权 | fake fixture 验证，真实 E2E 标未运行 | 借测试发送私有数据或产生费用 |

## 6. 附录 B：交付勾选与回退界限

### 6.1 可以宣布“核心迁移完成”的必要项

- [ ] `aeroric-core` 不依赖 Tauri/GUI；业务不靠 React listener 执行。
- [ ] 独立 CLI 可以不打开窗口完成完整 fake/受控真实任务生命周期。
- [ ] 同一 data-dir 单 writer，三客户端 mutation 有 revision/idempotency。
- [ ] 仅安装 Tauri、仅安装 Native Preview、只用 CLI 都能独立冷启动；三方同时冷启动只产生一个 host，缺 helper 不隐式启用第二 writer。
- [ ] 四族 launch/resume/session/权限/usage 与既有行为一致；OMP 不改交互协议。
- [ ] 原始 PTY 字节、resize、lease、IME、回放与 generation 都验过。
- [ ] SwiftUI 设置 Toggle 对 native/advanced 真正生效，不重启任务。
- [ ] unsupported 原生交互明确且可切到同一 PTY，不偷偷自动审批。
- [ ] F01–F26 的所有子项有 owner、文件、测试、review、最终状态。
- [ ] 复杂高级能力区分 mock 合同通过与真实上游可用，未支持项没有被冒充完成。
- [ ] 原 Tauri Windows/Linux 与 mobile/relay 协议保持可用。
- [ ] 本地 ad-hoc app 封印/Info.plist/embedded helper 验证完成，无 Team 依赖。
- [ ] 高对比/减少透明/键盘/VoiceOver/多窗口不退化。
- [ ] 备份与 native→Tauri 回退演练成功，没有不兼容数据写入。
- [ ] 每个核心模块都有实际 `grill-me` 调用记录，不只最终一次。
- [ ] 只有用户明确授权才提交/推送/发布；所有提交仍在 main。

### 6.2 回退界限

- P1–P7 的 preview 默认只写临时数据，可关闭 preview、继续原 Tauri；不能因此删除 preview 用户主动保存的测试成果。
- P8 cutover 后，**回退客户端，不回退同一 host 的数据格式**。原 Tauri external adapter 是第一回退目标，不能把一个完全不认识新数据的旧安装直接指向新目录。
- host 升级或 data migration 必须先停止写入、备份、一致性校验；不在活跃任务中强制更换运行时。
- 本计划不承诺让不同二进制接管一个已经丢失 parent 的现有 PTY master。host 真崩溃后，旧 process/PTY 无法恢复的任务标 interrupted，按各家族受支持的 session resume 策略继续，并要求明确操作。
- 本地 ad-hoc 不等于 Developer ID、不等于 notarized、不等于从互联网下载后无需系统提示；这不阻碍本机开发，但不能在交付说明中夸大。

## 7. 附录 C：外部 API 与工具依据

本地源码是迁移行为的主要依据。以下只用于核对系统 API/候选依赖/工具身份；实施时固定版本并以实际 SDK/package 编译结果为准，不能凭网页宣称已运行成功。

| 编号 | 一手资料 | 本计划采用范围 |
|---|---|---|
| W1 | Apple `NSVisualEffectView`：`https://developer.apple.com/documentation/appkit/nsvisualeffectview`；`behindWindow`：`https://developer.apple.com/documentation/appkit/nsvisualeffectview/blendingmode-swift.enum/behindwindow` | 真正的系统模糊材质、窗口背后混合；不等同于普通透明色 |
| W2 | Apple SwiftUI `Material`：`https://developer.apple.com/documentation/swiftui/material`；Liquid Glass：`https://developer.apple.com/documentation/swiftui/applying-liquid-glass-to-custom-views` | 系统材质与 glassEffect 的分工；最低部署系统与新 API 的 availability 由实际 SDK 编译验证，不用普通透明色冒充材质 |
| W3 | Apple Code Signing Guide：`https://developer.apple.com/library/archive/documentation/Security/Conceptual/CodeSigningGuide/Procedures/Procedures.html`；TN2206：`https://developer.apple.com/library/archive/technotes/tn2206/_index.html` | helper 先于外层签名、资源封印、签名验证与 Gatekeeper 不同职责；具体 flags 用本机 `man codesign` 复核 |
| W4 | SwiftTerm 官方项目：`https://github.com/migueldeicaza/SwiftTerm` | 原生 AppKit `TerminalView` 与 delegate；只是 renderer 候选，必须过 P5-01 能力测试 |
| W5 | XcodeGen 官方 Project Spec：`https://github.com/yonaskolb/XcodeGen/blob/master/Docs/ProjectSpec.md` | targets、local package、scheme/test target 的可复现工程生成 |
| W6 | `mattpocock/skills` 的 grill-me：`https://github.com/mattpocock/skills/blob/main/skills/productivity/grill-me/SKILL.md`；其委托目标：`https://github.com/mattpocock/skills/blob/main/skills/productivity/grilling/SKILL.md` | 本次复核入口委托 grilling 进行设计拷问，必须固定 revision 并核查依赖；不代表已安装或具有自动压力测试 CLI，不把某旧版本的提问轮次写成所有版本通用事实 |
| W7 | Tauri v2 Sidecar：`https://v2.tauri.app/develop/sidecar/`；配置：`https://v2.tauri.app/reference/config/` | externalBin 的 target-triple 文件命名与打包；macOS 配置覆盖、实际安装位置和签名还须按锁定 Tauri 版本构建验证 |

参考代码如需复制，先检查其 `LICENSE` / `NOTICE` / 使用限制与贡献来源，登记 `docs/native/reference-provenance.md`。默认学习架构并自行实现；不复制参考项目商标、音乐资源、商业账号配置、签名 Team 或私有凭据。此项是来源治理门禁，不是本计划给出的法律结论。
