# 任务成果与交付审阅样例 01 — 外部执行模型交接

版本：1.0 · 2026-10-19 · 范围批准：Q82（用户回复“认可”）。
依据：Q76–Q81，见 `task-deliverables-design-checkpoint-01.md` v0.1。

**仅交接独立SwiftUI＋苹果原生Liquid Glass离线样例。
不创建VM，App/Preview/真实UI运行仍暂停。本文不是立即开工或运行许可。**
用户明确把本文交新终端并要求执行后，须满足§7的独立任务创建授权、
计划审阅/start及文件/构建资源交接；只实施样例并做经审查的非窗口检查。
本线程不实施页面，不验收布局/运行，不修改其他已交接阶段包。

## 1. 政策与数据隔离先于实施

1. **不创建、克隆、导入、下载、恢复、启动、重建或准备替代VM，
   不安装/重装VM工具，不修复已退休VM路径。**旧任务/旧包许可不继承，
   VM不是交付条件，不删除历史测试证据或维护退休资源。
2. App/Preview/真实窗口及UI自动化继续PAUSED。不预热或启动App，
   不运行XCUITest、`pnpm native:ui-test`、`pnpm native:check accessibility`、
   GUI snapshot或等价输入/窗口runner。不调用runner“检查暂停是否生效”，
   不给立即launch命令，不用隐藏窗口、其他Space/显示器绕过。
3. 仅逐项审查的无窗口/无输入单元、fixture、静态、compile-only检查
   可以继续；名称叫unit不代表整套安全。用虚构数据、隔离临时根及
   显式注入的fake-home，不接真实数据/业务/凭据/Agent home。
4. 真实玻璃/圆角衔接、浅深/辅助偏好输出、按钮/菜单/sheet、键盘/
   焦点/IME、AX/VoiceOver、窗口/生命周期与用户旅程验收仍为
   **NOT RUN / DEFERRED**。编译、方法数、离屏装配不替代这些证据，
   不删断言/降floor补绿，不把暂停或零匹配当通过。
5. 恢复真实UI须新的明确授权、无VM执行方案与政策审查；不因此取得
   新账号/远程桌面、TCC/系统设置、CI采购、提交推送或发布许可。

入口隔离、文件owner、依赖或验证副作用不明确时，保留证据并停止
受影响部分。资源争用应报告，不强杀持有者、删锁、换真实数据、
建worktree/VM或自动下载安装工具补缺。

## 2. 目标、必读源与非目标

在现有macOS App target增加独立、最早DEBUG显式审阅入口，
以可重置虚构内存展示任务成果的收录/整理、交付版本、被动查看、
反馈交办及同任务返工。必须有真实SwiftUI View、Store动作与前后投影，
不是标题清单或仅有`#Preview`；但本阶段不得启动App或Preview观看。

仓库：`/Users/macbook/Downloads/同步空间/LYX/Aeroric`。按需读取：

- `AGENTS.md`、`.trellis/workflow.md`、`docs/native/ui-testing-policy.md`；
- `MIGRATION_PLAN.md`顶部Apple-only基线、ADR-039与ADR-001原生边界；
- `.trellis/spec/aeroric-macos/frontend/index.md`；
- 本文与 `docs/native/task-deliverables-design-checkpoint-01.md` v0.1；
- `.trellis/tasks/10-14-native-redesign-shell/research/harness-task-deliverables.md`
  的已确认节；历史“待确认/未生成”段落保留当时状态，不覆盖最新确认；
- 需要回溯时只读同目录`harness-task-collaboration.md` Q32–Q34、
  `harness-task-planning.md` Q37/Q38及`harness-inflight-communication.md`
  Q69–Q74，不整库注入，不重扫harness。

平台要求遵循迁移顶部基线：最低macOS/iOS/iPadOS26，Mac目标
Apple Silicon/macOS27与Intel/macOS26，不建立Intel/macOS27格。
不借样例修现有更旧deployment配置、缺失spec、签名、CI、商店或iOS；
冲突回报。样例仍是Mac有限交付，不缩减总体双原生客户端目标。

