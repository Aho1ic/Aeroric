# tauri → native 迁移评审记录:主窗口、设置窗口、菜单与导航(Task P4-04)

> **数字口径**:本文件里的 case 数／用例数一律是**验收当天的实测**,之后模块被别的 Task 扩过时以现场 `node scripts/native/check.mjs <module>` 与 `checks.json` 的 `minCases` 为准(口径原文见 `reviews/agents.md` 顶部横幅)。

## §4.2 七字段

- **tool 来源/revision**:`mattpocock/skills` @ `c55ee46073ed923f86ce59a5eb3b6d895095d1b7`——`grill-me/SKILL.md` 是转交 `grilling` 的入口(委托链见 `docs/native/tooling.md` §2/§3)。本会话 `skill://grilling` **加载成功**,委托链成立。
- **实际调用入口**:① `skill://grilling` 读取＋按其轮次格式发出 frontier(§1 三条 Q96–Q98,本轮用户以「跳过 UI 验证、继续计划」的显式裁定在场,裁定记录见下,其余两条按 P3-02 先例按推荐项记账、异议可重开);② 五轴自查(§1);③ 当日验收矩阵(§2 表,退出码逐条单读)。
- **审查 commit/diff hash**(G31 规矩,**当场记**):`git rev-parse HEAD`＝`c5db83a7359731000bab4186dcfb21bcf3ac6adc`(c5db83a,2026-10-02);`git status --porcelain`＝**1 条目**(`?? .zcodeignore`——用户未跟踪文件、非本任务、不入 diff);`git diff | git hash-object --stdin`＝`e69de29bb2d1d6434b8b29ae775ad8c2e48c5391`(**空 blob＝diff 为空**)。代码树即评审树:P4-04 全部实现分十一笔提交入库(`bcebf59` 实现＋两根修 → `9a4618e`/`2e83c22`/`65ed768` 标注 → `10b9d58`/`b5dd0b9`/`14a286a` 测试基建 → `6e771f6`/`889fbd8` G41 两层根修 → `9ead5cb` 注释措辞),评审文件与总表行随评审后 `docs(plan)` 终提交入库。
- **问答**:§1 frontier 三条(Q96 用户裁定＋Q97/Q98 按推荐项记账);另有两项历届挂账在 P4-04 兑现——Q93(bundle id 定型)按推荐**延后**、Q95(eyecare)未入本轮。
- **证据**:§1 五轴当日读数;§2 验收对照表;过程账全链在 `MIGRATION_PLAN.md` Task P4-04 小节补记三百零二～三百零八。
- **问题级别**:**四处真发现,全部当场根修并有机器实证**——① **容器级 accessibilityIdentifier 向下劫持叶子**(AX dump 实证 `bootstrap.environment` 曾被 `workspace.detail`/`bootstrap.info` 覆盖,AppLaunchTests 全线误报"没画出来";P4-02/03 只断言存在所以从未暴露)→移除容器标识符,AX 复验 11 叶子全可查;② **applicationWillTerminate 主线程自锁死锁**(信号量等 MainActor Task,每次退出孤儿化 serve 并毒化共享 scratch 锁)→`LiveChildSlot`+nonisolated 同步终止,slot-trace 实证退出即杀;③ **SIGTERM 掩码继承**(GUI 工作线程阻塞信号被 posix_spawn 子进程继承,serve 杀不死)→spawnattr SETSIGMASK/SETSIGDEF,对照实验实锤;④ **XCUITest 场景装配间歇缺失+reopen 幽灵窗口**(testmanagerd 会话级,随累计启动加剧;reopen 拒门实验证明与 AX 会话建立竞态后回退)→测试基建分层吸收(逐测试会话/fresh-instance 重试/firstMatch/值断言)。**一项用户裁定如实记**:UI 6/6 验收经用户明示**跳过**(2026-10-02「先标记一下跳过这个部分吧」),测试资产保留在库(最佳轮 5/6;各用例逻辑分别在 r6/r9/r10 单独通过),残余风险 owner＝用户,重开路径＝P10-02 QA 或随时重跑 `pnpm native:ui-test`。
- **修复后复验**:§2 表全绿均为当日实测;`shell` 3 case/24 用例 rc 0(评审当日,注释措辞触 Gate 一事后即修即绿——词形门禁不豁免注释,G09 同款教训再入账)。

## 1. 收尾拷问轮(2026-10-02,五轴)

