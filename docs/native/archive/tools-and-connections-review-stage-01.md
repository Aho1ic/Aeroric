# 工具库与资源连接审阅样例 01 — 外部执行模型交接

版本：1.0 · 2026-10-20 · 范围批准：Q93（用户回复“确认”）。
依据：Q83–Q92，见 `tools-and-connections-design-checkpoint-01.md` v0.1。

**仅交接独立SwiftUI＋苹果原生Liquid Glass离线样例。
不创建VM，App/Preview/真实窗口/UI运行继续暂停。本文不是立即开工许可。**
用户交新终端并明确要求执行后，仍须满足§7的独立任务创建授权、
规划审阅/start及文件/构建资源单写交接。本线程只交文档、设计与监工，
不实施页面，不验收布局/运行，不修改其他冻结交接包。

## 1. 政策与数据隔离先于实施

1. **不创建、克隆、下载、导入、恢复、启动、重建或准备替代VM，
   不安装/重装VM工具，不修复退休VM路径。**旧任务/旧包运行许可不继承，
   VM不是交付条件，不删除历史测试证据/退休回执或管理无关VM。
2. App/Preview/真实窗口与UI自动化PAUSED。不预热/启动App，不运行
   XCUITest、`pnpm native:ui-test`、`pnpm native:check accessibility`、
   GUI snapshot或等价窗口/输入runner；也不调用runner“看看是否拒绝”。
   不给立即launch命令，不用隐藏窗口、其他Space/显示器或远端绕过暂停。
3. 只允许逐项审查过的无窗口/无输入静态、fixture、单元及compile-only
   检查，使用虚构数据、隔离临时根和显式fake-home。target叫unit、
   命令有filter或HOME换值，不证明链接/发现/间接初始化全部无副作用。
4. 真实玻璃/圆角衔接、浅深/辅助偏好输出、清单/菜单/sheet、键盘/
   焦点/IME、AX/VoiceOver、窗口生命周期及用户旅程均为
   **NOT RUN / DEFERRED**。编译、静态、方法数、离屏装配不代替验收，
   不删断言/降floor、用零匹配/暂停拒绝/旧证据补绿或产假截图。
5. 恢复真实UI须新的明确授权、无VM执行方案与政策审查；本文不授权
   新账号/远程桌面、TCC/系统设置、CI采购、提交推送、签名发布或商店提交。

入口、文件owner、依赖或检查副作用无法核实时，保留证据并暂停相关项；
资源争用报告，不强杀持有者、删锁、换真实数据或自动下载安装工具补缺。

## 2. 目标、必读源与非目标

在现有macOS App target增加最早DEBUG显式入口，以可重置虚构内存展示
工具库的六组关系。必须有实际SwiftUI View、Store动作和前后状态消费，
不是标题清单、固定全绿卡片或只有`#Preview`；本阶段不启动页面观看。

仓库：`/Users/macbook/Downloads/同步空间/LYX/Aeroric`。按需读：

- `AGENTS.md`、`.trellis/workflow.md`、`docs/native/ui-testing-policy.md`；
- `MIGRATION_PLAN.md`顶部Apple-only基线、ADR-039与ADR-001；
- `.trellis/spec/aeroric-macos/frontend/index.md`；
- 本文与 `docs/native/tools-and-connections-design-checkpoint-01.md` v0.1；
- `.trellis/tasks/10-14-native-redesign-shell/research/harness-tools-and-connections.md`
  §5/7/9/11/13/15，及续页`harness-tools-and-connections-02.md` §4/6/8/10/12；
- 真需回溯才窄读同目录技能Q46–Q49、资源/续行Q64–Q67及成果Q76–Q82，
  不整库注入或重扫harness，不执行参考项目。

旧提案“待确认/尚未生成”保留其历史时间点，以最新确认/本交接为准。
仓库当前平台基线要求最低macOS/iOS/iPadOS26，Mac验证目标为
Apple Silicon/macOS27及Intel/macOS26；本样例不据此认证平台兼容，
不新建Intel/macOS27格，也不修现有旧deployment、签名或缺失spec。
总体仍双原生/双商店目标，Mac有限样例不削减iOS或首发产品要求。

