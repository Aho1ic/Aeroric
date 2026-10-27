# 使用统计审阅样例 01 — 外部执行模型交接

版本：1.0 · 2026-10-20 · 范围批准：Q103（用户回复“认可”）。
依据：Q94–Q102，见 `usage-statistics-design-checkpoint-01.md` v0.1。

**只交接独立SwiftUI＋苹果原生Liquid Glass离线样例。
不创建VM，App/Preview/真实窗口/UI运行继续暂停。本文不是立即开工许可。**
用户交新终端并明确要求执行后，仍须独立任务授权、规划审阅/start、
dirty保全及文件/构建资源单写交接。本线程只设计、交文档和监工，
不实施页面，不把文档审查当运行/用户验收。

## 1. 政策、数据与执行门槛

1. **禁止创建、克隆、下载、导入、恢复、启动、重建或准备替代VM，
   禁止安装/重装VM工具。** 不修复退休VM路径、不继承旧任务运行许可，
   保留历史证据和退休回执；VM不是交付条件。
2. App/Preview/真实窗口及UI自动化PAUSED。不启动/预热App，不运行
   XCUITest、`pnpm native:ui-test`、`pnpm native:check accessibility`、
   GUI snapshot或等价窗口/输入runner，也不调用runner验证其拒绝分支。
   不借隐藏窗口、其他Space/显示器、远程桌面/CI或真实账号绕过。
3. 只考虑逐项审查过的无窗口/无输入静态、fixture、单元和compile-only
   检查，使用虚构数据/隔离临时根；suite名、filter、fake HOME或无签名
   参数不证明发现、链接、初始化和脚本均无副作用。
4. 不接真实业务、Agent/API/MCP/PTY、监督/host/网络服务，
   不读写真实统计/会话/项目/账户/个人配置/managed-home/凭据；
   不下载安装技能/工具或获取现行价格/汇率。
5. 真实材质/圆角/浅深/辅助偏好输出、布局/交互、菜单/sheet、
   焦点/键盘/IME/AX/VoiceOver、生命周期和用户旅程均为
   **NOT RUN / DEFERRED**。静态、编译、离屏组装或旧accepted不替代它们，
   不删断言/降floor或以零匹配、政策拒绝、旧证据补绿。

政策先读 `AGENTS.md`、`.trellis/workflow.md`、
`docs/native/ui-testing-policy.md`；平台/分发遵循 `MIGRATION_PLAN.md`
顶部Apple-only基线及 `docs/native/adr/039-distribution-pivot-macos-ios.md`。
整体仍为SwiftUI macOS＋iOS/iPadOS、双App Store；本阶段不新增iOS业务、
不改deployment/签名/CI/商店配置。旧spec的14.0/旧材质下限服从新基线；
不为补齐缺失spec文件而扩围。

## 2. 目标、必读源与非目标

目标是看懂“花了什么、属于哪里、依据/缺口、受阻原因及动作影响”。
现有macOS App target内独立DEBUG离线入口，以实际View消费虚构内存、
动作及前后状态，不做标题页、静态全绿仪表盘或仅Preview。
本文无launch命令，页面待新授权后运行。

最小阅读：

- 本文及 `docs/native/usage-statistics-design-checkpoint-01.md`。
- `.trellis/tasks/10-14-native-redesign-shell/research/harness-usage-statistics.md`
  §5/7/9/11/13/15，续页 `harness-usage-statistics-02.md` §4/7/10/13。
- 同目录 `harness-execution-readiness.md` §2–4：Q65–Q67；
  资源规则不在本阶段重新批准。
- `.trellis/spec/aeroric-macos/frontend/index.md`、ADR-001仍有效约束，
  按需读Design/入口/构建链。
- 候选总库仅回指 `docs/native/harness-feature-candidates.md` 的L08；
  P05/M06沿Q65已有原则，不重扫harness、不新增观察ID。

**明确不做：**

- 不替换正式首页/主导航、Q11默认all-tasks、`UsageDashboard`、
  现有Maintenance/订阅业务或其他冻结样例/阶段包。
