# Agent Note: 规则、现状文档与决策笔记分三处放

Status: implemented

## Problem

几个月里多个 agent 把所有东西都写进文档：根目录一份 7000 行的迁移计划、术语表，`docs/native/` 下 40 多篇 ADR（正文里叠满"修正案/落实/更正"）、数十篇设计评审稿、阶段检查点、交接稿与夜间记录，`docs/research/` 的调研底稿。新会话找不到"现在系统怎么运转"，也分不清哪条决定仍然有效；`AGENTS.md`/`CLAUDE.md` 里还留着已被代码推翻的规则（例如 omp "绝不传 `--mode`"）。

## Decision

- **`AGENTS.md`（根）**：开工必读的硬规则，每条一两行并链到详情。`CLAUDE.md` 只指向 `AGENTS.md` 并给常用命令。根目录这两份保持 gitignore（用户既定）；`core/`、`macos/`、`ios/`、`contracts/`、`scripts/native/`、`docs/`、`remote-push/`、`remote-relay/` 各有一份入库的子目录 `AGENTS.md`，写该目录特有的规则。
- **`docs/`**：只写当前系统的事实。`docs/architecture.md` 是全景；`docs/subsystems/` 每个子系统一篇；`docs/cookbook/` 放构建、验证、发布等操作手册；`docs/backlog.md` 是产品候选库；`docs/native/` 只放门禁与生成器读取的机读工件（`command-map.json`、`feature-matrix.json`、`event-topics.json`、`acceptance-matrix.json`、`digest/` 及被脚本引用的两三篇说明）。正文不写"以前/现在/不再"等变迁叙事。
- **`.agents/notes/`**：决策与取舍，遵循 `.agents/skills/write-notes-like-deepseek/SKILL.md`，路径 `{proposed,implemented,rejected,archived}/{class}/yyyy-mm-dd-topic.md`，不建 `INDEX.md`。原 ADR 中仍有效的决定按主题合并改写为 implemented 笔记；只服务于已删除的 Tauri/React 客户端的决定（例如按平台分批移交写者角色）不再保留。
- 过程稿（评审、检查点、交接、迁移计划、术语表）直接删除，留在 git 历史里：提交 `e3836e62` 是删除前的最后快照。`docs/research/` 的调研底稿与 `docs/archive/` 中的一半文件曾被 `.gitignore` 排除、从未入库，整理时被误删且无法恢复；`.gitignore` 因此改为 `docs/` 全部入库，删除前必须先确认目标已入库。
- `docs/backlog.md` 是产品候选库，不得删除或覆盖。
- `export-schema` 对 `event-topics.json` 中 `pending: <task>` 行的校验从"迁移计划里存在该 Task 标题"改为"任务 id 格式合法"，生成的契约字节不变。
- 校验：`pnpm verify-notes` 依次跑笔记树、格式与归档封印三个脚本（Node 24 直接执行 `.ts`，无 npm 依赖）；`pnpm verify-docs`（`scripts/native/verify-docs.mjs`）检查 `docs/` 每篇不超过 300 行（`docs/native/` 与 `backlog.md` 除外）、正文没有"以前/不再/曾经/原先/如今"等变迁词、所有 `AGENTS.md`/`CLAUDE.md`/`docs/` 的相对链接可解析。两者在 CI 的 `docs-and-notes` job 里运行（`.github/workflows/checks.yml`）。

### 旧编号对照

代码注释里的 `ADR-0xx` 与 `MIGRATION_PLAN §…` 保留原样。ADR 原文用 `git show e3836e62:docs/native/adr/<文件>` 查看，迁移计划用 `git show e3836e62:MIGRATION_PLAN.md`；现行决定在下表的笔记里（路径相对 `.agents/notes/implemented/`）。

| ADR | 笔记 |
|---|---|
| 001 | `architecture/2026-09-21-native-boundary-and-single-writer.md` |
| 002、003、005、006 | `architecture/2026-09-22-rpc-method-surface.md` |
| 004 | `testing/2026-09-21-gate-floors-and-positive-controls.md` |
| 007 | `architecture/2026-09-22-event-topic-namespace.md` |
| 008 | `architecture/2026-09-22-error-detail-allowlist.md` |
| 009 | `architecture/2026-09-22-protocol-version-boundary.md` |
| 010、011、027 | `architecture/2026-09-22-data-dir-writer-eligibility.md` |
| 012、014、015 | `architecture/2026-09-22-content-fingerprint-write-cover.md` |
| 013、024 | `architecture/2026-09-22-project-identity.md` |
| 016、017、020、025、030、032 | `architecture/2026-09-22-task-run-generations.md` |
| 018、019、022、026、037 | `architecture/2026-09-23-task-writes-are-intents.md` |
| 021 | 无（按平台分批移交写者，随 Windows/Linux 客户端退场失效） |
| 023、033、034、035、036 | `architecture/2026-09-24-core-is-task-event-authority.md` |
| 028、029 | `architecture/2026-09-23-agent-families.md` |
| 031 | `architecture/2026-09-24-session-path-claim.md` |
| 038 | `architecture/2026-10-02-terminal-replay-and-lease.md` |
| 039 | `process/2026-10-12-apple-only-distribution.md` |
| 040 | `architecture/2026-10-13-push-plane.md` |
| 041 | `bug-fix/2026-10-13-kill-target-signals-by-pid.md` |
| 042 | `feature/2026-10-23-grouped-sidebar-navigation.md` |
| 043 | `architecture/2026-10-25-structured-agent-protocols.md` |

## Alternatives considered

- **把过程稿整体挪进 `docs/archive/`**：一个字都不丢。否掉是因为仓库依旧臃肿，新会话检索时仍会命中大量过期内容；git 历史已经保存了全部原文。
- **ADR 留在 `docs/native/adr/` 原位、只对新决定写笔记**：skill 允许沿用现成决定记录，改动最小。否掉是因为这批 ADR 的正文混着大量变迁叙事与已删除客户端的细节，且相关决定拆得过碎（同一机制分散在五六篇里），原样保留无法达到"一个事实只在一处讲透"。
- **1:1 把每篇 ADR 转成一篇笔记**：可追溯性最好。否掉是因为同一机制（运行世代、事件权威）会继续散落在多篇里互相修订。

- **变迁词只靠评审把关，不做词法扫描**：没有误伤（例如合法地描述"失败后不再重试"）。否掉是因为对 agent 而言散文里的规矩等于没有规矩，变迁叙事正是这批文档腐化的主要形态；误伤时改写成现在时而不是加豁免。扫描只对 `docs/` 正文生效，代码块与行内代码不扫。

## Consequences

- **收益**：每类文字只有一个家；开工必读的规则缩到一屏；决策可按目录切片检索。
- **代价与已知上限**：原 ADR 编号（ADR-0xx）失效，代码注释中的旧编号与文档路径需逐步改指向笔记；被合并改写的笔记丢失了原 ADR 里的逐次修订记录与实测数字，需要时从 `e3836e62` 取原文。若笔记数量增长到按目录切片也难以检索，按 skill 的归档规则做一次里程碑清理。
