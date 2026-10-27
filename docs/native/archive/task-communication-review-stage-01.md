# 执行中沟通审阅样例 01 — 外部执行模型交接

版本：1.0 · 2026-10-19 · 范围批准：Q75（用户回复“确认”）。
依据：Q69–Q74，见 `task-communication-design-checkpoint-01.md` v0.1。

**仅批准独立、可交互的SwiftUI＋苹果原生Liquid Glass离线样例范围。
不创建VM，App/Preview/UI运行仍暂停。本文不是立即开工或运行许可。**
用户明确将本文交新终端并要求执行后，再满足§7的独立任务创建授权、
计划审阅/start及文件/构建资源交接；仅实施和经审查的非窗口验证。
本线程只交文档、讨论和审查偏离，未实施页面或验收布局/运行。

## 1. 不创建VM、UI暂停及数据边界

1. **不创建、克隆、导入、下载、恢复、启动、重建或准备替代VM，
   不安装/重装VM工具，不修复已退休VM路径，不继承旧任务/旧包的许可。**
   VM不是本阶段交付/检查条件；不预订或维护已退休资源，不删历史证据。
2. App/Preview/真实窗口和UI自动化继续PAUSED。不启动/预热App，
   不运行XCUITest、`pnpm native:ui-test`、`pnpm native:check accessibility`、
   GUI snapshot、输入自动化或隐藏窗口/换Space/显示器的替代runner。
   不调用UI runner来“确认暂停”，不向用户提供立即launch命令。
3. 仅逐项审查后允许无窗口、无输入的内存/fixture/静态/compile-only检查；
   不凭“unit”名称认定整组安全。只用虚构数据和隔离临时fake-home，
   不读写真实业务、配置、权限、凭据、Agent home或个人历史。
4. 真材质/窗口/键盘/焦点/IME/AX/VoiceOver/生命周期与用户旅程验收均
   **NOT RUN / DEFERRED**，不算green、skipped-to-pass或用户已通过。
   编译、方法数、离屏装配不能代替这些证据，不改floor/删断言补绿。
5. 恢复真实UI须新的明确授权、无VM方案和政策审查；不由本文件授予
   账号、桌面/远程访问、TCC、系统设置、CI采购、发布或新VM权限。

缺依赖、锁/资源被占、入口隔离不足或命令不安全时，保留证据并局部停止，
不强杀持有者、删锁、下载安装工具、换真实数据或自动扩大范围。

## 2. 目标、必读源与非目标

在现有macOS App target中增加**最早DEBUG显式审阅入口**，
用有限内存演示输入用途、消息队列/更正/未知、成员收件、集中待决定、
下一轮接续与冲突处理。必须有真实SwiftUI View、Store动作和可见前后差异，
不能只列标题或`#Preview`；但当前不得启动它，真实体验后续另验。

仓库：`/Users/macbook/Downloads/同步空间/LYX/Aeroric`。开工按需读取：

- `AGENTS.md`、`.trellis/workflow.md`、`docs/native/ui-testing-policy.md`；
- `MIGRATION_PLAN.md`顶部Apple-only基线、ADR-039 §7；
- `.trellis/spec/aeroric-macos/frontend/index.md`、ADR-001原生边界；
- 本文与 `docs/native/task-communication-design-checkpoint-01.md` v0.1；
- `.trellis/tasks/10-14-native-redesign-shell/research/harness-inflight-communication.md`
  的当前确认节及本轮Q75，不把历史待确认段落当当前状态；
- 需要区分执行器/角色时读同目录 `evolution-family-compatibility.md`；
  需要追溯局部对齐时只读 `harness-task-planning.md` Q38。