**轮次输入**:P4-04 十四文件(Shell 六＋Settings 三＋AppCommands＋AeroricApp/AppDelegate/AppServices/State 改造)＋测试基建(AeroricUITests 两文件)＋门禁(shell 3 case/floor 24)全部落地;十三笔提交。

**五轴自查结论(事实,非问题——逐条当场可复验)**:

1. **唯一 owner**:窗口/场景单源＝`AeroricApp.swift`(`WindowGroup(id:"main")` 唯一入口,`AeroricCommands` 唯一命令注册点,`OpenWindowHook` 唯一捕获点);命令行为单源＝`WorkspaceActions.route`(菜单与命令面板同路由,task.new 无目标时空转);设置窗口单例＝`SettingsWindowController.shared`(重键复用,`windowShouldClose` orderOut 不销毁);宿主生命周期单源＝`AppDelegate`(`applicationWillTerminate`→`terminateChildForProcessExit` 同步杀 serve;`LiveChildSlot` 是子进程句柄在 actor 外的唯一出口);shell 状态单源＝`WorkspaceStateProviding`/`SettingsStateProviding` 注入(AeroricUI 无 client 依赖,INV-02);数据写入者唯一＝`AppServices.supervisor`(多窗口共享单连接,`openAnotherMainWindow` 只开场景不开连接)。
2. **反例**:`shell-no-web-navigation`(WKWebView/loadHTMLString 等 6 禁形＋6 正控证明 Shell 六文件在位)、`shell-no-hand-drawn-selection`(isSelected/isActive 等词形禁形＋2 正控;曾逮住评审注释字面量——词形门禁不豁免注释,G09 教训复账)、`shell-unit` floor 24(ThemePolicy 10＋PackageSmoke 4＋ShellPolicy 10:DetailState.resolve 四态映射×6＋filterCommands×4);UI 侧:`AppLaunchTests` 注入值精确比对、`ShellLifecycleTests` 值断言(`detail.selection` 读值,不依赖 AX isSelected——实测 outlineRow/行内文本恒 NO)。
3. **边界**:app 零直接存储访问(数据目录只透传给 serve;项目经真 `project.open` RPC);测试钩子 `AERORIC_TEST_OPEN_PROJECT`/`AERORIC_SERVE_STDERR_LOG` 双 env 门控,plain launch 零行为;scratch 数据目录逐 run rm+mkdir＋bundle id 与旧客户端隔离;`pkill` 清扫只匹配本 scratch 路径;`~/Library` 默认目录仅在未注入时派生(`com.aeroric.native.dev`,预览期 id,见 Q97)。
4. **回归**:当日矩阵全绿(§2);Rust/宿主/前端零面(无 Rust/TS 源变更——HostSupervisor 的 spawnattr/LiveChildSlot 属 Swift 客户端面);旧 Tauri 客户端零接触;`contracts` 12 份逐字节、`inventory` consistent、`doctor` 18/18。
5. **回滚**:十一笔提交各自独立 revert;`WorkspaceStore`→`ProjectStore`/删 `BootstrapView` 的逆向＝revert 对应提交后 `native:generate`;`AERORIC_TEST_OPEN_PROJECT`/`AERORIC_SERVE_STDERR_LOG` unset 即零行为;窗口恢复策略回退＝删 `applicationWillFinishLaunching` 的场景存储清除段;无数据迁移、无生产写入、TCC 零污染。

**frontier 三条(本轮用户裁定一条在场,两条按推荐项记账、异议可重开)**:

- ❓ **Q96** - **UI 6/6 验收的处置**:三层根修(死锁/信号掩码/测试基建)全部落地并有实证,但 testmanagerd 场景装配在长会话内随机缺失(开机内预算约 15-25 次启动),6/6 需要一次干净开机后的单轮复测。➡️ **用户裁定(2026-10-02,在场明示)**:「先标记一下跳过这个部分吧,继续干计划中剩余的内容」——UI 验收记**用户裁定跳过**,测试资产全保留(`pnpm native:ui-test` 随时重开;最佳轮 5/6,各用例逻辑分别在 r6/r9/r10 单独通过),残余风险 owner＝用户,重开路径＝P10-02 原生 QA 或任意会话重跑。**标注:用户已答,按裁定记账。**
- ❓ **Q97** - **bundle id 定型(Q93 兑现检查)**:P4-01 的推荐是「P4-04 真壳落地时定型最终 id 并一次性迁移」。本轮实际:壳已落地但 id 维持 `com.aeroric.native.dev`——迁移涉 TCC/存储域一次性切换,在 UI 验证未 6/6、无签名链的现状下迁移只会增加不可回退面。➡️ 推荐:**延后至 P8-05(生产数据迁移演练)一并做**,届时预览 id 使命结束;本轮记为 Q93 的显式改期。**标注:用户未答,按推荐项记账。**
- ❓ **Q98** - **侧栏键盘选择的实现形态**:实测 macOS List 既不经 AX 暴露选中态、也不自行响应箭头;落地为窗口级隐藏按钮 `keyboardShortcut(↑/↓)`(CommandPalette 逃逸按钮先例),系统 List 仍负责渲染。风险:P5-01 终端将占 detail 焦点,↑/↓ 窗口级等价物会与之冲突。➡️ 推荐:**P5-01 落地终端时**把这对按钮改为「sidebar 为 key window 时才生效」(以 focusedValue 或 key window 判定),本轮先不做;`detail.selection` 头部保留为 P5-03 项目页的天然挂点。**标注:用户未答,按推荐项记账。**

