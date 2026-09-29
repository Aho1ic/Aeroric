# tauri → native 迁移评审记录:sessions(Task P3-04)

> **数字口径**:本文件里的 case 数／用例数一律是**验收当天的实测**，之后模块被别的 Task 扩过时以现场 `node scripts/native/check.mjs <module>` 与 `checks.json` 的 `minCases` 为准；既不当回归证据复用，也不为对齐改带日期的记录（口径原文见 `reviews/agents.md` 顶部横幅）。

## §4.2 七字段

- **tool 来源/revision**：`mattpocock/skills` @ `c55ee46073ed923f86ce59a5eb3b6d895095d1b7`——`grill-me/SKILL.md` 是转交 `grilling` 的入口（委托链见 `docs/native/tooling.md` §2/§3；P1-05 已按 §4.1 确认「别名文件＋被转交技能同时存在」）。
- **实际调用入口**：本会话的已装载技能清单中的 `grilling`（`skill://grilling`，SKILL 正文本轮实读：rounds/frontier/「查证是 Agent 的事、决策是用户的事」）。发送载荷＝下方 §1 的五轴输入包，按 §4.2 模板逐轴追问。
- **审查 commit/diff hash**（G31 新规矩，**当场记**，非事后反查）：`git rev-parse HEAD`＝`a631af02bcbb9c03c4b1be482eef7764e628c182`（a631af0，2026-09-30）；`git status --porcelain`＝1 条目（`?? .zcodeignore`，用户文件、非本任务、不入 diff）；`git diff | git hash-object --stdin`＝`e69de29bb2d1d6434b8b29ae775ad8c2e48c5391`（＝空 diff：本轮跟踪文件的全部改动已提交（c818f7a 实现＋a631af0 标注），审查树即最终树——**先 commit 再 grill** 才让 diff 句柄可记，这正是 G31 要的形状）。注意空 diff 只覆盖已跟踪面，porcelain 清单必须并列读。
- **问答**：§1 的 Q80–Q82（frontier 三条；含与答题纸 Q6/Q7 的接续关系）。
- **证据**：§1 五轴里每条事实的当日用退出码与测试名；§2 验收对照表。
- **问题级别**：本轮**零缺陷级发现**（Q80–Q82 是产品决策，不属「发现的问题」）。中低风险接受人：见各条「标注」。
- **修复后复验**：本文写毕后当日矩阵全绿（§2 表），全部为**当日实测**，无引用旧读数。

## 1. 收尾拷问轮（2026-09-30，五轴）

**轮次输入**：七片全部落地——`sessions/{mod,parser,claude,codex}`（二百四十一）、`{discovery,dsh,omp}` 路径层（二百四十二）、dsh/omp 读侧＋解析（二百四十三）、`history`＋探测/分派（二百四十四）、`{resume,export}`（二百四十五）、`hooks`（HookEventReader＋事件路径布局，二百四十六）、watcher generation 接线（二百四十七）。宿主 `--lib` 以门禁 argv 单线程 2465/0；G04 结案、G03 收一站。

**五轴自查结论（事实，非问题——逐条当场可复验）**：

1. **唯一 owner**：会话四族的读侧（路径/解析/分页/尾窗/恢复/导出/hook 事件流）在 core `aeroric_core::sessions`；**决策面留在宿主的三样**各有具名理由：`AgentFamily`＋`validate_session_path_for`（允许根依赖托管 home，模块头/shim 头/`history.rs` 头三处同口径）、`is_codex_like_agent` 名录回退（app_settings 唯一真相）、dispatch/emit 节流（宿主句柄地盘）。并发/停机序：`watch:claude-lazy-attach:<task>:<gen>` 与 `watch:resume-session:<task>:<gen>` 入口与循环检查 token；`RunEvent::Exit` 先 `supervisor.shutdown()`（翻 token＋5 s 有界 join，`detached>0` 记日志）再关 LocalRouter/DSH；claimed 排他锁在 core `TaskRegistry`，嵌套方向单一（手写约束 4 的路径粒度例外未动）。
2. **反例**：core 集成测试当日全绿（parsers 6＋discovery 10＋dsh_omp 7＋history 7＋resume 7＋export 7＋hook_reader 7＝44 条读侧与形状；registry 两代与 runtime_lifecycle 的取消/幂等为 P3-01/P2-05 既有）；形状反例由 `watcher-discipline` 11 条负例承担且**当场变异实证过**（resume 站改回 `move |_token|` ⇒ 双判据点名红，还原 `cmp` 字节一致）。
3. **边界**：路径＝`validate_session_path_for` 的允许根闸门未弱化＋`Invalid task id` 字符类（13 条拒绝用例）；权限＝OMP 外部历史两支在宿主 wrapper 处 capability 默认 false（`session_omp.rs:91-104`，ADR-001 §5 门槛），core 只收编纯根构造；token＝本片零新增；日志＝登记失败 eprintln 只带错误调试形，不带会话内容；不可信数据＝events.jsonl 非法 UTF-8 不卡流（钉死）、残行不消费、200 MiB 导出上限具名文案；重放＝`HookEventReader.scan` 每完整行**恰好一次**（用例＋第二轮零重放断言）。
4. **回归**：旧 Tauri 客户端＝宿主 2465 条一条不少、其中 11 条既有 resume/export/pagination 测试经 re-export 直接跑在 core 实现上；前端/契约＝零 TS 改动面下当日 `contracts --check` 12 份逐字节、`pnpm typecheck` rc 0、全量 `pnpm test` 376/5483 rc 0（§2 表）；四族＝claude/codex/dsh/omp 的落盘格式判据只此一份（`is_codex_format` 唯一答案源覆盖摘要与导出两条宿主引用点；analytics.rs 的第二份探针属 usage 族，随 Q1 具名 P8-04——这是**已知分叉面**，不是未查事实）；协议 v1 与存储＝零方法零格式变更。
5. **回滚**：七片各自独立可 revert（每片一个 feat 提交＋随批件），无落盘格式变化、无新建用户数据、不改权限；`sessions` 未注册门禁模块（Q6 待答），回滚不涉及 checks.json 之外的面；generation 键与 claim 语义全程未动（P2-05 重键 wave 2 的键结构原样），不会出现「双开漏进程」类风险。

