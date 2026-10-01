# tauri → native 迁移评审记录:Swift actor IPC、宿主监督和状态 Store(Task P4-02)

> **数字口径**:本文件里的 case 数／用例数一律是**验收当天的实测**，之后模块被别的 Task 扩过时以现场 `node scripts/native/check.mjs <module>` 与 `checks.json` 的 `minCases` 为准；既不当回归证据复用，也不为对齐改带日期的记录（口径原文见 `reviews/agents.md` 顶部横幅）。

## §4.2 七字段

- **tool 来源/revision**:`mattpocock/skills` @ `c55ee46073ed923f86ce59a5eb3b6d895095d1b7`——`grill-me/SKILL.md` 是转交 `grilling` 的入口（委托链见 `docs/native/tooling.md` §2/§3）。本会话 `skill://grilling` 加载成功，委托链全程成立。
- **实际调用入口**:① `skill://grilling` 按其轮次格式发出 frontier（§3 两条 Q92–Q93，用户不在场，按 P3-02/P3-04/P3-05/P3-06/P3-07 先例记账）；② 五轴自查（§1，每条事实当场可复验）；③ 当日全矩阵（§2 表，退出码逐条单读）。
- **审查 commit/diff hash**（G31 新规矩，**当场记**）:`git rev-parse HEAD`＝`8197cad6ff9b0ee95660e00cbcc000988aa1a6ca`（8197cad，2026-10-01）——P4-02 代码与测试已分两笔入库（`df593bb` feat(macos) 15 文件 1512 插入＋`8197cad` test(macos) 8 文件 803 插入）；`git status --porcelain`＝**2 条目**（`M scripts/native/checks.json`＝G09 absent case `client-socket-io-single-exit`（评审期间当场新建并跑绿）＋`?? .zcodeignore`）——前者属评审产物，随收口提交入库；`.zcodeignore` 是用户未跟踪文件、非本任务、不入 diff。`git diff | git hash-object --stdin`＝`de022ebadcf79fa720647e584796ac271fbde7ae`。
- **问答**:§3 frontier 两条（Q92–Q93）；无产品决策悬置——G09 在本轮当场闭环（门禁 case 三跑收敛后绿），G22 的判决是「仍开放、owner P4-03」，不新增表态项。
- **证据**:§1 五轴的当日读数、测试名与行级引用；§2 验收对照表；补记二百九十八～二百九十九的逐片实测段（见 `MIGRATION_PLAN.md` Task P4-02 小节与续跑区）。
- **问题级别**:**一处环境级真发现（不可归因代码），一处门禁首跑即红的过宽禁形（当场收窄）**——① `pnpm native:ui-test` 的 XCUITest runner 四连「Timed out while enabling automation mode」（固定 71 秒），`testmanagerd` 以 root 运行且 `launchctl kickstart` 被 SIP 拦截（`Operation not permitted while System Integrity Protection is engaged`），无 sudo 不可修复；同日 `launchservices open` 冒烟证明窗口装配、supervisor 拉起与真宿主 serve 端到端全通（数据目录生成 host.sock/host.token/projects/runtime.json/terminal-history）——「UI 空白死锁」经逐点 DIAG 复核定性为**诊断方式的伪象**（直接 exec 二进制不装配 SwiftUI 场景，是直跑验证的系统性盲区，不是产品代码 bug）；② G09 absent case 第一版把 `connect\(` 收进禁形，当场被 `HostSupervisor.swift:218: try await connection.connect()`（合法的 actor 方法调用）打红——禁形收窄到真套接字调用后三 case 全绿，禁形宽度教训与 P3-06 `projects-core-has-no-ui-coupling` 的"只拦真实调用写法"教训同源，已写进 case `$note`。
- **修复后复验**:本文写毕前当日矩阵全绿（§2 表），全部为当日实测，无引用旧读数。唯一红项＝ui-test 环境级阻塞（runner init 超时），**非代码失败**，§2 如实记 blocked(external)。

