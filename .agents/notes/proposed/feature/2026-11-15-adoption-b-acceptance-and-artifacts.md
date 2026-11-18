# Agent Note: 调研落地批次 B：验收、证据与产物

Status: proposed

## Problem

[落地总纲](../architecture/2026-11-15-research-adoption-roadmap.md) 的批次 B 处理"完成"这件事的可信度。批次 A 让状态不说谎；这一批让"通过"两个字有依据。

Aeroric 已经有验收条件、自动检查、审查循环和换模型重跑。调研对照 207 个项目后指出的差距集中在四处：

- **验收可能装晚了。** 子任务先启动，验收义务后安装。子任务很快结束时，它可能在义务存在之前就被算作完成。
- **没改文件的回合不检查。** 只跑命令、只改了仓库外文件、或工作目录不是 Git 仓库的回合，契约里写的命令检查不会触发。
- **"通过"没有指向任何东西。** 验收结果只有 `id`、文字和状态。它不记是哪次检查、对哪个版本的文件得出的结论。文件之后再变，结论还显示"通过"。
- **交付物没有身份。** 回合产出的文件只存在于变更记录里。用户想问"这次交付了什么、哪个版本通过了检查"，没有对象可查；Agent 也没有工具读取上一个任务的产物。

本篇拆成 7 片。B1、B2 是缺陷修复，可以直接做；B3 依赖 B2；B6 是这一批里最大的一片；B7 依赖 B6 的版本概念但可以先做持久化部分。

所有位置都是 `main@8d4391c3` 上的。

## Proposal

### B1 验收义务先于第一条输入安装

2026-11-18 已实现，见 [B1 实现笔记](../../implemented/bug-fix/2026-11-18-b1-obligations-before-first-input.md)。

**用户会看到：** 没有新界面。带验收条件的委派，不会再出现"子任务结束了，验收栏是空的"。义务没存下来时，委派卡显示"没能启动：验收条件没存下来"，子任务不启动。

**现状：**

- `core/crates/aeroric-core/src/sessions/live/delegation/mod.rs:547–595`：开线程调 `host.start_child`，返回成功后才 `cp.add_obligations(&child.task_id, …)`。
- `core/crates/aeroric-host/src/session_live_services/delegation.rs:286–321`：子任务的 id 由 core 预先选定（`card.child_task_id`），`start_child` 里创建任务并发出第一条输入。

id 既然是预先定好的，义务可以在启动之前就写入。

**改法：**

1. 在调 `host.start_child` 之前，用预选的 `child_task_id` 调 `add_obligations`。
2. `add_obligations` 返回持久化结果。失败则不启动，卡片状态记失败并写原因。
3. `start_child` 失败时删除已安装的义务（同一个 id 之后可能被重试复用）。
4. 检查分叉路径（`delegation.rs:321` 的 `driver.prompt`）是否同样满足"先装后发"。
5. 审查循环、讨论模式若也给子任务装义务，同样改序。

**验证输入：** 假 host 的 `start_child` 在返回前就让子任务结束；义务存储注入写失败（落盘失败）；`start_child` 失败后义务库里不留该 id 的记录。

**完成条件：** 任何带验收条件的子任务，其第一条输入发出时义务库里已有对应记录。

### B2 没改文件的回合也跑契约里的命令检查

2026-11-18 已实现，见 [B2 实现笔记](../../implemented/bug-fix/2026-11-18-b2-command-checks-without-file-changes.md)。仅命令方式只跑任务自己的命令，项目自动发现的测试和诊断不跑；`inputRevision` 已写入检查更新，供 B3 使用。

**用户会看到：** 回合只跑了命令或只改了仓库外的东西时，契约要求的测试和命令检查照常运行，结果旁边写着"这一轮没有文件变化"。工作目录不是 Git 仓库时，写明"无法比较文件，只跑了命令检查"。

**现状：** `core/crates/aeroric-core/src/sessions/live/turn_changes.rs:130–205` 的 `turn_ended`：`checks.start(…)` 只在拿到前后快照差异（`found` 为 `Some`）且回合 `Completed` 时调用。`core/crates/aeroric-core/src/sessions/live/auto_check.rs` 的 `start` 在没有选中项且没有额外命令时返回 `false`。

**改法：**

1. 把"是否触发检查"和"是否有文件变化"分开。回合 `Completed` 时：有差异走现有路径；没有差异或没有快照时，只要任务有契约命令或未关闭的命令类义务，就以"仅命令"方式触发。
2. 检查记录加两项：`trigger`（文件变化、仅命令、手动）和 `input_revision`（有快照时是回合结束时的树对象 id；没有时为空并标注"无法确定版本"）。
3. 依赖文件差异的检查项（诊断、只测改动文件的测试）在仅命令方式下跳过，并在结果里写明跳过原因，不算通过。
4. 回合分级和待办里的显示沿用现有通道。

