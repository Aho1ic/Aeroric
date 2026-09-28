# Native migration tooling — real entrypoints and revisions

> 按 `MIGRATION_PLAN.md` §4.1 要求：**工具准备不能伪造**。本文件只记录实际探测、实际安装的来源与固定 revision，以及真实可用的调用入口。
> 建立时间：2026-09-21。交叉链接：`docs/native/environment.md`。

## 1. `grill-me`：分发形式判定

`grill-me` 是 **Agent Skill**，不是可执行二进制。证据：

- `command -v grill-me` 无输出（未装 CLI）。
- 来源仓库 `mattpocock/skills` 的 `skills/productivity/grill-me/` 目录内只有 `SKILL.md`（157 B）与 `agents/openai.yaml`（137 B），没有任何可执行文件、脚本或 `bin/`。

因此本计划中所有"执行 `grill-me [模块]`"一律按下文 §3 的技能入口发送，**不得**伪造成 `grill-me review --stress` 之类不存在的 shell 参数（§0.3、§4.1 明令）。

## 2. 来源与固定 revision

| 项 | 值 |
|---|---|
| 仓库 | `mattpocock/skills`（GitHub） |
| 许可证 | **MIT**（经 `api.github.com/repos/mattpocock/skills/license` 确认） |
| 固定 revision（本仓安装基线） | `c55ee46073ed923f86ce59a5eb3b6d895095d1b7`（main，2026-09-18T10:12:29Z） |
| `grill-me/SKILL.md` 末次改动 | `fcf0071560d3`（2026-08-15T20:46:38Z，"Switch Skill tool phrasing to `with \"name\"`, revert grill-wi…"） |
| raw 抓取基路径 | `https://raw.githubusercontent.com/mattpocock/skills/c55ee46073ed923f86ce59a5eb3b6d895095d1b7/skills/productivity` |
| 安装方式 | `curl -fsS` 原样落盘，未手工转录内容 |

安装产物与 sha256 前缀（用于日后核对是否漂移）：

| 文件 | 字节 | sha256（前 16） |
|---|---|---|
| `.claude/skills/grill-me/SKILL.md` | 157 | `caaf8b8de1684f96` |
| `.claude/skills/grill-me/agents/openai.yaml` | 137 | `c061e39c3e0f9d86` |
| `.claude/skills/grilling/SKILL.md` | 1987 | `10ff989e7498b23b` |
| `.claude/skills/grilling/agents/openai.yaml` | 113 | `1411d7df7d99b7e6` |

## 3. 依赖链（必须完整，不能只装别名）

§4.1 要求："若该 revision 是转交 `grilling` 的入口，必须同时确认被调用技能存在，记录完整调用链；不能只安装别名文件就算准备完成。"

实测该 revision 下 `grill-me/SKILL.md` 全文即为一行转发指令：

```text
Call the Skill tool with "grilling".
```

完整调用链：

```text
/grill-me  →  grill-me (SKILL.md, disable-model-invocation: true)
           →  Skill tool: "grilling"
           →  grilling/SKILL.md：以 design tree + frontier rounds 的方式逐轮追问，
              事实自己查（可派子代理），决策交用户，frontier 清空前不得动手实施
```

- `grilling` 已同时安装（§2 表），链路上没有悬空引用。
- 两个 `agents/openai.yaml` 只含 `interface` 展示字段与 `policy.allow_implicit_invocation: false`（grill-me 侧），**不含**任何额外工具依赖或网络端点。已逐字核对。
- 语义要点（影响本计划各 Gate 的执行方式）：`grilling` 要求"每轮把整条 frontier 一次性问完并等待用户回答"，且"未达成共识前不得实施"。这与 §4.2 的"可由源码/测试回答的先查证，产品决策交用户，不得把未回答的问题写成已通过"一致。

## 4. 本会话的真实可用性 —— 未通过，如实记录（并纠正一处早先的错误推断）

安装后实测：