### 明确不做

- 不接四族/custom Agent、模型/API/MCP/PTY、反思/治理、host/UDS/HTTP、
  SSH/SFTP、Docker、数据库、存储或其他真实服务，不构造prompt/argv。
- 不构造AppServices、生产Store/AeroricClient，不读真实任务/项目/
  会话/配置/权限/凭据/managed-home，不扫描用户CLI配置找密钥。
- 不扫描/监听/导入/解析真实工具目录、项目文件、技能包、连接或日志；
  不读写Keychain/密码/API Key/私钥，不跨Host分发、不操作剪贴板/导出。
- 不下载/安装/更新/回退/卸载/启动工具，不运行脚本、后台探测/连接/
  轮询，不做生产摘要/脱敏/缓存/消费回执/重试/恢复/幂等/权限强制引擎。
- 不用Timer、后台Task、真实clock或业务回调伪造外部进展；不建真实
  工作任务，不复制业务DTO或把局部Store升级为生产调用/调度writer。
- 不打开文件/URL/外部App，不用Quick Look/WebView/网页预览服务、
  文件/保存面板、远程资源解码、真实上传/发布充当查看或示例动作。
- 不从ANSI/原始JSONL推断业务；不改四族/custom身份或OMP真实PTY/TUI、
  flags、managed-home/history与hook边界，不复活RPC UI或第五执行器。
- 不替换正式首页/主导航/TaskWorkspace/已有技能资源业务和Review样例，
  不改Rust/core/protocol、iOS、legacy、构建/政策/签名/CI或发布配置。

## 3. 信息层级、呈现与内存动作

以下是有限样例的组织建议，不批准正式产品布局或全套资源管理中心：

1. **顶部**：常显“工具库与连接 · 离线虚构示例”，任务/项目范围、
   场景选择、重置和样例外观。状态不伪装成本机连接、安装或Agent运行。
2. **统一目录**：工作方法、工具能力、资源连接三类，用系统选择态
   切换/筛选；同一示例引用关系连接三者，不复制成多套技能或资源对象。
   名称/用途、当前情况、阻塞/下一步简洁，不用大量空白堆装饰卡片。
3. **所选详情**：用途/来源依据、配置/安装、项目/任务范围、执行器
   支持、服务/连接、操作授权分层；未知/未查/过时有文字原因。
4. **相关意图与审阅**：范围编辑、安装/更新审阅、停用、凭据使用与
   导出边界；风险/未覆盖说明贴近动作，不自动替用户勾选全部权限。
5. **结果与依据**：短结果直接显示，大结果固定内存预览/展开，
   来源和错误/限制清楚；返回、业务结果、读取/采用、核验/验收分别说明。

可横纵重排，不强制三栏；默认1260×860pt、兼顾1000×640与400×600
为设计目标，窄窗滚动而不隐藏错误、取消或范围。用系统标准窗口，
不画私有圆角/无边框壳；当前不运行窗口尺寸/布局验收。

### 意图不冒充外部事实

- 编辑草稿、取消、保存示例配置、项目启用、任务引用、审阅/停用、
  结果展开、外观切换、重置仅改局部内存。取消不应用，不写真实配置。
- 安装/更新审阅通过只记录“示例意图已获准/尚未执行”，不是已安装、
  已连接或已可用；有明确预授权的示例不重复弹审批，新技能独立人工同意。
- 外部安装/停止结果、条件恢复、读取/采用、核验或历史用户接受，
  只能来自清楚标注的预设事实或“载入示例事件”。不能由普通查看/
  授权按钮、时间流逝或模型自报暗中编造，未获证据保持未知。
- 有效等待交办在条件恢复并重核后自动更新可续行投影，不再多点一次
  审批；实际执行结果仍须示例事件。用户暂停/Stop/取消不被事件复活。