### 明确不做

- 不接Codex/Claude Code/OMP/DSH、自定义Agent、模型/API/MCP/PTY、
  反思/治理、host/UDS/HTTP或其他后台服务，不生成真实prompt/argv。
- 不构造AppServices、生产Store/AeroricClient或读取真实任务/项目/
  会话历史/配置/权限/凭据/managed-home；不解析ANSI或原始JSONL。
- 不扫描/监听/导入/复制/哈希真实项目文件，不实现快照、Git提交/
  worktree/分支、工作区回滚、数据库/持久化、容量清理或备份恢复。
- 不用Quick Look/外部App/URL/WebView、文件/保存面板、网页预览服务、
  执行/安装器或外部内容解码充当只读查看；不加载远程资源/上传/发布。
- 不实现生产锚点迁移、语义影响分析、可信来源/权限强制、消息队列、
  幂等重试、返工调度/会话恢复；不用Timer、后台Task/轮询或真实clock
  伪造Agent/检查进展，不为样例创建真实任务或复制业务DTO。
- 不把样例Store作为共享业务writer；生产owner仍在Rust core/host。
  不自研第五个编码执行器，不改四家族身份/现有路径、OMP原始PTY/TUI、
  `--mode`限制/managed history根，不复活rpc-ui或读取用户`~/.omp`。
- 不替换正式首页/主导航/TaskWorkspace、其他Review样例或共享组件；
  不改Rust/core/protocol、iOS、legacy、发布/CI/签名，不commit/push/tag。

## 3. 信息层级与样例动作

以下为样例建议，不冻结正式首页或产品布局：

1. **顶部**：虚构任务名，常显“离线成果示例 · 全部为虚构数据”；
   场景选择、重置、样例外观切换，不伪装真实运行/连接/计费状态。
2. **成果清单**：标题/类型、阶段或最终提交、来源工作包/提交者、
   版本与检查/未验证提示。临时内容不全收，空清单明确无已提交成果。
3. **版本与阅读面**：选定交付、关联版本、当前工作文件示意分别表达；
   只读正文/diff片段/图片示意，缺内容时给限制，不静默换版。
4. **依据与反馈**：原提交/固定检查范围、未运行/风险、原意见/来源位置、
   清楚收件人、草稿与明确交办动作、处理说明/关联新交付。
5. **本轮工作与历史**：正常完成/历史验收对应版本，与当前返工/阻塞/
   暂停Stop区分。可折叠无关详情，不建立“提交→全绿验收”单线进度。

可横纵重排，不强制三栏。初始1260×860pt、兼顾1000×640及400×600
为设计目标；窄窗可滚动，不压掉正文、取消/发送与未知说明。
用系统标准窗口，不画私有圆角/无边框外壳；当前不运行尺寸/窗口验证。

### 意图与外部事实分开

- “整理/补充示例成果/写意见/发送修改要求/发起返工”仅改变局部内存。
  取消不应用；普通草稿不发消息、不重开工作，不把收录/整理当用户验收。
- “成员已提交”“交出/投递未知”“Agent处理说明”“核对结果/历史用户
  接受”“条件恢复”等只能由清楚标注的**载入示例事件**或预设事实表达。
  不是本机采集/Agent消费/真实核验，事件入口不可被普通按钮暗中调用。
- 对已结束本轮工作的任务，“发起返工”自身即交办，不再另发一次或
  另批开工。条件满足的示例事件应让已交办轮次自动接续；不以再次
  点击“开始”弥补逻辑。等待/请求/实际进行仍按示例事实分开。
- 普通“发送修改要求”不越过暂停/Stop；明确恢复是独立意图。覆盖内
  不重复索权，但曾有Full Access不等于当前仍有效，也不代需求决策、
  监督核对或新技能人工同意。
