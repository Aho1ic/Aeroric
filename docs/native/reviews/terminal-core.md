# tauri → native 迁移评审记录:terminal-core(Task P3-03)

> **数字口径（2026-09-29 加，审计出处 `MIGRATION_PLAN.md` 补记一百七十九）**：本文件里的 case 数／用例数／`minCases` 一律是**验收当天的快照**，只作证据留档，不是今天的地板值——模块在此之后被别的 Task 扩过。要看今天的数字：现场跑 `node scripts/native/check.mjs <module>`，或读 `scripts/native/checks.json` 各 case 的 `minCases`（`inventory --check` 会核对地板与冻结清单一致）。**既不要把这里的旧数字当成回归证据，也不要为了两边对齐而去改任何一份带日期的记录。**

## 审查 commit 锚点（2026-09-29 反查补齐；§4.2 要求的这个字段当时没有记）

`MIGRATION_PLAN.md` §4.2 明文要求每份评审保存七项——**tool 来源/revision、实际调用入口、审查 commit/diff hash、问答、证据、问题级别、修复后复验结果**。本轮插入本节之前全量普查 `docs/native/reviews/`：**40 位十六进制串只有 2 处（插入之后本节自己会再引入锚点，那是预期结果），且两处都是外部技能 pin**（`c55ee46073ed923f86ce59a5eb3b6d895095d1b7`＝`mattpocock/skills` 的 grill-me，见 `docs/native/reference-provenance.md`）⇒ **九份载体没有一份记过被审查仓库的 commit**。这一节用 `git log -1 --until=<日期>T23:59:59` 反查补齐，**是事后重建，不是当时的记录**。

- **P3-03** 仍是 in_progress，没有「accepted_on」字段；可用的两个锚点＝提问轮当天最后一个提交 `2befb12bf4dcdacdd09f42ca0621ec254ef9b987`（2befb12，2026-09-27）与本文撰写时 HEAD `797e06c3c35ce9b28b53be3f0149894e6e038a8a`（797e06c，2026-09-28）

**局限（必须一起读）**：反查锚点只覆盖**当时已提交**的树；acceptance 常在脏树上完成，未提交部分**无法事后重建**，所以 §4.2 要的「diff hash」这一格今天仍然是空的。缺口已登记为 `G31`：今后的每一次 `grill-me` 必须**当场**写 `git rev-parse HEAD` ＋ `git status --porcelain` 的文件数与路径摘要（`docs/native/reviews/*` 的内容不被任何 case 解析，机器无法事后追补）。本文撰写时 HEAD＝`797e06c3c35ce9b28b53be3f0149894e6e038a8a`（797e06c 2026-09-28 11:04:54 +0800）。

## 0. 调用证据(§4.1/§4.2 要求的 provenance)

| 项 | 值 |
|---|---|
| 工具来源 | `mattpocock/skills`(MIT),固定 revision `c55ee46073ed923f86ce59a5eb3b6d895095d1b7`(见 `docs/native/tooling.md` §2) |
| 调用链 | `grill-me/SKILL.md` 全文即 `Call the Skill tool with "grilling"` → 本轮真实入口为 `Skill(skill="grilling", args=审查模块：terminal-core …)`(§4.2 口径,与 `reviews/agents.md`、`reviews/runtime.md` 同一条链) |
| 轮次协议 | grilling 的 design-tree + frontier rounds:本轮把整条 frontier 一次性提出并**等待用户回答**;未答问题不得写成已通过 |
| 审查基线 commit | `2befb12`(工作树仅 `.zcodeignore` 未跟踪;`git diff --check` 干净) |
| 门禁实测 | `pnpm native:check terminal-core` → **11 case / 92 用例**,exit **0**(本轮 2026-09-27 10:38 真实输出,逐条 ok:`terminal-unit-rust` 60、`terminal-pty-bytes-rust` 6、`terminal-lease-rust` 4、`terminal-replay-rust` 5、`terminal-pty-exit-rust` 10、四条宿主源码守卫、`terminal-history-rust` 7、`single-pty-activation-mechanics`) |
| 审查面 | core `terminal/{mod,stream,lease,pty,exit,replay,input,history,executor}.rs`;`tests/{pty_bytes,terminal_replay,terminal_lease,pty_exit}.rs` + `tests/fixtures/fake-agent.mjs`;宿主 `src-tauri/src/{lib,pty,storage}.rs`、`runtime/registry.rs` |

## 1. 五轴自查(事实,非问题)

1. **唯一 owner**:字节的真相 = core `ByteStream`(原始字节、offset=字节计数、1 MiB 窗口、显式 Gap);谁能改 PTY = core `ControlLease` + `decide_resize` 一张表;退出结论 = core `settle_exit` 唯一决策面;重放映射 = core `plan_replay`;磁盘历史的压实策略 = core `terminal/history.rs`。宿主剩的必须是**它才有的事实**:有没有活跃 master、尺寸信箱(`pending_pty_sizes`)、文件 I/O 与权限收紧、Tauri sink 的字符串投影。PTY 装配五步机械唯一一份在 `executor.rs::activate_pty_session`,宿主七份拷贝清零(守卫 `single-pty-activation-mechanics`)。
2. **反例面**:五次变异各有命中(摘掉租约早退、宿主换回内联 resize、`needs_compaction` 的 `>` 翻 `>=`、宿主重新出现本地 `utf8_tail_bytes`、暂存排水被 `.filter(|_| false)` 吞掉/宿主重新出现 `.spawn_command(`);活流码元游标的集成断言是补记一百按四A 记的"覆盖不均"补上的,补的时候当场纠正了四A 一条错误预期(拒收不是空手而回,是 `Reset{from:0, bytes=现存窗口}`);控制探针抓过一次我自己写错的正则(补记九十五)。
3. **权限/路径/token/日志边界**:终端历史文件名经 `safe_task_history_name → validate_storage_id → core_bridge::validate_id`(规则在 core,P2-02 Action 5),宿主有拒绝用例 `storage_ids_reject_path_traversal`(`storage.rs:1283`,在 2465 地板内);真实历史文件权限位 `-rw-------` 且目录走 `ensure_private_dir`;本模块零新增 token 面;日志面只有 `RefusedFull` 的一行诊断,不含会话内容。
4. **回归面**:旧客户端逐值等价有机器证(宿主 4 条既有 `terminal_history_*` 测试原样跑通;resize 判定"忽略集相同"由单元钉住;`tauri-adapter` 12/2526 含 src-tauri 2465 串行地板);`~/.aeroric` 真实语料旁证见 §2.4;全量 `pnpm test` 376/5483、`pnpm typecheck` 绿。
5. **回滚**:`ByteStream`/租约/决策面全在内存,无落盘格式变化;压实策略只是"谁算"搬家,文件上界与读语义未变;每个切片都是独立本地提交,回退点 = `git reset --soft HEAD~1`(不碰工作区),INV-01 全程未 push、无 tag、不发 release。

