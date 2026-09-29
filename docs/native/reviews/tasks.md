# Module review — `tasks` (Task P2-04)

> **数字口径（2026-09-29 加，审计出处 `MIGRATION_PLAN.md` 补记一百七十九）**：本文件里的 case 数／用例数／`minCases` 一律是**验收当天的快照**，只作证据留档，不是今天的地板值——模块在此之后被别的 Task 扩过。要看今天的数字：现场跑 `node scripts/native/check.mjs <module>`，或读 `scripts/native/checks.json` 各 case 的 `minCases`（`inventory --check` 会核对地板与冻结清单一致）。**既不要把这里的旧数字当成回归证据，也不要为了两边对齐而去改任何一份带日期的记录。**

- **Reviewed**: 2026-09-22
- **Design source**: `MIGRATION_PLAN.md` §Phase 2 · Task P2-04
- **Code**: `core/crates/aeroric-core/src/tasks/{mod,reducer,session_owner,launch_plan,executor,operations,service,handoff,cleanup}.rs`,
  `core/crates/aeroric-core/src/storage/{fingerprint,tasks,runtime}.rs`,
  `core/crates/aeroric-core/tests/{task_lifecycle,task_idempotency,task_parity,common}/*.rs`,
  `src/test/task-core-parity.test.ts`,
  `core/crates/aeroric-protocol/knowledge/task-parity-cases.json`,
  `contracts/native/v1/fixtures/task-transitions.json`
- **Gate**: `pnpm native:check tasks` → **7 cases / 99 test cases**
- **Gate status**: **PASS —— P2-04 accepted（2026-09-23）**；`grill-me [tasks]` 已真实调用，Q35～Q43 九个答案落成 ADR-014～019 + 代码 + 反例

## 审查 commit 锚点（2026-09-29 反查补齐；§4.2 要求的这个字段当时没有记）

`MIGRATION_PLAN.md` §4.2 明文要求每份评审保存七项——**tool 来源/revision、实际调用入口、审查 commit/diff hash、问答、证据、问题级别、修复后复验结果**。本轮插入本节之前全量普查 `docs/native/reviews/`：**40 位十六进制串只有 2 处（插入之后本节自己会再引入锚点，那是预期结果），且两处都是外部技能 pin**（`c55ee46073ed923f86ce59a5eb3b6d895095d1b7`＝`mattpocock/skills` 的 grill-me，见 `docs/native/reference-provenance.md`）⇒ **九份载体没有一份记过被审查仓库的 commit**。这一节用 `git log -1 --until=<日期>T23:59:59` 反查补齐，**是事后重建，不是当时的记录**。

- Task **P2-04** accepted_on＝2026-09-23 ⇒ 当天最后一个提交＝`bf57afaf88dd190d1fa8a514e271c1ac1134ea83`（bf57afa，2026-09-23，「feat(native): P2-05 执行器与运行身份落地——Tauri adapter 的宿主侧收口到一半」）

**局限（必须一起读）**：反查锚点只覆盖**当时已提交**的树；acceptance 常在脏树上完成，未提交部分**无法事后重建**，所以 §4.2 要的「diff hash」这一格今天仍然是空的。缺口已登记为 `G31`：今后的每一次 `grill-me` 必须**当场**写 `git rev-parse HEAD` ＋ `git status --porcelain` 的文件数与路径摘要（`docs/native/reviews/*` 的内容不被任何 case 解析，机器无法事后追补）。本文撰写时 HEAD＝`797e06c3c35ce9b28b53be3f0149894e6e038a8a`（797e06c 2026-09-28 11:04:54 +0800）。

## 1. 验收标准