- 运行/安装/外部打开按钮仅记相应“未执行”的内存意图与示例限制，
  不显示真实成功toast，不执行命令、启动解析器/服务或打开任何内容。
  来源依据只展开样例内存，不用真实文件链接/外链。

## 4. TD1–TD8有限场景与可核对结果

编号仅为Q82五类关系的细分，不是八个新模块/来源、生产wire状态或
通用执行状态机。每项需真实Store方法、View消费和前后断言；
不能只摆固定标题/成功常量。样例标识、类型名可在白名单内细化，
但不新增生产能力或扩大动作范围。

### TD1 明确提交、集中收录与用户整理（Q76/Q77）

预置虚构软件任务、两个工作包、阶段文档/代码差异等明确成果，以及
未作为成果提交的临时日志说明。提交/失败记录与清单展示分开。

- 载入阶段提交事件后汇集明确提交项，不要求逐条“批准收录”或等最终
  完工；必要日志可作为依据，明确交付的日志可作为成果。
- 用户通过固定fixture选择补充“示例成果”，不使用文件选择器/磁盘导入。
  标题/说明编辑取消不应用；确认只改展示元信息，不改原提交/内容。
- 从清单移出不删内容/提交/失败依据，不标用户已接受；普通缓存/临时日志
  不全塞入清单。空清单不编造安装包或成果。

断言提交记录与内容保全、整理前后/取消差异、临时内容未默认收录。

### TD2 交付版本、当前文件与检查适用性（Q78）

预置v1/v2不同内容/来源与检查范围、另一个当前工作文件示意；diff
附固定基线说明。它们不是磁盘文件，也不是生产快照。

- 切换v1/v2仅切阅读投影；旧记录内容与当时检查不被覆盖。
- v1的通过只适用它的范围，v2修改影响点显示待核对；预设无关检查
  可保留其明确适用范围，不做“一处变化全部作废”的假算法。
- 某交付仅留引用/无法回看时显示限制，不用当前内容、路径或hash
  补成副本；没有基线不能声称可重建完整代码/项目。

断言选定内容/检查关联、旧内容不变、当前文件另看、不可回看不换版。

### TD3 只读查看与明确操作意图（Q79）

预置可读文档/源码或diff、图片示意，以及仅有类型/来源说明的安装包/
未知格式。不调用真实解码、Quick Look、浏览器或文件/网络API。
正文可用`Text(verbatim:)`或受控固定布局，外链不可点击外部打开。

- 查看默认对应选中版本，不编辑、不运行宏/脚本/构建/测试或启动服务。
- 未知格式显示类型/版本/来源/限制，不能用执行它来假装预览。
- 运行、安装、外部打开分别表达未执行的意图，展示固定示例授权/能力
  限制；不改权限或把查看/意图记录当执行成功、检查/用户验收通过。

断言只读内容未变、意图类型独立、没有实际执行/安装/外部打开副作用。

### TD4 反馈锚定版本与位置，不硬套新版（Q80）

对某段固定文档/diff片段建立意见，保留成果ID、选定版本、可靠位置/
原文依据；无位置时允许整体反馈，不能伪造行号。

- 切换成果/版本后，既有意见与草稿的原关联不随当前选择静默漂移；
  新建意见可明确用新选择，未发送内容可取消，不偷偷改原意见对象。
- 预设v2位置对应已知/未知两种场景，未知保留v1依据并提示待核对，
  不以行号相同或旧行消失推定问题已修复。

断言原版本/位置关联稳定、不可映射时的明确限制及内容没有被批注修改。

### TD5 草稿、明确发送与收件人（Q80/Q69–Q74）

默认协调负责人，可明确定向成员；意见、成果版本/位置与收件人可见。
输入/选择变化只改草稿，不默认广播，不靠当前查看的成员静默换收件人。

- 空意见不交办；存草稿/取消编辑不发送。明确发送后形成一次局部交办
  意图，保留原内容和关联，不将点击当已交出/消费/采纳。
