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
| XcodeGen | **已安装**（用户授权 `brew install xcodegen`） | `2.46.0`，`/opt/homebrew/bin/xcodegen`。P1-04 / P4-01 的 `pnpm native:generate` 必须锁定该版本并在 §3.2 的 argv 数组里以绝对路径或经 PATH 校验调用。**2026-09-28 起这个「锁定」有机检**：`scripts/native/doctor.mjs` 的 XcodeGen 检查带 `match: /2\.46\.0/`，版本一变 `native:doctor` 即 `MISS` + exit 1（此前它只断 exit 0，任何版本都算通过） |
| Xcode / Swift | 许可已由用户自行接受 | `Xcode 27.0 (27A266a)`，`Apple Swift 6.4`，target `arm64-apple-macosx27.0.0` |
| Rust | 可用，链接已实测通过 | `rustc` / `cargo` 1.93.0，`~/.cargo/bin` |
| Node / pnpm | 可用 | v25.9.0 / 10.33.0，锁文件一致 —— **此行的 Node 版本已过时**，以 §8（2026-09-27 重测：v22.23.1，`~/.hermes/node/bin/node`）为准；保留原文只作时间线 |
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

## 9. Digest 分片机制（`scripts/native/digest.mjs`）—— 2026-09-28 首次成文

**为什么要存在**：Trellis 的 `context_injection.max_file_bytes` 默认 **32 768 B**（`.trellis/config.yaml` 里整块是注释，所以走 `common/config.py` 的默认值），超限的文件会被注入器截断。`docs/native/feature-matrix.json`（2026-09-29 工作树实测 **138 178 B**，同一文件在 HEAD `797e06c` 里是 43 275 B ⇒ 未提交批次的增长本身就在顶预算）与 `docs/native/command-map.summary.json`（**33 296 B**）都超，直接列进 `implement.jsonl`／`check.jsonl` 就是 补记一百二十二 记下的 Deviation D8（台账被静默砍半）。`digest.mjs` 把这两份大工件切成**逐片不超限**的小片，供注入清单引用。

**片名怎么读，不在这份文档里枚举**：固定的几片是 `command-map-{head,modules,surfaces,unreachable}.json`、`feature-matrix-{head,features}.json`，外加 `feature-matrix-gaps-1..N.json`——**N 由台账体积决定，会长**（2026-09-28 一天内 N 从 2 变到 3 再变到 4）。所以当前片名、每片字节与 headroom 一律就地读：`docs/native/digest/index.json`，或 `node scripts/native/digest.mjs --check`。`index.json` 同时记 `capBytes: 32768` 与两个源各自的 sha256。**同一条规矩适用于注入清单**：`.trellis/tasks/*/check.jsonl`、`implement.jsonl` 里引用 gaps 时一律指向 `docs/native/digest/index.json`，**不得枚举 `gaps-N.json` 片名**——片数由台账体积驱动，枚举必然腐烂（2026-09-29 就抓到 10 份清单指着已删的未分片旧名，已照此改法收敛）。

> **本节不写死分片数量与体积**：它们随台账增删变动（2026-09-28 一天里最大片就从 20 827 → 21 926 → 23 486 → 24 743 → 26 959 B，同一张表被同日新登记的 gap 连续作废四次）。抬 `chunks.gaps` 的时机不由人判断——`digest-unit` 里那条「每片必须 < 80% 上限」的用例会**直接判红**并点名是哪一片（截至 2026-09-29 已行使过七次否决：第一次把 `gaps` 切成 2 片，其后 `G23` 2→3、`G27` 3→4、`G32` 4→5、`G37` 5→6；第 6 次**没有任何登记**，只是给全部 open gap 盖 `verified_on` 就把 gaps-2 推到 26 393 B＝80.5%（6→7）；第 7 次是 2026-09-29 第 14 项定 ② 之后关闭 `G25` 并登记 `G39`（7→8，gaps-2 到 26 436 B＝80.7%）⇒ **一进一出、净增一条也吃掉一格**，"台账条数差不多"不等于"体积差不多"），所以正确动作只有一句：照它报的那片把 N 加一，再同步 `.trellis` 注入清单、`digest.test.mjs` 的分片数期望与本节。