**frontier 三条（产品决策，按提问轮格式；本轮用户不在场，处理照 P3-02 先例＝按推荐项记账、异议可重开，不当作用户答复）**：

- ❓ **Q80** - **`sessions` 门禁模块装什么**：即答题纸 **Q6**（五件已点名的 core 集成测试全部存在并已被 `storage-core-rust` 全量跑着；额外要不要纳入可选宿主子集＝8 模块 125 条、floor 过滤器写法、以及 Q7 会话引用解析阶梯留宿主还是收 core）。本 Task 的验收行 `pnpm native:check sessions` 因注册未决而**不冒充跑过**——四族覆盖今天的机器形状是 `storage` 5/412（含 44 条 sessions 用例）＋`skeleton` 24/188（含 watcher-discipline 12 条）。➡️ 推荐：Q6 答复时把五个具名测试文件原样注册、宿主子集**不**纳入（污染源 opt-in 的先例口径）；在那之前这条验收记「由 `storage`/`skeleton` 当日读数代偿，缺口具名＝Q6」。**标注：用户未答，按推荐项记账。**
- ❓ **Q81** - **`adopt_session_for_agent` 的转录复制 I/O 要不要在 sessions 域上收**：Q78（P3-02 记账）说过「真要迁属 P3-04 领地、会话根抽象先建」——现在根抽象建好了，本片仍把复制留在宿主（它的事务里混着 claim 顺序 ADR-031 与 emit）。➡️ 推荐：**维持现状**——复制 I/O 与宿主登记事务耦合、验收「跨族 handoff 生成新会话」行为无变化；如要上收另立小片。**标注：用户未答，按推荐项记账。**
- ❓ **Q82** - **G04 的 `spawn_unique` 两站（`session.rs:1376/1385`）要不要升级为可取消**：G04 原文自认「是设计选择不是缺陷」；第七片结案文本按维持记账。➡️ 推荐：**维持**——两站闭包自终止（转录 watcher 通道断连即返），升级不改变任何停机时刻。**标注：用户未答，按推荐项记账。**

**裁定（§4.2 四条判据对照）**：实际调用证据存在（本文件＋G31 三件套当场记）；必选测试通过（§2 当日矩阵）；无未解决的崩溃/数据丢失/安全越界/功能阻断问题（三条 frontier 全是决策非缺陷）；中低风险明确由谁接受（Q80–Q82 按推荐项记账、具名可重开；行为面端到端退出演练挂行政 B／P10-05，具名不静默）。**⇒ P3-04 标 accepted；`sessions` 门禁注册作为具名移交随 Q6 待答（与 `G30` 的 canary 要求同批），不阻塞 Phase 3 内下游（P3-05 的输入不依赖它）。**

## 2. 验收对照（2026-09-30 当日实测，退出码逐条单读）

| 命令 | 结果 |
|---|---|
| `cargo test -p aeroric-core`（core 全量） | **412/0 rc 0**（sessions 族 44 条在内） |
| `cargo test --manifest-path core/Cargo.toml`（workspace） | **455/0 rc 0** |
| `cargo test --locked --lib -- --test-threads=1`（宿主，门禁口径 argv） | **2465/0 rc 0（65.37 s）** |
| `pnpm native:check storage` | **5/412 rc 0** |
| `pnpm native:check skeleton` | **24/188 rc 0**（含 `watcher-discipline-src`＋`-unit` 11） |
| `pnpm native:check protocol` | **6/98 rc 0** |
| `pnpm native:check tasks/projects/runtime/agents/terminal-core` | **7/151、5/21、1/5、1/2、13/107 全 rc 0** |
| `pnpm native:contracts --check` | rc 0，12 份逐字节 |
| `pnpm typecheck` | rc 0 |
| 全量 `pnpm test` | **Test Files 376 passed／Tests 5 483 passed，rc 0（176.73 s）**，与前日基线逐字相同 |
| `pnpm native:doctor` | 18/18 rc 0 |
| `inventory --freeze/--freeze-reachability/--freeze-check/--check` | 全 rc 0（consistent；G04=closed-by-implementation、G03 剩两站；`F05.reviews` 派生进 `reviews/sessions.md`） |
| `extraction --write/--check`（sessions 清单） | 11 行／13 wrapper rc 0 |
| `digest --write/--check` | 14 片 rc 0 |
| 两 workspace `cargo fmt --check`＋`clippy --all-targets --locked -D warnings` | 全 rc 0 |
| `git diff --check` | rc 0 |
| `pnpm native:check sessions` | **未运行**（模块未注册＝Q6 待答，不冒充） |
| Swift／XCTest UI | 未运行（P4-01 前不存在 target，口径见 `tooling.md` §10） |