- 用示例事件表达交出或交付未知；未知不当成功/失败，不盲重发。
  已交出后的更正沿既有追加关联，不重写历史；不重做Q75完整队列引擎。
- 非已结束工作按既有补充/下一轮语义交办，不强行打断工具；
  具体安全边界只为固定场景事实，不解析CLI或推测四家族能力。

断言草稿与交办区别、取消/空草稿、收件对象不串、未知与更正关系。

### TD6 处理说明、新交付与核验/验收区别（Q78/Q80）

从TD5意见载入“Agent说明已修改”事件，再载入关联v2与明确范围的
“示例核对结果”。事件均醒目标注，不当真实Agent或测试输出。

- 原意见下可回到说明、新版与旧依据；自报修复不自动产生核验通过。
- 新交付可关联该意见，但不会使全部意见自动结案；代码行消失或v2
  出现不证明已经落实，位置未知仍保留。
- 历史“用户已接受v1”只由明确预设/示例事实表达，不新增生产验收
  操作或因看v2/技术核验通过就变成用户接受v2。

断言发送/自报/核验/用户接受各有依据与适用版本，旧记录不改写。

### TD7 已结束工作，原任务内发起返工（Q81）

预置正常完成或交付待验收的虚构任务与v1、原检查/历史接受记录。

- 草稿/查看/旧批注不重开；明确“发起返工”发送选定修改要求并在
  同任务内关联新一轮，不要求再确认开工或另建任务。
- 当前本轮与旧完成事实分开；后续v2关联返工/意见，旧v1/检查保留，
  不自动沿用旧通过，不修改无关成果或回滚当前工作文件示意。
- 明显新目标用固定场景提示可另建关联任务，不自动创建真实/新样例
  任务、不擅改目标/验收标准，不实现自然语言分类器。

断言同任务归属、交办次数、历史/当前轮次与范围区别；
同任务标签不等于旧PTY/会话恢复，不静默换执行器/模型别名。

### TD8 接续条件、暂停/Stop与局部阻塞（Q81/Q73）

为已经交办的返工预置当前授权/资源/监督条件不足、可接续以及暂停/
Stop等固定状态，不以计时器或后台轮询驱动。

- 缺条件只显示相应等待，不假装已执行；载入条件满足事件时，已交办
  轮次自动进入样例允许的接续投影，不再次发送/开工确认。
- 暂停/Stop在条件满足和队列非空时仍阻止接续；旧消息不唤醒。
  显式恢复只撤销对应样例暂停意图，其他缺条件仍有效，不复活失效权限。
- 原执行未知先待核实，不能仅凭已完成标签启动重叠替代写入；
  局部受阻不推翻无关成果，不声称Stop请求等于原进程已停。

断言条件恢复和Stop优先、失效授权不复活、当前/历史事实独立；
不实现生产隔离/信号/监督/审批或共享写入机制。

## 5. 原生装配、内存owner与DEBUG入口

### 共享样式与辅助偏好

- 复用`NativePanelFoundation`、`GlassPanel`、`NativePanelGeometry`、
  `NativeIconButton`及NativeSelection，不另造主题/选择组件系统。
  SwiftUI/AppKit Material与受支持交互控件的苹果原生Liquid Glass
  沿现有availability门控；不拿半透明RGB假玻璃或整篇正文套glassEffect。
- A样式：短按钮/输入/选择项胶囊、独立图标无永久圆环，正文/差异用
  可读语义阅读面；连续材质基底覆盖圆角缝隙，不贴局部三角补丁。
- 相邻互斥的成果、版本、收件人、用途/场景等用系统Picker/List/Menu
  和NativeSelection；不手画active背景/下划线。独立开关/展开按既有例外。
- 语义accent/success/warning/danger配文字/图标，不靠颜色区分全部
  事实；glass/classic独立于interactionMode、任务/模型/API和权限。
  减少透明度/增强对比/减少动效实际消费到View；示例注入只加强系统
  偏好并标明示例值，不改系统设置或UserDefaults。