最低系统与目标矩阵按迁移顶部基线：原生Liquid Glass、最低macOS/iOS/
iPadOS26，Mac目标Apple Silicon/macOS27与Intel/macOS26；
不建立Intel/macOS27组合。本阶段不新增iOS业务，也不修改现有较旧
deployment配置、签名、CI或商店工作；配置冲突如实报告，不借本阶段修迁移。
总库仍10来源、89观察＋U01，仅回指C08/X04/N02既有调查，不重扫harness、
复制其界面/引擎、运行参考项目或宣称上游已实现全部方案。

### 明确不做

- 不接真实Codex/Claude Code/OMP/DSH或其他Agent、模型/API/MCP/PTY/
  反思治理进程、host/UDS/HTTP服务；不生成真实prompt、argv或系统任务。
- 不构造AppServices、生产Store、AeroricClient连接或执行器；
  不解析ANSI/原始JSONL猜问题/批准，不使用CLI登录作为监督API凭据。
- 不读写真实任务/工作包、项目/Git、会话历史、消息/问答、权限、凭据、
  UserDefaults/偏好域、CLI配置或managed-home；不打开文件/URL/外部App。
- 不实现生产路由/队列、调度器、Timer/后台Task/轮询、数据库/持久化、
  幂等/重试/恢复、原子领取/租约、语义分类/冲突引擎或可信来源/权限强制。
- 不自研第五个编码执行器，不改四家族现有路径，尤其不改OMP原生PTY/
  TUI、`--mode`限制或managed-home/history根，不复活rpc-ui、不读`~/.omp`。
- 不改正式首页/主导航/TaskWorkspace、共享业务owner、Rust core/host/
  protocol、iOS或legacy；不把当前样例变成真实任务沟通入口。
- 不改既有Review、共享Design或已冻结阶段包/快照/MiMo研究；
  旧包的UI/VM流程不继承。不建分支/worktree/clone，不commit/push/tag/发布。

## 3. 建议的信息层级与动作语义

以下安排仅用于样例；不冻结正式首页/布局或新增全局消息中心。

1. **顶部**：虚构任务名，常显“离线沟通示例 · 全部为虚构数据”，
   场景菜单、重置及窗口内外观预览；不放假在线/真实计费状态。
2. **工作记录**：普通补充、下一轮队列、回答及更正关联；简洁显示
   来源、收件人、用途、已知事实/未知和待处理内容，不靠颜色判断全部状态。
3. **统一输入区**：当前补充默认；可选下一轮，或从原问题进入回答。
   用途/收件人清楚；编辑草稿可取消，不改原记录；答复态锚定原问题/提问者，
   切换查看其他成员不能把答案误投。成员定向不扩权限/改监督职责。
4. **待你决定/上下文**：原问题、提问方/工作包、推荐/理由、阻塞影响、
   可独立继续的工作；不是让每个成员反复弹窗。原记录和更正依据可展开。

可横纵重排，不强制三栏；建议初始1260×860pt、兼顾1000×640及400×600，
窄窗滚动而不压掉输入/取消/未知说明。尺寸仅设计目标，当前不运行窗口验证。
使用系统标准窗口，不自绘无边框外壳/私有窗口圆角。

### 操作、交付、采纳和完成分别呈现

- “发送/排到下一轮/回答（示例）”只改变有限内存中的交办意图；
  实际已交出、交付未知、消费、当前轮次结束/可接收等通过明确标注的
  **“载入示例事件”**呈现，不是假装来自本机或真实执行器。
- 控件点击不自动等于已送达/消费/采纳/已整改；回答发出不自动把问题
  关联的工作、阶段、技术检查或用户验收涂绿。缺依据保留未知。
- 未交出可控性、交付事实与要求是否被更正是不同维度。原要求被更正
  不等于原消息未投递/旧动作已撤销；不建立全绿线性进度条。
- 下一轮已经发送后，载入“本轮结束且允许接续”的事件就应自动推进
  样例内待处理意图，不再要求第二次批准/发送。该事件是外部边界的
  虚构替身，不是生产流程每次要用户点事件按钮。