- 不实现生产采集、账本、归因/去重、价格/换汇/账单核对、补报协议、
  留存/同步/删除、权限强制、预算准入/收束/恢复或跨Host计时内核。
- 不直接读真实统计源、不解析原始JSONL/ANSI猜用量/权限，
  不拼Agent argv、不新建生产DTO或Swift共享业务writer。
- 不真实导出/外发/剪贴板/文件选择/删除/持久化，
  不调真实额度、继续、重试、切模型/API或授权接口。
- 不用真实时钟、Timer、后台轮询、睡眠/随机延迟推动场景；
  所有日期、报告事件、价格/汇率、阈值与模型/API标签是示例，不是产品默认值。
- 不顺手修旧契约缺键回落0、旧测试/构建失败或其他终端实现。

## 3. 信息组织、内存owner与动作语义

围绕简洁工作面按需展开范围、指标、依据和受阻原因，不冻结正式布局。
US1–US6用原生场景选择定位；任务/项目/全局是同批fixture的不同视图，
角色/执行器/API/模型是独立切面，汇总说明覆盖。
普通任务可无监督，示例不强制三角色。

新`NativeUsageStatisticsReviewStore`是独立`@MainActor @Observable`内存owner，
只持有样例值/选择/草稿/快照/投影，依赖显式fixture/事件；
无AppServices、生产Store/client/transport/supervisor、单例、偏好、
文件/网络/Process/会话解析器。入口也不查home/目录（§6）。

- **查看类动作**：选择范围/维度/期间/示例时区、展开来源/限制、查看摘要、
  创建内存导出预览；不新增调用/使用，不给角色扩大上下文或改变执行状态。
- **草稿类动作**：自定义价、调额、清理范围/导出字段先预览；
  取消不改变已应用投影。确认只更新明确命名的样例状态，
  不把确认意图当成外部事实。
- **模拟事实事件**：可靠补报、修订、刷新失败、停止核实等由专用、
  清楚标为“模拟”的事件驱动；不由普通打开/刷新/继续按钮编造成功。
  有些事件可以改变已报告/收束等fixture事实，但不能声称发生真实调用。
- **重置与实例**：重置回到固定初始状态，同时清除草稿、修订和内存预览；
  两实例相互独立，无跨实例/重开持久化。重复查看/同笔重复事件不新增使用。

建议将同一状态的显示文字/数值/来源/限制/操作可用性做成可测试投影，
View实际消费这些投影与动作。不能只测试无人使用的helper或静态文案存在。
用量缺失、0、未知、估算、已报告、冲突/待核对不是同一个标记；
金额/时间/范围分别保留口径，不用绿色大总数掩盖局部未知。

## 4. US1–US6有限场景与可核对结果

以下仅为样例关系，非生产算法/新来源观察。
可用有限子场景，保留负向/未知/失败路径；
Store事件、View消费与断言须对应，不因简化删原则。

### US1 层级与归属（Q94/Q96）

固定数据包含：主执行一次有消耗的失败及一次真实重试，
一项适用反思调用、两个任务关联的一笔无法拆分的共享治理使用，
无项目任务以及缺少某归属维度的记录。角色可有不同API/模型；
保留Codex/Claude Code/OMP/DSH及custom身份选择/标签，
不要求每一家都伪造相同计量支持。

- 切任务→项目→全局、角色→执行器→API→模型与明细展开，
  明确哪些是同批使用的切面，哪些是已知覆盖/未归属/未分摊；
  父项含子项不再相加，跨维度不重复求和。
- 显式模拟同笔重复到达，断言数值不增加；模拟已发生的独立重试事件，
  断言仅那次真实fixture使用增加。失败/停止前已知使用不因结果差而丢弃，
  被拒/未启动不造调用消费。
- 两任务的共享使用只在适用范围显示一次共享/未分摊关联，
  不给每任务复制全额/平均分。示例更换当前模型配置不重标旧归属，
  缺维度不从当前选择猜补。统计事件也不启动真实重试或改变验收。

### US2 来源与费用（Q95/Q98）

固定数据同时有Token已报告、费用估算/自定义价、缺价或缺分项、
冲突待核对、两种原币、带固定虚构汇率的部分折算参考、独立订阅信息。
每个fixture价格/汇率带“示例”及适用依据/时点，不宣称供应商现价。