1. `pnpm native:check tasks`：完整状态转换表、同 ID 重试只 spawn 一次、旧 generation 的 exit 不覆盖新 generation、取消/手动完成/迟到 hook 的竞态。
2. fake runtime 没有 GUI listener 时 start/resume/cancel 全部可执行；持久化失败时不得启动进程。
3. **继承 P2-02 的阻塞项（ADR-012 决策 2）**：任务写入必须过内容指纹（`(sha256,size)` 不符 → `REVISION_CONFLICT`，并同步 bump `RUNTIME_SCHEMA_VERSION`）；反例至少两条——旧客户端先写过、人工把文件换回旧内容。
4. **调用 `grill-me [tasks]`**；特别追问 React 尚有哪一个业务决策未移走，清单必须为零或明确阻塞项。

## 2. 交付形状

| 端口 | 内容 |
| --- | --- |
| `reducer` | 状态机纯函数：`apply_status_transition`（completedAt 首值规则、absent≠null）、归档/收藏/改名、`rollback_mutation`（只回滚"本次真改了 ∧ 现在还是本次的值"）、`merge_changed_session_fields` |
| `session_owner` | 会话归属优先级（sessionFamily > sessionCodexLike(+dsh 特判) > 唯一存在族 > 配置 agent）、四族字段、原生续跑/收养/交接三策略、`apply_resolved_session` 的"一族独占" |
| `launch_plan` | 记录 → 带类型的请求：worktree 覆盖项目路径、dsh 走自身通道且 **resume 不带原 prompt**、取消通道、完成三路分支（`is_live_for_completion` 与 `TaskStatus::is_live` 在 detached 上刻意不同） |
| `executor` | `TaskExecutor`/`TaskRuntimeObserver` 端口，请求与事件都带 generation。P2-05 注入 legacy executor，P3-01 换实现 |
| `operations` | `(taskId, kind, generation)` 认领 + 世代计数，落在 core 独占的 `task-operations.json`；崩溃对账 `reconcile` → `interrupted`，绝不自动重启 |
| `service` | 唯一编排者：**先认领 → 再持久化 → 才 spawn**；持久化失败 = 零 spawn；spawn 失败 = 回滚本次字段并写 failed+原因，同时释放认领 |
| `handoff` | 终端控制序列剥离（安全边界）+ 重绘帧/.spinner/状态栏/400 行去重窗（可读性），`src/sessionHandoff.ts` 的逐条移植 |
| `cleanup` | 保留期与调度：正向终态过滤、starred/未处理 worktree 豁免、`completedAt ?? createdAt`；周槽的时区问题下沉为 `LocalCalendar` 端口（core 定规则，宿主报时区，与 ADR-010 同形状） |

## 3. 门禁问答（Q35～Q43，2026-09-23）

| # | 问题 | 决定 | 落点 |
| --- | --- | --- | --- |
| Q35 | 内容指纹的摘要用 `sha2` 还是 core 内手写 | **用 `sha2 0.10.9`**：它已在发货产品的 `src-tauri/Cargo.lock:7774` 里，不是新供给链；离线可解析；实现只包在 `Fingerprint::of` 一处，事后反悔不会改变磁盘上任何已存指纹。代价如实记录：`--locked` 会传播到产品 lockfile，必须同步更新 | ADR-014 |
| Q36 | 指纹基线的语义 | **"只能覆盖 core 上一次发布的字节"，core 从未写过的文件首次一次性采纳现状**。反证：读取时也刷新基线会让检查永久失效（每个写路径都先 load），实测 M7 三条变红；没有基线就一律拒绝则迁移无法开始 | ADR-015 |
| Q37 | generation 与认领住在哪里 | **两处都写，`task-operations.json` 为唯一权威**；任务记录里是镜像，每次写按权威值改写（含改小、含 0），因为"看着权威其实不是"比丢历史更糟 | ADR-016 |
| Q38/Q39 | 去重键与并发语义 | **重试身份 = 调用方的 operation key，不是 core 的运行编号**；同一任务允许多代同时在飞（双开）。反例 M4 证明"先取号后查重"会让去重形同虚设 | ADR-017 |
| Q40 | 重叠时终止事件怎么归属 | **还有别的代在飞就不落终态**，任务回 `running`、不盖 `completedAt`；最后一代结束才落终态与原因。连带修正：陈旧判定从"比最新号小"改成"那一代已无在飞认领"，否则 run 1 的真实退出会被当成旧事件丢掉 | ADR-017 决策 5 |
| Q41/Q42 | 崩溃对账到什么程度、确认挡哪些动作 | **需要用户确认；只挡 `resume`**。`start` 顺手清掉待确认（从头开始本身就是确认），删除一并清理。因状态词表在 v1 冻结（schema/Swift/TS 三处），确认状态不占词汇表，住在账本里 | ADR-018 |
| Q43 | 剩余残余项 | **登记为明确阻塞项**，不在本轮悄悄扩大范围：会话引用阶梯→P3-04；local router 探测/切换→P3-01/02；转录探测与收养落盘→P3-02/04；worktree 的 git 副作用→P6-05（P2-05 临时端口，不许静默 no-op）；保存队列的合并节奉与 revision 对账→P2-05 | 计划 §Phase 2 各任务行 |
| 实现期自证 | 顺手抓出并修掉三处 | ① 对账给 `interrupted` 盖了 `completedAt`（它不是终态）；② `switch_config` 曾在末尾才认领（同键重试会双开）；③ 远程附件/worktree 只在 UI 里挡 | ADR-019、代码 |