- 暂停/Stop独立，先阻止相关新增接续；在途停止请求不等于已停。
  只显示模拟控制与未知/核实事实，不发送信号、不实现停止级联或回滚。
- 普通问题与权限审批明确分开；Full Access不代答需求、不绕新技能
  人工同意等门槛。只显示固定安全别名，不读真实模型/API/权限配置。

## 4. TC1–TC8有限场景

编号只是Q75五类关系的细分，不是新来源ID、八个新模块、生产wire状态
或已选定通用状态机。每项都需Store方法、View实际消费、明确前后投影；
用固定值类型/fixture关系，不造语义解释器、事件总线或后台执行循环。

### TC1 输入用途与原问题锚定（Q69/Q71）

准备“主Agent/协调负责人（示例）”“接口成员（示例）”与固定原问题。
默认补充当前工作，发送后只是待投递意图；可切下一轮，不挤进当前工具。
用能力受限fixture说明只能等待安全边界，不伪装即时注入。

从问题进入答复后，即使查看另一成员，答案仍关联原问题与原提问方；
用途/收件人可见，不能静默丢问题关联再当普通消息发给当前成员。
若用户明确取消答复，保留原问题未回答；普通空白草稿不生成消息。
审批/Stop不是普通用途选项，也不从终端文本自动识别问题。

### TC2 可控队列的编辑、取消与撤回竞争（Q70）

一条确认仍由样例owner控制的未交出消息可以编辑；取消编辑保留原文，
确认修改后原草稿不会再作为另一条旧消息送出。撤回只有确认仍可控才
显示“已撤回（示例）”；不能仅删列表行就认为撤回。

增加“编辑中载入已交给执行器/状态未知”示例：可控条件变化后，提交/
撤回不应仍报成功或覆盖原记录，而应显示事实变化并提供追加更正。
示例不是证明已经实现生产原子领取；不得读真实home做竞争实验。

### TC3 已交出、未知与追加更正（Q70）

预置一条已交出和一条交付未知消息。更正与原记录关联追加，
保留原来源/对象/文本和交付事实；编辑/删除UI不把它改成“从未收到”。
未知不当失败或未发送，不盲重发，不报撤回成功。

载入“示例已消费”记录前不得声称消费；消费后也不自动等于已按新要求
整改/验证通过。更正不是Stop、自动重启或代码/外部操作回滚。
只用有限事件演示，不实现断线恢复、超时重试或真实消费回执。

### TC4 定向成员与局部同步（Q71）

总任务输入默认主Agent/当前协调负责人，允许明确成员定向，记录实际
收件人/工作包。以固定关系演示一条影响共享接口的更正：需要协调负责人
及实际受影响成员核对，独立文档工作仍可继续，不自动广播整个团队。

显示“需同步/示例已交出/消费未知”等差异，不以一处转达把所有人标为
已理解/按新版本执行。可查看监督角色别名及其只读职责说明；
定向给反思/治理不改变权限或将其变成编码者。机器意见/转达保留其来源，
不自动变成新的人类批准。无成员在线探测或实际委派。

### TC5 集中待决定，答案不是审批（Q72）

至少两个不同成员的问题集中显示原提问者/工作、依据、推荐/理由及
不答的影响。重要阻塞有文字/图标提示，普通问题集中查看，不触发系统
通知或一串自动弹窗；无需在成员对话中来回寻找。

未回答/稍后查看不当同意，不自动选推荐答案；回答锚定原问题，发送后
只显示样例答复意图/交付事实，不把依赖工作直接显示已完成。
缺关键答案只约束相关工作，安全独立工作保持可继续。
展示一项独立“权限审批不在此代答”的固定说明，不造真实审批引擎/
新增全局通知中心，不改变Full Access覆盖内免重复索权原则。

### TC6 下一轮条件接续与暂停/Stop（Q73）

