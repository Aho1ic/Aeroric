# tauri → native 迁移评审记录:真正无头的 `aeroric-cli`(Task P3-07 ＝ Phase 3 Gate)

> **数字口径**:本文件里的 case 数／用例数一律是**验收当天的实测**，之后模块被别的 Task 扩过时以现场 `node scripts/native/check.mjs <module>` 与 `checks.json` 的 `minCases` 为准；既不当回归证据复用，也不为对齐改带日期的记录（口径原文见 `reviews/agents.md` 顶部横幅）。

## §4.2 七字段

- **tool 来源/revision**：`mattpocock/skills` @ `c55ee46073ed923f86ce59a5eb3b6d895095d1b7`——`grill-me/SKILL.md` 是转交 `grilling` 的入口（委托链见 `docs/native/tooling.md` §2/§3）。本会话 `skill://grilling` **加载成功**（与 P3-06 当日 404 不同，设备在册、文件可读），委托链在本轮全程成立。
- **实际调用入口**：① `skill://grilling` 读取＋按其轮次格式发出 frontier（§1 三条 Q89–Q91，用户不在场，按 P3-02/P3-04/P3-05/P3-06 先例记账）；② 五轴自查（§1，每条事实当场可复验）；③ 当日全矩阵（§2 表，退出码逐条单读——dsh 轮教训「收尾批必含 contracts+doctor」在本轮执行）。
- **审查 commit/diff hash**（G31 新规矩，**当场记**）：`git rev-parse HEAD`＝`a49ed85f9e683de08b9e937a2eeb27ef60f1fca1`（a49ed85，2026-10-01）；`git status --porcelain`＝**14 条目**（`M MIGRATION_PLAN.md`＋`M docs/native/digest/*` 11 份＋`M docs/native/feature-matrix.json`＋`M docs/native/task-status.json`＋`?? .zcodeignore`）——M 条目全是**评审当日注定落盘的文档与台账**（总表行改写、G05 分片判决、task-status accepted、digest 重写），按既定纪律随评审后的 `docs(plan)` 终提交一并入库；`.zcodeignore` 是用户未跟踪文件、非本任务、不入 diff。`git diff | git hash-object --stdin`＝`bda9d7a998d06d7fc89a34d87288f83f00ce254a`。代码树即最终树：⑤ `71fce43`（attach＋termios＋host 服务＋cli 测试）／⑥ `0093bde`（smoke＋checks.json cli 模块）／`942b278`（Q86 杀链＋core 串行闸＋措辞＋floor 4→5）＋`a49ed85`（clippy 尾巴）**全部先 commit 再评审**。porcelain 与 diff hash 并列读。
- **问答**：§1 frontier 三条（Q89–Q91）；无产品决策悬置——Q86（P3-06 移交的子进程策略）在本轮**已落地**（`kill_all_live`＋HUP 免疫 e2e，见 §1 轴 1/§2），不是悬置而是收账。
- **证据**：§1 五轴的当日读数、测试名与行级引用；§2 验收对照表；补记二百八十二～二百九十五的逐片实测段（见 `MIGRATION_PLAN.md` Task P3-07 小节与续跑区）。
- **问题级别**：**一处结构性真发现，根因修复**——smoke-headless 首跑绿、二跑红的竞态：`rpc task.run` 报 `StorageError::ContentDiverged`（同一二进制分钟前刚过门禁 ⇒ flaky）。根因＝core 内部 `save_covering→bump_covering` 两段写与后随 mutation 的 `check_cover` 无锁撞窗（表持旧指纹、盘持新字节）；**重试×3 修补实证无效**（µs 级重试追不上毫秒级落盘，3 跑 2 红）且修复后分析发现重试语义本身危险（一旦表盘重新对齐，陈旧快照会**静默盖回** settle 的状态更新＝丢更新）。终修＝`TaskService` 加 `mutations: Mutex<()>`＋`serial()`，包裹六入口（`run_intent`/`create`/`begin`/`complete`/`cancel`/`apply_observation`，互不嵌套已核）——`commit` 文档承诺的"每个变更先 load ⇒ 我看到的即盘上"只在变更读改写不被并发撕开时成立，桌面端由 React mutation 串行队列单线程保序，core 侧此前缺这一层。**一处执行事故，如实记**：前轮 clippy 复核命令带了 `head -8` 截断，`-D unused-mut` 的 error 行被截掉、"clippy 干净"结论失真；收尾轮全输出复跑当场逮住（`cli_headless.rs:549` 多余 `mut`），修后复验 0 error。**教训**：门禁/`lint` 裁定必须读全输出或读退出码，截断 grep 的"零命中"不是证据。**一行 OS 限制如实记**：termios 恢复覆盖 SIGTERM/SIGHUP（处理器存档→恢复→`SIG_DFL`→`raise`，128+15 死法）；**SIGKILL 无处可装处理器**＝OS 语义本身，不是缺口。
- **修复后复验**：本文写毕前当日矩阵全绿（§2 表），全部为当日实测，无引用旧读数。