## 4. 必须问过的六件事（原 frontier，已全部走完）

1. **依赖**：内容指纹用 `sha2 0.10.9`（已在 `src-tauri/Cargo.lock:7774` 的既有图里，core 离线可解析）还是零依赖手写？P2-02 曾以"零新增依赖"为方向。
2. **指纹基线的语义**：现在的规则是"只能覆盖 core 上一次发布的字节"，且 **core 从未写过的文件第一次直接采纳现状**。若改成"读取时也刷新基线"，检查会永久失效（每次写前都读）；若改成"没有基线就拒绝"，迁移永远无法开始。这条边界要确认。
3. **generation 归属**：世代计数放在 core 独占的 `task-operations.json`，而不是任务的记录里（旧客户端整表覆盖会抹掉它）。这使 generation 无法随项目导出/导入，要不要在契约里补一个跨进程可见的字段？
4. **认领键的判定顺序**：现在是"该 (task,kind) 有未决认领 → 拒绝"，因此**跨世代的并发启动也会被拒**（两个窗口都想跑同一个 todo，只有一个能起）。这是不是要的语义？另一种是只按精确 generation 去重，那等于允许双开。
5. **`reconcile` 的边界**：`detached` 保留、终态不动、其余改成 `interrupted` 并盖 `completedAt`。要不要把"上一次进程可能被我们没看见的方式结束"做成需要用户确认的状态（而不是自动 `interrupted`）？
6. **React 尚存的业务决策**：委派侦察给出 50 条清单（`docs/native/extraction/task-lifecycle-react.md`），本轮把状态机/归属/时序/幂等/保留期都搬了过来，但**会话引用解析阶梯**（`read_session_id` → legacy → `recover_task_session`）与 **local router 目标切换 / 转录收养落盘** 仍在宿主侧（它们读磁盘和 home，属于 P3-02/P3-04 的端口）。这两项要确认是"明确的阻塞项"而不是"漏掉"。

## 5. 反例与门禁自证

每条都是"改坏实现 → 门禁必须变红 → 还原并逐文件 `cmp` 确认字节一致"，全程不使用 git 破坏性命令。9 次（M8/M9 为 Q38/Q40 补跑），全部还原后逐文件 `cmp` 确认字节一致；其中一次当场抓到自己的守卫写错。

