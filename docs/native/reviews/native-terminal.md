# tauri → native 迁移评审记录:用原生终端 renderer 承接同一 PTY(Task P5-01)

> **数字口径**:本文件里的 case 数／用例数一律是**验收当天的实测**,之后模块被别的 Task 扩过时以现场 `node scripts/native/check.mjs <module>` 与 `checks.json` 的 `minCases` 为准(口径原文见 `reviews/agents.md` 顶部横幅)。

## §4.2 七字段

- **tool 来源/revision**:`mattpocock/skills` @ `c55ee46073ed923f86ce59a5eb3b6d895095d1b7`——`grill-me/SKILL.md` 转交 `grilling`(委托链见 `docs/native/tooling.md` §2/§3)。本会话 `skill://grilling` **加载成功**,委托链成立。
- **实际调用入口**:① `skill://grilling` 读取＋五轴自查(§1)＋frontier(Q99–Q100,按 P3-02 先例按推荐项记账、异议可重开);② 当日验收矩阵(§2 表,退出码逐条单读);③ 用户裁定(Q96)对 UI 验收面的延续适用。
- **审查 commit/diff hash**(G31 规矩,**当场记**):`git rev-parse HEAD`＝`741a38d7928831ce4e4cc3256db44cc1c25e00b3`(741a38d,2026-10-02);`git status --porcelain`＝**1 条目**(`?? .zcodeignore`——用户未跟踪文件、非本任务);`git diff | git hash-object --stdin`＝`e69de29bb2d1d6434b8b29ae775ad8c2e48c5391`(**空 blob＝diff 为空**)。P5-01 五笔提交:`31685d8`(ADR-038)→`e73ae92`(修正案)→`5f59d35`(刀一宿主三方法)→`0a04e71`(刀二 SwiftTerm＋四文件)→`0460f0c`(刀三客户端＋smoke);评审文件随 `docs(plan)` 终提交。
- **问答**:§1 frontier 两条(Q99/Q100 按推荐项记账);Q96 用户裁定(跳过 UI 验证)对本期 XCTest UI 部分延续适用。
- **证据**:§1 五轴读数;§2 矩阵;`smoke-headless.mjs` H 段实录(plan_replay 28B seamless/resumeAfter 续读/租约往返/过期票据 no-op——ADR-038 全决策真机走通);`test-hook.log` 无关本任务;过程账在 `MIGRATION_PLAN.md` Task P5-01 小节补记三百一十。
- **问题级别**:**四处真发现,当场根修**——① **契约机制修正案**:决策 4 原拟把三方法记进 methods.schema.json,动刀前实测 532 条自 P2-01 冻结且唯一来源是 Tauri 命令清单,P3-07 的宿主方法本就不在 schema(注册在 dispatcher＋rpc 可达即 §3.1 #8 兑付)→修正案改走 dispatcher＋capability 声明,不谎报不重写交叉断言;② **UnknownStream 双义拆分**(Q2 兑付):宿主分类 notStarted/evicted/unknownStream,判定材料(任务注册表/任务记录)在宿主手里,core 不改;③ **`task.output` 与 plan_replay 的关系澄清**:前者(P3-07 ④,CLI attach 用)是同字节环的粗粒度尾巴,后者是 renderer 的全语义读法——同环两读法非双真相,收敛留 P5-01 后续;④ **三处工程坑当场修**:generation 是 Quantity 十进制串(smoke 首跑逮住)、CLI 二进制缓存旧版致 UnsupportedCapability(smoke 的 ensureCliBinary 只在缺失时构建)、`missingOutputTokens` 大小写敏感(mustContain 必须小写)。**一项用户裁定延续**:XCTest UI 验收(`TerminalInteractionTests.swift`)与双 renderer 对比/手测按 Q96 跳过,测试资产与门禁在库。
- **修复后复验**:§2 表全绿当日实测;`native-terminal` 3 case/12 用例、`cli` 5/9(含 smoke H 段)、`host` 6/44、`skeleton` 24/188。

## 1. 收尾拷问轮(2026-10-02,五轴)

**轮次输入**:ADR-038(含修正案)＋宿主三方法＋SwiftTerm 1.2.5 钉版＋Terminal 五文件(Spike/Store/InputPolicy/Coordinator/NativeTerminalView)＋客户端 TerminalStreamClient＋smoke e2e;九笔提交。

**五轴自查结论(事实,非问题——逐条当场可复验)**:

1. **唯一 owner**:renderer 单源＝SwiftTerm 1.2.5 `TerminalView`(`NativeTerminalView` 只包它,**永不 spawn 进程**——`native-terminal-no-local-process` absent 门禁);cursor 单源＝`TerminalStore`(唯一续看凭证＝resumeAfter 字节;reset 后也推进到 resumeAfter＝与 core `resume_after()` 同算法;applyTail 不计 attach 计数);策略单源＝`TerminalInputPolicy`(租约/可见/IME 闸门、OSC52 永不自动、scheme 白名单);租约单源＝宿主 `ControlLease`(票据单调永不复用;`LeaseHandle::ticket()/from_ticket()` 是协议面的唯一出入口);字节环单源＝core `ByteStream`(task.output 与 plan_replay 是同环两读法)。
2. **反例**:`native-terminal-no-local-process`(禁形＋2 正控);`native-terminal-swifterm-pinned`(forbid 空＋mustContain 小写钉 1.2.5——runner 先降 haystack 再大小写敏感包含);`shell-unit`/`native-terminal-unit` floor 双写(36/12);`TerminalCoordinatorTests` 12 例(无租约拒绝输入、无租约拒绝 resize、IME 组合拒绝、100 次 attach/detach 记账守恒、OSC52 拒绝、链接白名单拒绝 file/javascript);`TerminalStreamClientTests` 5 例(缺失字段/base64 坏形显式报错);宿主 5 例(notStarted/evicted/unknownStream 分类、cursorBeyondStream、租约往返);smoke 断言(seamless 字节非空、resumeAfter 可用、过期票据 released=false)。
3. **边界**:coordinator **不 spawn 进程、不持定时器**(轮询由持有方驱动);测试钩子双 env 门控;`AERORIC_SERVE_STDERR_LOG` 仅诊断;capability `terminal.replay` 只在装配了重放面的宿主上声明(`v==1` 从不蕴含);`unitsFromStart` 游标诚实降级(core 旧规则);OSC 52/URL 永不自动。
4. **回归**:当日矩阵全绿(§2);host-lib 地板 20→25、shell-unit 24→36、native-terminal-unit 12 双写;`task-status.test` F04 期望随开工更新(native_state→in_progress);contracts 12 份逐字节(schema 未动——修正案);inventory consistent;doctor 18/18。
5. **回滚**:九笔提交各自独立 revert;Package.resolved 回退＝删依赖后 `swift package resolve`;宿主三方法未注册时客户端得到服务端 `UNSUPPORTED_CAPABILITY`(结构性行为,不是错误码约定);无数据迁移、无生产写入、scratch 隔离不变。

**frontier 两条(按推荐项记账、异议可重开)**:

- ❓ **Q99** - **task.output 与 plan_replay 的收敛**:同字节环两读法(CLI attach 粗尾巴 vs renderer 全语义)。➡️ 推荐:**两者并存**至 P5-03 消费侧接线完成,此后评估把 CLI attach 迁到 plan_replay(迁移前 task.output 不动——attach 的 16ms 轮询已实测稳定)。**标注:用户未答,按推荐项记账。**
- ❓ **Q100** - **UI 挂载时机**:coordinator/客户端已就绪,但终端场景需要任务列表(P5-03)才有 attach 目标。➡️ 推荐:**P5-01 按本评审收口**(渲染链路＋协议面全通;UI 挂载随 P5-03 场景落地,XCTest UI 面按 Q96 延续跳过)。**标注:用户未答,按推荐项记账。**

**裁定(§4.2 四条判据对照)**:实际调用证据存在(本文件＋G31 三件套);必选测试通过(§2:native-terminal 3/12、cli 5/9、host 6/44、skeleton 24/188、smoke e2e 真链路、contracts 逐字节;**XCTest UI/双 renderer 对比/手测按 Q96 用户裁定跳过**);无未解决的崩溃/数据丢失/安全越界/功能阻断(四处真发现根修;OSC52/URL 默认拒绝是安全正向;不做纯 TextEditor 降级——SwiftTerm 是真终端引擎);中低风险明确由谁接受(Q99/Q100 按推荐记账;Q96 残余风险 owner＝用户)。**⇒ P5-01 标 accepted(含 Q96/Q100 注记)。**

## 2. 验收对照(2026-10-02 当日实测,退出码逐条单读)

| 命令 | 结果 |
|---|---|
| `pnpm native:check native-terminal` | **3 case/12 用例 rc 0**(unit 12＋no-local-process absent＋swifterm-pinned forbid) |
| `swift test --filter TerminalCoordinatorTests` | **12/12 rc 0** |
| `swift test --filter TerminalStreamClientTests` | **5/5 rc 0** |
| `swift test --filter AeroricUIUnitTests` | **36/36 rc 0**(ThemePolicy 10＋Package 4＋Shell 10＋TerminalCoordinator 12) |
| `swift test --filter AeroricClientTests` | **33/33 rc 0**(含 TerminalStreamClient 5) |
| `node scripts/native/smoke-headless.mjs` | **rc 0 全绿**——H 段新增:plan_replay **28B seamless**/resumeAfter 续读(空尾合法)/acquire→ticket→release true→stale release false |
| `pnpm native:check cli` | **5 case/9 用例 rc 0** |
| `pnpm native:check host` | **6 case/44 用例 rc 0**(host-lib 25/25:含 terminal_replay_tests 5 例) |
| `pnpm native:check skeleton` | **24 case/188 用例 rc 0** |
| `pnpm native:contracts --check` | **12 份逐字节** rc 0(schema 未动——修正案) |
| `node scripts/native/inventory.mjs --check` | consistent(51 task/21 accepted) |
| `node scripts/native/digest.mjs --check` | verified 15 shards(gaps 9 片) |
| `pnpm native:doctor` | **18/18 rc 0** |
| `cargo clippy -p aeroric-host --all-targets -- -D warnings`/`cargo fmt --check` | **0 警告/0 diff** |
| `pnpm native:ui-test` | Q96 用户裁定跳过(资产在库) |
| 手测 OMP/vim/less/交互选择器 | Q96 用户裁定跳过(P10-02 QA 重开路径) |