## 2. 本轮新查明的四条事实(逐条附出处)

1. **信号信息没丢在 OS 层,丢在 crate 的 API 边界**:`src-tauri/vendor/portable-pty/src/lib.rs:164-226` 的 `ExitStatus` 有**私有** `signal: Option<String>`,`From<std::process::ExitStatus>` 在 unix 分支用 `status.signal()` + `libc::strsignal` 填它,`Display` 会说 `Terminated by …`;公开面只有 `success()`/`exit_code()`。该 vendor 是**仓库内跟踪的 path 依赖**(`src-tauri/Cargo.toml:142`,`git ls-files` 命中),加一个访问器是仓内改动而非上游改动。结论:core "看不见信号"是**可修的**,不是平台限制。
2. **失败文案是端到端不透明文本,零处解析**:`host_sentence()` → `core_adapter.rs:688` `report_task_status_with_reason` → `TaskEvent::Status{failure_reason}` → Tauri `task-status` + `reducer.rs:133` 落 `tasks.json` 的 `failureReason`;前端唯一展示点 `RunningView.tsx:1119`,唯一内容敏感消费者是 `agentRuntimeRecovery.ts:6-21` 的 `includes("claude")`(新文案不得含 `claude`);远程 `events_bridge.rs:13` 原样转发,mobile 的 patch 处理器**故意忽略**该字段。全仓无一处对 `"Process exited with code"` / `"non-zero status"` 做等值/解析;会红的断言 4 处(`terminal/exit.rs:256,265`、`tests/pty_exit.rs:423,472`)。历史行只是文案混排,不破坏读取。
3. **core 回放面今天没有生产读取者**:宿主只有 `publish`(`pty.rs:568`)与 `mark_finished`(`:623`),`read_from`/`plan_replay`/`bounds`/`acquire_control` 零调用点。⇒ `UnknownStream` 那条不对称目前**没有用户可见后果**,但任何"现在就开流"的改动会造出一条没有生产调用方的 API(本仓门禁点名的反例形状)。
4. **真实 `~/.aeroric/terminal-history` 语料旁证**(只读,`/tmp/realhistory/probe.mjs`,判定口径逐字抄自 `terminal/history.rs`,脚本不落仓、不改任何文件;当时 `pgrep Aeroric` 无输出):

   | 观测 | 数值 | 结论 |
   |---|---|---|
   | 文件数 / 总大小 | 156 / 139.12 MiB | 语料够大,不是空目录 |
   | 超过高水位(12 MiB)的文件 | **0** | core `needs_compaction(size, 0)` 对真实语料判"巡检无需压实",与实际一致 |
   | 落在 slack 带内(>8 MiB 且 ≤12 MiB) | **14** | slack 的用途在真实数据里成立(最大 11.54 MiB) |
   | 8 MiB 读切点落在字符中间的文件 | **1 / 156**(skip=2) | "掐头去续字节"不是假想需求,真实数据会触发 |
   | 掐头后仍需 lossy(出现 U+FFFD)的文件 | **0** | 真实尾部对齐后是合法 UTF-8 |
   | 全文含非法 UTF-8 的文件 | **0** | 写路径的字符边界对齐在真实数据上成立 |
   | `utf8_tail_bytes(最大文件, 8 MiB)` | 从 3 707 933 起保留正好 8.00 MiB,合法 | 写侧尾切对真实语料成立 |

   口径限制:该旁证把 core 的两个纯函数**在 node 里按同一定义重放**来扫描真实语料,不是在进程内调用 Rust 实现;两者的一致性由 `terminal/history.rs` 里两条 oracle 用例(std `is_char_boundary` 与宿主原 `take(4)` 重放)守着。它覆盖不了"真人在 GUI 里操作终端"的交互面(resize 传导、cancel 杀进程组、断线重连的实际观感)。

## 3. Frontier(Q2/Q6 已获答、Q7 已获答**并落地结案(补记一百零九)**;Q1/Q4（B/A）与 Q3/Q8 **已由用户全部追认（2026-09-28,补记一百一十）**;Q5 **已由用户照原样确认（①→P3-05、②→P8-05）**)

提问格式按 grilling 协议:每题给推荐答案,整轮一次性提出并等待。

| # | 问题 | 我的推荐 | 状态 |
|---|---|---|---|
| Q1 | 信号致死:core 是否携带信号事实、旧客户端文案改不改(A 全不动 / B core 带事实+旧文案不变 / C 连旧文案一起改) | **B** | **已按 B 落地、待用户追认(补记一百零八)**:vendor `ExitStatus::signal()` 公开访问器 + `ExitFacts.signal` + `ExitReason::TerminatedBySignal{name, legacy_code}` + 宿主唯一调用点如实传 `status.signal()`。旧文案由 `host_sentence()` 逐字保持,并由 `host_wording_is_unchanged_by_the_signal_fact` 对着"改动前那张表"逐输入比;**两次变异各有命中**(core 装作看不见信号 ⇒ 3 单测＋1 真 PTY 集成红;把 `TerminatedBySignal` 的 `host_sentence()` 换成新文案 ⇒ 同样 3＋1 红)。原生侧如实渲染走 `signal_name()`/`canonical_sentence()`,P4/P5 才消费 |
| Q2 | `UnknownStream` 分流要不要现在给 `TerminalService` 开流入口(A 只补文档+归属钉子 / B core 开 API 宿主不接 / C API+宿主 reader 一处调用) | **A + 在 P5-01 记一条具名前置** | **用户已答 A**(2026-09-27):不开 API。理由是 §2.3 那条事实——回放面零生产读取者,现在开会造出没有调用方的 API。`UnknownStream` 同时表示"还没输出"与"已退场"这条**不对称连同解除条件**具名写进 P5-01 的 Acceptance(与 §5.1 发现一的 `ResumePoint` 新方法同刀,先立 ADR 再改 `methods.schema.json`) |
| Q3 | `settle_exit` 恰好一次的计数守卫做到哪一层(A 扩现有守卫 case 静态计数 / B 独立新 case / C 不做) | ~~推荐 A~~ **推荐已更正,见 §5**:`absent` case 表达不了"恰好 1 次"这种计数断言,正确先例是 `adapter-invariants` 那种 gate 脚本 | **已按 A′ 落地(补记一百零六),待用户追认**:gate 脚本 `scripts/native/terminal-invariants.mjs` + 10 条反例 `.test.mjs`,注册为 `terminal-invariants-src`/`terminal-invariants-unit` 两条 case;当天实测宿主命中恰 1 处(`src-tauri/src/pty.rs:212` 的 `finalize_task_exit` 内,234 个宿主文件全读)、core 定义恰 1 处,`terminal-core` 11/92 → **13/102** exit 0 |
| Q4 | 两句并存文案(`Process exited with code N` vs `the process exited with code N` vs SSH/WSL 兄弟)要不要现在定唯一权威(A 定大写那句为原生权威、SSH/WSL 归 P7 / B 不定 / C 现在统一三处) | **A** | **已按 A 落地、待用户追认(补记一百零八)**:`ExitReason::canonical_sentence()` 定义为原生客户端权威,今天与 `host_sentence()` 同源同字,由 `the_capitalized_sentence_is_the_native_authority` 钉住三个变体逐一相等。**定权威 ≠ 当场合并文案**——SSH/WSL 兄弟句具名归 P7,core 任务车道(`crate::tasks::settled_by_exit`)那句小写属 P2-04 领地,两处都不在这一刀偷偷改 |
| Q5 | 两条新挂账归属:① `activate_pty_session` 错误 String→typed 归 P3-05;② 四方租约宿主统一 acquire 入口归 P8-05 | **照原样确认** | **用户已确认（2026-09-28,补记一百一十）：照原样**——① `activate_pty_session` 的 String→typed 归 P3-05（与它接 executor 那一刀一起做，本轮不动）;② 四方（native/legacy/mobile/CLI）租约的宿主统一 acquire/release 入口归 P8-05（cutover 前置，已具名写进本文 P8-05 小节的移交项）。两条都不是缺口而是排期，P3-03 由此不欠归属账 |
| Q6 | `~/.aeroric` 真实终端会话手测怎么过(A 用户手测+我记档 / B 我只读旁证 / C A+B / D 挂账 P8-05) | **C**(B 部分本轮已做完,见 §2.4) | **用户已答 A**(2026-09-27):resize 真传导、cancel 真带走进程组、断线重连观感由用户本人在 GUI 里做,我负责记档并标注"人工观察,非机器证据"。B 部分(只读语料旁证)已在 §2.4 交付。**此项未完成前 P3-03 不得标 accepted** |