## 1. 收尾拷问轮（2026-10-01，五轴）

**轮次输入**：①②a②b③a/b1.x/④⑤⑥ 全落地（补记二百八十二～二百九十四）＋⑦ 收口三件：Q86 子进程策略（`942b278`）、smoke 竞态根修（同提交）、措辞清理（SSH/WSL/dsh unsupported 边界三处对齐）＋cli 门禁地板 4→5 双写。

**五轴自查结论（事实，非问题——逐条当场可复验）**：

1. **唯一 owner**：无头变更入口＝`aeroric-cli` 命令树一处（`args.rs` 静态命令树；`rpc` 只到 host 注册表，无默认分支＝Action 6 的"不是任意代码执行"结构性成立）；缺 `--data-dir` 的硬断言在一处（`mutations_without_data_dir_exit_two_without_writing` 八条 mutation 全数拒绝 exit 2、隔离 HOME 零写入——§6.2 第一条边界从"没有入口"变成"入口自己拒绝"）；任务状态单一写者＝core `TaskService`（本轮串行闸把"写序"也收到一处：六个变更入口互斥，settle 线程与 RPC 线程不再同席竞写）；families 判定一处＝core `agents/families.rs`（b1.0，宿主薄壳）；session 解析一处＝P3-04 core 读侧（CLI `session history/export` 直接复用同一组 parse 函数，零复制）；approval 瀑布一处＝core `agents/dsh_host/approvals.rs`（CLI host 只注册，不复刻）。嵌套方向单一：cli→host→core，core 不知 cli（依赖图机器可查，cli-no-tauri-deps 门禁在册）。
2. **反例**：cli 门禁 5 case/9 用例当日全绿——其中形状反例：八条 mutation 缺 `--data-dir` 全部 exit 2 且零写入（不是单条）；attach 非 TTY 在**连接前**拒绝并提示 `events`/`session history` 替代读取命令（不是错误信息里含糊指引）；approval `deny`/`missing`/重复 `replay` 三条拒绝面全走真 CLI 子进程（`cli_headless.rs:276/:291/:229`）；cancel 后 settle＝`cancelled` 不是 `done`（宿主同款语义洞的 CLI 侧反例在 executor_pty 补测钉住）；HUP 免疫假 agent 在 serve 优雅退出后**仍然消亡**（`serve_sigterm_kills_running_task_children_even_hup_immune`——退出序从隐式 SIGHUP 升级为显式 `kill_all_live`，旧世代无具名句柄仍靠 master-close SIGHUP 兜底，注释两条路各记其因）；smoke 竞态本身是本轮最大反例（根修见 §4.2）。
3. **边界**：真实家零污染——cli_headless 全部走 `Scratch` 隔离 data-dir＋隔离 HOME，smoke 建 temp data-dir＋temp fake agent 且 `finally` 自清（serve SIGTERM→3s→SIGKILL→`rmSync` scratch，`smoke-headless.mjs:274–291`），fake agent 只写 `custom_agents` 于 scratch settings（补记二百九十四 fixture 案底的修正形态）；秘密＝host token 文件 0600（P3-06 载体沿用）；网络面＝零 TCP（UDS-only 沿 P3-06）；SSH/WSL/dsh 边界＝明确 unsupported 文案三处落字（`executor.rs` 模块头/拒绝理由、`services.rs`——SSH/WSL 归 P7-01/02、dsh 启动归宿主 `dsh_host::executor`，不借用 GUI 私有端口）；SIGKILL＝OS 限制如实记（见 §4.2）；`task input` 控制租约＝短顶替（acquire 即顶替前台优先、票据过期 release 为 no-op，attach 长持有不被误关）。
4. **回归**：core 全量 **488/0**（当日，含 b1.0 判定迁入的 4 条与既有全部族套件）；cli **9/0**（4 单元＋5 e2e）；host 门禁 **6/39**（executor-pty 3 条反例在内）；skeleton **24/188**（评审文件落盘后全绿，四红全是文档同步态：inventory/task-status 载体在评审文件创建后复位）；smoke **3× PASS**（当日三连）；doctor 18/18、contracts 12 份逐字节、`git diff --check` 0——本轮零 src-tauri/前端面（宿主 `--lib`/`pnpm test` 按零面惯例未跑，与补记二百九十三/二百九十四口径一致）；旧 Tauri 客户端零接触（P3-07 不动 `core_adapter.rs`）。
5. **回滚**：本 Task 六笔提交各自可独立 revert（`71fce43`⑤/`0093bde`⑥/`942b278`/`a49ed85`，前置三片在补记二百八十二～二百九十一各有提交号）；`--data-dir` 缺省语义回滚＝给 CLI 带上缺省参数即恢复旧行为（旧 CLI 本就不存在，无兼容面）；core 串行闸回滚＝删 `serial` 包裹即回原状（纯内存序，**零落盘格式变化**：runtime.json/tasks.json/lease 全是既有形状，bump_revision/指纹写入点未动）；smoke/fixture 全部自清，无新建用户数据、无生产路径写入（真实家对账同款判据由隔离形态结构性保证）。