| 调用 | 结果 |
|---|---|
| `Skill(skill="grilling")` | **失败**：`Skill "grilling" not found`，并回显当前已注册技能全集（不含 `grill-me` / `grilling`） |
| 同一会话二次复测 | 同样失败，注册表内容一字未变 |

### 4.1 早先的"会话启动快照"假设是错的，已用反证推翻

先前解释是"注册表在会话启动时快照，所以本会话装不进、重启即可"。反证：本项目 `.claude/skills/` 里的 `trellis-check` frontmatter 完全合法（`name` + `description` 齐备），却同样不在注册表中；若"会话启动快照 + 读项目 `.claude/skills`"成立，它应当在里面。所以真正的原因不是时序，而是**目录根本不被读取**。

### 4.2 实测出的技能加载根

| 位置 | 是否被本 CLI 读取 | 证据 |
|---|---|---|
| `<project>/.claude/skills/` | **否** | 其中 `trellis-*` 九个技能全部未注册，只有恰好也内置的 `shadcn` 出现在注册表里 |
| `~/.claude/skills/` | **是（但是符号链接）** | `ls -ld` → `lrwxr-xr-x … /Users/macbook/.claude/skills -> ../.mirasim/skills`；其下 6 个条目（`eval`、`mimo-browser-use`、`source-command-zcf-git-worktree`、`subagent-orchestration`、`systematic-debugging`、`verification-before-completion`）与注册表中的非插件技能**逐一对应、无一多余** |
| `~/.agents/skills/` | 同上，同一个目录 | 亦为 `-> ../.mirasim/skills` 符号链接 |
| `~/.qoder/skills`、`~/.qoder-cli/skills` | 无该目录 | `ls` 结果为空 |
| `~/.qoder-cn/skills`、`~/.qoder-cn/plugins/*/skills` | 无该目录 | 全盘 `find -name skills` 只命中 `security-resources/security-scan/skills` |
| `shadcn` 的真实来源 | CLI 内置，与项目目录无关 | 全盘只在 `~/.claude/projects/<本仓>/<session>.jsonl`（历史会话记录）里出现，没有对应的安装目录实体 |

**结论**：真正生效的用户级技能根是 **`~/.mirasim/skills`**（经 `~/.claude/skills` 与 `~/.agents/skills` 两个符号链接暴露）。本计划把 `grill-me`/`grilling` 装到了 `<project>/.claude/skills/`，**那是一个不被读取的位置**，因此：

- **重启会话不会让 `/grill-me` 出现**——此前写给用户的选择项（"重启后先过 grill-me"）基于错误的目录假设，需要在正确位置安装后才成立。
- 正确做法（一条命令、可逆、但**影响全局**，须用户批准，见 §4.3）：把两个技能目录放进 `~/.mirasim/skills/`（等价地 `cp -R` 到 `~/.claude/skills/`，因为它是符号链接），然后重启会话。

### 4.3 为什么 Agent 没有自行执行这一步

`~/.mirasim/skills` 是被 `~/.claude` 与 `~/.agents` 共同符号链接的**跨工具全局技能目录**，往里写入会影响本机所有项目与所有读取这两个入口的工具，超出本仓库范围。用户对"安装 grill-me"的批准是在"装进本仓库"的假设下给出的，不构成对全局目录的默示授权。因此本步**留待用户明确批准**，Agent 只准备好可复制的确切命令：

```bash
# 需要用户执行或明确授权；来源已在 §2 固定到 revision，落盘文件的 sha256 见 §2 表
cp -R "<repo>/.claude/skills/grill-me" "<repo>/.claude/skills/grilling" ~/.claude/skills/
# 复核（应各返回 SKILL.md 路径）
ls ~/.claude/skills/grill-me/SKILL.md ~/.claude/skills/grilling/SKILL.md
# 之后重启会话，/grill-me 才可能进入注册表
```

`<project>/.claude/skills/` 下的两份**保留不动**，作为本仓的固定 revision 副本与 provenance 依据。