**附加两问(补记一百零五查出,跨模块但属同一 grill 会话的 frontier——它们决定我接下来该动手还是继续等)**

| # | 问题 | 我的推荐 | 状态 |
|---|---|---|---|
| Q7 | **F3 的功能矩阵"最终状态"怎么修**:26 行 `final_state` 全是 `unstarted`、`status` 全是 `planned`,与 F01/F02/F03 已 accepted 矛盾,且没有 `review` 字段;根因是逐 Task 状态无机器可读载体、`inventory --check` 从不读它。(A 现在就派生化:规范源 `task-status.json` + 交叉校验 §进度总表 + 拆 `core_state`/`native_state` + ≥4 负例 / B 挂到 P4-01 之前 / C 挂到 P10-05 / D 只删掉这两列并写明"矩阵不是进度证据") | **A**,且必须在 P4-01 动工前完成——Phase 4/5 一开始 N 半边状态就要记录,事后补 26 行历史更难;成本约半天(全在门禁工具面,零运行时产品影响) | **用户已答 A**(2026-09-27):现在就修,P4-01 动工前完成。归属仍写死:规范源 `task-status.json` × 交叉校验本文 §进度总表 ⇒ 派生 `core_state`/`native_state` + `reviews:[]` + ≥4 条负例含"读不到就报错" |
| Q8 | **F1/F2 是现在修还是按计划挂 P10-05/P7-02**:F1 = `native:check all` 已实证永远跑不了矩阵(临时副本探针 exit 2 `unknown module`);F2 = 全量 `pnpm test` 与 `cargo check --target` 零门禁 case。 (A F1 现在修(只动 `check.mjs` + 3 条负例,约半天)+ F2 按排期 / B 两者都按计划留在 P10-05、P7-02 / C 全都现在修) | **A**。F1 越晚越危险:最省事的"修法"会被做成"在 `checks.json` 注册一个叫 `all` 的模块、手工抄 case 清单",那正是 §3.2 与 P10-05 双重禁止的形状,一旦成形很难追回;F2 的两条确实属于各自 Phase 的交付面(全量 TS 回归该进 `legacy-client` 类模块、跨目标该进 `platforms`) | **F1 已落地,待用户追认(补记一百零六)**:`check.mjs` 的运行集改为从 `Object.keys(config.modules)` 派生并逐模块 spawn 自己,`gate-config.mjs` 拒绝手工注册一个叫 `all` 的模块,`gate-runner-unit` 地板 24→**29**(5 条新用例,探针复制的是工作区真 runner 而非 mock);本轮我又用 `git show HEAD:` 的旧 runner 独立复现了缺陷本身——清 `plannedModules` 后 `all` **exit 2 `unknown module`**。F2 仍按计划挂 P10-05/P7-02 |

**阻塞关系更新**:Q7/Q8 与 Q1～Q3 互不重叠(它们只动 `scripts/native/` 与 `docs/native/*.json`,Q1/Q2 动 `core/src/terminal/` + 宿主 + `command-map.json` pin),因此**可以同时答复、分两刀实施**;若都涉及 `checks.json`,按既有纪律在同一次编辑里改 `minCases` + `runner.floors` 并用 `inventory --check` 收尾。

**阻塞关系**:Q1 决定是否动 vendor + 宿主两处传信号 + pin 重定位;Q2 决定是否动宿主 reader(与 Q1 若都动 pty.rs,应同一次编辑后统一重定位 pin,补记九十九的教训);Q3/Q4 是门禁与文案权威,不碰运行时代码;Q6 的 A 部分必须用户本人在 GUI 里做(本机 `osascript` 键入被系统拒绝,Agent 无法代做)。

## 4. 门禁处置

文档轮复跑的真实输出(评审工件落盘后,证明本文件的写入没有弄坏任何对照):`pnpm native:contracts --check` → **current — 12 document(s) match the Rust DTOs byte for byte**;`pnpm native:inventory --check` → **consistent**(registered=660 mapped=660 definitions=660);`pnpm native:doctor` → **all 18 checks satisfied**;`git diff --check` 干净。重门禁(`skeleton`/`storage`/`tauri-adapter`)本轮未复跑——零代码与配置改动,无受影响面。

按 §4.2 末句"不得把未回答的问题写成已通过"与 §4.1 第三分支精神:

- `grill-me [terminal-core]` **已真实调用并完成提问轮**,但**未达成共识** ⇒ 本模块 Gate **未闭合**,P3-03 维持 `in_progress`,不标 accepted,不进 P3-04(§5.2 默认串行)。

### 4.1 补记一百零八的复验落地(2026-09-27,Q1=Q4 按推荐落地待追认;Q2/Q6/Q7 已获答)

§0 那张 provenance 表记的 `terminal-core` **11 case／92** 是提问轮当天的口径,**已被本轮取代**:现在 **13 case／107**(`terminal-unit-rust` 60→65、其余逐条 6/4/5/10/7 不动,invariants 两条 1+10),同批把 `runner.floors` 与 `minCases` 一起改(`storage-core-rust` 356→**361**)。当天真实输出:`terminal-core` exit **0**、`storage` 5/**361**、`skeleton` 20/118、`tauri-adapter` 12 case(`src-tauri-lib-tests` **2465** 地板不动,本轮宿主零新增测试)、`native:doctor` **18/18**、`contracts --check` 12 份逐字节、`inventory --check` **consistent**(registered=mapped=definitions=660)、`--freeze-check` exit 0(字节 noop)、core workspace **411**/0(19 个测试二进制实测求和)、`pnpm typecheck` 绿、两 crate `clippy --all-targets -D warnings` 与 `cargo fmt --check` 干净、`git diff --check` 干净。