- 展开逐指标依据，区分来源报告、估算、未知、已知部分；
  未知不补0，不让Token已报告把费用自动标成已报告/已核对。
  同范围来源冲突保留待核对，不盲加或以最后到达覆盖。
- 预览/取消/应用一项自定义价格，显示自定义/估算及作用范围；
  缺价/必要输入仍未知，不套相似模型价格。
  应用不能暗改旧快照/原报告，也不是供应商扣款或预算余额更新。
- 切原币与折算参考，保留原金额、汇率依据及缺口，
  不直接多币相加、不因切币种改变资源控制事实。
  订阅额度/包月与按量参考分区，缺扣款/可能在套餐内不当实际费用0。
  输入/输出/缓存已包含的分项不再次求和。

### US3 更新与时间（Q97/Q101）

以固定日期/区间创建初始内存快照；含可靠发生时间的迟到报告、
只能给跨期累计的记录、观察缺口与两个并行10分钟的执行者。
示例日期只供fixture，不使用`Date.now`/系统时区自动推进数据；
可在样例内切两个显式给定统计时区，产品初始本地时区仍是未来原则。

- 模拟可靠补报/有据上调或下调，关联原使用并说明发生/收到时间与修订；
  同笔不新增一次消费，后到不自动已核对。
  对费用只补Token时，仍按价估算，不整体升级成真实账单。
  初始快照保持原值/截至时间，显式创建新预览才带新口径。
- 模拟刷新失败，旧值/上次成功及本次失败可辨，不清零或假更新时间；
  无旧值子场景保持尚未取得。普通刷新不执行任务、工具、付费工作或恢复Stop。
- 切固定自然日/滚动区间与示例时区，可靠发生依据归期，
  收到日不挤账；跨期缺拆分依据保留未分配，只相交不取全额/猜比例。
  显示并行角色累计20分钟与实际经过10分钟不同，
  不用创建时间/进程存活/PTY静默补等待/模型时间；
  缺起止/跨Host不可比较时标观察缺口，不强制归零或拼精确效率。
  时区/筛选不改变原始依据、旧快照、预算/订阅周期或执行事实。

### US4 可见范围与外发（Q99）

分别呈现用户获准全局视图、当前主执行/反思/治理的任务必要摘要，
及一个明确预置的跨项目统计授权范围。角色层级与范围是两件事；
全部内容都是虚构，不构造实际鉴权/模型上下文或让API消费。

- 用户打开全局不会改变任何角色摘要。切治理角色也不自动得全库；
  共享额度受阻解释不泄露其他任务/项目名称或内容。
  不可见范围不补0、也不借筛选/差额伪造完整全局账目。
- 显式切换“已有授权覆盖”的fixture并重新查看必要摘要，
  覆盖内不重复模拟索权；任务Full Access不自动成为跨项目/外发授权。
  越界只显示范围缺口/请求意图，不出现“已真实授权”。
- 选择最小必要字段，预览内存导出，保留指标/币种/来源/覆盖/修订口径，
  不带秘密/完整账户标识/个人绝对路径/对话/代码/终端全文。
  本地查看、文件导出意图、发送外部模型意图区分；
  本样例只生成预览，后两者明确未执行，无文件/剪贴板/网络。
  关闭/取消预览不外发，重置清除预览；脱敏不称绝对匿名或强制隔离。

### US5 留存与清理（Q100）

固定同一范围的明细、汇总、覆盖信息，以及只读标记的成果/监督验收依据、
预算/重试/用户Stop投影。清理先给任务/项目、期间、层级及影响预览。
无真实清理接口或默认期限，不通过fixture设置选定生产留存天数。

- 取消清理，原投影不变；确认示例明细清理后，显示“明细已清理，
  汇总仍保留”，来源/估算/覆盖限制保留，不再声称可逐次追溯。
- 显式选择移除该范围示例汇总，趋势显示缺失/断点而不是0；
  普通刷新不重建已清理旧明细。若展示新报告，须明示它是新模拟事件，
  不能以“刷新”偷偷复活旧明细。
