# tauri → native 迁移评审记录:认证 UDS 宿主(Task P3-06)

> **数字口径**:本文件里的 case 数／用例数一律是**验收当天的实测**，之后模块被别的 Task 扩过时以现场 `node scripts/native/check.mjs <module>` 与 `checks.json` 的 `minCases` 为准；既不当回归证据复用，也不为对齐改带日期的记录（口径原文见 `reviews/agents.md` 顶部横幅）。

## §4.2 七字段

- **tool 来源/revision**：`mattpocock/skills` @ `c55ee46073ed923f86ce59a5eb3b6d895095d1b7`——`grill-me/SKILL.md` 是转交 `grilling` 的入口（委托链见 `docs/native/tooling.md` §2/§3）。本会话 `skill://grilling` 解析失败（清单在册、设备 404），处理照 `reviews/dsh.md` 同族口径：**评审实质完成**（六片逐片实测＋收口轮当日全矩阵＋frontier 三条按格式发出），工具 404 不折算成「未评审」。
- **实际调用入口**：① 收口轮的根隔离验证＝真实家跑前后对账（§1 轴 3，行级读数＋哈希，G21 出口判据的原文形状）；② 当日全矩阵（§2 表，退出码逐条单读——dsh 轮教训「收尾批必含 contracts+pnpm test」在本轮执行）；③ 五轴自查（§1，每条事实当场可复验）。
- **审查 commit/diff hash**（G31 新规矩，**当场记**）：`git rev-parse HEAD`＝`8b889d4396cb5c1bf435862636f8fb65ebbc8bd7`（8b889d4，2026-09-30）；`git status --porcelain`＝1 条目（`?? .zcodeignore`，用户文件、非本任务、不入 diff）；`git diff | git hash-object --stdin`＝`e69de29bb2d1d6434b8b29ae775ad8c2e48c5391`（＝空 diff：六片实现（94da06b events／09595c4 codec-uds-startup／ec85812 三道闸／aac0a6a serve+注册／1a3e1d5 diagnostics+drain／99f8716+8b24efe 收口）＋slow-client 补测（8b889d4）全部先 commit 再评审，审查树即最终树）。porcelain 与 diff hash 并列读。
- **问答**：§1 frontier 三条（Q86–Q88）；无产品决策悬置——D 系列（transport/端口簇/G17）已在 dsh 轮由用户答复（补记二百六十四），本轮的根隔离/退役/死订阅判定都是计划内工程决策。
- **证据**：§1 五轴的当日读数、测试名与真实家对账表；§2 验收对照表；补记二百七十五～二百八十一的逐片实测段。
- **问题级别**：**一处中风险发现，当场修复**——`run_dsh_task` 改名丢 `_core` 后缀触发 Action 4 方向扫描两红（dsh 接线片 268 埋入，因当时未跑 tauri-adapter 潜伏到本轮才爆——**覆盖面教训第三例**，同族 252/266 已入各自补记；修复＝改回 `run_dsh_task_core`＋名字分工写进模块头）。**一处执行事故，如实记**：收口轮修复窗口内一次未隔离的 `cargo test --lib` 直跑写了真实家（audit +3 伪造 invite-created、usage +25 行）——污染不可撤回（伪造行留在真实日志），防线＝宿主套件此后只走 runner 注入或 `HOME=$(mktemp -d)` 二进制直跑两种隔离形态（补记二百八十 §2）。零缺陷级残留。
- **修复后复验**：本文写毕前当日矩阵全绿（§2 表），全部为当日实测，无引用旧读数。

## 1. 收尾拷问轮（2026-09-30，五轴）

**轮次输入**：六片全部落地——core `events/{mod,log,snapshot}`（seq 单一原子边界＋watermark 合并，二百七十五）、host `codec/uds/startup`（流层封顶＋sun_path 前置拒绝＋探活＋五步启动，二百七十六）、`auth/dispatcher/subscriptions`（三道闸＋无默认分支注册表＋状态机，二百七十七）、serve 接线＋四集成文件＋`native:check host` 注册（二百七十八）、diagnostics Redactor＋退出序（二百七十九）、收口片（根隔离三层读序＋G39 默认恢复＋七移交两 topic，二百八十）＋slow-client 补测（二百八十一）。

**五轴自查结论（事实，非问题——逐条当场可复验）**：