> **2026-10-08 同一余量门禁再次生效**：P8-05 切片十六补记 G43 后，直接 TAP 复现最大 gaps 片 **26,404 B**，超过 80% 线。依上述 N+1 策略加片，完整重建、孤儿负例与 80% 判据均保留；注入清单仍经 `index.json` 发现当前片集，不枚举新片名。

**两个被切的源**：`docs/native/command-map.summary.json`（33 296 B）→ 4 片；`docs/native/feature-matrix.json`（**本身就超单文件上限**）→ `head`＋`features`＋`gaps-1..N`，其中 `gaps` 数组按顺序自动分块、**N 只读 `index.json` 或 `digest --check`，本节不写**（这里原先写死的"→ 4 片"和源的字节数都已被同日台账增长作废）。**块边界与每块条数随体积移动**——本节先写「今天 12＋9」就被同日的 `G22` 作废（实测已是 13＋8＝21 条），所以结论是：任何文档与注入清单都**不得枚举「某条 gap 落在哪一片」**，要读两片文件本身或 `digest --check` 的输出。

**四条硬保证**（各由 `digest.test.mjs` 的一条负例守着，`digest-unit` 现 17 用例）：
1. 任一目标片超 32 768 B ⇒ 报红拒绝写（不是静默截断）。
2. **从磁盘上的片重组 canonical** 并对不上源文件的 sha256 ⇒ 报红（曾经只比「计划载荷」，对手工改过的片全盲）。
3. 计划不再产出的同名片留在盘上 ⇒ 孤儿片报红（注入清单**按路径**读文件，留着就是永久毒饵）。
4. `pnpm native:digest --check` 这类「同时给两个模式」的请求 ⇒ CLI 直接 exit 2 拒绝，因为 `package.json` 的别名自带 `--write`，`--check` 展开成 `--write --check` 会**一直在写**（补记一百二十三）。

**进门禁的方式**：`skeleton` 模块两条 case —— `digest-shards-current`（`node scripts/native/digest.mjs --check`，gate）与 `digest-unit`（`node --test digest.test.mjs`，minCases 17）。日常改完台账的动线：编辑源工件 → `node scripts/native/digest.mjs --write` → 同脚本 `--check` → `native:check skeleton`。**分片文件属于门禁工具面（§3.2／P1-04 那一族），不由任何产品 Task 逐个列出**——这也是它们此前在 `MIGRATION_PLAN.md` 里查无此名的原因，本节即其归属。

**清单腐烂有现成的检查器，跑就行（2026-09-29 实测）**：`python3 .trellis/scripts/task.py validate <dir>` 逐行解析任务目录的 `implement.jsonl`／`check.jsonl`，`file` 字段指向的路径不存在就打印 `File not found: <path>` 并 **exit 1**；同一条命令还会对超过 `max_file_bytes` 的被引用文件打印 `… injection will truncate it`（只警告、不失败，这正是本节第 4 条要防的静默截断）。今天全库 27 个任务目录跑一遍 ⇒ **24 rc=0／3 rc=1**，12 个不存在的路径全在 `.trellis/spec/**`（包目录与 `index.md` 在、被点名的分篇 guideline 从未写过）；负控（临时目录里写一条已删除的 `digest/feature-matrix-gaps.json`）⇒ 第 1 行被点名、rc=1。**本仓没有任何门禁读 `.trellis/`（`.gitignore:74`），所以清单腐烂只能靠人跑这条命令发现**——动过注入清单后的例行动作：`for d in .trellis/tasks/*/; do python3 .trellis/scripts/task.py validate "$d"; done`，迁移相关目录要求 rc=0，非迁移目录的已知断链逐条豁免并记档（台账 `G23` 第三个面）。

## 10. `native:*` 别名 ↔ 实现脚本 ↔ 门禁覆盖（2026-09-28 逐条实测）

此前工具文档只用 pnpm 别名说话，**从没写过实现这些别名的文件名**：`build.mjs`／`dev.mjs`／`generate.mjs`／`ui-test.mjs`
在 `docs/native/` 与 `MIGRATION_PLAN.md` 里按文件名搜都命中 0 次，所以按文件找实现的人（以及 §3.1 #1 的归属回查）会点不出出处。本表补上这层映射；exit code 与状态今天重测。