- 叶子AX identifier用`taskDeliverablesReview.`前缀及可读label，
  保留原生键盘、取消/确认及长文/窄窗滚动设计；不由静态标识宣称
  实际AX/VoiceOver、Tab/焦点/IME或玻璃输出已通过。

### 内存owner与隔离

样例根持有`@MainActor @Observable` Store，新窗口新Store。值类型、
fixture、内容、草稿/反馈、交办意图/示例事件、版本/返工关系均局部
内存，不共享生产类型/单例。编辑取消不应用，重置还原全部fixture，
不同实例不串标题/移出/版本/草稿/收件人/返工/暂停与事件。

不使用AppStorage/UserDefaults、文件/数据库写、真实clock、后台Task/
Timer、网络/进程/socket/业务回调；例外只允许入口的只读目录元数据
检查与测试自有临时fixture目录，不读内容或触及真实home。
“保留版本”在本样例是不可被普通动作改写的内存提交记录，不是
生产不可变存储、备份恢复、权限或并发安全保证。

### 新入口契约（本文交接时尚未实施）

新增键 **`AERORIC_NATIVE_TASK_DELIVERABLES_REVIEW`**。
新模型/fixture/Store/View、入口决策、拒绝页及生命周期全部DEBUG限定；
在`AeroricEntryPoint.main`最早DEBUG分流，先于任何现存Review/
产品App/delegate/services的构造。不是产品已初始化后隐藏业务View。

| 请求 | 结果 |
|---|---|
| 新键缺席 | inactive，保持旧链及旧键解释完整不变 |
| 新键`1`，`AERORIC_ENV=ui-test`，显式`AERORIC_DATA_DIR`通过既有严格helper，其他审阅键无`1` | 仅本样例分支，先于任何产品/旧Review owner构造并返回 |
| 新键存在但为`0`/空/非法，环境/目录不符，或其他审阅键任一为`1` | 仅本样例只读拒绝分支，绝不回落产品或按顺序选另一个样例 |

其他七键含已存在与交接预留键，不能按旧MD的六键集合漏算：
`AERORIC_NATIVE_TASK_COMMUNICATION_REVIEW`、
`AERORIC_NATIVE_WORK_ARRANGEMENTS_REVIEW`、`AERORIC_NATIVE_SKILL_REVIEW`、
`AERORIC_NATIVE_KNOWLEDGE_REVIEW`、`AERORIC_NATIVE_EVOLUTION_REVIEW`、
`AERORIC_NATIVE_VISUAL_REVIEW`、`AERORIC_NATIVE_COLLABORATION_REVIEW`。
新键存在时其他键`0`不冲突，缺席不等于`0`；预留不证明已实现。
开工若出现更多同期审阅键，先报主线程核对入口/冲突最小扩围，
不改旧Review的冲突语义或重排它们。

复用`NativeVisualReviewEntry.isIsolatedDataDirectory`只读Caches子目录
helper，测试显式注入fake-home，不改helper或造宽松路径prefix。
合法未存在子目录可通过但不创建，拒绝不回显真实路径/环境值/密钥。
helper不证明Caches以上全部无符号链接、OS沙箱、权限强制或偏好完全隔离；
`ui-test`只是入口标签，**不是UI运行许可**。

样例/拒绝页有专属无业务生命周期与唯一窗口标识，只管自身标准、
不恢复状态的窗口；不复用产品/旧Review delegate、业务菜单/偏好清理，
不全局扫描窗口、激活/关闭其他实例或加焦点补丁。关闭释放根/Store，
重开新建；不宣称OS无任何窗口持久痕迹。当前只实施/静态核对路径，
不调用生命周期启动/显示/重开方法或`NSApplication.shared`。
ReleaseLocal排除全部新实现，仍走原产品链，只compile-only/核对产物，
不得启动Release或测试flag。

## 6. 产品/测试文件白名单