同一成员排队两条独立说明，另一成员有独立工作。当前轮次未结束时
保持排队；载入“本轮结束、执行器可接收且条件满足”示例事件后，
第一条按顺序自动进入后续待处理/已交出示例投影，不再点第二次继续；
后续内容保留，不只执行最后一条。具体每轮批次不是生产协议决定。

用有限fixture分别展示待答案、缺授权、资源受限、能力不明的等待，
不得借排队越过审批/监督或安装新技能。单纯获准资源等待在条件恢复时
可按既定规则继续，但暂停/Stop/撤权/取消后的队列不因本轮结束或资源
恢复自行唤醒，不自动重开会话/换执行器。交付/原操作未知先核实，
不反复重发；独立成员不被强加全队串行。

暂停/Stop动作与“已核实停止”的示例事件分开；保留待处理内容与历史，
不自动回滚。用手动载入的有限事件，不读时钟/通知、不建Timer/后台任务。

### TC7 普通补充与明确更正（Q74）

普通新增保留已有其他要求；对一条明确关联的旧要求提交更正后，
保留新旧关联，只替代对应部分。若旧动作尚未执行，后续不机械先做
已过时动作再做新动作；无关要求/消息继续有效，不最后一条覆盖全部。

明确关联且边界内的更正不额外再问一次。用用户显式选择的关联对象/
固定示例关系，不根据任意输入文字自动判断“清楚”。
若旧操作已发生或事实未知，仍保留实际/未知记录，只按影响核对；
被更正不伪装为已撤回/消费/回滚。新时间戳不授额外权限。

### TC8 含糊实质冲突与局部澄清（Q74/Q72）

预置两条范围不清且不能同时适用的说明，显示原文/来源/对象、具体冲突
与推荐，关联到“待你决定”；只等待实际依赖这一决定的工作。
未回答不默认选择较新/推荐方案，独立文档工作继续。

明确答复后只在对应范围形成关联决定/待投递或需核对状态，
不把全队/相关成果自动显示已改好。模型建议/机器转达不自行覆盖用户
已确认要求。用显式fixture，不实现NLP冲突检测或批量代答机制。

## 5. 原生装配、内存owner与入口

### 共享样式与可访问性

- 复用`NativePanelFoundation`、`GlassPanel`、`NativePanelGeometry`、
  `NativeIconButton`及NativeSelection，不另造主题/选择组件系统。
- SwiftUI/AppKit真实Material，受支持交互控件使用苹果原生Liquid Glass；
  沿用availability门控，不用半透明颜色假玻璃，不给整个长文pane套glassEffect。
- A样式：短按钮/输入/选择项胶囊，独立图标不加永久圆环；正文/原文/
  更正差异用可读语义阅读面，连续基底覆盖面板圆角缝隙，不贴三角补丁。
- 相邻互斥用途、收件人、场景/视图选择等用系统Picker/List/Menu与共享
  NativeSelection，不手画active背景/下划线；独立开关/展开控件按既有例外。
- 语义accent/success/warning/danger配文字/图标，不只靠颜色；glass/classic
  与interactionMode、权限、模型/API分离。辅助偏好实际消费到View；
  演示注入只加强系统偏好，明示系统/演示值，不改系统设置或持久化。
- AX identifier只放叶子，统一`taskCommunicationReview.`前缀、可读名称；
  保留原生Tab/方向键/Escape、取消/确认、长文本与窄窗滚动设计。
  当前不声称真实键盘、IME、AX/VoiceOver或材质/焦点已通过。

### 内存owner

`@MainActor @Observable` Store由样例根持有，新窗口新Store；值类型、
草稿、问题、队列、事件/来源关系均局部内存。取消不应用、重置还原fixture，
各实例不共享输入/回答/更正/暂停或事件。
不使用单例、AppStorage/UserDefaults、数据库/文件写、后台Task/Timer/
真实clock/网络/进程/业务回调。模型事件与投影仅说明有限样例，不作为
生产队列、权限/来源或并发安全保证。