1. **唯一 owner**：数据根解析在 `storage::aeroric_dir()` 一处（三处逐字重复并一；读序 SLOT > `AERORIC_HOME` > 真实家，优先级由 core_adapter_tests 9 红当场立法）；seq 分配/发布在 core `events::log` 一处（`Inner{next_seq,entries,evicted}` 单临界区，宿主的 `remote/event_log.rs` 原样未动——它是 legacy 客户端的地盘，退役归 P10 全量 Review）；补发/直播去重在 `events::snapshot` 一处（dispatcher 与测试都消费它，第二份「比大小」不存在）；认证判据在 `auth` 一处（token 定长比较＋peer-UID 双平台）；方法可达性在 `ServiceSet` 一处（无默认分支）。嵌套方向单一：host→core 调用，core 不知道 host 存在（`aeroric-core` 不依赖 `aeroric-host`，依赖图机器可查）。
2. **反例**：host 36 条全绿当日实测——其中形状反例：overlong 在**凑齐行之前**断（等换行的实现会 OOM/永挂）、partial 不猜（半帧关闭＝断连不是超时）、malformed 断连不降级、gate 不泄露方法存在性（未认证的未知方法与未认证的 mutation 得同一个 Unauthenticated）、再握手拒且身份不覆盖、空值登记被 Redactor 拒绝（空串匹配一切＝自毁不是安全）、二次启动 lock 步拒绝（`step 1 (lock):` 前缀可分类）、crash 残留探活清绑 vs 正常退出 unlink（两条路都有主）、104 并发 append 的补发序与集合精确（seq 不丢不重＋严格递增＋三个 watermark 的补发长度＝latest−watermark）。
3. **边界**：真实家零污染（当日对账：remote-audit.log 行数 2173→2173 与哈希不动、agent-credentials 边车哈希不动、terminal-history 166 文件不动；usage 行级 +9 核为在跑客户端正常采集非测试签名——G21「按内容不按 mtime」的出口判据原文形状）；权限＝socket 0600（断言）＋token 文件 0600（断言）＋跨 uid 拒绝由 accept 类型面保证（单测边界如实记：跨 uid 活体拒绝需第二用户，不假装测过）；秘密＝`HostToken` Debug 掩码＋`AuthError` 文案不带明文＋Redactor 兜第三方文本；网络面＝host 库零 TCP 构造（G07 host 侧闭合的机器依据）；ready_fd 的 `from_raw_fd` 有 SAFETY 注释且 Drop 单关路径。
4. **回归**：旧 Tauri 客户端＝宿主 `--lib` 2452 条**一条不少**且经 runner 默认必选通道跑（G39 恢复）；`adapter-acceptance-real-kernel` 19 条默认必选＋scratch 根下全绿（agent 二进制走 PATH、managed home 由 app_settings 每轮生成——pre-place 不需要）；前端＝`pnpm test` 376/5483、`typecheck` 0、`contracts` 12 份逐字节；协议 v1 与存储＝零方法零格式变更（load_runtime 消费既有文件、bump_revision 既有写入口）；`native:check` 九模块当日全绿（§2）。
5. **回滚**：六片各一个 feat/test 提交＋随批件（lock/floor/锚点/pin），逐片可独立 revert；`AERORIC_HOME` 未设时行为与回滚前逐字相同（生产零变化）；两条 case 的默认化回滚＝恢复 `optInEnv`（pin 测试会红着提醒）；`plan_start` 删除可 `git revert` 找回；无落盘格式变化、无新建用户数据。

**frontier 三条（产品决策，按提问轮格式；本轮用户不在场，处理照 P3-02/P3-04/P3-05 先例＝按推荐项记账、异议可重开，不当作用户答复）**：