| # | 改坏什么 | 必须变红的用例 | 实测 |
| --- | --- | --- | --- |
| M1 | detached 迁移守卫换成别的状态 | task_parity + task_lifecycle | 1 + 1 红 ✓ |
| M2 | `completedAt` 改成每次都刷新 | task_parity | 1 红 ✓ |
| M3 | 去掉 `check_cover` 的比对 | task_lifecycle + storage 单测 | 4 + 1 红 ✓ |
| M4 | `open_claim` 永远查不到（模拟"先取号后查重"） | task_idempotency | **5 红** ✓ |
| M5 | dsh resume 也带上原 prompt | task_lifecycle + launch_plan 单测 | 1 + 1 红 ✓ |
| M6 | 去掉 spinner/状态栏过滤 | task_parity | 1 红 ✓ |
| M7 | `commit` 改用调用方刚读到的基线（而不是 core 上次发布的） | task_lifecycle | **3 红** ✓ —— 这条证明"读取时也刷新基线"会让指纹永久失效 |
| M8 | 让 `attempt_key` 不再包含 operation key（等价于退回"运行编号就是重试身份"） | task_idempotency + task_lifecycle | 1 + 1 红 ✓（Q38 的机制面） |
| M9 | 终止事件无条件落终态（去掉重叠判断） | task_idempotency | 1 红 ✓（Q40） |

一次自曝：M4 首次跑时把 `operations.rs` 忘了备份，改坏后无法用备份还原，只能用逐字符反向替换恢复并当场验证 8/8 转绿——已记入"度量陷阱"。

## 6. 度量口径

- `pnpm native:check tasks` **7 case／105 用例**：27（lifecycle：时序、内容指纹两反例、对账+待确认闸、完成三路、删除保护、远程通道限制、配置切换全链）+ 10（幂等：同键一次、跨键双开、陈旧/未来代、重启存活、镜像不作权威）+ 4（parity 重放：**283 条**客户端期望逐条一致，0 条声明分歧）+ 61（纯端口单测）+ 3（TS 侧派生断言）。
- 回归：`storage` 5／**183**、`projects` 4／18、`protocol` 5／88、`skeleton` 20／113、`doctor` 18/18、`pnpm typecheck` 0 错、`clippy --all-targets -D warnings` 干净、`cargo fmt --check` 干净。
- parity 表 283 条分布：状态跃迁 90、迁移守卫 36、会话字段 32、终端清洗 34、切换策略 28、归属/谓词 24、完成命令 9、调度 9、回滚 5、resolved session 5、保留期 3、列表变更 6。
- 回归：`storage` 5/177、`projects` 4/18、`protocol` 5/88、`skeleton` 20/113、`doctor` 18/18。
- 全量 `pnpm test` 与 `cargo test --manifest-path src-tauri/Cargo.toml --locked --lib` 见 §6 实测输出。
- 真实 `~/.aeroric`：本轮不写真实目录（所有测试都在临时目录），口径沿用 `reviews/storage.md`。

## §4.2 七字段落点（2026-09-29 反查补齐，出处 `MIGRATION_PLAN.md` 补记一百九十二；本节只回答「每个字段落在本文哪一处、哪一格当时是空的」，不重述内容）

- **tool 来源/revision**：无独立 provenance 小节；pin `c55ee46073ed923f86ce59a5eb3b6d895095d1b7` 见上面锚点节，权威出处 `docs/native/reference-provenance.md` ⇒ **当时未记**。
- **实际调用入口**：§3 的问答即调用内容（标题带日期与 Q 范围）；发送原文未记档。
- **审查 commit/diff hash**：见上面「审查 commit 锚点」一节——commit 由 `git log -1 --until` **事后反查**，diff 句柄**当时没记、今天补不回来**（缺口＝`G31`）。
- **问答**：§3「门禁问答（Q35～Q43，2026-09-23）」9 条 ＋ §4「必须问过的六件事（原 frontier，已全部走完）」。
- **证据**：§1「验收标准」＋§2「交付形状」＋§5「反例与门禁自证」。
- **问题级别**：**未使用级别词汇**（事后判读）：§5 的结论属**门禁有效性级**（反例确实能翻红，不是空跑）；§6「度量口径」是记录级更正；无崩溃／数据丢失级发现。**接受人**：本文无待接受项。
- **修复后复验**：2026-09-29 同日 `native:check tasks` **7／151 exit 0**；当时的改坏—修回对账在 §5。