### 4.4 门禁处置（不变）

按 §4.1 第三分支与 §5.4 的处置：

- 涉及 `grill-me` 的模块 Gate 一律标 `BLOCKED: grill-me entrypoint unavailable`，不得宣布通过，不得用 Agent 自审伪装成工具调用。
- 复验要求不变：真实调用后按 §4.2 把来源/revision、实际调用入口、审查 commit/diff hash、问答、证据、问题级别、修复后复验结果写入 `docs/native/reviews/<module-id>.md`。
- 待办：`docs/native/reviews/skeleton.md` 的"Actual invocation"仍为空，直到上述调用真实发生。

## 5. 其它工具状态（本轮实测）

| 工具 | 状态 | 版本 / 备注 |
|---|---|---|
| XcodeGen | **已安装**（用户授权 `brew install xcodegen`） | `2.46.0`，`/opt/homebrew/bin/xcodegen`。P1-04 / P4-01 的 `pnpm native:generate` 必须锁定该版本并在 §3.2 的 argv 数组里以绝对路径或经 PATH 校验调用 |
| Xcode / Swift | 许可已由用户自行接受 | `Xcode 27.0 (27A266a)`，`Apple Swift 6.4`，target `arm64-apple-macosx27.0.0` |
| Rust | 可用，链接已实测通过 | `rustc` / `cargo` 1.93.0，`~/.cargo/bin` |
| Node / pnpm | 可用 | v25.9.0 / 10.33.0，锁文件一致 |
| python3（Trellis 脚本用） | 可用 | 3.9.6（`/usr/bin/python3`，Xcode shim；许可未接受时同样退出 69） |
| Trellis CLI | 可用 | `0.6.15`（注意：`MIGRATION_PLAN.md` §0.3 写的是 0.6.16，环境已变化） |
| Homebrew / gh | 存在 | brew 6.0.19；`gh` 未用于本文件结论（来源核对走未认证 GitHub API + raw） |

## 6. 下一步

1. 用户重启会话或改用 Claude Code 后，首次真实调用为 `grill-me [skeleton]`，产出 `docs/native/reviews/skeleton.md`。
2. 该证据齐备前，Phase 1 Gate（P1-05 验收第 2 条）保持未闭合。
3. 参考代码如需复制，先按 §7 末段登记 `docs/native/reference-provenance.md`；`mattpocock/skills` 为 MIT，本处仅安装其两个技能文本，保留来源与 revision 记录。

## 7. 委派 agent 通道（用户提供，2026-09-22 起启用）

用户给了一个可用来分担侦察工作的 agent 配置（`mimo-cpa`，`omp` 家族）。它**不是门禁的一部分**，本节记录的是"怎样安全地用它、以及它的产出算不算证据"。

**形状**

```
env -u PI_PROFILE -u OMP_PROFILE \
    PI_CODING_AGENT_DIR="$HOME/.cache/aeroric-delegate/omp-worker" \
    omp --cwd "$PWD" --model "mimo-v2.6-pro" --max-time 900 -p "@/tmp/delegate-task-N.md"
```

- **模型/网关**：本地网关 `127.0.0.1:8317/v1`（用户配置里的 base_url），不是公网端点。配置里带一个 API key —— **它从不进 argv、日志、产物、审查文档或 git**；本节故意指代，不复述。
- **独立 home**：`~/.cache/aeroric-delegate/omp-worker`。既不是用户的 `~/.omp`，也不是 Aeroric 托管的 `~/.aeroric/agent-homes/omp`（AGENTS.md 的 OMP 规则）。`PI_PROFILE`/`OMP_PROFILE` 必须显式清空，否则按上游 `dirs.ts` 的语义整个 `PI_CODING_AGENT_DIR` 会被忽略，会写到用户自己的 home 里去。
- **任务形状**：只派"读多写少"的侦察活，且任务书里写明：只准写**一个**指定产物文件；禁止 git 命令、禁止网络、禁止构建；每条结论必须带 `文件:行号`。
- **超时**：`--max-time` 是硬约束；超时退出（exit 1）不影响门禁，因为它从来不是验收证据。