最大允许范围如下，不要求全改；开工已存在的新路径须核对owner，
不覆盖他人实现。类型可在五份新源内拆分，不额外造目录/target。

| 路径 | 允许职责 |
|---|---|
| `macos/AeroricKit/Sources/AeroricUI/TaskDeliverablesReview/TaskDeliverablesReviewModels.swift`（新） | 样例值类型、版本/检查/反馈/返工关联，不定义生产DTO |
| 同目录`TaskDeliverablesReviewFixtures.swift`（新） | TD1–TD8固定虚构内容、来源、事件与限制 |
| 同目录`TaskDeliverablesReviewStore.swift`（新） | 局部内存owner、动作/草稿取消/重置与有限投影 |
| 同目录`NativeTaskDeliverablesReviewView.swift`（新） | 原生成果/版本/只读正文、反馈与返工，实际消费状态/动作 |
| 同目录`NativeTaskDeliverablesReviewEntry.swift`（新） | 只读入口决策、拒绝内容、专属无业务生命周期 |
| `macos/AeroricApp/Sources/AeroricApp.swift` | 仅最早DEBUG分流与新样例App装配，旧链/产品逻辑不动 |
| `macos/AeroricKit/Tests/AeroricUIUnitTests/NativeTaskDeliverablesReviewTests.swift`（新） | 经逐项审查的无窗口纯入口/内存/隔离检查 |

独立任务的PRD/design/implement/context及本次证据另依Trellis管理，
不改整壳/迁移等其他任务metadata/context，不整库自动注入。

**只读/不在白名单**：所有已有Review/测试、共享Design/ThemeTokens/
NativeSelection、Package.swift、project.yml/xcconfig、core/protocol/
业务/iOS/legacy、`macos/AeroricUITests/`、`scripts/native/`的runner/
policy/checks/floors、签名/CI、本文和其他冻结阶段包/快照。
不顺手修旧失败、降floor/删断言。既有generate产生工程派生物可在
资源交接后生成，不手改工程或新增target；确需额外hunk先回主线程
审查最小扩围，构建命令不授额外源文件修改权。

## 7. 新终端开工顺序与共享资源

1. 确认用户明确交本文要求执行；遵守独立任务创建授权，形成自己的
   PRD/design/implement及按需context，经审阅后才`task.py start`。
   不激活本线程整壳任务，不借其他终端状态或旧VM/UI批准开工。
2. 在main保存开工status、白名单原文/SHA-256/diff及缺席记录到
   `.artifacts/native/task-deliverables-review/<run-id>/`。以当时dirty树
   而非HEAD为基线；冻结本文/快照与其他交接包，不stash/reset/clean。
3. 取得App入口共享文件的明确单写交接；每次编辑前重核hash/diff，
   有同期改动先协调，不覆写、不把他人的实现/通过记录算成本轮。
4. 逐项核对本地工具/冻结依赖齐备、继承环境与命令副作用；不自动
   安装/下载/更新工具或访问真实账号/服务补缺。HOME改值不是完整隔离，
   测试必须显式传fake-home，不拿真实home作破坏性负向测试。
5. 编译前交接SPM `.build`/缓存、core target、生成工程、固定DerivedData、
   Debug bundle/helper/开发签名产物；不覆盖用户/未知进程持有的bundle，
   不强杀/删锁。资源不足就记相关检查DEFERRED，不以VM/worktree绕过。
6. 先编写值类型/fixture/纯入口决策与定向测试，再Store/View、最小App
   分流。测试不得构造/显示窗口、触发`NSApplication.shared`、生产/
   偏好Store、进程/socket或真实home。只能检查未启动生命周期策略，
   不调用其启动/显示/重开方法，不新增真实UI用例。
7. 分列静态、定向单测和compile-only证据；按§8逐项重核实际目标、
   测试发现/链接及副作用。既有失败单列，不顺手修或扩大回归范围。
8. 交相对开工快照patch与报告后停止，等主线程审查；只修自己仍拥有
   的具体hunk，先核对后续改动，不全文件回滚、不删除历史证据。