- 所有记录标明示例来源，无真账号/地址/秘密。使用`demo`标识/内存文本
  即可，不生成可执行命令、真实路径/连接串或可点击外链。

## 4. TC1–TC6有限场景与可核对结果

每组至少有一个正向和一个受阻/未知分支，测试前后关系，并在View中
实际消费对应值/方法。编号是六组关系，不是六个生产模块或新来源ID。
值类型与方法名可在§6内细化，不要求照抄生产schema/枚举。

### TC1 目录、配置复用与明确范围（Q83/Q84）

- 虚构“接口核对方法”“开发库查询能力”“演示开发库连接”，引用同一
  示例条目，不读取真实技能/资源目录。方法表示怎样做，能力表示操作，
  连接表示明确目标；目录选择/查看不安装、调用或授操作权。
- 项目甲/项目乙、两个任务及无项目任务可作有限上下文。保存配置后
  不自动全局启用；项目甲启用不影响乙，不直接授任务查询权限。
- 已获准任务按需引用，临时引用仅供本任务，不写回项目默认；无项目
  任务无需先建项目。用户可指定/收窄；不强制每次勾选整张工具清单。
- 可编辑范围草稿，取消不应用；实例隔离和重置恢复原fixture/引用。

**核对**：保存→项目启用→任务引用→操作授权仍是不同事实，跨项目和
仅任务范围不串用；查看/引用前后外部执行计数不增加，不把引用当成功。

### TC2 分层状态、缺项与范围内选用（Q85/Q86）

- 同时展示配置/安装、启用范围、执行器支持、服务/连接、任务授权；
  例如“示例MCP服务运行中，但本任务未获开发目标访问”，不能总绿。
- 至少包含未安装、未启用、执行器不支持、连接失败、权限不足及
  未查/过时/未知的有限例子。不要为每个对象编造不适用的状态层。
- “示例拟选工具”只在已启用/可用/获准集合中改变拟选，用户禁用/
  指定优先；选择不触发调用/消费。获准替代不能绕禁用或暗换目标、
  执行器/API/模型，目标歧义标待澄清；独立文档工作不被全队阻塞。
- 显示原因与下一步，查看详情不跑连接测试。四族支持值明确为虚构
  预设，不宣传本机四家族已适配，也不把模型自报当宿主证据。

**核对**：服务运行≠目标授权≠调用成功，未知不退化成可用；受阻原因
和拟选投影实际影响View，查看/筛选/外观变化不会改模型、授权或PTY。

### TC3 来源审阅、预授权、新技能与更新（Q89/Q90）

- 新外部工具/MCP审阅给用途、来源/可获得版本依据、运行位置、
  依赖/脚本/联网/权限及范围；未知明示，远端服务不伪装本机安装。
- 无安装预授权时形成一次集中审阅意图，可取消；已有明确安装预授权
  含Full Access确覆盖部分不重复问，仍不授全项目/全资源权限。
  审阅同意不直接标已安装，外部结果要靠单独示例事件。
- 附带新技能时仍人工同意，不从Full Access或工具包装绕过；普通技能
  Q49及U01路径不重新实现。仅演示界限，不安装/调用任何技能。
- 已接入工具v1在途，v2候选说明来源/能力/依赖/权限差异。普通更新
  默认先审阅，明确更新预授权内不重问；安装授权不是未来所有更新授权。
  在途不暗换，v1失去许可/安全可用条件时局部受阻；远端不可锁版明示。
- 新版失败：旧版仍获准、安全兼容且回退操作获准才投影可回退；
  任一条件缺/未知保留受阻。回退不改已发生业务结果，不重放写入、
  自动回滚远端或复活Stop，不创建真实版本/依赖/迁移事务。

**核对**：普通审阅/预授权/新技能人工同意分开；安装与更新范围不外溢，
查看候选不发生外部动作，在途依据和失败/未知不被确认按钮抹平。

### TC4 停用、在途待核实与恢复（Q87/Q88）