**裁定(§4.2 四条判据对照)**:实际调用证据存在(本文件＋G31 三件套当场记);必选测试通过(§2:`native:check shell` 3/24、`swift test` AeroricUIUnitTests 24/24、`native:build` 六步链 rc 0、skeleton 24/188、doctor 18/18、contracts 12 份逐字节、inventory consistent、digest verified;**UI 6/6 一项为用户裁定跳过**,非测试红);无未解决的崩溃/数据丢失/安全越界/功能阻断(四处真发现全部根修;零窗为环境层、根因已修、测试资产在库);中低风险明确由谁接受(Q96 用户自担并具名、Q97/Q98 按推荐记账可重开)。**⇒ P4-04 标 accepted(含 Q96 跳过注记),Phase 4 Gate 随本轮申报。**

## 2. 验收对照(2026-10-02 当日实测,退出码逐条单读)

| 命令 | 结果 |
|---|---|
| `pnpm native:check shell` | **3 case/24 用例 rc 0**(shell-unit 24＋shell-no-web-navigation＋shell-no-hand-drawn-selection;注释字面量触词形禁形一事即修即绿) |
| `swift test --package-path macos/AeroricKit --filter AeroricUIUnitTests` | **24/24 rc 0**(ThemePolicy 10＋PackageSmoke 4＋ShellPolicy 10) |
| `swift test --filter AeroricClientTests` | **28/28 rc 0**(spawnattr/LiveChildSlot 改动回归) |
| `pnpm native:generate` | rc 0(XcodeGen 2.46.0) |
| `pnpm native:build` | rc 0——六步链(含 cargo `--locked`、xcodebuild、三 helper 内→外签名、strict 验签) |
| `pnpm native:ui-test` | **用户裁定跳过 6/6**;资产在库且分层实证:最佳轮 **5/6**(r9:AppLaunch/Settings/ShellIdentifiers/ShowMain/Sidebar 键盘选择)、TwoMain 点击流通过(r6);钩子 RPC 全链路(`test-hook.log`:`opened alpha-project`+`opened beta-project` 2 秒内)、`projects.json` 持久化、`projects.json` 落盘;零窗为 testmanagerd 会话层(G41 第三层,随开机内累计启动加剧,重启复位),两层根修(死锁/信号掩码)slot-trace 与对照实验实证 |
| `pnpm native:check skeleton` | **24 case/188 用例 rc 0** |
| `pnpm native:doctor` | **18/18 rc 0** |
| `pnpm native:contracts --check` | **12 份逐字节** rc 0 |
| `node scripts/native/inventory.mjs --check` | consistent(660 mapped;51 task) |
| `node scripts/native/digest.mjs --check` | verified 14 shards |
| `git diff --check` | 输出 0 字节 |
| `cargo clippy`/`cargo fmt --check`/宿主 `--lib`/全量 `pnpm test` | 未跑(Rust/TS/宿主零面;HostSupervisor 改动属 Swift 客户端面,已由 AeroricClientTests 28/28 覆盖) |
| 真实家对账 | 结构性保证:app 零直接存储访问＋scratch 逐 run 全新＋bundle id 隔离;测试钩子双 env 门控 |
| GUI 手测(OS:macOS 27.0,arm64) | launchservices `open --env` 探针多轮:窗口装配、侧栏钩子行、ready 连接、设置窗口单例复用、菜单六项在位;XCUITest 全套归 `pnpm native:ui-test`(Q96 跳过注记) |