### DEBUG显式入口契约（本文交接时未实施）

新增键 **`AERORIC_NATIVE_TASK_COMMUNICATION_REVIEW`**。
所有新源/模型/fixture/Store/View、决策/拒绝页/生命周期整体DEBUG限定。
在`AeroricEntryPoint.main`最早DEBUG分支，先于当前
WorkArrangements → Skill → Knowledge → Evolution → Visual → product链：

| 请求 | 必须行为 |
|---|---|
| 新键缺席 | inactive；旧链完整不变，不重排、不解释/修复其他键 |
| 新键`1`、`AERORIC_ENV=ui-test`、显式`AERORIC_DATA_DIR`通过既有严格目录helper、其他审阅键无`1` | 仅新样例；任何产品App/delegate/services或旧Review owner构造之前返回 |
| 新键存在但`0`/空/非法，或环境/目录不符，或其他审阅键任一为`1` | 仅新只读拒绝页，绝不回落产品或按入口顺序选择其他样例 |

其他六键：`AERORIC_NATIVE_WORK_ARRANGEMENTS_REVIEW`、
`AERORIC_NATIVE_SKILL_REVIEW`、`AERORIC_NATIVE_KNOWLEDGE_REVIEW`、
`AERORIC_NATIVE_EVOLUTION_REVIEW`、`AERORIC_NATIVE_VISUAL_REVIEW`、
`AERORIC_NATIVE_COLLABORATION_REVIEW`。新键存在时其他键`0`不冲突；
新键缺席不等于赋值`0`。协作键目前只是冲突预留，不据此承诺已实现。

复用`NativeVisualReviewEntry.isIsolatedDataDirectory`只读Caches子目录helper，
显式注入fake-home进行测试；不改helper，不造宽松prefix，不创建数据目录，
不回显路径/环境值/密钥。该helper检查Caches以下候选/沿途元数据与规范路径，
不证明Caches以上全部无符号链接、OS沙箱、权限强制或偏好域完全隔离。
`ui-test`只是一项入口标签，**不是解除UI暂停的许可**。

新样例/拒绝页有专属无业务生命周期与唯一窗口标识，只管理自身标准
不恢复状态的窗口。禁止复用产品/旧Review delegate、业务菜单、偏好清理、
全局窗口扫描、焦点补丁、激活/关闭其他实例或进程。关闭释放根/Store，
重开新建；业务代码不写偏好，但不宣称操作系统绝无窗口持久痕迹。
当前只实施/静态核对这些路径，不调用显示方法/NSApplication.shared。
ReleaseLocal排除全部新实现并走原产品链，只compile-only/产物核对，
不启动Release测试flag。

## 6. 产品/测试文件白名单

以下为最大范围，不要求全改。新路径若开工时已存在，先核对owner，
不覆盖其他终端成果；类型可在五份新源内拆小，不新造更多目录/target。

| 路径 | 允许职责 |
|---|---|
| `macos/AeroricKit/Sources/AeroricUI/TaskCommunicationReview/TaskCommunicationReviewModels.swift`（新） | 仅样例值类型、输入/对象/问题/已知与未知/替代关系，不定义生产DTO |
| 同目录`TaskCommunicationReviewFixtures.swift`（新） | TC1–TC8虚构数据、固定关联、有限场景事件 |
| 同目录`TaskCommunicationReviewStore.swift`（新） | 窗口内存owner、草稿/取消/重置与有限转换 |
| 同目录`NativeTaskCommunicationReviewView.swift`（新） | 原生输入/记录/队列/待决定及详情，真实消费状态与动作 |
| 同目录`NativeTaskCommunicationReviewEntry.swift`（新） | 只读入口决策、拒绝内容、专属无业务窗口生命周期 |
| `macos/AeroricApp/Sources/AeroricApp.swift` | 仅最早DEBUG新分流/新样例App装配，旧Review/产品逻辑保持 |
| `macos/AeroricKit/Tests/AeroricUIUnitTests/NativeTaskCommunicationReviewTests.swift`（新） | 逐项审查的无窗口决策/内存/隔离检查 |