**验证输入：** `auto_check_tests.rs` 加：回合只执行了 shell 命令；工作目录不是 Git 仓库；有差异但契约没有命令（行为不变）；用户接管后的手动回合。

**完成条件：** 契约里写了命令的任务，每个 `Completed` 回合之后都有一条检查记录，或一条写明原因的"未检查"。

### B3 验收结果带依据，文件再变就不算通过

2026-11-18 已实现，见 [B3 实现笔记](../../implemented/feature/2026-11-18-b3-acceptance-evidence-and-staleness.md)。判旧复用 `staleness::stale`，在子任务汇报时和有人读委派列表时算，不存。

**用户会看到：** 每条验收旁边能点开"依据"：哪次检查、什么时候、对哪一版文件。文件之后又变了，这条验收变成"需要重新确认"，不再显示通过。

**现状：** `core/crates/aeroric-protocol/src/delegation.rs:228` 的 `AcceptanceResult` 只有 `id`、`text`、`state`；`:307` 的 `acceptance_passed()` 只看状态。结果投影在 `delegation/mod.rs:951` 附近。

**改法：**

1. 协议新增 `EvidenceRef`：`kind`（检查记录、回合、人工确认、审查结论）、`ref_id`、`revision`（B2 的 `input_revision`）、`at_ms`。
2. `AcceptanceResult` 和 `Obligation` 加可选的 `evidence: Vec<EvidenceRef>`、`checked_revision`、`stale: bool`。
3. 义务从 `Open` 变 `Passed` 时必须带至少一条依据。人工确认也是依据，`kind` 为人工确认，记确认人是用户。
4. 判旧规则：任务的后续回合产生了文件变化，且 `checked_revision` 不等于最新版本，则 `stale = true`。复用 `staleness.rs` 的既有机制（[批量撤销与过期结论](../../implemented/feature/2026-11-05-batch-undo-stale-conclusions.md)），不另写一套。
5. `acceptance_passed()` 改为"全部通过且没有过期项"。
6. `context.delegations` 的返回带上依据和过期标记，上级 Agent 能看到。
7. Swift：验收列表每行加依据入口和"需要重新确认"的状态。

**验证输入：** 检查通过后同名文件被替换（同名替换）；撤销回合后验收变旧；人工确认的验收在文件变化后同样变旧；旧数据（没有依据字段）读出来显示"依据未记录"，不算过期也不伪造依据。

**完成条件：** 界面上任何一个"通过"都能点到一条具体的依据；不存在版本已变而仍显示通过的验收。

### B4 检查结果保留原始判定

2026-11-18 已实现，见 [B4 实现笔记](../../implemented/feature/2026-11-18-b4-checks-keep-raw-verdicts.md)。可选项的规则已就位，目前没有哪类检查是可选的。

**用户会看到：** 检查失败时除了"测试 3 项失败"，还能看到原始退出码、发现的问题数、哪些是必须项、检查时数据是否完整。

**现状：** `core/crates/aeroric-protocol/src/turn_ledger.rs:63` 的 `TurnCheck` 只有 `passed` 和一句 `summary`。`auto_check.rs` 里的 `CheckSpec`、`CheckFailure` 有更多信息，但没有带到协议（动手前复核具体字段）。

**改法：**

1. `TurnCheck` 加可选的 `items`：每项有名称、`required`、原始判定（通过、失败、超时、没跑、被跳过）、退出码、问题数、耗时、输出是否被截断。
2. `passed` 的计算规则写明并测试：所有 `required` 项通过才为真；"没跑"和"超时"不算通过。
3. 数据不完整（输出被截断、诊断服务没响应）时单独标记，界面写"结果可能不完整"。
4. 审查门禁（[审查门禁开关](../../implemented/feature/2026-11-08-review-gate-flags.md)）读新的逐项结果，不再只读汇总布尔值。

**验证输入：** 必须项超时；可选项失败而必须项通过；输出超过截断上限。

**完成条件：** 汇总为"通过"时，逐项里没有未通过的必须项。

### B5 审查循环没有进展就停

2026-11-18 已实现，见 [B5 实现笔记](../../implemented/feature/2026-11-18-b5-review-loop-stops-without-progress.md)。

**用户会看到：** 审查循环在两轮之间没有实质变化时提前停下，写明"上一轮和这一轮的问题相同，改动也没有变化，交给你决定"。

