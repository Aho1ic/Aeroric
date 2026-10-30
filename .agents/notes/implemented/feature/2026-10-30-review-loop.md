# Agent Note: 审查回环（B4-1）：另一族 Agent 只读审查每回合改动，写代码的会话逐条答复

Status: implemented

## Problem

单个 Agent 写完代码后，没有人用"新眼睛"看一遍。前沿调研（`docs/frontier-roadmap.md` §5.2 形态 A）认为审查回环是多 Agent 协作里收益最确定的一步。另外，跨 Harness 组合（例如 Codex 写、Claude 审）是 Aeroric 独有的能力。owner 2026-10-30 确认了 Q-B1–Q-B7：B4 拆成三批，先做 B4-1（Trellis 任务 `10-30-review-loop`）。

## Decision

**核心状态机**（core `sessions/live/review`）

- `ReviewLoop` 是一个 sink 层，位置在宿主 sink 和 CheckpointSink 之间：host ← ReviewLoop ← CheckpointSink ← TurnChangeTracker。这样它能看到 reviewer 与 coder 的回合、消息，以及 `autoCheck` 结果。
- 回合改动追踪器在每个被跟踪回合的快照完成后调 `turn_settled`。参数包括：
  - 两个树 id 和文件列表（`TurnDiff`）；
  - 项目检查是否接着要跑（`AutoChecker::start` 的返回值）。
- 新回环的触发：会话开关打开、回合正常结束、有改动。项目开了检查时，等 `autoCheck` 终态再审；终态和快照到达的先后不定，两种顺序都处理。
- 状态：`reviewing → waitingSend（暂停）/ waitingCoder → passed / settled / escalated`，另有 `failed` 和 `interrupted`。
- 判定规则：
  - 无阻断问题即 `passed`（Q-B7），不论审查者自己怎么说。
  - coder 有任何 `ask`，或某条没有答复 → `escalated`。
  - 没有采纳，或采纳了但本回合没改动 → `settled`。
  - 采纳且有改动 → 下一轮，复用同一个审查会话；第 3 轮之后仍需改 → `escalated`（P17）。
- 宿主侧的启动会话和发消息都在单独线程上执行，sink 回调从不等待会话。
- 每次状态变化：
  - 向父会话日志追加一条 `review` 更新，同一 `reviewId` 的新卡替换旧卡；
  - 存到 `<data>/reviews/<task>.json`。
- 重启后，从磁盘读到仍在进行中的回环，改记为 `interrupted`，卡上给"重新审查"。

**提示词与解析**

- reviewer 拿到的内容：本回合 diff（6 万字符以内内联，否则只给文件列表和 `git -C … diff <before> <after>` 命令）、检查点里的验收条款与约束、自动检查结论，以及上一轮的意见与答复。拿不到对话（Q-B4）。
- reviewer 末尾给一个 JSON 代码块。coder 收到逐条意见，末尾给 `responses`。
- 解析取最后一个 json 代码块，词汇宽松：阻断、blocker 都认；采纳、adopt 都认。

**宿主**（`session_live_services/review.rs`）

- 任务记录的 `reviewLoop` 字段存 `{enabled, reviewer: "auto" | {agent, agentConfigId?, model?}, paused}`。
- reviewer 是一个真实任务：
  - `reviewOf=<父任务>`，`permissionMode=plan`；
  - 复制父任务的 worktree 字段，因此在同一工作目录运行；
  - 名字为"<父名>（审查）"。
- 带 `reviewOf` 或 `branchOf` 的任务不会被审查。
- 候选审查者只有本机的 Claude Code 与 Codex。产品二进制会检查是否已安装，测试宿主不检查。auto 优先选与 coder 不同族的，没有时退回同族并标"同族"。
- 方法：`review.get / set / request / send / retry / reviewers / attention`。

**App**

- 时间线：每个 `reviewId` 一张审查卡，就地更新。问题行按严重度排，展开可看理由。按钮有：发给写代码的会话、之后先别自动发 / 之后每轮自动发、重新审查、打开审查会话。
- 改动行旁有"请人审查"胶囊按钮。
- 进度页新增"审查"小节：开关、审查者胶囊菜单、"请人审查"、本会话最近一次回环的摘要。
- 侧栏：审查会话、侧支缩进挂在父会话下，带眼睛或分支小图标。
- 需要我处理：新增"审查要你定"。

## Alternatives considered

- **用 fork 给 reviewer**：带着 coder 的对话，失去"新眼睛"（Q-B4）。
- **reviewer 直接写补丁**：违反单写者原则（J1），只让它用文字给建议。
- **每轮新开审查会话**：要重复说明背景。复用同一会话不破坏隔离，因为审查者自己的历史不是 coder 的对话。复用只在审查者选择没变时成立：回环记下它按哪个选择挑的审查者（`chosen`），选择变了，下一个回环就新开审查会话；同一回环内不换人，因为审查者要记得上一轮的意见。被换下的旧审查者如果晚到一个回合，不算当前回环的结论。
- **把回环状态写进任务记录**：任务记录频繁改写、受写入基线约束；改为和检查点一样单独存文件。
- **复用 `attach_session`/通用 `task.fork` 起 reviewer**：reviewer 必须是一个新会话、只读、带首条提示，所以直接 `create` 后 `start`。

## Consequences

- 与 design.md 的偏差：
  - "本轮先别发"实现为"之后先别自动发"：作用于之后的轮次。已经发出的意见收不回。
  - 审查卡不显示审查用掉的 token。
  - 第一版不支持远程、omp、DSH 当审查者。远程会话没有本地快照，也不会被审查。
- 已知风险：Claude 在 plan 权限下有时会调用 ExitPlanMode 请求批准，审查回合会停在审批上。这时用户在审查会话里拒绝即可，回合结束后照常解析。没有用真实 Claude 验证。
- 测试：
  - core：状态机 13 个，解析 3 个；
  - 协议：2 个；
  - 宿主：端到端 1 个。假 Codex 写文件，假 Claude 只读审查（提示不含对话），阻断意见回到 Codex，驳回后 settled，进入 attention；
  - Swift：契约 1 处、文字 4 个。
- 2026-10-31 owner 用 computer use 真实目检（cline → deepseek-v4.1-flash，Codex 写、Claude Code 审），自动触发、暂停待发、手动发送、ask 升级、adopt 后复审通过、失败卡重试都跑通。修了两处：
  - B4-CU-01（B4′ 遗留）：core 的 accepted 回执多报一项 `clarify`，Swift `NativeSubmissionReceipt.matches` 仍按 10 项核对，新建会话一律落入"接收结果未知"，核对也失败。Swift 补上 `clarify`，测试覆盖 clarify 为假/真两种回执和查询。
  - B4-CU-02：改选审查者后，新回环仍复用旧审查会话。见上文 `chosen`；测试 `a_changed_reviewer_choice_starts_a_new_reviewer`。
- 未覆盖：自动检查完成后再审、验收条款传递、全驳回结束、3 轮上限、ExitPlanMode、远程/omp/DSH 的运行期限制。