- 停用示例连接前明确任务/项目范围，确认后阻止该范围新增拟调用、
  排队和重试；其他项目及独立文档工作保持，不模拟关闭共享MCP服务。
- 在途仅记“示例停止请求已提出/结果待核实”，不立刻已停/回滚；
  明确停止结果只能载入示例事件。配置、成果/历史引用仍保留。
- 普通等待且有效交办允许等待时，条件恢复事件重核目标/授权/资源/
  依赖，满足就更新可续行投影，不再重复审批。条件仍缺只阻塞相关步骤。
- 暂停/Stop/取消不因重新启用或恢复事件续行，须明确恢复意图并重核；
  到限沿Q67，不重造资源续行策略。失联前结果未知，恢复也不重放原写入
  或启动重叠替代；变化不静默换目标/执行器/API/模型。

**核对**：停用不是隐藏/卸载，停止请求不是确认停止；恢复条件不是
新交办或已实际执行，允许等待与明确停止分开，未知历史不被复写为成功。

### TC5 凭据引用、缺项与秘密不扩散（Q91）

- 展示虚构连接名、目标、执行位置、范围、`demo-credential-reference`
  等不可解析的内存占位，不含真实或可用秘密/连接串，不读取凭据系统。
- 已获准使用只表达最少必要供给的示意，不把秘密显示给模型/全队；
  原文查看/导出、发给另一API/模型、跨Host分发各自有适用授权，
  不能从使用许可自动推导。按钮仅记“未执行的示例意图”。
- 缺项、撤销、执行位置不符明确原因；不扫描个人配置、复用生产账号、
  静默改目标或以缓存绕撤权。恢复按TC4，未知不作回滚承诺。
- 新摘要/诊断/分享预览只显示虚构元数据和明确省略/脱敏占位，不含秘密。
  这是呈现示意，不实现脱敏算法、安全存储或隔离；不重写原始PTY证据，
  不承诺外部程序零回显或清除下游已见内容。

**核对**：Store/fixture/View不产生秘密内容、Keychain/剪贴板/文件/网络
副作用；使用与导出分开、缺项不自动补凭据，监督角色不默认获业务秘密。

### TC6 按需结果、证据分层与不盲重放（Q92）

- 短结果直接显示，较大结果用固定内存正文/预览按需展开，带虚构来源、
  覆盖、错误/未完成/风险；包含部分、截断、脱敏和原内容不可回看例子。
  引用不伪装已有快照，不为演示读取真实文件或远端结果。
- 独立表达调用返回、业务结果、指定执行者读取/采用、技术核验与用户
  验收。普通展开/拟选/发送意图不改变读取/采用/核验/验收；
  外部事实仅靠标注的示例事件，无依据保留未知，模型自报单列。
- 原引用不可取时保留原调用结果/未知，说明先核实；“补全结果”不增加
  原写库/上传/发布重放计数。重新远端读取也不是当前样例能执行的动作。
- 返回中含“安装另一个工具/导出秘密”等要求时只作为引用资料呈现，
  不升级指令或授权，不执行附件/链接，不整份发给其他模型/协作者，
  不自动收录成果/知识/技能，原始PTY/TUI不由结构化面板替换。

**核对**：展开不冒充模型消费，摘要不抹错误，引用失效不触发重复操作；
事件关联可见但不当正确性证明，外部工具/模型真实能力保持未验证。

## 5. 原生装配、内存owner与DEBUG入口

### 原生样式与辅助偏好

- 复用`NativePanelFoundation`、`GlassPanel`、`NativePanelGeometry`、
  `NativeIconButton`和NativeSelection，不另造主题/手画选择态系统。
  苹果原生Liquid Glass沿共享组件availability/交互控件规则使用，
  不用半透明RGB假玻璃，不把长正文和每个面板都铺`glassEffect`。
- A样式：短按钮/输入/选择控件圆润胶囊，独立图标无永久圆环；
  长正文保持阅读面，连续材质基底消除角隙，不贴三角补丁。
