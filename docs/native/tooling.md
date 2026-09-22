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
