# Agent Note: B3 验收结果带依据，文件再变就不算通过

Status: implemented

## Problem

[调研落地批次 B](../../proposed/feature/2026-11-15-adoption-b-acceptance-and-artifacts.md) 的 B3。委派卡上的验收结果只有 `id`、文字和状态：

- "通过"不指向任何东西：是哪次检查、对哪一版文件得出的，没记；
- 文件之后又变了，结论还显示"通过"，`acceptance_passed()` 只看状态；
- 检查点里已经有判旧机制（R2-3a，`staleness.rs`），比较结论成立时和现在的文件内容，但只有进度页打开时才算，委派卡和上级 Agent 读到的验收不用它。

## Decision

- **协议**：
  - `Obligation` 加 `evidence`（依据列表）和 `checkedRevision`（检查时的树对象 id）；
  - `AcceptanceResult` 加 `evidence`、`checkedRevision`、`stale`；
  - `EvidenceKind` 加 `userConfirmation`；
  - 新字段都可选，旧数据读出来是空。
- **谁写依据**：
  - 自动检查判定一条验收时，依据是那次检查更新（`autoCheck`，记 `seq`、回合号、回合 id），`checkedRevision` 取 B2 记下的 `inputRevision`；
  - 手动设定（`checkpoint.setObligation`）时，依据是"用户确认"。
- **判旧**：复用 `staleness::stale`（R2-3a），不另写一套：
  - 委派的 `Delegations` 拿到回合账本（`set_ledger`），投影验收时对已通过的条目算一次；
  - 结论依赖的文件后来变了（后续回合、用户手改、撤销），`stale = true`；
  - `acceptance_passed()` 改为"全部通过且没有过期项"，信誉统计、"宣告完成"探针都跟着变。
- **什么时候算**：子任务汇报时算一次。另外，上级 Agent 调 `context.delegations`、App 读委派列表之前重算一次（`refresh_acceptance`）：
  - 包括还在运行、合同里带验收条件的卡片；
  - 有变化才推送卡片更新。
- **App**：
  - 过期的通过显示"需要重新确认"（警示色），不计入"验收 x/y"；
  - 每条验收旁一行依据："依据：自动检查 · 第 N 轮"、"依据：你确认的"、"依据未记录"（B3 之前的数据，不伪造）；
  - 悬停看检查时的文件版本和过期原因。

## Alternatives considered

**把 `stale` 存进检查点，文件变化时就地标记。** 最强的理由是读的时候不用跑 git。没选：文件可能被 Aeroric 以外的东西改（用户手改、别的工具），只有读的时候比较才不会漏；R2-3a 已经定了"只读、按需计算、不存"，这里沿用。

**依据只记检查的回合 id，不记树对象 id。** 最强的理由是字段少。没选：同一回合的检查可能被重跑，树对象 id 才是"对哪一版文件"的确切答案，B2 已经免费算出来了。

## Consequences

- **收益**：每个"通过"都能说出依据；文件变了的通过不再算数，上级 Agent 和用户看到的是同一个结论。
- **代价**：
  - 读委派列表前要对已通过的验收跑一次 git 比较（批量 `cat-file`，和进度页同一套）；
  - 运行中带验收条件的卡片，现在就会显示"未核对"的验收行。
- **验证**：
  - delegation 新增 `a_passed_criterion_has_its_evidence_and_goes_stale_when_files_change`：用真实仓库和回合账本，自动检查通过后依据和版本都在；手改文件后重算变成过期、不算通过；手动设定后依据是用户确认；
  - Swift 新增 `testACriterionSaysWhatItRestsOnAndAStalePassIsNotPassed`；
  - 界面待目检，记在路线文档 §8。