- 成果/任务/原始会话引用/监督结论/验收依据及预算/重试/Stop投影不变。
  必要记录不能清理的子场景说明范围/原因，不假称全部删除，
  不把缺记录作为硬限制放行依据。此处只验证内存不连带，
  不宣称磁盘、来源、备份、同步端或外部副本已删除。

### US6 资源与执行（Q102，沿Q65–Q67）

子场景至少区分：有据预警、项目共享上限命中但任务局部仍有余、
资源计量未知、并发准入满但已有合法在途、对在途生效的消耗/时长到限，
以及用户Stop/尚待反思治理或验证。阈值/额度/周期全为显式示例，
不暗定80%或其他产品默认值。

- 查看限制，说明适用范围/周期/单位/来源；未知不当0或100%可用，
  不同单位/币种/期间不拼精确余额。预警不自动变成拦截，
  共享受阻定位该范围而不把全部独立工作显示已停。
- 并发满只影响新准入，不强停合法在途；
  模拟在途到限后区分请求收束/收束中/已停止或未知，
  “已停止”只来自明确的模拟核实事件，不是按下按钮/等待超时。
  必需硬限制不支持时显示能力受阻，不伪装已花完或已能硬控。
- 预览/取消/应用调额只变样例策略；查看/调额/继续/“补额并继续”意图不同。
  补额不默认恢复已收束执行；不清零独立重试上限，也不满足必需监督/核验。
  原执行未核实收束时，不投影为可重叠替代写入。
- 已明确配置资源恢复自动续行且仍有有效交办的fixture，
  显式模拟重核目标/输入/依赖、原执行/副作用、授权及全部资源后，
  才显示仍需完成工作的样例准入结果，覆盖内不再索同一许可。
  失败条件子场景保持待处理；用户Stop/撤权/显式禁令仍阻止，
  不重放已完成或结果未知操作。普通资源等待覆盖内重核可继续，
  不倒退成每次手动批准；这些不是实际调度/恢复能力证据。

## 5. 原生呈现要求

复用现有Design组件和NativeSelection；相邻互斥范围/维度/场景/期间
使用Picker、List(selection:)、Toggle或Menu系统选择态，
不自绘active底色/下划线。独立展开、复选/开关不误当互斥tab。
若现有wrapper不能满足，先提最小扩围，不另造选中态或擅改共享文件。

SwiftUI＋苹果原生Liquid Glass；语义色为信息、状态、范围提供层次，
配文字/图标，不只靠红绿或整页警报。减少透明度/提高对比度/减少动态效果
有可消费的呈现分支；glass/classic与interactionMode分开，
外观/筛选切换不影响会话、执行器、API/模型、权限、cwd、PTY或任务事实。
短控件/输入合适胶囊圆角，单图标无永久圆环；长内容不强行胶囊压缩。
连续材质与合理间距处理角隙，不贴三角补丁、不手绘系统窗口四角。
只审查静态/纯投影分支，真实玻璃和可访问性输出仍待授权后验收。

现有`GlassPanel`的`.glass`路径是原生Material，名称不等于原生Liquid Glass
已实现或验收。新样例在适用系统的交互层使用系统原生玻璃API，
正文/图表不层层叠玻璃；复用组件不免除调用点核查。
不把旧系统仿玻璃当产品基线，也不为样例修改部署/签名；
availability与classic/辅助偏好分支如实区分，真实输出仍DEFERRED。

## 6. 当前接入快照与新入口契约

以下是2026-10-20只读当前dirty源码所得，**不是实现或运行证据**。
新终端开工时重核，不能把行号、键集合、顺序或旧包组合数当永恒事实。

### 6.1 当前源码事实

- `macos/AeroricApp/Sources/AeroricApp.swift:11–79` 当前DEBUG链：
  Deliverables → Communication → WorkArrangements → Skill → Knowledge →
  Evolution → Visual → 产品App。新模式尚不存在。