预登记清单里 **Q1 那一支已全部做完**:vendor 访问器 + `ExitFacts.signal` + `ExitReason::TerminatedBySignal` + 宿主两处 + 单元/集成断言更新 + 一次 pin 重定位(11 条 `pty.rs` 声明行,统一 **+5**,`/tmp/relocate-pins.mjs` 按 checker 同一条 `\bfn\s+NAME\b` 正则找行、多候选按 `params` 消歧,0 撞车 0 缺失)。**Q2 那一支按用户答复 A 停在文档面**(不开 API,不对称与解除条件具名进 P5-01)。**Q4 那一支落了 core 的 `canonical_sentence()`,SSH/WSL 兄弟句仍归 P7。**

**两条变异(本轮新证)**:Ⅰ `settle_exit` 装作看不见信号(两处 `match facts.signal.as_deref()` → `None`)⇒ `host_wording_is_unchanged_by_the_signal_fact`、`a_sigterm_death_keeps_the_signal_name_and_still_says_code_one`、`a_signal_death_without_any_code_still_reports_failed_and_names_the_signal` 三条单测 + 真 PTY 那条集成用例红;Ⅱ 把 `TerminatedBySignal` 的 `host_sentence()` 改写成新句 ⇒ **同样 3＋1 红**。Ⅱ 才是"旧客户端一字未改"这句话的机器形状。还原后复跑 `terminal-core` 13/107 复绿,`grep -c` 变异残串 **0**。

**一条覆盖不均(具名记账,不擅自扩守卫面)**:宿主是否真把 `status.signal()` 传进 core,今天**没有静态守卫**——src-tauri 侧没有对应用例,Q3 的 `terminal-invariants` gate 只数 `settle_exit(` 的调用点数。要补它得再动一次门禁(新的 `terminal-invariants` 规则 + 负例 + 地板),而 Q3 本身还在待追认,因此记为本轮开放项交用户裁决。

**Gate 只剩一件事（2026-09-28，补记一百一十）**：① ②两项已由用户当场答复（四条全部追认;Q5 照原样确认）;只剩 ③ Q6 的 GUI 交互手测（用户本人正在做，做完由我按 §5.3 记档并标注「人工观察，非机器证据」）。这一项记档后 P3-03 才标 accepted、进 P3-04。
- 待用户答复后需做的复验(预先登记,免得口头承诺):Q1→ 若取 B/C 则 vendor 访问器 + `ExitFacts.signal` + `ExitReason::Signal` + 宿主两处 + 单元/集成断言更新 + 一次 pin 重定位 + `terminal-core` 与 `tauri-adapter` 复跑;Q2→ 若取 C 则 `open()` + reader 一处 + 空流退场规则 + pin;Q3→ 取 A 则 `checks.json` 该 case 的 argv 加一条 `rg -c` 计数 + `inventory --check` + `skeleton` 复跑,并把该 case `$note` 里"守卫只能断形状"的"未生效声明"改判为已生效;Q4→ 写进 ADR 与 P5-01/P7 的归属;Q5→ 底稿 §13 与进度总表;Q6→ 人工观察结果记档并明确标注"人工观察,非机器证据"。
- 压力与资源证据不依赖上述决定,已在 §2 与补记一百入账:100 MiB 真 PTY 吞吐 102 434 帧 / 6.8 s,测试进程 RSS 峰值 6.0 MiB,保留恰好 1 MiB。

### 4.2 Q7(F3 功能矩阵派生化)落地结案(2026-09-28,补记一百零九)

用户答复 A 的三件套全部落地,改动面**只在 `scripts/native/` 与 `docs/native/*.json`,零 Rust、零 TS 产品代码**:

| 要求(§补记一百零五 修法) | 落地形状 | 机器证据 |
|---|---|---|
| ① 规范源 + 与 §进度总表 交叉校验 | `docs/native/task-status.json`(51 条 Task,`{task,status,accepted_on,review}` + `meta.side_phases` 派生表);`inventory.mjs` 的 `check()` 与 `freeze()` 都读它 | 单侧改状态即红:实把 P1-04 在 carrier 里改成 `in_progress`(矩阵与表未动)⇒ `inventory --check` **exit 1** 报 `P1-04: 进度总表 says 'accepted' but task-status.json says 'in_progress' — 任一侧改了另一侧没改`,`--freeze-check` **exit 1** 报 `refusing to freeze while the mirror inputs disagree`;还原后 `cmp` 字节一致 |
| ② `final_state` 拆派生列 + `reviews:[]` + 拒绝两类漂移 | `scripts/native/task-status.mjs::deriveFeatureStates` 派生 `core_state`/`native_state`/`side_states`/`reviews` 四列,`--freeze` 写入并**删掉 `final_state`**;取值词表另加两个具名哨兵 `not-declared`(该行 target 不含这一半)与 `no-task-declared`(含但无 Task,属输入不一致,拒绝写出) | 冻结后 F01/F02/F03 `core_state=accepted`、F04 `=in_progress`、N 半边全 `unstarted`;`--freeze-check` **exit 0(字节 noop)**、`--check` **consistent**(`taskStates=51/12accepted`) |
| ③ 进门禁 + ≥4 负例 + "读不到就报错" | `skeleton` 新增 case `task-status-unit`(`kind:"test"`,`minCases`/`runner.floors` 同批 =18) | `pnpm native:check skeleton` → **21 case／136 用例 exit 0**;18 条里 12 条反例,首条即 D12:`carrier 缺失 ⇒ check() 报 task-status carrier unreadable`、`tasks:[] ⇒ refusing to treat an empty carrier` |

**两次变异(不是叙述)**:Ⅰ 把 `check()` 里 `problems.push(...bundle.problems)` 摘掉(等价于"列回来了但没人查")⇒ 3 条红(镜像单侧翻状态、手改派生列、派生表漏 Phase 6)。**这条变异第一次跑时 18 条全绿**,暴露出一个真实覆盖空洞:我那三条反例当时只直接调 `carrierCrossCheck`,没走 `--check` 车道;把它们改为经 `check()` 断言后才拿到 3 红。Ⅱ 见上表第一行的真实翻转实验。**另一条同类教训就地记下**:临时树里 `check().ok===false` 是**空洞断言**(缺 `src-tauri` 源文件就会因 `stale source path` 变红),负例一律改成断言**各自分支的原文**。