| 别名 | 实现脚本 | 被门禁 case 引用 | 今天实测 |
|---|---|---|---|
| `native:build` | `scripts/native/build.mjs` | 否 | exit **78** `NOT READY — implemented by Task P4-01`（11 行纯桩，只 `console.error` 再退出） |
| `native:dev` | `scripts/native/dev.mjs` | 否 | exit **78**，同上 |
| `native:generate` | `scripts/native/generate.mjs` | 否 | exit **78**，理由写明 `macos/project.yml` 还不存在；版本锁另由 `doctor.mjs` 的 `match: /2\.46\.0/` 守（§5） |
| `native:ui-test` | `scripts/native/ui-test.mjs` | 否 | exit **78**，理由是 `AeroricNative.xcodeproj` 与 UI tests target 由 P4-01 创建 |
| `native:check` | `scripts/native/check.mjs` | —（它就是门禁执行器） | 按模块运行；`all` 在 38/43 计划模块未 ready 时**拒绝运行**（exit 3），首个真全矩阵属 P10-05 |
| `native:contracts` | `scripts/native/contracts.mjs` | 是 | `--check` 12 份契约逐字节比对；`--write` 才重生成 |
| `native:digest` | `scripts/native/digest.mjs --write` | 是（`digest-shards-current` 直调 `--check`） | **别名自带 `--write`**，所以 `pnpm native:digest --check` 会展开成 `--write --check`；CLI 现在拒绝同时给两个模式（exit 2）。要「只查不写」请直调 `node scripts/native/digest.mjs --check`（§9） |
| `native:doctor` | `scripts/native/doctor.mjs` | **否** | 18/18；属 F2 那一族——没有任何 gate case 跑它，所以它绿不代表门禁绿 |
| `native:inventory` | `scripts/native/inventory.mjs` | 是 | `--check` 一致性；`--freeze` 派生交付列；`--freeze-check` 字节 noop 校验 |

要点：**四个 NOT READY 桩「不进门禁」是设计而非缺口**（§3.2 禁止恒 exit 0 的占位；P1-04 Action 5），但 `native:doctor` 不在门禁里是**缺口**，与 F2（没有 case 跑全量 `pnpm test`／`cargo check --target`）同族，归 P10-05 收。

## 11. 命令台账的两个指针字段：只能引用 `definition_resolved`（2026-09-28 补记一百六十六 实测）

`docs/native/command-map.json` 的每条命令同时带 **两个** 指向源码的字段，它们**不是同一个东西**：

| 字段 | 语义 | 门禁怎么守 | 能不能引用 |
| --- | --- | --- | --- |
| `definition_resolved` | 该命令 `fn` **声明所在行** | `inventory.mjs` 的 G10 会读那一行并断言 `\bfn\s+<命令名>\b` 真的在里面（`inventory.mjs:196-198`） | **能**——660/660 都被机器钉在声明行 |
| `definition_candidates` | 扫描期命中的**候选行**（分辨率低） | 只在 `inventory.mjs:116` 校验**文件存在**，不校验行 | **不能**当定义位置引用 |

当日测到的规模与形状：660 条里 **55 条** 的 `resolved` 与 `candidates[0]` 不是同一行，候选行实测落在三种错误位置上——注册宏所在的那一行（`hide_main_window`：候选 `lib.rs:556` 其实是 `generate_handler!` 列表里的一条 `lsp::` 登记项，真值 `lib.rs:304`）、函数体内部（`adopt_session_for_agent`：候选 `:1696` 在体内，真值 `:1684`）、以及签名参数行（`run_task`：候选 `pty.rs:1421` 是 `texts: Option<...>` 参数，真值 `pty.rs:1412`）。

同批还量到两条可直接引用的事实：**`#[tauri::command]` 字面出现 660 处／66 个文件**，与 `generate_handler!` 及台账同为 **660**，且**没有一个带属性的命令未登记**（＝不存在"死命令"）；**`definition_unique === false` 只有 1 条**（`skills::list_skills`，仍解析到 `skills.rs:1190`）——P8-01 动 skills 时要单独确认这一条为何非唯一。

口径提醒：以上"三方都是 660"是**用两套独立仪器**量的——台账与注册表的一致性 `inventory --check` 本来就双向检查（`registered but not mapped` / `mapped but not registered`），但**属性行数 660** 是从 Rust 源码直接 `grep` 出来的第二仪器。若只信前者，就是拿工具自己的输出证明工具没错。