## 8. 验证：候选检查，不是当前通过记录

本文交接没有运行以下构建/单测，也没有启动App/UI。未来每条命令
必须重核当时源码、环境/依赖、资源owner和实际执行范围；不凭旧绿灯、
case名或过滤器推断安全。

### 8.1 必须静态核对的接入与副作用

- 2026-10-19只读dirty源码快照：`macos/AeroricApp/Sources/AeroricApp.swift`
  15–69当前为Communication→WorkArrangements→Skill→Knowledge→Evolution
  →Visual→product。产品App构造delegate/AppServices在188–200；
  新分流须在Communication之前，缺席保持整个旧链。Communication
  已有源码不等于本线程验收；Collaboration仍只是冲突预留，
  Q75冻结MD的“最早WorkArrangements/Communication未实施”不是当前事实。
- `NativeTaskCommunicationReviewEntry.swift` 48–79有纯决策/home注入，
  字符串条件不符先拒绝再考虑目录；只参考模式，不修改旧Review。
  目录helper在`Design/NativeVisualFoundationReviewView.swift` 63–92，
  检查Caches以下而非根及所有上层，不自动创建目录。
- `macos/AeroricKit/Package.swift` 24–33按默认目录收新源/单测，
  静态推断无需增加target；定向suite仍可能编译整包，
  过滤不隔离测试发现/链接/间接初始化；没有匹配测试必须报告失败。
- 不默认跑`native:check shell/native-design/swift-client`或`check all`：
  可能涉及整个测试组、离屏视图/偏好域、临时UDS/假宿主进程及固定日志，
  不是只测本suite。doctor也不是无副作用的必跑项。
- build链可能generate/cargo/Xcode/helper复制/开发签名并改共享产物，
  需重读脚本/hooks。不能因未见launch就称它无副作用；不扩大到打包/
  发布，不改脚本/floor来允许样例。
- front-end spec的旧deployment与当前迁移基线不同，缺失引用不补
  占位；冲突报告，不借样例修规范/部署或降低产品要求。

以上行号可漂移，开工按符号重核；只是检查义务与静态快照，
不认证任意现有runner安全或他人增量已验收。

### 8.2 新suite与View静态消费至少覆盖

- 新键缺席对旧键各值/组合inactive；新键存在的`0`/空/非法、错环境/
  缺目录/非法目录均拒绝不回落。合法请求与其他七键任一为`1`及
  **127种非空组合**均拒绝；其他键`0`不冲突。旧链仅源码/纯决策核对。
- fake-home只用本测试临时目录：Caches根、相对/点段、路径前缀碰瓷、
  非目录、候选/沿途符号链接拒绝；合法未存在子目录接受但不创建，
  留前后目录清单。helper能力限制如§5，不破坏真实home/系统权限。
- TD1–TD8逐一映射真实Store方法→View消费→前后断言，覆盖取消/
  空草稿/不自动收录/移出不删提交、版本/当前区别/不可回看不换版、
  操作意图未执行、位置稳定/未知不硬套、收件不串、未知/更正、
  自报与核验/用户接受区别、同任务返工/历史保全/新版本不继承通过。
- 条件恢复自动接续已交办意图，但Stop/暂停/失效授权等仍阻止；
  无关成果不被全盘重置，原执行未知不假装已停/安全交接。
- 多实例不串数据；编辑取消/重置还原fixture，标题/移出不改原记录，
  选中新版不漂移旧意见的来源、草稿或历史验收适用版本。
- 静态消费证据包括App最早分流/旧链保持、DEBUG/Release排除、
  View真实绑定、共享原语/原生选择/语义色与辅助偏好、安全只读呈现、
  无业务依赖/副作用路径。Store过了而View没有消费不能交差。

若提出离屏装配另逐项审查；不得创建NSWindow/`NSApplication.shared`
或生产/偏好服务。不能确定就DEFERRED，不为装配启动App。