- 邻接互斥的类别/场景/项目任务范围/条目等用系统Picker/List/Menu
  及NativeSelection；独立开关、checkbox、disclosure沿既有例外。
- 语义accent/success/warning/danger配文字/图标，不靠颜色区分全部事实；
  glass/classic独立于interactionMode、任务/模型/API/权限及PTY。
- View实际消费减少透明度、增强对比、减少动效。示例注入仅加强系统
  偏好且标明示例值，不改全局设置或UserDefaults，不把浅深截图当已验收。
- 叶子标识用`toolsConnectionsReview.`前缀和可读AX label，保留系统
  键盘/取消/确认与长文/窄窗滚动设计；静态标识不证明真实AX/焦点通过。

### 内存owner与隔离

新根拥有`@MainActor @Observable` Store；每个窗口/实例独立，不共享
生产类型/单例。值类型、fixture、范围草稿、审阅/停用/操作意图、
示例事件、结果文本及证据投影全部局部内存。取消不应用，重置清空
局部变更/事件并还原fixture，不同实例的范围/选用/审批/Stop/结果不串。

不使用AppStorage/UserDefaults、文件/数据库、真实clock、Timer/
后台Task、socket/网络/进程/业务回调；唯一文件访问例外是下述入口
只读目录元数据校验及测试自己的临时fixture目录，不读目录业务内容。
模拟授权/停止/凭据供给不是生产强制或安全隔离证明。

### 新入口契约（本交接尚未实施）

新键 **`AERORIC_NATIVE_TOOLS_CONNECTIONS_REVIEW`**；
新模型/fixture/Store/View、纯决策、拒绝页及生命周期均DEBUG限定。
在`AeroricEntryPoint.main`最早DEBUG分流，**先于TaskDeliverables及
任何旧Review/产品App/delegate/services构造**，不是业务已启动后隐藏View。

| 请求 | 结果 |
|---|---|
| 新键缺席 | inactive，保留完整旧链/旧键解释，不顺手修旧入口 |
| 新键精确为`1`，`AERORIC_ENV=ui-test`，显式数据目录通过严格helper，其他旧键无`1` | 仅新样例分支，先于旧/产品owner并返回 |
| 新键存在但为`0`/空/非法，环境/目录不符，或其他旧键任一精确为`1` | 仅新样例只读拒绝分支，绝不回落产品/另一Review |

当前旧键共8个（7个已有入口＋1个预留），**非空冲突组合255种**：

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

新键存在时其他键`0`不构成冲突，缺席不等于`0`。COLLABORATION是预留，
不证明已实现；开工发现更多键或顺序变化，先回主线程核对最小扩围，
不把255永久硬编码为所有未来组合，不重排旧链或统一旧入口冲突语义。

复用`NativeVisualReviewEntry.isIsolatedDataDirectory`，入口决策显式
可注入home，测试只传自有fake-home。要求Caches严格子目录、不用宽松
prefix；合法未存在子目录可以通过但不创建，拒绝不回显路径/环境值/
秘密。helper只检查Caches以下元数据，不认证全部上层无符号链接、
TOCTOU安全、OS沙箱或偏好完全隔离。`ui-test`仅入口标签，不是UI运行许可。

样例/拒绝页用专属无业务生命周期、标准非恢复窗口，建议标识
`native-tools-connections-review`与`native-tools-connections-review-refusal`。
只管理自己的窗口，不复用产品/旧Review delegate、菜单/偏好清理，
不全局枚举/激活/关闭其他窗口。关闭释放根/Store，重开新建；不借
“末窗关闭是否退出”改旧样例策略，不承诺OS完全不留窗口痕迹。
当前只实现/静态核对策略，检查不调用show/launch/reopen或
`NSApplication.shared`，不构造真实窗口/hosting view。
ReleaseLocal排除全部新实现，保留产品链，只compile-only核对，不启动验证flag。

## 6. 产品/测试文件白名单