任务私有PRD/design/implement/context及本次证据另按Trellis管理，
不改整壳/迁移任务metadata/context，不整库自动注入执行Agent。

**只读/不在白名单**：所有既有Review/单测、共享Design/ThemeTokens/
NativeSelection、Package.swift、project.yml/xcconfig、业务/core/protocol、
iOS、legacy、`macos/AeroricUITests/`、`scripts/native/`的runner/policy/
checks/floors/清单、签名/CI/个人配置。禁止降floor、删断言或顺手修旧失败。
生成工程只允许既有generate工具的派生产物，不手改或新增target。
若接入/验证确实需要额外hunk，先回主线程审查最小扩围；
一般构建说明不能当作额外文件修改权。

## 7. 新终端开工顺序及共享资源

1. 确认用户明确交本文要求执行；遵守Trellis独立任务创建授权，编写独立
   PRD/design/implement和按需context，经计划审阅后才`task.py start`。
   本线程任务仍不激活，不借其他执行者任务状态开工，不用旧VM/UI许可。
2. main上保存开工status、白名单当前原文/diff/SHA-256及缺席路径记录到
   `.artifacts/native/task-communication-review/<run-id>/`，以当时dirty树
   而非HEAD为准；冻结其他阶段包/快照及本文/设计快照，不stash/reset/clean。
3. 取得App入口等共享文件的明确单写交接，每次编辑前重核hash/diff；
   若WorkArrangements或其他终端正在修改/验证同一文件，先协调等待。
   不覆盖他人成果，不把其增量/通过记录纳入本次验收。
4. 先核对本地工具/冻结依赖齐备、命令副作用与继承环境；不自动安装/
   下载/更新依赖工具或访问真实账号/业务/网络服务补缺。HOME改值不等于
   完整隔离；fake-home应显式传入入口/测试API，不用真实home作负向测试。
5. 编译前取得SPM `.build`/缓存、core target、生成工程、固定DerivedData、
   Debug bundle/helper/签名产物单写交接；不能重写用户或未知进程持有的
   App bundle，不强杀/删锁、不建worktree/VM绕过。条件不足相关编译DEFERRED。
6. 先写值类型/fixture/纯入口决策单测，再Store、View、最小App分流；
   单测逐项审查不构造窗口、不触发NSApplication.shared/生产服务/偏好
   Store，不启动进程/socket、不读真实home。只允许未显示的生命周期
   策略对象，不调用其启动/显示/重开方法。
7. 按§8分列静态、定向单测与编译。当前不新增/运行真实UI用例、不改UI
   清单/runner/floor；需要额外回归先列命令和实际副作用供审查。
8. 交相对开工快照patch、证据与报告后停止，等主线程审查，不继续真实
   引擎/正式首页/运行UI。修复只动自己仍有owner的具体hunk；
   先核对无后续协作者修改，不全文件回滚或删除历史证据。

## 8. 验证：仅后续经审查的非窗口检查

以下是执行者的候选检查，不是本次已运行或通过记录。逐条重核当时
源码/环境/依赖/实际目标与资源，不能凭命令名、旧floor或旧通过数推断安全。

### 8.1 本次静态接入事实与限制

- 当前App最早WorkArrangements→Skill→Knowledge→Evolution→Visual链，
  产品`AeroricNativeApp.init`构造AppServices；新分流必须在其前，
  不能在已构造产品的View里放“离线”按钮。
- 既有WorkArrangements Entry有纯决策/注入home接缝与专属生命周期；
  只复用模式和只读目录helper，不修改旧Review来认识新键。新键缺席
  保持旧行为，新键存在则由最早分支拥有样例/拒绝两种结果。