### 8.3 前提满足后的候选命令

每条分开审查、留独立日志：完整命令、环境差异（无密钥）、退出码、
实际suite/匹配数、passed/failed/skipped及编译产物；不存在记未存在，
零匹配不是通过。下面**不是一键盲跑脚本，也不是当前线程执行记录**：

```sh
RUN_ID="$(date '+%Y%m%d-%H%M%S')"
RUN_ARTIFACT="$PWD/.artifacts/native/task-deliverables-review/$RUN_ID"
mkdir -p "$RUN_ARTIFACT"

# 本地依赖齐备、逐项审查无窗口/服务、资源已交接后，定向新suite。
swift test --package-path macos/AeroricKit \
  --disable-automatic-resolution --skip-update \
  --filter NativeTaskDeliverablesReviewTests

# 逐项审查当时脚本/hooks并交接所有共享产物后，仅编译/开发签名。
pnpm native:build
```

冻结解析参数不保证离线/无网络，不隔离SPM缓存或`.build`。本地依赖缺失
就不跑，不自动下载补齐；独立任务可先审查scratch/cache隔离方案，
但不能复制真实业务/home。suite过滤不等于运行全回归，编译范围与执行
suite分别报告。不为单suite改checks/floors。

既有generate准备工程并取得资源交接后，ReleaseLocal仅compile-only：

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

有效DEBUG设置、编译/链接与产物符号共同核对新实现排除，不能只看
`#if`或配置名；无法确认就未验证。不运行`swift run`、Xcode Run、
App/Preview、UI runner或任何VM命令。不启动Release验证flag。
仅本地不签名编译不是商店签名/审核，开发签名不冒充发布验收。

### 8.4 必须保留的未验收项

真实清单/版本切换与阅读、菜单/sheet/草稿编辑/收件操作、返工体验、
窗口尺寸/关闭重开/重启、玻璃/圆角缝隙/浅深/辅助偏好、键盘/焦点/
IME/AX/VoiceOver、Intel/Apple Silicon真机旅程全部NOT RUN/DEFERRED。
不产假截图/通过记录，不因编译/离屏证据宣布布局或双商店首发完成。

## 9. 回报与监工停止点

保存`.artifacts/native/task-deliverables-review/<run-id>/report.md`，至少：

```text
# 原生成果与交付样例01：实施回报
## 用户执行指令、独立任务创建授权/审阅/start与main
## 开工dirty原文/hash/diff、相对快照patch、白名单与同期他人增量
## 入口/构建资源单写交接；新增键/127冲突组合与旧链保全
## TD1–TD8 Store方法/View消费、前后结果、失败/未知与未覆盖分支
## 明确收录/整理不删提交，版本/当前文件/检查范围/不可回看限制
## 查看与运行等意图未执行；无真实文件/网络/进程/业务写入
## 意见版本/位置/收件人、草稿/发送、自报/核验/用户接受区别
## 同任务返工、历史保全、条件接续/Stop优先及局部等待
## fake-home目录只读/不创建、多实例/取消/重置、辅助偏好与原生选择
## 原始命令/日志/退出码、实际suite和passed/failed/skipped、编译产物
## Debug/Release排除、实际OS/SDK/架构、既有失败/依赖/资源阻塞
## 不创建/恢复/启动VM；App/Preview/UI未运行，各运行验收DEFERRED
## 偏离、残余风险与需用户/主线程决定的问题
```

主线程按开工快照、本次patch、实际View消费与新鲜原始日志审查；
模型自报、文档一致、单测通过均不等于用户验收。产物标记“未启动/
未经UI验收”，不建议用户直接打开绕过暂停。缺条件可交局部成果与
具体阻塞，不声称全部完成或沿用其他终端通过记录。

交回后停止，不扩真实成果存储/消息/返工恢复、正式首页或UI/VM范围；
不擅改权限/审批/验收规则或安装技能。是否交新终端执行由用户决定。
