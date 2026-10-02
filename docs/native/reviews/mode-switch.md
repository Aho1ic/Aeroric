# tauri → native 迁移评审记录:设置中的原生模式按钮与无损切换(Task P5-02)

> **数字口径**:本文件里的 case 数／用例数一律是**验收当天的实测**,之后模块被别的 Task 扩过时以现场 `node scripts/native/check.mjs <module>` 为准(口径原文见 `reviews/agents.md` 顶部横幅)。

## §4.2 七字段

- **tool 来源/revision**:`mattpocock/skills` @ `c55ee46073ed923f86ce59a5eb3b6d895095d1b7`;本会话 `skill://grilling` **加载成功**,委托链成立。
- **实际调用入口**:① `skill://grilling` 读取＋五轴自查(§1);② 当日验收矩阵(§2 表);③ Q96 用户裁定(跳过 XCTest UI 面)延续适用。
- **审查 commit/diff hash**(G31,**当场记**):`git rev-parse HEAD`＝`7ce70e3`(2026-10-02);`git status --porcelain`＝**1 条目**(`?? .zcodeignore`);`git diff | git hash-object --stdin`＝空 blob `e69de29bb2d1d6434b8b29ae775ad8c2e48c5391`。P5-02 四笔提交:`cc89916`(刀一 Swift 四文件＋宿主 e2e)→`34cc363`(开工标注)→`7ce70e3`(mode-switch 门禁＋React N/A)→本评审。
- **问答**:§1 frontier(Q101–Q102 按推荐项记账);无产品决策悬置。
- **证据**:§1 五轴读数;§2 矩阵;宿主 e2e `hundred_mode_switches_leave_the_task_protocol_untouched`(2.1s,spawn 标记=1)。
- **问题级别**:**两项真发现,当场修**——① **settings_mode.rs 改道**:interactionMode 是客户端 UI 偏好(§2.4「不改变底层 Agent」),进宿主设置=层级违反;持久化落 UserDefaults bundle 域,Rust 测试改道为宿主 e2e(100 次切换 spawn=1)——偏离记录在计划(补记三百一十二);② **React 侧判 N/A**:旧客户端本就是 web 呈现、无 interactionMode 概念,计划条件句「若暴露对应设置」不成立,`interaction-mode.test.tsx` 与 GeneralPanel 修改记录为不适用。**一处延续**:App 场景接线(终端/原生面板的实际挂载)随 P5-03 任务 UI 落地,Q100 记账。
- **修复后复验**:§2 表全绿当日实测。

## 1. 收尾拷问轮(2026-10-02,五轴)

**五轴自查结论(逐条当场可复验)**:

1. **唯一 owner**:模式枚举与迁移默认单源＝`InteractionMode`(migrationDefault:旧客户端存在→advanced);持久化单源＝`InteractionModeController`(UserDefaults bundle 域 `aeroric.interaction-mode`);呈现决策单源＝`SessionPresentation.resolve`(mode+能力→native/terminal/requiresAdvanced);切换顺序单源＝`ModeSwitchSequence`(六步,forbiddenMethods 禁 start/resume/cancel/reset);设置 UI 单源＝`InteractionSettingsView`(挂 `SettingsRootView` application 分区,P4-04 占位转真)。
2. **反例**:`ModeSwitchTests` 9 例——迁移默认两向、跨实例持久化、**保存失败回滚**(failingSave 注入:mode 保持旧值＋原因上屏)、Toggle 双向映射、呈现三态、六步顺序、同模式 no-op、禁用法逐个命中;宿主 e2e:100 次切换后 **spawn 标记恰 1 行**、generation/sessionId 不变、原始输出在场。
3. **边界**:模式是客户端偏好(不进宿主设置——host 是 agent 无关的);unsupported 会话显示状态卡＋**用户触发**的切换按钮(不自动改全局偏好);切换序列无 start/resume/cancel/reset;测试域 defaults 用独立 suite 隔离。
4. **回归**:mode-switch 1 case/9 用例、skeleton 24/188、cli 5/10(含 100 次 e2e)、host 6/44、native-terminal 3/12、contracts 逐字节、inventory consistent、digest verified、doctor 18/18;React/TS 零面(N/A 记录)。
5. **回滚**:四笔提交各自独立 revert;UserDefaults 键回退＝removePersistentDomain;SessionPresentation 纯决策无持久化;宿主 e2e 可独立重跑。

**frontier 两条(按推荐项记账)**:

- ❓ **Q101** - **迁移默认的判据**:「旧 Tauri 客户端 Application Support 目录存在」是代理判据——目录存在但从未用过的用户会被判 advanced。➡️ 推荐:**接受**该粗糙度(误判的代价只是默认进高级模式＝旧行为,方向安全);P8-05 生产迁移时可改为读旧客户端的会话计数。**标注:用户未答,按推荐项记账。**
- ❓ **Q102** - **native 模式下 P5-04 之前的会话呈现**:显示任务元数据＋「未接通」状态卡,不伪造 composer。➡️ 推荐:**P5-03 落地任务列表时**挂 `SessionPresentation`,本 Task 的决策函数已就位。**标注:用户未答,按推荐项记账。**

**裁定(§4.2 四条判据对照)**:调用证据存在;必选测试通过(mode-switch 1/9、skeleton 24/188、cli 5/10 含 100 次 e2e;**XCTest UI 按 Q96 跳过**);无未解决问题(两项真发现根修/记录;DSH 真 composer 集成验收按计划移 P5-04,不以占位冒充);中低风险由谁接受明确(Q101/Q102 记账)。**⇒ P5-02 标 accepted(含 App 场景接线随 P5-03 注记)。**

## 2. 验收对照(2026-10-02 当日实测,退出码逐条单读)

| 命令 | 结果 |
|---|---|
| `pnpm native:check mode-switch` | **1 case/9 用例 rc 0** |
| `swift test --filter ModeSwitchTests` | **9/9 rc 0** |
| `swift test --filter AeroricUIUnitTests` | **45/45 rc 0**(含 ModeSwitch 9＋TerminalCoordinator 12) |
| `cargo test -p aeroric-cli --test cli_headless hundred_mode_switches` | **rc 0(2.1s)**——spawn 标记 1 行/generation/sessionId 不变/输出在场 |
| `pnpm native:check cli` | **5 case/10 用例 rc 0**(cli-headless 6/6) |
| `pnpm native:check skeleton` | **24 case/188 用例 rc 0** |
| `node scripts/native/inventory.mjs --check` | consistent(51 task/22 accepted) |
| `node scripts/native/digest.mjs --check` | verified 15 shards |
| `pnpm native:doctor` | **18/18 rc 0** |
| `pnpm native:ui-test`/手测 | Q96 用户裁定跳过(资产在库) |
| `src/test/interaction-mode.test.tsx` | **N/A 记录**:旧客户端无 interactionMode 概念(计划条件句不成立) |
| core `tests/settings_mode.rs` | **改道记录**:interactionMode 是客户端 UI 偏好(§2.4),持久化在 UserDefaults bundle 域,不进宿主设置;宿主侧验收＝100 次 e2e(spawn=1);偏离记录于补记三百一十二 |