最大允许范围如下，不要求全改；开工新路径若已出现须核对owner，
不覆盖他人实现。类型在五份新源内细化，不另加目录/target/依赖。

| 路径 | 允许职责 |
|---|---|
| `macos/AeroricKit/Sources/AeroricUI/ToolsConnectionsReview/ToolsConnectionsReviewModels.swift`（新） | 样例值类型/引用/范围/证据，不定义生产DTO |
| 同目录`ToolsConnectionsReviewFixtures.swift`（新） | TC1–TC6固定虚构内容、来源、事件与限制 |
| 同目录`ToolsConnectionsReviewStore.swift`（新） | 局部内存owner、草稿/取消/重置、意图与有限投影 |
| 同目录`NativeToolsConnectionsReviewView.swift`（新） | 原生列表/详情/审阅/结果，实际消费状态与动作 |
| 同目录`NativeToolsConnectionsReviewEntry.swift`（新） | 纯入口/只读目录校验复用、拒绝内容、专属生命周期 |
| `macos/AeroricApp/Sources/AeroricApp.swift` | 仅最早DEBUG分流与新样例App装配，旧链/产品逻辑不动 |
| `macos/AeroricKit/Tests/AeroricUIUnitTests/NativeToolsConnectionsReviewTests.swift`（新） | 逐项审查的无窗口入口/内存/隔离检查 |

独立任务的PRD/design/implement/context和证据另按Trellis管理，不改
整壳/迁移/其他任务metadata/context，不整库自动注入。

**只读/不在白名单**：全部旧Review/测试、共享Design/NativeSelection/
ThemeTokens、现有技能/连接/基础设施/数据库/存储/工具业务、Package.swift、
project.yml/xcconfig、Rust/core/protocol、iOS/legacy、
`macos/AeroricUITests/`、`scripts/native/`、政策/runner/checks/floors、
签名/CI、本文/快照及其他冻结包。不得顺手修旧失败、补缺失spec或降门槛。
既有generate派生工程在资源交接后生成，不手改工程/新增target；
确需额外hunk先回主线程审查最小扩围，构建不授额外源码修改权。

## 7. 新终端开工顺序与共享资源

1. 用户明确交本文要求执行；独立取得任务创建授权，形成PRD/design/
   implement与按需context，经审阅才`task.py start`。不激活本线程
   `10-14-native-redesign-shell`或借其他任务/旧UI许可开工。
2. 在main保存开工status、白名单原文/hash/diff和新路径缺席记录到
   `.artifacts/native/tools-connections-review/<run-id>/`。以当前dirty树
   而非HEAD为基线，保全他人修改，不stash/reset/clean或全文件覆盖。
3. 取得共享App入口明确单写交接，每次编辑前重核hash/diff；同期变化
   先协调，不认领他人实现或把旧通过记录当本轮证据。冻结本文/快照。
4. 逐项核对本地工具、锁定依赖和继承环境/脚本/hooks，不自动下载/
   安装/更新补缺，不接真实账号/业务/服务。测试显式fake-home，
   不用真实home作负向路径测试，HOME替换也不当完整数据隔离。
5. 编译前交接SPM `.build`/缓存、core target、生成工程、固定DerivedData、
   Debug bundle/helper/开发签名产物。独立scratch也要审查缓存/共享资源，
   不覆盖用户/未知进程持有的产物；争用/依赖不足记受阻，不强杀/删锁。
6. 先值类型/fixture/纯入口与定向测试，再Store/View和最小App分流。
   检查不得构造/显示窗口、启动生命周期、触发`NSApplication.shared`、
   生产/偏好Store、网络/进程或真实home，不新增真实UI测试。
7. 按§8分别记录静态、定向单测与compile-only；逐项审查实际链接/
   发现/初始化与副作用。旧失败单列，不为了单suite扩大修改/回归范围。
8. 交相对开工快照patch、报告及命令证据后停止，等主线程审查；
   仅修仍有owner的具体hunk，先核对同期增量，不全文件回滚或删历史证据。