- `AeroricNativeApp.init`在`:215–228`已构造`AppServices`并关联产品delegate；
  不是等`.task`才有副作用。`AppServices.swift:32–60`构造生产Store，
  `InteractionModeController.swift:48–65`含偏好及应用支持目录读取。
  产品`AppDelegate.swift:47–51`还可能清除注入测试域。
  `AppServices.swift:121–173,250–257`的启动链涉及目录、bookmarks、
  supervisor及测试清理进程；**必须完全绕开，而非事后阻止host启动**。
- `TaskDeliverablesReview`可参考owner结构，但Entry`:50–84`无效/冲突
  后仍可能校验目录；不复制其gate、默认`NSHomeDirectory()`或目录测试。
- `TaskWorkspaceView.swift:356–364`装配现有`UsageDashboard`；
  `Operations/MaintenanceStore.swift:58–105`含真实client请求，
  只能只读了解，不当样例数据源。
- `macos/AeroricKit/Package.swift:24–33`已有默认目录发现的AeroricUI/
  AeroricUIUnitTests；`macos/project.yml:50–72`已有本地链接和App Sources。
  此有限范围不需要改Package/project或新增target/依赖。

本次App/Kit源码窄查确认的旧审阅请求键为：

```text
AERORIC_NATIVE_TASK_DELIVERABLES_REVIEW
AERORIC_NATIVE_TASK_COMMUNICATION_REVIEW
AERORIC_NATIVE_WORK_ARRANGEMENTS_REVIEW
AERORIC_NATIVE_SKILL_REVIEW
AERORIC_NATIVE_KNOWLEDGE_REVIEW
AERORIC_NATIVE_EVOLUTION_REVIEW
AERORIC_NATIVE_VISUAL_REVIEW
AERORIC_NATIVE_COLLABORATION_REVIEW
```

依据：`TaskDeliverablesReview/NativeTaskDeliverablesReviewEntry.swift:48–70`。
Collaboration仅见冲突引用、无独立App路由，仍须检查；
`AERORIC_ENV`/`AERORIC_DATA_DIR`及Skill launch ID不是审阅模式。
旧MD的未来入口不当现状，键变更先更新本任务计划/测试，不改旧链/冻结包。

### 6.2 新契约：纯值先决策，再拥有审阅或拒绝

拟用新键：`AERORIC_NATIVE_USAGE_STATISTICS_REVIEW`。
当前窄查未见同名键/类型或`UsageStatisticsReview/`目录；
开工须再确认缺席/owner，不覆盖同期新实现。

1. 新`NativeUsageStatisticsReviewEntry.decide(environment:)`只接收显式
   `[String: String]`，无默认home、URL解析/探测、文件/偏好访问或全局初始化。
   新键不存在才`.inactive`；存在则只接受精确值`"1"`。
   空串、`"0"`、空白变体或其他值均`.refused`，不等于“不启用”。
2. 新键存在时，只要任一当时旧审阅键**显式存在**就拒绝，包含旧值为
   `"0"`、空串或非法值；不只检查`== "1"`。
   这是新模式的保守冲突规则，不改旧键独自启动时的行为。
   无效/冲突理由是纯值，不回显任意环境内容/秘密。
3. 在`AeroricEntryPoint.main()`现有第一个DEBUG gate之前新增判定。
   `.review`和`.refused`均由新的独立DEBUG审阅App拥有并立即return，
   不回落旧样例、产品App、delegate、Store或菜单。
   `.inactive`只继续原链，完整保留旧顺序/行为，不顺手修其gate。
4. 仅`.review`能构造新根View/Store；拒绝只用最小只读理由内容，
   不能因初始化默认表达式先创建样例owner。
   新App/lifecycle不连接产品AppServices、AppDelegate、Settings、
   AeroricCommands、openURL、supervisor或真实偏好。
   独立生命周期只拥有样例/拒绝内容及标准关闭/重开管理；
   实际窗口操作全部待新的UI授权，本阶段测试不得触发。
5. 新调用点、App/lifecycle、Entry/Model/fixtures/Store/View及测试引用
   全部受`#if DEBUG`保护，ReleaseLocal不激活也不链接新实现。
   配置名/源码`#if`不独自证明排除，需有效编译设置和产物证据。