**frontier 三条（产品决策，按提问轮格式；本轮用户不在场，处理照 P3-02/P3-04/P3-05/P3-06 先例＝按推荐项记账、异议可重开，不当作用户答复）**：

- ❓ **Q89** - **vendored portable-pty 的路径位置观察**：`cargo tree -p aeroric-cli` 对 `tauri` 的 2 处命中全部是**路径**（`src-tauri/vendor/portable-pty`），不是 crate 依赖——`cli-no-tauri-deps` 门禁按 crate 名（`tauri v`/`tauri-`）判禁，实测通过。但 vendored 目录**永久顶着 `src-tauri` 前缀**，而它实际经 `aeroric-core` 依赖图进入（P3-03 已接受的形态）。➡️ 推荐：**本轮不搬**（搬迁会动 core 依赖图与 P3-03 已接受记录，收益是纯观感）；把「vendor 目录名与消费方不一致」记为观察，归 **P10 全量 Review** 与未来 vendor 目录整理一并定夺。**标注：用户未答，按推荐项记账。**
- ❓ **Q90** - **core 变更串行闸的定性**：`TaskService` 六入口互斥锁是**core 内等价物**，不是对 G05 第三处（React mutation 串行队列）的替代：两端保序语义对齐（后写者基于前写者的完整落盘），宿主队列仍是跨窗口/多面板的权威保序面，core 闸只关"core 进程内并发线程"。已知代价如实记：闸内含 `executor.launch`（进程诞生）与 `executor.complete/cancel`（进程停止），慢启动/慢停止会顶住同进程后续 mutation；无头宿主单用户、启动/停止有界，可接受。备选（按任务粒度分锁）被否＝过度工程：`check_cover` 的拒绝语义本就按表全局指纹，分锁救不了它。➡️ 推荐：**维持现状**，把"闸内含进程启停"记入 `serial()` 注释，P4-01 起 SwiftUI 客户端并发面上若出现可感知延迟再议细分。**标注：用户未答，按推荐项记账。**
- ❓ **Q91** - **G05 残片的两条 owner**：P3-07 片（`pendingTaskStartsRef`）本轮已闭合（判决块见 Task P3-07 小节）；`manuallyCompletedDshTasksRef`→P3-05（该小节**无分片落判记录**，本轮不代判）、`remoteTaskMutationQueuesRef`→P8-03（未开工）。➡️ 推荐：**owner 不变、G05 整条维持 open**；P3-05/P8-03 各自的收口轮补各自判决，台账 `retriage_note` 本轮只记"P3-07 片已闭合＋判决指针"。**标注：用户未答，按推荐项记账。**