**现状：** `core/crates/aeroric-core/src/sessions/live/review/mod.rs:442`：只有轮数达到 `MAX_ROUNDS` 才停。作者反复提交同样的改动、审查者反复给出同样的问题时，会把轮数耗完。

**改法：**

1. 每轮结束时记两个指纹：未解决的阻断问题集合（按 `parse.rs` 解析出的问题做规范化后哈希）和该轮改动的树对象 id。
2. 连续两轮两个指纹都相同，即判定无进展，停止并交给用户，原因写入卡片。
3. 只有问题集合相同而改动不同，继续（作者在尝试）；只有改动相同而问题不同，继续一轮后再判。
4. `MAX_ROUNDS` 保留为上限。

**验证输入：** `review/tests.rs` 加：假审查者每轮返回同样的问题且假作者不改文件；问题措辞略有不同但指向同一位置（规范化要能识别）；正常收敛的循环不受影响。

**完成条件：** 无进展的循环在第二个重复轮结束时停下，不再消耗后续轮次。

### B6 产物登记：交付物有身份和版本

2026-11-19 第一步（登记与只读）已实现，见 [B6 实现笔记](../../implemented/feature/2026-11-19-b6-artifact-registry.md)。和下文的出入：被打断的回合也登记；按任务存 JSON，不建表；`artifacts.*` 和其他实时方法一样不进 `methods.schema.json`；不复用 DEBUG 样张 `TaskDeliverablesReviewStore`。第二步（Context MCP 工具、委派结果引用版本）未做。

**用户会看到：** 任务页有"交付物"一栏，列出这个任务产出的文件，每个有版本列表、来自哪一轮、检查和验收的结论。可以打开任一版本、比较两个版本。其他任务里的 Agent 能通过工具读取指定版本。

**现状：**

- 回合变更记录（[回合撤销](../../implemented/feature/2026-11-04-turn-undo.md)）已经为每个回合保存前后树快照，文件内容在 Git 对象库里。
- `macos/AeroricKit/Sources/AeroricUI/TaskDeliverablesReview/` 下有 `TaskDeliverablesReviewStore` 和 `NativeTaskDeliverablesReviewView`，调研称目前只在调试构建里启用（动手前复核它的数据来源和启用条件）。
- Context MCP 没有读取产物的工具。

**改法：**

分两步，第一步完成后即可交付。

**第一步：登记与只读。**

1. 协议新增三个类型：
   - `Artifact`：`id`、`task_id`、`path`（相对工作目录）、`kind`（代码、文档、数据、图片、其他）、`created_ms`、`latest_version`。
   - `ArtifactVersion`：`artifact_id`、`version`（递增）、`turn_id`、`blob`（Git 对象 id）、`size`、`content_hash`、`at_ms`、`superseded`。
   - `ValidationRecord`：`artifact_id`、`version`、`evidence: EvidenceRef`（B3）、`verdict`。
2. 登记时机：回合 `Completed` 且有文件变化时，由 `turn_changes.rs` 把该轮新增或修改的文件登记为新版本。**只记引用，不复制字节**；内容从回合快照的对象里读。
3. 哪些文件算交付物：默认是该轮改动里未被忽略、非临时目录的文件；用户可以在交付物栏把某个文件标为"不算"。不做自动的重要性判断。
4. 存储放在现有任务数据库里新增两张表，遵守单写者约束。
5. 方法：`artifacts.list(taskId, cursor)`、`artifacts.versions(artifactId)`、`artifacts.read(artifactId, version, range)`。读取有字节上限并分页；对象已被 Git 回收时返回"这个版本的内容已不可用"，不返回空内容。
6. Swift：决定复用还是替换 `TaskDeliverablesReviewStore`（总纲决策 5）。能复用就把它的数据来源改为上述方法，并去掉调试限制。

**第二步：给 Agent 用。**

7. Context MCP 加 `artifact_list`、`artifact_read`（`core/crates/aeroric-cli/src/mcp_context.rs` 的 `method_of` 和工具清单）。读取范围受现有的任务令牌约束：只能读同一项目内、且与当前任务有委派或交接关系的任务的产物。
8. 委派结果卡和交接说明里，产出的文件改为引用 `artifact_id@version`。

**验证输入：** 同一路径在三个回合里各改一次，得到三个版本；回合被撤销后，该版本标记为已撤回而不是删除；同名替换（删除后以同名新建）算新版本并保留旧版本；跨项目读取被拒绝；对象不可用时的返回；分页不全时返回游标。

**完成条件：** 任务页能回答"这个任务交付了哪些文件、各自哪一版通过了哪次检查"；Agent 通过工具读到的内容与界面上打开的同一版本逐字节一致。