## 8. 验证候选与未验收项

**本文交接没有运行产品构建/单测、App/Preview/UI或VM。**
下列都是新终端满足独立开工与逐项审查后才可考虑的候选，不是当前
通过记录或一键执行脚本。命令前重读当时脚本/源码/依赖与资源owner。

### 8.1 当前源码快照与静态义务

- 2026-10-20只读dirty快照：`macos/AeroricApp/Sources/AeroricApp.swift`
  15–79是TaskDeliverables→Communication→WorkArrangements→Skill→Knowledge
  →Evolution→Visual→product。产品delegate在216，AppServices解析在223，
  supervisor启动调用在255/260。新分流在TaskDeliverables之前；
  不据这几个位置声称已穷尽真实host构造链或已运行验证。
- Deliverables入口`NativeTaskDeliverablesReviewEntry.swift` 48–85有
  缺席/存在拒绝和旧7键冲突模式；本次新增它为第8个旧键，所以255，
  不能照搬旧MD的127。COLLABORATION仍仅预留。
- 旧入口冲突语义并不完全统一，尤其Evolution仅检测Visual，
  Visual本身不形成相同拒绝分支；新键缺席时旧解释原样保持，
  不借此次顺手强化/修复旧入口，也不据此允许真实App探测。
- 目录helper在`Design/NativeVisualFoundationReviewView.swift` 63–92，
  保持只读元数据和Caches以下边界；测试全部注入自有临时home。
- `macos/AeroricKit/Package.swift` 24–33默认目录包含新源/现有单测target，
  静态判断无需新增target/改manifest。但filter不会隔离整包编译、
  测试发现/链接/全局初始化，必须重核副作用，零匹配不是通过。
- 不默认跑`native:check shell/native-design/swift-client`、`check all`
  或doctor：可能涉及全套测试、偏好、离屏/窗口装配、假宿主进程/
  socket、固定日志或共享资源，不能推断为只检查本suite。
- `scripts/native/build.mjs`会generate/cargo/Xcode/helper复制/开发签名/
  bundle验证，非无副作用；`--locked`/冻结解析不保证离线，
  不能因没launch就直接运行或宣称无网络。依赖/安全前提不满足就DEFERRED。
- 当前native spec index仍有旧macOS14基线且引用`native-ui.md`缺席；
  按迁移顶部基线报告冲突，不补占位或借样例修deployment/spec/政策。

行号可漂移，开工按符号重新核对；这些是静态事实/义务，不认证他人
增量、任意runner安全、产品能力或UI旅程已通过。

### 8.2 定向suite与View消费至少覆盖

- 新键缺席对旧8键全部256种组合保持inactive；新键存在的`0`/空/
  非法、错环境/缺或非法目录均拒绝不回落。有效新请求与旧8键全部
  **255种非空组合**冲突；其他键`0`不冲突，缺席/存在语义区分。
- fake-home的Caches根、相对/点段、prefix碰瓷、非目录、候选/沿途/
  悬空符号链接拒绝；合法未存在子目录不创建。元数据校验前后无内容/
  业务读取或目录写入，拒绝文案不回显环境/目录/秘密。
- TC1–TC6每组动作/前后投影与受阻/未知分支，场景ID可追溯到View
  控件/Store方法/断言，不只检查方法数量或标题字符串。
- 范围不串、拟选不执行、查看不探测；预授权内不重复问、新技能人工
  同意不被跳过；在途不暗换、回退不改业务历史；停用与确认停止分开。
- 等待恢复重核后可续行、Stop不复活、原结果未知不重放；凭据仅占位
  引用不导出；普通展开不伪造读取/采用/核验/验收，错误/限制不被摘要抹掉。
- 草稿取消、场景重置、多Store实例隔离；只读生命周期元数据/未启动
  策略无窗口，不调用启动/显示/reopen，不创建hosting view。