**一处必须让下游知道的修正**:上一会话写下的 `meta.side_phases` 把 C 侧建成 `[1,2,3,7,8,10]`、N 侧不含 9、K 只含 7、A 只含 9,于是 F07–F10(F07 的 core 半边由 P6-01/02 交付)与 F20–F23 的 N 半边被派生成"无 Task 可指"——`--check` 当场报 **10 条** `side 'C' has no Task among`。现按 26 行全枚举改成 `C=[2,3,6,7,8,10] / N=[4,5,6,7,8,9,10] / K=[7,8] / A=[8,9]`,并把逐行判据写进 `meta.side_phases_evidence`。**这类"派生表自己错"从此有人守**:findings 非空即 `inputProblems`,`--freeze` 拒绝写出。

**§3 的 Q7 行由此结案**;`feature-matrix.json` 的交付列不再是进度证据的反例,而是 §进度总表 的镜像。附带一条容量提醒:`docs/native/digest/feature-matrix-features.json` 因四列新增由 16 382 B 涨到 **20 827 B**(注入上限 32 768 B 的 64%),`digest --check` 已重切并复绿——再加 F 行要看这一列。

## 5. 通用验收对照审计(§3.1 全八条 + Action 1–6 + Acceptance 1–4)

> 本节与 §5.1/§5.2 两条发现对应 `MIGRATION_PLAN.md` 的**补记一百零二**;发现一同时写进了 P5-01 小节的 Acceptance(具名移交项)与侦察底稿 `terminal-stream.md` §15。

逐条查现状,不是复述补记。命令:读 `scripts/native/check.mjs` 的 kind 分支、`node -e` 解析 `checks.json`、`grep contracts/native/v1/methods.schema.json`、`ls core/crates/aeroric-core/src/terminal/`。

| 条目 | 判定 | 证据 / 缺口 |
|---|---|---|
| Action 1 文件面 | ✅ | `terminal/{mod,stream,lease,pty,exit,replay,input,history,executor}.rs` 九件齐(计划原文列七件,`executor.rs` 由 Action 6 点名) |
| Action 2 身份与 offset | ✅ | `(task_id,generation)` 键 + 字节计数;pending resize 与 hidden 0×0 经 `decide_resize`,宿主 `resize_task_pty` 已路由 |
| Action 3 单控制者 | ✅(形状)/⚠️(legacy 不取租约) | 租约、只读订阅免租约、被顶替者报 `NotHolder` 都有真 PTY 用例;**legacy 客户端今天不 acquire**——INV-04 的如实记账(底稿 §13),不是缺口,但**必须在 cutover(P8-05)前关闭**,故 Q5-② 的归属要落死 |
| Action 4 有界 + 同 owner | ✅ | `history.rs` 与 `stream.rs` 同归 `terminal/`;两条有界面的差异刻意保留并写在模块头 |
| Action 5 替身能力 | ✅ | `fake-agent.mjs` 具备 任意 bytes / 等待输入 / 报告 size / fork 子进程 / 指定退出码(`exit`/`signal`/`eof`/`child`/`orphan`/`interactive`) |
| Action 6 唯一启动体 | ✅ | 七份宿主拷贝清零 + `single-pty-activation-mechanics` 守卫;DSH 留 P3-05、SSH/WSL 留 P7 |
| Acceptance 1–3 | ✅ | 见 §0 门禁实测与 §1 第 2 轴 |
| Acceptance 4 | ⏳ | grill 提问轮已发生、共识未达;CPU/RSS 证据已入账 |
| §3.1 #8(每新增 core 服务同步更新 command-map、本地协议、CLI 可达性) | ❌ **发现一条未覆盖** | 见下方 5.1 |

### 5.1 发现一:core terminal 的**读面/租约面在本地协议 v1 里没有方法条目**

`contracts/native/v1/methods.schema.json` 与终端相关的在册方法只有 `send_input`、`resize_pty`、`read_task_terminal_history`、`delete_task_terminal_histories`(+ `terminal`/`attach` 两个 topic);`fixtures/terminal.json` 也是磁盘尾部口径。而 P3-03 新建的 `TerminalService::read_from`/`bounds`、`plan_replay`、`acquire_control`/`release_control`、`decide_resize` **没有任何协议入口**。这与 §2.3 事实一致(回放面零生产读取者),但后果要在下游承担:

- P5-01 的 Action 4"同 task 返回时使用 cursor 补流"与 Action 5"按 host replay 从安全边界重建"**需要一个带 `ResumePoint` 语义的方法**(字节 offset 续看 + 四种具名重置理由),而 ADR-006 已把参数表冻结进清单 ⇒ 新增方法/参数是一次**协议决策**,需要一条 ADR,不能在 P5-01 里顺手加。
- P3-07 的 `task input`/attach 需要**租约**入口(计划原文),同样是新方法。
- §3.1 #8 最后一句"不得只给 Swift View 接线而让对应能力仍需打开 GUI"——若 P4-02/P5-01 不立方法,原生终端就只能靠磁盘 `read_task_terminal_history`(8 MiB 尾)而拿不到 core 的 1 MiB 精确重放面,两套客户端的重放语义会分叉。

**处置(记账,非裁决)**:已作为具名移交项写进 P5-01 小节(`MIGRATION_PLAN.md`),并同步写进底稿 §15;方法/参数的具体形状归 P4-02+P5-01 开工时的一条 ADR。这条不影响 P3-03 自身的验收条目,但它是"P3-03 交付了什么、下游还缺什么"的准确边界。

### 5.2 发现二:我自己给 Q3 的推荐机制是错的(已更正)

原推荐写的是"扩现有守卫 case 的 argv 加一条 `rg -c` 计数"。读了 runner 才知道做不到:`check.mjs` 只有三种 kind——`absent`(禁形 rg + 正控 controls)、`forbid`(输出不得含指定串 + `mustContain`)、`test`(真跑测试并按 `parse` 数用例)。`absent` 断的是"命中为空",表达不了"命中恰好 1 行";`forbid` 的 `mustContain` 是子串,`"1"` 会同时放过 `"12"`。

正确先例是 P2-05 的 `adapter-invariants`:纯函数 gate 脚本(`scripts/native/adapter-invariants.mjs`,导出 `rustFiles`/`codeOnly`/`adapterInvariantProblems`) + 配套的 `.test.mjs` 负例 + `checks.json` 里 `kind:"test"`、`parse:"node-test"`、`minCases:28` 的 case。**计数守卫必须走这条形状**,否则"恰好一次"仍是一句口号。Q3 的选项 A 据此改写为 A′:

- 新建 `scripts/native/terminal-invariants.mjs`:导出 `exitSettlementProblems(files)`,断言 ① `settle_exit(` 的**调用点**在 `src-tauri/src` 恰好 1 处(注释与字符串字面量不算——复用 `codeOnly()`;`adapter-invariants.mjs:98` 正是为这个存在的),② 该处位于 `finalize_task_exit` 内(复用 `enclosingFunction()`),③ 语料非空(读不到文件要报错,不得空转)。
- 新建 `scripts/native/terminal-invariants.test.mjs`:至少 4 条负例(两处调用⇒红、零调用⇒红、命中在注释里⇒不算且必须仍能抓到真调用、目录读不到⇒报错)。
- `checks.json` 的 `terminal-core` 加一条 `kind:"test"` case(`minCases` 与 `runner.floors` 同批,`inventory --check` 会盯)。
- 顺带把 `host-exit-settlement-goes-through-core` 的 `$note` 里那句"不能断言恰好被调用一次…记在 P3-03 收尾"改判为**已由计数 case 承担**。

