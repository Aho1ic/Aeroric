# Native migration environment report

> 采集时间：2026-09-21（Aeroric `main` @ `ca69a4b`）。
> 采集者：执行 Agent，按 `MIGRATION_PLAN.md` Task P1-01 Action 1/2/3/4/5。
> 本文只记录**实际执行过的命令与其输出**。未运行的项显式写在 §3，不得被下游 Task 当作已通过。
> 交叉链接：Trellis 研究工件见 `.trellis/tasks/09-21-native-macos/research/environment.md`。

## 1. 已验证（本会话真实执行）

### 1.1 基础工具链

| 命令 | 退出码 | 实际输出 |
|---|---|---|
| `sw_vers` | 0 | macOS 27.0，BuildVersion 26A28 |
| `uname -m` | 0 | `arm64` |
| `xcode-select -p` | 0 | `/Applications/Xcode.app/Contents/Developer` |
| `xcodebuild -version` | 0 | `Xcode 27.0` / `Build version 27A266a` |
| `xcrun swift --version` | 0 | swift-driver 1.168.6 / Apple Swift 6.4 / target `arm64-apple-macosx27.0.0` |
| `rustc --version` | 0 | `1.93.0 (254b59607 2026-01-19)`，来自 `~/.cargo/bin`（非 Xcode 分发） |
| `cargo --version` | 0 | `1.93.0 (083ac5135 2025-12-15)` |
| `node --version` | 0 | `v25.9.0` |
| `pnpm --version` | 0 | `10.33.0` |
| `python3 --version` | 0 | `Python 3.9.6`（`/usr/bin/python3`，系统自带；机器上无 Homebrew python3） |
| `trellis --version` | 0 | `0.6.15` |
| `trellis platforms` | 0 | 仅 `Claude Code (claude-code) — .claude` |
| `command -v xcodegen` → 安装后 | 0 | `/opt/homebrew/bin/xcodegen`，`Version: 2.46.0`（经用户明确授权 `brew install xcodegen`） |

### 1.2 许可阻塞与解锁过程（对应 Action 2）

首轮采集时 `xcrun swift --version`、`xcrun --sdk macosx --show-sdk-path`、`python3 --version`、`cc --version` **全部返回退出码 69**，报 "You have not agreed to the Xcode license agreements"。

由此得到的、比 `MIGRATION_PLAN.md` §0.3 更严格的结论，并已在一次性临时目录用真实 `cargo build` 验证：

- Xcode 许可未接受时，**阻塞面不止 Swift/Xcode**：`/usr/bin/cc` 与系统 `python3` 同为 Xcode shim，因此
  - Rust **链接**失败（临时 bin 探针 `cargo build` 在 link 阶段报同一许可错误）；
  - 全部 `.trellis/scripts/*.py` 无法运行（P1-01/P1-03 的 Trellis 步骤整体不可执行）。
- 不触发链接的 `cargo check`（纯 bin 探针）当时仍可返回 0；不能作为"Rust 工具链可用"的证据。

处理方式符合 Action 2 与 §5.4：Agent **未**代为接受许可、**未**使用 sudo。诊断与阻塞项上报后，由用户在自有终端自行接受许可。许可解锁后复测：上表所有命令退出码为 0，临时探针 `cargo build` + 运行输出 `ok`（退出码 0）。

**对本计划的直接影响**：§0.3 中"打印版本不证明编译工具可用"的告警在本机上被证实为真实故障而非理论风险；此后任何"构建可用"的断言必须以一次真实链接（而非 `--version`）为准。

### 1.3 DBX 依赖（对应 Action 3）

| 检查 | 结果 |
|---|---|
| `scripts/dbx-ref.txt` pin | `79042315661307745edb0c20763ea4390e97e3d6` |
| `git -C ../dbx rev-parse HEAD` | `79042315661307745edb0c20763ea4390e97e3d6` — **与 pin 一致**，无漂移 |
| `git -C ../dbx status --short` | 38 个脏文件，含 `crates/dbx-core/Cargo.toml`、`crates/dbx-core/src/table_import.rs`、`vendor/wry/**` |
| `patches/dbx-security.patch` 存在 | 是（10843 字节） |
| `git -C ../dbx apply --reverse --check <patch>` | 通过 → **安全补丁已应用**，`scripts/prepare-dbx.sh` 的幂等首分支（"already applied" → exit 0）成立 |

按 Action 3 的要求：工作区 dirty，**未**执行任何 `checkout`/`reset`/`stash`；`prepare-dbx.sh` 也未被运行（运行它会在脏树上继续改动 `../dbx`，不属于本 Task 授权范围）。`../dbx` 的脏改动归属用户既有工作，本迁移不得触碰，也不得把"patch 已应用"误当成"工作区干净"。

### 1.4 工程依赖状态

`pnpm install --frozen-lockfile --ignore-scripts --offline` 退出码 0，输出 `Lockfile is up to date` / `Already up to date`（4 个 workspace 项目）。即：`node_modules` 已与锁文件一致，无需变更依赖。