## 1. 收尾拷问轮（2026-10-01，五轴）

**轮次输入**:补记一百二十四移交项（command-map `modules` 口径，`0c7c7b2`）→ 六源文件＋State 三 Store＋App 层三文件（`df593bb`）→ 四测试文件＋checks.json swift-client 模块（`8197cad`）→ stash 污染事故恢复（PackageSmoke/ProtocolV1 公开 init/PackageSmokeTests batch API 逐一重建）→ G09 case 当场建＋三跑收敛。

**五轴自查结论（事实，非问题——逐条当场可复验）**:

1. **唯一 owner**:AeroricClient 的 socket I/O 唯一出口＝`HostConnection.swift`（`socket()` :40、`Darwin.connect` :56、`SocketTransport.connect` :165），G09 absent case `client-socket-io-single-exit` 把这条从 grep 审计升级成门禁（`--glob '!HostConnection.swift'` 后其余七文件禁形零命中）；请求路由唯一权威＝`RequestRegistry` actor（connect 外层伪 id 校验已删除——迟到/重复/未知 id 返 false 不 fault）；宿主进程代与重连唯一权威＝`HostSupervisor`（相位机 launching→connecting→ready→degraded→reconnecting→failed）；App 侧 supervisor 唯一入口＝`AppServices.startSupervisor()`（幂等 guard，`.task`＋`onChange(scenePhase)` 双调用点收敛到一处）。
2. **反例**:`pnpm native:check swift-client` 3 case 当日全绿——其中形状反例:①encodeLine 自带尾 `\n`，`HostConnectionTests` 断言发送帧不再二次加换行（旧版 `request()` 手动 append 产生的空帧→splitter 吐空行→decodeLine fault，测试面有直接反例）；②宿主→客户端 `.request` 帧静默丢弃（P4-02 不支持，不 crash）——测试钉住这是**协议合法**而非漏处理；③`testChildKillLeadsToReconnecting` 断言 redesign 成强不变量（新 pid≠旧 pid＋degraded 相位出现过——原 `attempt==1` 相位窗口 <100ms 在 TSan 下不可靠，属写测试手法反例）。
3. **边界**:测试全部走 `TestSupport.ScriptedServer`（进程内假宿主，bind 前 unlink 旧 socket——与真宿主行为对齐，kill 后 respawn 不再 bind 冲突 attempt 无限涨）；App 冒烟用 launchservices open＋`com.aeroric.native.dev` 预览 bundle 数据目录（生产 `com.aeroric.desktop` 零接触，ADR-001 双 bundle 隔离）；`childEnvironment` 白名单 `TEST_RUNNER_*`，宿主不继承 GUI 会话环境；token 0600 与 UDS-only 沿 P3-06 载体；无 TCP、无 PATH 搜索（cliExecutable 默认 Bundle 内 `Contents/MacOS/aeroric-cli`）。
4. **回归**:AeroricClientTests **28/28** 绿（exit 0）＋TSan `--sanitize thread` **28/28** 绿（exit 0，EventReducer 10＋HostConnection 10＋Supervisor 4＋PackageSmoke 4）；`pnpm native:build` BUILD SUCCEEDED（strict concurrency 下 AppServices 闭包强捕获＋`await MainActor.run` attach）；`pnpm native:check skeleton` 24 case/188 用例绿；`pnpm native:check swift-client` 3 case/56 用例绿；inventory `--check` consistent（registered=660 mapped=660）；doctor 18/18；launchservices 冒烟（真宿主 serve 端到端）。**blocked(external)**:`pnpm native:ui-test` runner init 超时（testmanagerd root＋SIP，四连复现固定 71 秒）——`AppLaunchTests` 的 `bootstrap.connection` 存在性断言已入库但本轮无法执行；需用户重启系统后复跑。
5. **回滚**:本 Task 两笔提交各自可独立 revert（`df593bb`/`8197cad`）；G09 case 是 checks.json 追加（revert 该 hunk 即回 grep 审计时代）；State Store 是新文件（删除即回 P4-01）；AppServices 的 supervisor 字段回滚＝删 startSupervisor/stopSupervisor 两方法＋AeroricApp 两 modifier（BootstrapView connection/application 两行同步删）；协议公开 init 回滚＝EventReducer/HostConnection 改 @testable 构造（Product target 不可用，需同步回滚两文件——孤立 revert `ProtocolV1.swift` 会红编译，rollback 文档已在 P4-02 小节记明组合）。