- Package自动收新源/单测；不需要改Package/target。但定向suite可能编译
  整个package；过滤不隔离测试发现/链接或间接初始化，仍须审查，
  不等于执行全量suite，也不免既有编译失败或共享缓存争用。
- `shell`/`native-design`当前unit case会跑整个AeroricUIUnitTests，
  含离屏View/测试偏好域；`swift-client`含临时UDS/Python假宿主进程。
  **不能默认运行整组，或把它们称为无服务/只测新suite。**
- check工具继承环境/写固定日志，不提供本样例专属suite过滤；
  doctor含工具探针/邻接项目git读取，不自动当必跑。不要运行`check all`。
- 原生build链含generate、cargo、Xcode Debug、helper复制与开发签名/核验，
  会改共享产物；静态未见App启动不等于命令无副作用，执行前重核脚本/hooks。
  frontend spec index指向的native-ui.md缺失，不补占位或顺手改规范；
  不足/冲突回主线程，当前基线不被旧spec覆盖。

上述只是2026-10-19仓库静态快照，可能含其他协作者dirty成果，
不是其实施验收、未来开工状态或运行安全认证。

### 8.2 新suite与静态消费至少覆盖

- 新键缺席对旧键各值/组合均inactive；新键存在的`0`/空/非法、
  错环境、缺/非法目录都拒绝，不回落产品。
- 合法新键与其他六键分别为`1`及全部63种非空组合时均拒绝；
  新键存在、其他键`0`不冲突。旧链只做纯决策/源码核对，不启动旧App。
- fake-home内测试Caches根、相对/点段、前缀碰瓷、非目录、候选及沿途
  符号链接拒绝；合法未存在子目录接受但不创建。只临时目录元数据，
  保留前后目录清单，不对真实home/配置/权限做破坏性负向测试。
- TC1–TC8真实Store方法前后投影：用途/对象、答复锚定、空草稿、
  编辑取消/撤回竞争、交付未知/关联更正、局部同步/独立工作、
  待决定未答/答复不代批、轮次边界/FIFO/阻塞恢复与Stop优先、
  普通补充/明确更正/含糊冲突，不只测试固定成功常量。
- 各实例与重置不共享草稿/问题答复/队列/事件；被更正、已交出与未知
  能并存，不偷改原事实；新时间戳不改变样例授权/监督边界。
- 静态核对View真实绑定Store状态/动作、原生选择/语义色/辅助偏好，
  App最早分流、旧链保全、DEBUG/Release排除、无业务导入/副作用路径。
  若提出离屏装配，另逐项审查不创建NSWindow/NSApplication.shared、
  不初始化生产/偏好/服务；不确定则该项DEFERRED，不为装配启动App。

不要求主观“采纳”伪造可测回执；只证明有限样例行为，不认证生产消息
引擎、外部API或四家族即时反馈。没有真实View消费也不能只凭Store通过交差。

### 8.3 前提满足后的候选命令

每条先逐项审查并保留独立日志、环境差异（无密钥）、完整命令/退出码、
passed/failed/skipped和实际执行suite；新suite不存在记“未存在”，
零匹配不算通过。下面**不是可一键盲跑的脚本**：

```sh
RUN_ID="$(date '+%Y%m%d-%H%M%S')"
RUN_ARTIFACT="$PWD/.artifacts/native/task-communication-review/$RUN_ID"
mkdir -p "$RUN_ARTIFACT"

# 仅依赖本地已齐且经审查无下载/更新时；定向新suite、不得显示窗口。
swift test --package-path macos/AeroricKit \
  --disable-automatic-resolution --skip-update \
  --filter NativeTaskCommunicationReviewTests

# 已核对当时脚本/hooks及所有共享产物owner后，仅编译/开发签名，不launch。
pnpm native:build
```