**它的产出怎么用（P2-04 的实际做法）**

1. 底稿 `docs/native/extraction/task-lifecycle-react.md`（172 行，含 50 条"仍留在 React 的业务决定"）。
2. **抽样回查后才当输入**：随机取 20 处 `file:line`（覆盖它自己没读、我也没读过的 `taskSwitch.ts`/`worktreeOps.ts`/`taskLaunch.ts`/`taskFlush.ts`/`App.tsx`/`types.ts`/`appProjectState.ts`/协议 `task.rs`），逐条 `sed` 出那些行号核对 —— 20/20 命中。
3. 它自己列的"无法证实的条目"（15 条）不当结论：其中"legacy 幂等键是否存在""generation 谁分配"两条，我是靠读源码+反例自己定的（ADR-017 正是被它列进"未核对"的那条引出来的）。
4. 它在 §5 承认没读 `remote/tasks_rpc.rs`（P2-04 Input 清单里有），那份是我自己读的。

**红线**

- 它的输出**不写进** `knowledge/`、`contracts/`、parity 表或门禁 case：派生链的每一环都必须能被门禁重算，委派 agent 不在链上。
- 它的产物若要引用进计划，必须先声明回查方式与命中率（`MIGRATION_PLAN.md` 续跑指令第 2 条就是这么写的）。
- 不做 git 操作、不提交、不 push；也不碰 `~/.aeroric`（真实数据目录）。

## 8. 工具状态整段重测（2026-09-27，MIGRATION_PLAN 补记一百零四）

§5 的表写于 2026-09-21，其中两项**环境已经变了**，按"§0.3 在环境变化后须整段重写"的同一条规矩在此回填（`MIGRATION_PLAN.md` §0.3 已同步重写）。当天真实命令与 rc 全在 §0.3 表里，此处只记差异与影响：

| 工具 | §5 旧值 | 2026-09-27 实测 | 影响 |
|---|---|---|---|
| `python3` | 3.9.6（`/usr/bin/python3`，Xcode shim） | **3.13.2**，`/opt/miniconda3/bin/python3` | 全部 `.trellis/scripts/*.py` 今天跑在 conda python 上；`task.py validate/current/start` 实测成功（父任务 9/4、phase-3 7/4）。**不是**同一解释器了，日后 python 相关故障排查别按 3.9.6 假设 |
| `node` | v25.9.0 | **v22.23.1**（`~/.local/bin/node` → `~/.hermes/node/bin/node`） | 所有 `scripts/native/*.mjs` 与门禁跑在它上面且当天全绿；major 版本降了，`--test-reporter=tap` 行为已随 `skeleton`/`inventory-unit` 等实测有效 |
| `xcodegen` | 已安装 2.46.0 | 2.46.0（`/opt/homebrew/bin/xcodegen`） | 无变化；`macos/AeroricNative.xcodeproj` 当下**不在磁盘上**（`.gitignore:71` 视为派生物），`project.yml` 也不存在 ⇒ P4-01 的 `native:generate` 无当日证据 |
| `grill-me` / `grilling` | §4 记"本会话不可调用（装错目录）"，后由用户批准拷入 `~/.claude/skills`（→ `~/.mirasim/skills`） | **真实可用**：`Skill(skill="grilling")` 本轮已实际调用（P3-03 收尾轮，`docs/native/reviews/terminal-core.md` §0） | §4/§4.4 的 `BLOCKED: grill-me entrypoint unavailable` 处置已成历史，仅对 P1-05 当时成立 |
| `trellis` | 0.6.15 | 0.6.15，并打印 `⚠️ update available: 0.6.15 → 0.6.16` | 提示存在 ≠ 授权 `trellis update`（原约束沿用） |