## 2. 当日验收对照

| 门禁 | 读数 | 出处 |
|---|---|---|
| `swift test --filter AeroricClientTests` | 28 tests, 0 failures, exit 0 | 当日 `swift test` |
| TSan（`--sanitize thread` 同 filter） | 28 tests, 0 failures, exit 0 | 当日 `swift test` |
| `pnpm native:check swift-client` | 3 case(s) passed, 56 test cases executed（unit 28＋tsan 28＋G09 absent） | 当日 `pnpm native:check` |
| `pnpm native:build` | BUILD SUCCEEDED（App 链接 AeroricClient product） | 当日 `pnpm native:build` |
| `pnpm native:check skeleton` | 24 case(s) passed, 188 test cases executed | 当日 `pnpm native:check` |
| `node scripts/native/inventory.mjs --check` | consistent（registered=660 mapped=660） | 当日 `inventory --check` |
| `node scripts/native/doctor.mjs` | all 18 checks satisfied | 当日 doctor |
| launchservices 冒烟 | host.sock/host.token/projects/runtime.json/terminal-history 生成；supervisor 端到端 serve | 当日 `open` 冒烟 |
| `pnpm native:ui-test` | **blocked(external)**:XCUITest runner「Timed out while enabling automation mode」×4（固定 71 s）；testmanagerd root＋SIP 不可重启 | 当日 `xcodebuild test` |

## 3. frontier 两条（产品决策，用户不在场，按先例记账）

- ❓ **Q92 - ui-test 环境阻塞的处置**:XCUITest runner 初始化超时是系统级（testmanagerd 以 root 跑、SIP 拦 kickstart），本轮四连复现；恢复手段只有重启系统（用户侧动作）。➡️ 推荐:**P4-02 以「ui-test 环境阻塞待复跑」收口**，`AppLaunchTests` 断言已入库、恢复后在 P4-03 开工前补跑一次即可；不因此阻塞 P4-02 accepted（真宿主 serve 的 launchservices 冒烟已覆盖"窗口装配＋宿主拉起"的主链）。**标注:用户未答，按推荐项记账。**
- ❓ **Q93 - 直跑二进制的诊断价值定性**:「直接 exec app 二进制不装配 SwiftUI 场景」使**手动直跑**从此不可作为 UI 装配证据；有效通道＝launchservices `open`（场景装配＋进程环境正常）与 XCUITest（断言面）。➡️ 推荐:**把「验证 GUI 必须走 launchservices/XCUITest，直跑二进制只验主线程健康」写进 P4-03+ 的验证纪律**（并入 §3.1 工程惯例，不新立门禁）。**标注:用户未答，按推荐项记账。**

## 4. G09/G22 判决（本 Task 小节要求的逐条落地）

- **G09（AeroricClient 唯一 RPC 出口反例门禁）→ 已闭合**:门禁 case `client-socket-io-single-exit` 当场建于 checks.json swift-client 模块（absent 型，禁形＝真套接字调用，排除 `HostConnection.swift`；三条正控防"路径错→rg 读空"的恒真假绿），当日三 case 全绿。owner 移交 P10 全量 Review 维护（随模块扩改动保持）。
- **G22（AGENTS.md/spec 的 `NativeSelection.swift` 现在时错述）→ 仍开放、owner P4-03**:该文件由 P4-03 创建（ADR-001 §4 `(P4-03)` 括注），到时错述自动转真；措辞将来时化是用户说明书编辑（decision_item 7），不代改。P4-02 名下无新增义务。