6. **不依赖`AERORIC_ENV=test`、`AERORIC_DATA_DIR`或fake-home目录门禁。**
   原因是本样例根本无数据访问能力，不是因为任意目录变得安全。
   新模式下这些环境值不解析、不校验、不创建/读取；测试使用纯字典，
   不传真实home作负例、不复用旧目录helper。
   外层测试进程仍须审查继承环境/缓存并使用隔离临时根，
   这不等于整个进程已有强制沙箱。

静态核对顺序，纯值断言覆盖缺席/有效/非法、逐旧键/多键冲突；
组合从实际集合生成，不复用固定“8键/255组合”。拒绝不构造owner、
查home/目录/偏好或进入旧链；缺席保留原链。
可测注入factory/纯投影，不运行App/refusal窗口。

## 7. 产品/测试文件白名单

最大范围如下，不必全改；不新增target/依赖/生产结构，新路径已存在先核对owner。

| 路径 | 允许职责 |
|---|---|
| `macos/AeroricKit/Sources/AeroricUI/UsageStatisticsReview/NativeUsageStatisticsReviewModel.swift`（新） | 样例值、范围、来源/覆盖、费用/时间/资源投影；非生产DTO |
| 同目录`NativeUsageStatisticsReviewFixtures.swift`（新） | US1–US6固定虚构事实/事件，无时间/随机/文件或服务读取 |
| 同目录`NativeUsageStatisticsReviewStore.swift`（新） | 局部内存owner、草稿/意图/快照/重置，有限投影，不是生产账本 |
| 同目录`NativeUsageStatisticsReviewView.swift`（新） | 实际原生控件/列表/详情/预览，消费状态与动作 |
| 同目录`NativeUsageStatisticsReviewEntry.swift`（新） | 纯环境判定、拒绝理由、专属生命周期，无home/目录helper |
| `macos/AeroricApp/Sources/AeroricApp.swift` | 仅最前DEBUG分流和独立样例App装配，原链/产品逻辑不动 |
| `macos/AeroricKit/Tests/AeroricUIUnitTests/NativeUsageStatisticsReviewTests.swift`（新） | 逐项审查的纯值/内存fixture/入口测试，不创建窗口/View或目录 |

独立任务自己的PRD/design/implement/context、相对快照patch与证据，
按Trellis管理；不改本线程整壳/迁移/其他任务metadata/context。

**只读/不在白名单**：AppServices/产品delegate/commands/navigation、
全部旧Review/测试、UsageDashboard/Maintenance/订阅与其他业务、
共享Design/NativeSelection/ThemeTokens、Package.swift/锁文件、
project.yml/xcconfig、Rust/core/protocol、iOS/legacy、
`macos/AeroricUITests/`、`scripts/native/`、政策/runner/checks/floors、
签名/CI、本文/快照与既有阶段包。
不因组件名含glass就宣称满足原生玻璃，不为小改动改共享组件。
确需额外hunk先回主线程审查最小扩围。
既有generate产生的派生工程仅在资源交接及检查审查后生成，
不手改工程或新增target；构建不授权额外源码修改。

## 8. 新终端开工、验证审查与共享资源

### 8.1 开工顺序

1. 用户明确交本文要求执行；独立取得任务创建授权，形成PRD/design/
   implement与按需context，经审阅才`task.py start`。
   不激活`10-14-native-redesign-shell`或其他旧任务来借范围/运行许可。
   本文形成时无active task，两旧重设计任务仍planning。
2. 在main保存开工status、白名单原文/hash/diff和新路径缺席观察，
   以**当前dirty树而非HEAD**为基线，放入独立
   `.artifacts/native/usage-statistics-review/<run-id>/`证据目录。
   不stash/reset/clean、全文件覆盖、清理他人产物或认领他人实现。
3. 取得App入口明确单写交接，每次编辑前复核hash/diff；
   并行增量先协调，不能用旧包/旧测试证明新变更。冻结本文/快照。
4. 逐项重核§6当前入口/键/构造依赖，检查本地工具与锁定依赖、
   继承环境/脚本/hooks/测试初始化，无额外业务访问或下载安装。
   条件不明先记受阻，不用真实账号/home补缺或自动找替代密钥。