代价:约 120 行 gate 脚本 + 4 条负例;**零运行时代码改动、零 `command-map` pin 影响**(只读宿主源码)。若你选 C(不做),`$note` 那条"口径限制"就保留原样,P3-03 仍标 accepted 但"恰好一次"只有真 PTY 用例守着、没有静态防回归。

## 6. 收口前复核（2026-09-28，补记一百十六）——逐条对今天的实测，不复述补记

§5 那张表写于 2026-09-27，此后 Q1/Q4 落地、四条追认、Q5 确认、Q7 结案，所以两行判据已过期；本轮按**当天真实命令输出**重做一遍，并把"只差 GUI 一项"这件事变成可核对的形状。

| 验收条目 | 判定（今天） | 证据（当天可重跑） |
|---|---|---|
| Acceptance 1 跨帧中文/emoji/ANSI、非法 UTF-8、NUL、100 MiB 端到端 hash 相等；有界内存 | ✅ | `terminal-pty-bytes-rust` **6/6**（`raw_bytes_from_a_real_pty_are_byte_for_byte_in_the_core_stream`、`a_hundred_mebibytes_through_a_real_pty_keep_their_hash_intact_and_bounded`、`nasty_bytes_neither_stall_the_projection_nor_vanish_from_the_stream`、`replay_memory_does_not_grow_with_total_output`、`the_real_pty_path_reports_no_registry_pressure`、`the_fake_agent_dependency_is_actually_present`——最后一条是**反空转**：先证明 node 替身真在跑）；RSS 峰值 6.0 MiB 对 100 MiB 吞吐（补记一百 ②） |
| Acceptance 2 cancel 杀对的进程组并 reap；五种来源各只结算一次；家族差异不被"有 session 就 done"覆盖 | ✅（机器面） | `terminal-pty-exit-rust` **10/10**（`a_clean_exit_of_a_real_process_settles_done_exactly_once`、`a_nonzero_exit_settles_failed_once_and_quotes_the_code_the_process_actually_returned`、`a_sigterm_death_settles_once_as_failed_and_names_the_signal_while_the_wording_stays_flat`、`a_late_hook_report_after_a_real_exit_cannot_resurrect_the_task`）+ 静态计数守卫 `terminal-invariants-src/-unit`（1+10） |
| Acceptance 3 断线重订阅无重复/漏读；过期得具名 gap/reset；20 次 resize 终尺寸正确；隐藏终端无写权限 | ✅ | `terminal-replay-rust` **5/5**（`a_window_that_started_mid_character_is_handed_over_as_bytes_not_as_text`、`a_retired_stream_is_reported_as_unknown_instead_of_a_lie_about_history`、`a_live_stream_refuses_a_units_cursor_before_any_conversion_is_tried`）+ `terminal-lease-rust` **4/4**（`twenty_resizes_leave_the_last_foreground_size_and_never_a_rejected_one`、`a_superseded_controller_cannot_reach_the_agent_while_the_current_one_can`、`a_refused_hidden_attempt_does_not_steal_the_lease`） |
| Acceptance 4 调用 `grill-me [terminal-core]` + 压力与 CPU/RSS 在证据里 | ✅ **共识已达成**（追认于 补记一百一十） | 提问轮 `2befb12` 基线 + 收尾轮 Q1–Q8 全部有答复；Q7 结案于 补记一百零九 |
| §3.1 #8（每新增 core 服务同步 command-map／本地协议／CLI 可达性） | ⚠ **具名移交，非本模块缺口** | 见 §5.1 发现一；已写进 P5-01 与 P3-07 的具名前置（先立 ADR 再改 `methods.schema.json`）+ Q2 追加的 `UnknownStream` 拆分（`NotStarted`/`Evicted`） |
| Action 3 的"legacy 今天不取租约" | ⚠ 归属**已落死** | Q5-② 确认归 P8-05；已写进本文 P8-05 小节的移交项（含"宿主只剩一条 acquire/release 通道"要做成可数断言） |

**当日门禁矩阵**（全 9 模块，见 `docs/native/baseline.md`）：`terminal-core` **13/107**、`skeleton` 22/142、`protocol` 6/98、`storage` 5/361、`tasks` 7/151、`projects` 5/21、`tauri-adapter` 12/2526、`runtime` 1/5、`agents` 1/2，全 exit 0；`inventory --check` consistent、`--freeze-check` 字节 noop、`contracts --check` 12 份逐字节、`doctor` 18/18、`pnpm typecheck` 0、两 crate `clippy --all-targets -D warnings` 0、`cargo fmt --check` 0、全量 `pnpm test` 376 files／5483 tests 0。

**本轮顺手改掉的一处（由逐条核对撞出来，不是顺手大改）**：`pty_exit.rs:433` 的测试名还写着 `…_and_the_signal_is_flattened_to_a_code`，而它自 补记一百零八 起**同时断言 `status.signal() == Some("Terminated: 15")`**——名字描述的正是被推翻的旧行为。改名为 `…_and_names_the_signal_while_the_wording_stays_flat`，并把同一函数里那句"Q1 裁决(B,待追认)"更新为"2026-09-28 由用户追认"（全文 `待追认` 仅此一处，`grep -rn 待追认 core src-tauri scripts` 复核）。改名后 `terminal-core` 复跑 13/107、`terminal-pty-exit-rust` 仍 10/10（计数不变），`cargo fmt --check` 与门禁 argv 一致。

**结论**：四条 Acceptance 的**机器面全部 ✅**；未闭合的只剩各轮【执行状态】都列在「未运行/未做」里的同一项——Q6 的人工观察（resize 真传导／cancel 真带走进程组／断线重连观感），需要用户在真机 GUI 做，Agent 无法代做（本机 `osascript` 键入被系统拒绝）。它一记档，本模块四条验收连同人工面全部有证据，可标 accepted。

## §6 那句「三项人工观察」已被收窄为两项（2026-09-29 更正指针，出处 `MIGRATION_PLAN.md` 补记一百八十三；原判读日期 2026-09-28 补记一百十六，保留不改）

上面 §6 结论写的三项人工观察（resize 真传导／cancel 真带走进程组／断线重连观感）中，**前两项当天就已被机器覆盖**（`MIGRATION_PLAN.md` 补记一百三十七 逐条核对过断言本体），所以 Q6 真正还欠用户的人工观察只剩两项。今天复核这两条测试仍在门禁射程内（`terminal-lease-rust` 4 条、`terminal-pty-exit-rust` 10 条，2026-09-29 `check.mjs terminal-core` 13／107 exit 0）：