- 静态核对：App最早分流/旧链保全，DEBUG/Release边界，无产品owner/
  数据/网络/进程路径，系统选择态/共享材质和辅助偏好真实消费到View。
  静态消费证明不等于实际布局、材料输出、键盘/AX或用户交互验收。

### 8.3 前提满足后的候选命令

每条单独审查与留日志：完整命令、无密钥环境差异、退出码、实际匹配
suite/case与passed/failed/skipped、编译产物。不存在/零匹配如实失败，
不用旧方法数或总绿灯代实际本suite结果。

```sh
RUN_ID="$(date '+%Y%m%d-%H%M%S')"
RUN_ARTIFACT="$PWD/.artifacts/native/tools-connections-review/$RUN_ID"
mkdir -p "$RUN_ARTIFACT"

# 仅本地依赖齐备、发现/链接/初始化无窗口/服务、资源交接后，定向新suite。
swift test --package-path macos/AeroricKit \
  --disable-automatic-resolution --skip-update \
  --filter NativeToolsConnectionsReviewTests

# 仅逐项重审脚本/hooks及全部共享产物后：编译/helper复制/开发签名，不启动。
pnpm native:build
```

冻结解析参数不保证离线，也不隔离`.build`/SPM缓存；缺依赖不自动下载，
不能保证检查不访问真实数据/网络时不跑。编译范围与实际执行suite分别
报告；确需scratch/cache方案先审查，不为本suite改checks/floors。

既有generate准备工程、依赖齐备及资源交接后，ReleaseLocal候选仅编译：

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

结合有效DEBUG设置、编译/链接与产物符号检查排除全部新实现，
不能只看`#if`或配置名。无法核实就未验证，不启动Release测试flag，
不运行`swift run`、Xcode Run、App/Preview、UI runner或VM命令。
本地不签名/开发签名编译不是商店签名、审核或发布验收。

### 8.4 必须保留的未验收项

真实类别/范围/项目任务切换、列表详情/菜单/sheet、审阅/停用/恢复/
结果展开体验，窗口关闭重开/多实例/重启，玻璃/圆角/浅深/辅助偏好、
键盘/焦点/IME/AX/VoiceOver与Intel/Apple Silicon真机旅程全部
**NOT RUN / DEFERRED**；真实安装/更新/停止、秘密保护、MCP/四族接线
也未验证。不因编译/离屏证据声称布局通过或双商店首发完成。

## 9. 回报与监工停止点

保存`.artifacts/native/tools-connections-review/<run-id>/report.md`，至少：

```md
# 工具库与连接离线样例01：实施回报
## 用户执行指令、独立任务创建授权/规划审阅/start与main
## 开工dirty原文/hash/diff、相对快照patch、白名单及同期他人增量
## App入口/构建资源单写交接，新键/旧8键/255冲突与缺席旧链保全
## TC1–TC6 Store动作/View消费/定向断言、前后投影与受阻/未知
## 范围引用/选用，安装更新预授权、新技能人工同意与在途/回退边界
## 停用/停止核实、等待重核可续行、Stop不复活、未知结果不重放
## 仅虚构凭据引用，结果预览/来源/错误，读取采用核验验收分层
## 无真实业务/home/凭据/文件/网络/进程，示例意图与外部事实分开
## fake-home只读/不创建，取消/重置/实例隔离、原生选择与辅助偏好
## 原始命令/日志/退出码、匹配suite/case、passed/failed/skipped与产物
## Debug/Release排除、实际OS/SDK/架构、既有失败/依赖/资源阻塞
## 无VM，App/Preview/UI未运行，各运行验收NOT RUN/DEFERRED
## 偏离、残余风险及需用户/主线程决定的问题
```

若接入要求改旧Review/共享Design/生产业务/测试政策、引入真实配置/
秘密/服务、安装工具、启动App/UI/VM，或共享文件/资源owner不明，停止
受影响工作并带最小证据回报，不以“完成样例”为由擅自扩围。
交回后等待本线程审查，不自动进入生产接线、下一阶段或commit/push。