- ❓ **Q86** - **`ShutdownPolicy` 三件的归属定法**：Action 5 的「取消 watcher、flush storage、按退出策略处理子进程」在 aeroric-host 当前射程内如实不存在（二百七十九 §2 记账：watcher 等价物＝连接 drain 已实现；flush＝写者路径本就原子写；子进程＝host 库不 spawn 任何东西，CLI 接通在 P3-07）。➡️ 推荐：**子进程策略随 P3-07 的 `aeroric-cli serve` 入口立**（那时才有真子进程可管），P3-06 不为凑验收造空壳 enum。**标注：用户未答，按推荐项记账。**
- ❓ **Q87** - **死订阅 TS 监听的删除时机**：`app-exit-requested`/`app-restart-requested` 判 not-forwarded（Rust 无发射点，`src/tauriEvents.ts:41-42` 的监听是历史遗留）。TS 侧现在删会动 legacy 客户端；留着则是「两份真相」的温床。➡️ 推荐：**P4-01（SwiftUI 工程）时随前端退役一并删**，本轮只在 pending 表记判定、不动 TS。**标注：用户未答，按推荐项记账。**
- ❓ **Q88** - **G07 P8-03 侧的钉法**：remote/server.rs 的 0.0.0.0 scope 判定（任一配对设备或公网 endpoint 即绑全网卡）要求「host 侧保留同等保守」——P3-06 侧已闭（UDS-only），P8-03 侧的「同等保守」需要一个机器判据（测试或门禁），否则 requirement 只是散文。➡️ 推荐：P8-03 开工时把 scope 判定做成**纯函数＋负例测试**（有配对设备⇒绑环回被拒），归 `remote` 模块门禁。**标注：用户未答，按推荐项记账。**

**裁定（§4.2 四条判据对照）**：实际调用证据存在（本文件＋G31 三件套当场记＋真实家对账表）；必选测试通过（§2 当日矩阵，含验收行的拒绝面/并发序/slow client/crash 残留/二次启动/ready 形状全部有载体）；无未解决的崩溃/数据丢失/安全越界/功能阻断问题（改名翻案与污染事故都当场修复/如实记；`AERORIC_HOME` 通道经 pin 焊死——新增 opt-in 或丢注入都会红）；中低风险明确由谁接受（Q86–Q88 按推荐项记账、具名可重开；跨 uid 活体拒绝的单测边界由 accept 类型面兜底，具名不静默）。**⇒ P3-06 标 accepted；`ShutdownPolicy` 的子进程半边随 Q86 具名移交 P3-07，G07 的 P8-03 半边随 Q88 具名移交，均不阻塞 Phase 3 内下游（P3-07 的输入不依赖它们）。**

## 2. 验收对照（2026-09-30 当日实测，退出码逐条单读）

| 命令 | 结果 |
|---|---|
| `pnpm native:check host`（本 Task 点名的新模块） | **5 case / 36 rc 0**（lib 20＋ipc_auth 5＋ipc_frames 5＋event_replay 3＋host_lifecycle 3） |
| `pnpm native:check tauri-adapter` | **12 case / 2513 rc 0**——含 G39 恢复默认的两条：`src-tauri-lib-tests` **2452**、`adapter-acceptance-real-kernel` **19**（均带 `AERORIC_HOME=<scratch>` 注入） |
| `cargo test -p aeroric-core`（core 全量） | **473/0 rc 0**（events 族 7 条在内） |
| `pnpm native:check storage` | **5/473 rc 0**（floor 链 451→462→463→466→473 逐批实测在册） |
| `pnpm native:check skeleton` | **24/188 rc 0**（gate-runner 36/36 含 opt-in 退役 pin；inv02/core-discovers 全过） |
| `pnpm native:check protocol` / `projects` / `tasks` | **6/98、5/21、7/151 全 rc 0** |
| `pnpm native:check runtime` / `agents` / `terminal-core` | **1/5、1/2、13/108 全 rc 0** |
| `pnpm native:contracts --check` | rc 0，**12 份逐字节** |
| `pnpm typecheck` | rc 0 |
| 全量 `pnpm test` | **Test Files 376 passed／Tests 5483 passed rc 0（170.62 s）** |
| `pnpm native:doctor` | 18/18 rc 0 |
| `inventory --freeze/--freeze-reachability/--check` | 全 rc 0（consistent；G34 closed-by-decision、G19/G20/G21/G24/G39 closed、G07 收窄 P8-03、两 topic not-forwarded 全过词表与 T1 规则；storage 5＋platform 2 锚重锚） |
| `digest --write/--check` | rc 0 |
| 两 workspace `cargo fmt --check`＋`clippy --all-targets -- -D warnings`（新口径：`cd <ws> && … -p <pkg>`，读输出不读 rc） | 全 rc 0 |
| `git diff --check` | rc 0 |
| 真实家对账（跑前后） | audit 行数/哈希、边车哈希、history 计数**全不动**；usage +9＝在跑客户端采集（签名判据全零） |
| Swift／XCTest UI | 未运行（P4-01 前不存在 target，口径见 `tooling.md` §10） |