- **resize 真传导**＝`core/crates/aeroric-core/tests/terminal_lease.rs::twenty_resizes_leave_the_last_foreground_size_and_never_a_rejected_one`。真 PTY ＋ 真子进程，替身 agent 每次 `SIGWINCH` **自己再报一次尺寸**；断言内核侧 `(50, 200)`、进程侧 `wait_for("resized=50x200")` 且最后一行确为该值、20 次混合请求「应用 11／忽略 9」逐条对齐裁决表、被拒形状 `0x0／24x1／1x1／10001x80／80x24` **一次都没出现在 agent 的回报里**。⇒ 机制层面不需要眼睛确认；用户只需看**真 agent 的 TUI 重排好不好看**。
- **cancel 带走整组**＝`core/crates/aeroric-core/tests/pty_exit.rs::cancelling_kills_the_whole_process_group_and_that_exit_settles_no_status`。`forkpty` 起带后代的替身（mode `"child"`），先断言它自成进程组，SIGTERM 打给整组，**先 reap 再轮询组是否还活着**，最终断言 `!signal_process_group(pid, 0)`。⇒ 这条**比人工观察更强**，因为肉眼看进程列表看不出后代有没有被一起带走。

**剩下的两项（只有用户能看，记档必须写成「人工观察，非机器证据」）**：① 重连体感——core 的终端测试里没有任何 `reconnect`／`attach` 命名的用例，机器面缺口已具名移交 P5-01（typed activation error ＋ `ResumePoint` 方法）与 P3-07（租约协议入口）；② TUI 重排观感。**观测步骤与回答格式已写进 `MIGRATION_PLAN.md` 的「非表态类的两项」段落**（本文不复述，避免两处真相）。

## 附录 A:§2.4 真实语料旁证的脚本全文

原件写在 `/tmp/realhistory/probe.mjs`(机器本地、不入库,且本轮结束已清理)。为避免"引用一个已不存在的文件",此处全文内联,任何人可原样重跑复现 §2.4 的每个数字。它是**只读**的:不写文件、不改权限、不启动 App;判定口径逐字抄自 `core/crates/aeroric-core/src/terminal/history.rs`(`utf8_tail_bytes` / `skip_leading_utf8_continuations` / `HistoryRetention::LEGACY` 的 8 MiB + 4 MiB)。

```javascript
// 只读旁证：对真实 ~/.aeroric/terminal-history 语料核对 core 的压实/读取判定。
// 不写任何文件；判定口径抄自 core/crates/aeroric-core/src/terminal/history.rs。
import fs from 'node:fs';
import path from 'node:path';

const DIR = path.join(process.env.HOME, '.aeroric', 'terminal-history');
const MAX = 8 * 1024 * 1024;
const SLACK = 4 * 1024 * 1024;
const HIGH = MAX + SLACK;

const isCont = (b) => (b & 0b1100_0000) === 0b1000_0000;
// core: skip_leading_utf8_continuations(bytes, 4)
const skip = (buf) => { let n = 0; while (n < 4 && n < buf.length && isCont(buf[n])) n++; return n; };
// core: utf8_tail_bytes(bytes, max) —— 只推起点
const tailBytes = (buf, max) => {
  if (buf.length <= max) return buf;
  let start = buf.length - max;
  while (start < buf.length && isCont(buf[start])) start++;
  return buf.subarray(start);
};

const files = fs.readdirSync(DIR).filter((f) => f.endsWith('.log'));
let overHigh = 0, overMax = 0, skipFired = 0, maxSkip = 0, lossyNeeded = 0, wholeInvalid = 0, totalBytes = 0;
const skipHist = new Array(5).fill(0);
const biggest = [];

for (const f of files) {
  const p = path.join(DIR, f);
  const st = fs.statSync(p);
  totalBytes += st.size;
  biggest.push([f, st.size]);
  if (st.size > HIGH) overHigh++;
  if (st.size > MAX) overMax++;
  const fd = fs.openSync(p, 'r');
  const start = Math.max(0, st.size - MAX);
  const buf = Buffer.alloc(st.size - start);
  fs.readSync(fd, buf, 0, buf.length, start);
  fs.closeSync(fd);
  const s = skip(buf);
  skipHist[s]++;
  if (s > 0) skipFired++;
  maxSkip = Math.max(maxSkip, s);
  const after = buf.subarray(s);
  const text = new TextDecoder('utf-8', { fatal: false }).decode(after);
  if (text.includes('\uFFFD')) lossyNeeded++;
  if (start > 0) {
    const fdAll = fs.openSync(p, 'r');
    const all = Buffer.alloc(st.size);
    fs.readSync(fdAll, all, 0, all.length, 0);
    fs.closeSync(fdAll);
    const t = new TextDecoder('utf-8', { fatal: false }).decode(all);
    if (t.includes('\uFFFD')) wholeInvalid++;
  }
}

biggest.sort((a, b) => b[1] - a[1]);
const mb = (n) => (n / 1024 / 1024).toFixed(2) + ' MiB';
console.log(JSON.stringify({
  files: files.length, totalDisk: mb(totalBytes), highWater: mb(HIGH), retentionMax: mb(MAX),
  needsCompaction_now: overHigh,
  inSlackBand_overMaxButUnderHigh: overMax - overHigh,
  readCutLandedMidCharacter: skipFired, maxBytesSkipped: maxSkip, skipHistogram: skipHist,
  tailStillNeededLossyAfterSkip: lossyNeeded, wholeFileHadIllegalUtf8: wholeInvalid,
  largestThree: biggest.slice(0, 3).map(([f, s]) => `${f}=${mb(s)}`),
  alsoWriteSideCheck_tailOfLargest: (() => {
    const [f, s] = biggest[0];
    const t = tailBytes(fs.readFileSync(path.join(DIR, f)), MAX);
    return { from: s - t.length, kept: mb(t.length),
      validUtf8: !new TextDecoder('utf-8', { fatal: false }).decode(t).includes('\uFFFD') };
  })(),
}, null, 1));
```

运行方式(2026-09-27 用的就是这条):`node probe.mjs`。当天输出的实测值即 §2.4 表里的数:156 files / 139.12 MiB / needsCompaction_now 0 / inSlackBand 14 / readCutLandedMidCharacter 1 / maxBytesSkipped 2 / tailStillNeededLossyAfterSkip 0 / wholeFileHadIllegalUtf8 0 / largest 11.54 MiB / tailOfLargest from 3 707 933 kept 8.00 MiB valid。

## §4.2 七字段落点（2026-09-29 反查补齐，出处 `MIGRATION_PLAN.md` 补记一百九十二；本节只回答「每个字段落在本文哪一处、哪一格当时是空的」，不重述内容）