5. 编译前明确SPM `.build`/scratch/缓存、生成工程、DerivedData、
   core target、bundle/helper/签名及日志的单写owner。
   独立scratch不隔离全部缓存，原生build仍可能写固定共享路径；
   未获交接不运行，争用报告，不强杀持有者/删锁或换未知资源绕过。
6. 先纯值/fixture/入口与定向断言，再内存Store/View和最小App装配。
   逐项审查检查路径后才运行对应无窗口验证。
   不构造产品services，不启动App/窗口或新UI suite。
7. 保存相对开工快照patch、完整日志与报告后停止，交主线程审查；
   修订前仍须有hunk owner并重核同期增量，不全文件回滚。

### 8.2 源码审查义务，不能凭“unit”放行

本轮**未运行产品构建/测试**。未来逐条读源码、包/锁文件、发现/链接/
初始化、脚本/hooks及argv，确认无窗口/输入、真实业务/服务和未获准联网；
证据不足则NOT RUN/受阻。

| 候选路径 | 当前只读观察及未来审查点 |
|---|---|
| 新纯suite | 审查属性/全局初始化、setup/teardown、helper、注入factory；不创建View/NSHostingView/NSWindow，不触`NSApplication.shared`或show/launch/reopen，不读home/偏好或建删目录；核对匹配case不是0 |
| `swift build/test` | `Package.swift:18–33`/Package.resolved有固定版本远程SwiftTerm；锁版本不保证离线。filter不是单文件/单target沙箱，需审查被链接测试和发现初始化；本地依赖不齐就停 |
| App build-only | `project.yml:50–72,124–127`已有App依赖/源和UI test scheme；只允许经审查的`build`，不用`test`/`test-without-building`或Run |
| `pnpm native:build` | `scripts/native/build.mjs:42–88`重生工程、cargo、Xcode、复制helper、ad-hoc重签/验签；不是窄Swift检查，不是本阶段默认命令。若需它，另审generate/签名/验签/依赖及全部共享产物；不因§1允许编译就自动获准 |
| `native:check` | runner按模块执行case、继承环境并写/覆盖日志，某些模块过滤整个AeroricUIUnitTests。不能直接跑native-design/shell/all替代新suite；逐case审查后再决定，本文不授权整模块 |

已有UIUnitTests中含NSHostingView、home衍生校验及生命周期调用；
部分`showReviewWindow()`传`.inactive`并不证明开过窗口，但也不能免审
整个目标。现有Deliverables测试helper还会建删临时目录，不作为纯值模板。
本阶段不新增离屏hosting测试；不为单suite去修旧测试或删除原断言。

### 8.3 定向断言与View消费至少覆盖

- 入口缺席/有效/非法/全部当前旧键冲突，拒绝不构造owner且无目录/home访问，
  App最早分流与原链保全、Debug/Release边界分别审查。
- US1–US6逐组记录：fixture初值→动作/显式模拟事件→Store/投影变化→
  实际View的消费点→断言及不应改变项；不只数方法/搜按钮标题。
- 报告/估算/未知/真实0、覆盖/来源冲突，归属去重/真实重试/共享未分摊，
  价格适用/多币/订阅，补报/修订/旧快照/失败旧值/跨期及并行时间。
- 用户/角色范围、最小预览/无外发，清理取消/明细/汇总缺失/刷新不复活，
  控制/成果证据不连带，资源/执行/核验/Stop分层及自动续行的成功/受阻条件。
- 草稿取消、重复动作、场景切换、重置、两个实例隔离；
  无持久化/真实时钟/Timer/后台任务/网络/进程/生产Store依赖。
  原生系统选择态、语义色及辅助偏好投影可静态检查，
  不把这些断言写成真实材质、AX或UI交互通过。

### 8.4 前提满足后的命令候选，不是当前执行许可

以下逐条审查后选用，非整段照跑。`RUN_ARTIFACT`先创建并取得单写权，
不复制他人目录/日志；记录隔离根、无秘密环境差异、OS/SDK/架构、
完整命令/日志/退出码/产物。冻结解析不保证离线，缺依赖则停，不自动下载。