### 1.5 Trellis 任务指针（对应 Action 5）

`task.py create "Native macOS migration" --slug native-macos` 已获用户明确授权后执行，返回真实路径 **`.trellis/tasks/09-21-native-macos`**（日期前缀由脚本产生）。

```
$ python3 ./.trellis/scripts/task.py current --source
Current task: .trellis/tasks/09-21-native-macos
Source: session:qoder-native-macos-20260921-aeroric
```

`python3 ./.trellis/scripts/get_context.py --mode packages` 真实成功，当前三包：`aeroric-mobile`（default，path `mobile`）、`design-system`、`remote-contracts`，另有 `.trellis/spec/guides/index.md` 共享指南。P1-04 将新增 `aeroric-core` / `aeroric-macos` / `aeroric-desktop` / `aeroric-web` 四包。

#### 1.5.1 会话身份：每个新 shell 必须重新导出（下游必读）

`create` 与 `current` 首轮均在 **degraded mode**（无 session identity）下运行，因为本平台未注入 `TRELLIS_CONTEXT_ID`。`common/active_task.py::resolve_context_key` 只认 `TRELLIS_CONTEXT_ID` 环境变量或平台注入的 session/conversation/transcript key。

**约束（会改变后续每个 Task 的操作方式）**：

- 本会话指针 key 为 `qoder-native-macos-20260921-aeroric`。
- Agent 的每次 Bash 调用是独立 shell，**不继承前一次的 `export`**。任何读写该指针的 `task.py` / `get_context.py` 调用都必须先在本条命令内 `export TRELLIS_CONTEXT_ID="qoder-native-macos-20260921-aeroric"`（或与脚本调用同一行加前缀）。
- 该值不是凭据，仅用于会话隔离的指针分片；不得复用到其它并发会话（P1-01 Action 5 要求"本会话唯一"，且不得覆盖其他会话指针）。

#### 1.5.2 发现的计划与实现冲突（已在计划正文标注）

P1-01 Acceptance 同时要求「`task.py current --source` 指向本次真实任务」与「任务仍处于 planning」。但 Trellis `0.6.15` 的 `task.py` 里 `set_active_task()` 仅在 `start` 路径被调用（`task.py:134`），而 `start` 无条件把 `planning` 翻成 `in_progress`（`task.py:141-145`，degraded 分支同样翻，见 `task.py:124-131`）。即两条验收在现有实现下不可能靠单条官方命令同时成立。

按 §2.1 的处理原则（"注释与实现有出入时先测试、再修正文档"）：先实测确认该行为，然后 `start` 写入指针、再把 `task.json` 的 `status` 显式改回 `planning`，使两条验收的字面意图都成立——指针有效且实现未被授权。此偏离记录在案，供 P1-03 与 P1-05 Gate 复核；`start` 的 `after_start` hook 已随该次调用触发。

## 2. 环境可用性结论

- Rust（含链接）、Node/pnpm、Python/Trellis 脚本、Swift 工具链、XcodeGen 2.46.0：**可用**，均有本轮真实退出码支撑。
- 尚未验证的：任何**本项目**的 `cargo check`/`cargo test`、`swift build`、`xcodebuild`。这些属于 P1-04/P1-05/P4-01 的验收，此处只证明工具链存在且能链接。
- Apple Developer 账号：**不需要**。按 INV-09 保持 `src-tauri/tauri.conf.json` 的 ad-hoc 基线；Swift 侧的无账号构建在 P4-01 落实。

## 3. 明确未运行 / 未创建（不得被当作已通过）

- `pnpm lint` / `pnpm typecheck` / `pnpm test` / `pnpm build`：未运行（属 P1-05 基线）。
- `cargo check --manifest-path src-tauri/Cargo.toml --locked`：未运行（属 P1-05 基线）。
- `grill-me`：未安装、未调用（用户已授权安装并固定 revision，见 §4 待办）。
- `docs/native/{feature-matrix,command-map}.json`、`docs/native/adr/001-native-boundary.md`：未创建（P1-02）。
- `core/`、`macos/`、`contracts/`、`scripts/native/`：未创建（P1-04）。
- 参考仓库（`../ZCode`、`../opencode`、`../MiMo-Code`、`../minimax-code`、`../primuse`）：本 Task 未读取；§1.1 的检视记录沿用计划正文既有内容，本次未新增。

## 4. 下一步（P1-01 出口条件）

1. 安装并固定 `grill-me` 的来源 revision（含其委托目标 `grilling`），写入 `docs/native/tooling.md` — P1-05 门禁的前置。
2. 进入 P1-02：只写规划工件（`feature-matrix.json`、`command-map.json`、`adr/001-native-boundary.md`），不写脚本、不改运行时代码。
3. P1-03 建父子任务与依赖边；`implement.md` 记录 P1→P2→…→P10 阻塞关系。
4. 任何提交前按 `.trellis/workflow.md` 取得确认；本计划不授权 push/tag/发布（INV-01）。