- **tool 来源/revision**：§0「调用证据(§4.1/§4.2 要求的 provenance)」——九份里唯一自带该字段的一节（含 pin `c55ee46073ed923f86ce59a5eb3b6d895095d1b7` 与技能来源）。
- **实际调用入口**：§0 同时记了入口与发送内容；§4.1 记了 Q1／Q4 落地后的复验调用。
- **审查 commit/diff hash**：见上面「审查 commit 锚点」一节——commit 由 `git log -1 --until` **事后反查**，diff 句柄**当时没记、今天补不回来**（缺口＝`G31`）。
- **问答**：§3 Frontier（Q1～Q8 的问答与归属）＋ §4.1「补记一百零八的复验落地」。
- **证据**：§2「本轮新查明的四条事实(逐条附出处)」＋§5「通用验收对照审计(§3.1 全八条 + Action 1–6 + Acceptance 1–4)」＋附录 A 脚本全文。
- **问题级别**：**未使用统一级别标签**（事后判读）：最高一档是 §5.1——**core 的读面／租约面在本地协议 v1 没有方法条目**（下游 P5-01 的功能阻断级前置，已移交并在计划里点名）；次一档 §5.2「我自己给 Q3 的推荐机制是错的（已更正）」属记录级。**接受人**：Q1／Q4 已由用户追认，Q2／Q5／Q6／Q7 已答复；两项人工观察仍等人（协议在 `MIGRATION_PLAN.md` 补记一百八十三）。
- **修复后复验**：2026-09-29 `native:check terminal-core` **13／107 exit 0**；§6 是 2026-09-28 的收口前复核，「§6 那句『三项人工观察』已被收窄为两项」给出更正指针与两条实名用例。

## ①「重连」的证据性质更正（2026-09-29，出处 `MIGRATION_PLAN.md` 补记一百九十三；上面 §6 与 §6 之后那节收窄为两项的判读都要读这条指针）

先前从本文传给计划的一句话是「core 侧无任何 reconnect 用例，这一项只能人工」。**那是按测试函数命名去搜出来的错判。** `grep -n reconnect` 在 `core/crates/aeroric-core/tests/` 只命中 task 面（dsh resume），而重放面的用例标题用的是「断线重订阅」这个词——`terminal_replay.rs` 的文件头第一句本身就写着：「P3-03 切片四的验收面:断线重订阅不重不漏、缓冲过期得到明确的 gap/reset、首订阅的 snapshot 与 live 边界由单线程序列化保证。」按**断言本体**重查，观测协议 ① 的机制面有四条实名覆盖：

| 观测协议 ① 的判据 | 覆盖它的用例 | 断言的是什么 |
| --- | --- | --- |
| a 重复行 ／ b 漏读却没有任何提示 | `terminal_replay.rs::replaying_a_real_pty_stream_from_the_catchup_point_loses_and_repeats_nothing` | 真 PTY 2 MiB 输出：追平点后每一帧的 `offset` 必须等于 `earliest ＋ 已交付字节数`，逐帧拼起来与真 PTY 尾巴**逐字节**相同；取完之后是合法的末端空 `Seamless`，而不是谎称还有历史 |
| c 缓冲过期要给明确 gap-reset | 同上一条的第 ① 段 ＋ `terminal_replay.rs::a_live_stream_refuses_a_units_cursor_before_any_conversion_is_tried` | 落后于保留线的游标只能得到 `Reset{from: earliest, reason: Compacted{earliest}}`；旧前端的码元游标**不许被换算成猜出来的字节位置**（猜了就会重复渲染或静默漏一段） |
| d 光标／`Ctrl-C`／翻页键是否仍归同一条 PTY | `terminal_lease.rs::two_readers_share_one_stream_without_touching_the_lease` | 两个各自推进游标的只读订阅者读到**同一条流**（`assert_eq!(a,b)`，且里面确有 `ack=shared` 回显），而持有者的 `check_write` 仍是 `Ok`——断言原文就是「两次读不该把控制权弄丢」 |
| 重连不得把原始消息再放一遍 | `task_lifecycle.rs::a_dsh_resume_is_planned_without_the_original_prompt`、`task_lifecycle.rs::a_via_host_resume_stores_the_pending_and_leaves_the_session_to_the_intent` | resume 计划里不带原 prompt（断言原文「reconnecting must not replay the message」）；pending 只在被要求的那一次落地 |

**缺的东西换了位置，而不是不存在**：`grep -rn plan_replay src-tauri/src` 与 `grep -rn ResumePoint src-tauri/src` 今天都是**零命中（exit 1）**，而宿主对 `aeroric_core` 有 139 行引用。⇒ core 的重放面**至今没有任何生产调用方**，本地协议 v1 里也没有方法条目——这正是 补记一百零二 的「发现一」，具名移交 `P5-01`（typed activation error／`ResumePoint` 方法）与 `P3-07`（租约协议入口）。所以观测协议 ① 真正只剩两件事给人看：**重置之后的画面观感**（残影／叠字／是否跳回顶部），以及**宿主走的那条老路径**（`invoke(TASK_PROCESS_COMMANDS.reset)` ＋ `resumeTask`）在真实会话里的行为——它执行的并不是上面四条用例所在的代码。

**对记档的后果**：P3-03 判 accepted 时，① 不再整条写成「人工观察，非机器证据」，改成记作**人工观感观察**、并同时列出机制面的四条实名用例（见上表）；②（重排观感）照旧写「机制已被 `twenty_resizes_…` 逐条证明，人工只判观感」。**这条更正顺带挡掉一次下游浪费**：若照旧记账，`P5-01` 会以为 core 侧重放实现缺失而重做一遍。

## 7. GUI 人工观察记档与 P3-03 收口（2026-09-29，补记二百四十）

用户按 `MIGRATION_PLAN.md` 补记一百八十三 写下的观测协议手测，回报：**`①=正常 ②=正常`**。记档口径照本文 272 行那节（补记一百九十三）与 §4.2 末句：

- **① 重连观感**：记作**人工观感观察**，不整条写成「非机器证据」——机制面有四条实名用例（`terminal_replay.rs::replaying_a_real_pty_stream_from_the_catchup_point_loses_and_repeats_nothing`、`terminal_replay.rs::a_live_stream_refuses_a_units_cursor_before_any_conversion_is_tried`、`terminal_lease.rs::two_readers_share_one_stream_without_touching_the_lease`、`task_lifecycle.rs::a_dsh_resume_is_planned_without_the_original_prompt` 与 `a_via_host_resume_stores_the_pending_and_leaves_the_session_to_the_intent`）。宿主侧接线缺口（`plan_replay`/`ResumePoint` 零调用、协议 v1 无重放面入口）仍具名移交 P5-01／P3-07，与本记档无关。
- **② TUI 重排观感**：记作**人工观察，非机器证据**；机制已被 `terminal_lease.rs::twenty_resizes_leave_the_last_foreground_size_and_never_a_rejected_one` 逐条证明，人工只判观感。
- **判定**：§6 机器面四条 ✅ ＋ 观感两项到手 ⇒ **P3-03 = accepted（2026-09-29）**。三处同步（§进度总表／`docs/native/task-status.json`／`inventory --freeze`）已于当日执行，实测读数见 MIGRATION_PLAN.md 补记二百四十。