```sh
# 依赖齐备、发现/链接/初始化审查后，仅运行新纯值/内存suite。
swift test --package-path macos/AeroricKit \
  --scratch-path "$RUN_ARTIFACT/KitScratch" \
  --disable-automatic-resolution --skip-update \
  --filter NativeUsageStatisticsReviewTests
```

保留实际suite/case及passed/failed/skipped，零匹配不是通过；
只编Kit/跑此suite不能证明App最早入口已接入。

工程已按当前配置生成且依赖/资源已交接后，App编译候选如下；
`CONFIGURATION`分别明确选`Debug`或`ReleaseLocal`并分开留证，
不让配置名隐含运行/签名授权。每条先审查，不带test或App启动步骤。

```sh
xcodebuild -project macos/AeroricNative.xcodeproj -scheme AeroricNative \
  -configuration "$CONFIGURATION" -destination 'platform=macOS' \
  -derivedDataPath "$RUN_ARTIFACT/$CONFIGURATION-DerivedData" \
  -disableAutomaticPackageResolution -onlyUsePackageVersionsFromResolvedFile \
  CODE_SIGNING_ALLOWED=NO -showBuildSettings
xcodebuild -project macos/AeroricNative.xcodeproj -scheme AeroricNative \
  -configuration "$CONFIGURATION" -destination 'platform=macOS' \
  -derivedDataPath "$RUN_ARTIFACT/$CONFIGURATION-DerivedData" \
  -disableAutomaticPackageResolution -onlyUsePackageVersionsFromResolvedFile \
  CODE_SIGNING_ALLOWED=NO build
```

结合实际DEBUG编译条件、编译/链接及产物符号证据核对新实现Release排除，
不只搜`#if DEBUG`或运行Release flag；不能核实就明确未验证。
本地无签名/开发签名或ad-hoc编译不代表商店签名、sandbox、审核/发布验收。
依赖解析、编译或旧失败分开报告，不为获得绿灯扩大源码范围。

**持续禁止/未授权**：`native:dev`（会清理dev-data并启动App）、
`swift run`、Xcode Run/App/Preview、任何review/refusal窗口或UI runner，
`xcodebuild test/test-without-building`、XCUITest、accessibility及VM命令。
不运行拒绝分支“试试看”；未审查整包/整模块也不跑。
恢复真实UI须新的明确授权、无VM方案与政策审查，本文不授权远程账号/
TCC/CI采购或系统变更。所有真实旅程与材质/交互验收仍NOT RUN/DEFERRED。

## 9. 回报、证据分层与监工停止点

交回 `.artifacts/native/usage-statistics-review/<run-id>/report.md`，至少包括：

```md
# 使用统计离线样例01：实施回报
## 用户执行指令、独立任务授权/规划审阅/start与main
## 开工dirty原文/hash/diff、相对快照patch、白名单与同期增量
## App入口/构建资源单写交接、实际旧键集合/冲突覆盖及缺席旧链保全
## 纯环境gate、owner/初始化边界、拒绝不回落/不查询home目录
## US1–US6动作/模拟事件、前后投影、实际View消费点与定向断言
## 来源/归属/费用/补报/时间，范围/预览/清理及控制不连带
## 资源/执行/核验/Stop、调额/继续与自动续行重核
## 无真实访问/外发/文件删除/剪贴板/计时器，取消/重置/实例隔离
## 系统选择/语义色/辅助偏好；命令/环境/日志/退出码及case结果
## Debug/Release产物、OS/SDK/架构、依赖/旧失败/争用/残余风险
## 无VM；App/Preview/UI未运行，真实旅程NOT RUN/DEFERRED
## 偏离、需扩围/用户决定与主线程审查停止点
```

分别报告设计已记录、源码已改、静态/fixture/编译实际结果、
未运行UI/生产链与用户未验收，不汇总成“统计功能已完成”。
任何新增文件/动作越界、生产依赖、入口竞争、真实数据/外发/控制需求、
未知执行/资源持有者或验证副作用，立即停止相关项并回主线程。
不得自行切框架、另造App/WebView、扩权限、安装技能/工具、
删锁/强杀、回滚他人工作、删证据或改验收标准。
交回后等监工审查；本包不决定下一设计分支或接管其他终端。