**不做：** 不做通用的二进制预览器；不把聊天里贴出的代码块算作产物；不做跨项目的产物库。

### B7 重跑对比要说明输入是否一致

**用户会看到：** 并排比较两次运行时，顶部写明两边的起点是否相同：工作目录起点、提示词、权限模式、技能版本。有差别时列出差别，并提示"结果的差异不一定来自模型"。

**现状：** 换模型重跑和并排比较已落地（[换模型重跑](../../implemented/feature/2026-11-11-rerun.md)），宿主入口在 `core/crates/aeroric-host/src/session_live_services/rerun.rs`。调研指出：重跑组的准入和输入快照没有持久记录，宿主重启后无法证明两边输入相同（动手前复核）。

**改法：**

1. 重跑发起时冻结一份输入清单并持久化：工作目录的起点树对象 id、提示词哈希、权限模式、挂载的技能及版本、Harness 与模型。
2. 每个重跑成员启动时记录自己实际使用的清单。
3. 比较视图逐项对比，不一致的项列出来。
4. 重跑组的成员关系持久化，宿主重启后比较视图仍可用。
5. 有了 B6 之后，比较的对象改为两边的产物版本。

**验证输入：** 重跑前用户改了文件（起点不同）；两次之间技能被升级；宿主重启后打开比较。

**完成条件：** 比较视图里"输入一致"的结论有持久记录支撑，不是推断。

## Alternatives considered

**B3 用调研建议的统一 `EvidenceLink` 大表，所有子系统的证据都进一张表。** 它最强的理由是以后查询"某个结论的全部依据"只需一处。没选它，因为 [第二份评审的复核](../architecture/2026-11-02-roadmap-recheck.md) 已经判断过：现在只有验收和产物两个使用方，各自内嵌一个小引用就够；等出现第三个使用方再抽。

**B6 把产物内容另存一份到应用数据目录。** 它最强的理由是不依赖 Git 对象的存活，非 Git 目录也能用。没选它，因为回合快照已经保存了同样的内容，另存会带来两份数据不一致和磁盘占用的问题。非 Git 目录本来就没有回合快照，这类任务的交付物栏如实显示"这个目录无法记录版本"。

**B5 用语义相似度判断两轮问题是否相同。** 它最强的理由是能识别措辞变化。没选它，因为需要再调一次模型，而且判断结果不可复现；按文件位置和问题类别做规范化已经能覆盖主要情形。

**B2 没有文件变化就不检查，只在界面上写"未检查"。** 它最强的理由是不增加运行开销。没选它，因为只跑命令的回合恰恰可能改变了数据库、生成了仓库外的文件，契约里的命令就是为这种情况写的。

## Acceptance criteria

- B1、B2、B5 各有先失败后通过的测试。
- B3 之后，协议里的 `AcceptanceResult` 在 `Passed` 状态下必有依据；旧数据兼容读取。
- B6 第一步完成时，`artifacts.*` 方法出现在 `contracts/native/v1/methods.schema.json`，并有夹具。
- `cargo test --manifest-path core/Cargo.toml --workspace`、`cd core && cargo fmt --check`、`swift test --package-path macos/AeroricKit` 通过。
- 落地时更新 [委派](../../implemented/feature/2026-10-31-delegation.md)、[审查循环](../../implemented/feature/2026-10-30-review-loop.md)、[换模型重跑](../../implemented/feature/2026-11-11-rerun.md) 三篇里的对应事实；B6 写自己的 implemented 笔记。
- 界面变化记入路线文档 §8 待目检清单。

## Risks

- **B2 会让一些任务多跑检查。** 只跑了一条查询命令的回合也会触发契约命令。要有去重：输入版本和命令都与上一次检查相同时复用上次结果，并注明"沿用上一次的结果"。
- **B3 的判旧可能过于敏感。** 改了一个无关文件也会让全部验收变旧。第一版接受这一点并提供"一键重新检查"；之后可以按检查实际读取的文件范围收窄，但那需要检查本身报告范围。
- **B6 依赖 Git 对象不被回收。** 回合快照使用的引用如果被清理，旧版本会读不到。实现前确认回合记录是否持有引用；没有的话，给登记过的版本加保留引用，并在磁盘占用里显示。
- **B6 的默认范围可能太宽。** 一次大重构会登记几百个文件。交付物栏默认按回合折叠，并限制单个任务的登记总数，超出时写明"只列了最近 N 个"。
- **B7 的技能版本依赖批次 C 的 C7。** C7 落地前，这一项如实显示"未记录"。