冻结参数不保证离线/无网络、不隔离SPM缓存或`.build`；缺本地依赖不跑，
不靠自动下载补齐。可在独立任务审查scratch/cache方案，不复制真实业务。
不要默认`native:check shell/native-design/swift-client`等整组安全，
不要为单suite改checks/floors。编译范围与真正执行suite分开回报；
未跑全回归就明示未跑，既有失败另记而不顺手修/降门槛。

既有generate工具准备工程后，ReleaseLocal只compile-only核对新实现排除：

```sh
xcodebuild -project macos/AeroricNative.xcodeproj -scheme AeroricNative \
  -configuration ReleaseLocal -destination 'platform=macOS' \
  -derivedDataPath "$RUN_ARTIFACT/ReleaseDerivedData" \
  -disableAutomaticPackageResolution -onlyUsePackageVersionsFromResolvedFile \
  CODE_SIGNING_ALLOWED=NO -showBuildSettings
xcodebuild -project macos/AeroricNative.xcodeproj -scheme AeroricNative \
  -configuration ReleaseLocal -destination 'platform=macOS' \
  -derivedDataPath "$RUN_ARTIFACT/ReleaseDerivedData" \
  -disableAutomaticPackageResolution -onlyUsePackageVersionsFromResolvedFile \
  CODE_SIGNING_ALLOWED=NO build
```

核对有效DEBUG设置、本次产物符号/链接与原产品链；仅源码`#if`或配置名
不等于排除证据。无法确认则未验证，不启动Release验证开关；
不运行`swift run`、Xcode Run、Preview、App、UI runner或任何VM命令。
本次文档交接未运行上述构建/单测命令。

### 8.4 必须保留的未验收项

真实按钮/菜单/sheet、队列操作手感、键盘/焦点/IME、AX/VoiceOver、
浅深/glass/classic及辅助偏好真实输出、面板圆角缝隙/材质、
窗口尺寸/关闭重开/重启与Intel/Apple Silicon真机旅程均NOT RUN/DEFERRED。
不登记虚假截图/通过记录，不因暂停宣告首发/双商店/真实沟通能力完成。

## 9. 回报与监工停止点

保存`.artifacts/native/task-communication-review/<run-id>/report.md`，至少：

```text
# 原生执行中沟通样例01：实施回报
## 用户执行指令、独立任务创建授权/审阅/start、main及文件/构建资源交接
## 开工dirty原文/hash/diff、相对快照patch、实际文件与同期他人增量
## TC1–TC8 Store方法/View消费、预期/实际、失败/未知及未覆盖分支
## 输入用途/收件人/原问题锚定、来源与审批/监督边界
## 编辑撤回/交付未知/更正、顺序与过时要求、局部待决定/独立工作
## 自动接续触发证据、授权/依赖/资源阻塞、暂停/Stop不被唤醒
## 入口矩阵/63冲突组合、旧链保持、fake-home目录只读/不创建
## 无生产依赖/写入、纯内存owner/取消重置、共享原语与辅助偏好消费
## 原始命令/退出码、实际suite及passed/failed/skipped、编译产物
## Debug/Release有效设置/排除、实际OS/SDK/架构、既有失败/共享资源阻塞
## 无VM声明；App/Preview/UI未运行，交互/材质/键盘/AX等DEFERRED
## 偏离、残余风险及需要主线程/用户决定的问题
```

主线程按开工快照、本次diff、实际View消费与新鲜日志审查；
模型自报/文档一致/单测通过不等于用户验收。交源码位置和实际产生的
编译产物时标“未启动/未经UI验收”，不建议用户直接打开App绕过暂停。
缺条件可交局部成果和具体阻塞，不声称全部实现/验收完成。

交回后停止，不扩大旧包、真实队列/模型接线、正式首页或UI/VM范围。
本线程不创建/激活任务、不实施/运行产品、不安装技能、不提交推送；
是否将本文交新终端执行由用户决定。