**裁定（§4.2 四条判据对照）**：实际调用证据存在（本文件＋`skill://grilling` 本会话加载成功＋G31 三件套当场记）；必选测试通过（§2 当日矩阵：cli 门禁真子进程真 PTY、skeleton 全绿、core/host/cli/smoke/doctor/contracts 全 rc 0；approval 四态、cancel-settle、HUP 免疫子进程、termios 双路径恢复各有载体）；无未解决的崩溃/数据丢失/安全越界/功能阻断问题（smoke 竞态根修＋重试反模式实证拆除；clippy 截断事故当场逮修；ContentDiverged 误拒与丢更新两条险路都被串行闸结构性封死）；中低风险明确由谁接受（Q89–Q91 按推荐项记账、具名可重开；SIGKILL 不恢复 termios＝OS 语义、具名不静默；闸内进程启停延迟具名记入 Q90）。**⇒ P3-07 标 accepted；Phase 3 Gate 通过——P4-01～P4-04 的阻塞项（总表 Phase 3 Gate 行）自此解除，G05 余片不阻塞 Phase 4（P4 输入不依赖它们）。**

## 2. 验收对照（2026-10-01 当日实测，退出码逐条单读）

| 命令 | 结果 |
|---|---|
| `pnpm native:check cli`（本 Task 点名的新模块） | **5 case／9 用例 rc 0**（cli-no-tauri-deps 门禁＋cli-help-matches-contract 门禁＋cli-smoke-headless 门禁＋cli-unit-rust **4**＋cli-headless-rust **5**，floor 4→5 双写在案） |
| `cargo tree -p aeroric-cli`（AC 第 1 条） | 不含 Tauri crate：仅 2 处 vendored **路径**命中（`src-tauri/vendor/portable-pty`，观察入 Q89）；`cli-no-tauri-deps` rc 0 |
| `cargo run -p aeroric-cli -- --help`（AC 第 2 条） | 命令树与 Action 1 契约一致：doctor/serve/project/run/task/session/approval/rpc/events/attach＋`--data-dir` 必填前置＋exit-code 表写进 `args.rs::exit`（0/2/3/4/5/130 六档在码） |
| `node scripts/native/smoke-headless.mjs`（AC 第 2 条） | **当日 3× PASS**（run→done exit 0、settled input 拒写、resume rc 0、cancel→cancelled、session list/history、approval list；`finally` 自清进程与 scratch，无孤儿 fake agent） |
| `cargo test -p aeroric-core`（core 全量） | **488/0 rc 0** |
| `cargo test -p aeroric-cli`（单测＋headless） | **9/0 rc 0**（4 单元＋5 e2e；approval 四态＝`cli_headless.rs:212 allow / :229 replay / :276 deny / :291 missing`，全部真 CLI 子进程） |
| `pnpm native:check host` | **6 case／39 用例 rc 0**（executor-pty 3 反例：done/cancelled 0.55s/session-history 家族分派） |
| `pnpm native:check skeleton` | **24 case／188 用例 rc 0**（评审文件落盘前 4 红全为文档同步态：inventory-consistency/-freeze-is-noop/-unit＋task-status-unit 的载体＝`reviews/cli.md` 不存在，落盘后单轮复位，非代码回归） |
| `pnpm native:doctor` | **18/18 rc 0** |
| `pnpm native:contracts --check` | rc 0，**12 份逐字节** |
| `inventory --freeze / --check` ＋ `digest --write / --check` | freeze 在评审文件就位后 rc 0；task-status accepted 三处同步一致；digest 14 shards ≤ 32 KiB cap，`--check` rc 0 |
| `cargo clippy -p aeroric-core -p aeroric-cli -p aeroric-host --all-targets -- -D warnings` | **0 error**（全输出复跑；前轮截断事故见 §4.2，`a49ed85` 即其修复） |
| `cargo fmt --check`（core/cli/host） | rc 0 |
| `git diff --check` | rc 0 |
| 宿主 `--lib`／全量 `pnpm test` | 未跑（零宿主/前端面；口径同补记二百九十三/二百九十四） |
| 真实家对账 | 结构性保证：cli 全套件走 Scratch＋隔离 HOME，smoke 全程 temp data-dir＋自清；无任何用例直写 `~/.aeroric`（形态同 host.md 的 G19/G20 教训修正后口径） |
| Swift／XCTest UI | 未运行（P4-01 前不存在 target，口径见 `tooling.md` §10） |
