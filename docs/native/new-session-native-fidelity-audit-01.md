# 新建会话保真校准 01 — 只读差异与接收台账

日期：2026-10-22。配套入口：`new-session-native-fidelity-01.md`。
**这是读码观察，不是对最新原生包的目视评审或产品缺陷最终裁定。**
源码和回执指纹/时间见 `new-session-native-fidelity-sources-01.json`；
如其他执行者已改动，先提交其新指纹/patch，不能据本表恢复旧源码。

## 1. 当前状态与方法确认

用户对先用新建会话跑通原生保真闭环回复“可以”，随后“继续”。
记录为**方法确认**，不拆写成任何新页面/状态批准或运行授权。
包件已准备；当前执行者未 ACK，G0 未完成，G1–G4 均未由本轮验收。
现有执行任务的实施批准独立保留；本包不是其命令通道或接管凭证。

## 2. 基准冲突，先对账再动局部

下表 OPEN 指待对账/验证，不代表应全盘停工、此前用户决定无效或一定要重问。
执行者先在已有回执里找明确适用范围，无法解决才交用户。

| ID | 只读证据与影响 | 关闭条件 | 当前 |
| --- | --- | --- | --- |
| D01 批准账不一致 | `main-shell-reconstruction-review-v1.md` §1/§6仍全PENDING；既有任务 `research/native-implementation-approval-2026-10-22.md` 记四屏已确认。不能把任一记录机械推广到所有组合 | 现有 owner 指定 MSR01 实际采用稿/状态及回执来源；区分文字交接、视觉范围、运行例外，并对接其审批账；本线程不替它改账 | OPEN |
| D02 壳几何 | 后续设计 §3为25/7/6、1260下240/714/280；读取时 `NativePanelGeometry` 仍18/8/8，`WorkspaceRootView.body`仍消费旧值。若交付包消费同一快照，则不会自动成为新预算 | 绑定采用版本，具名变体/单一owner及普通入口实消费；旧review样本不改。之后仍需指定包实际核对 | OPEN，静态候选差异 |
| D03 输入圆角 | 已获审V6为桌面32/窄28；MSR01 `style.css` 的 `.composer` 为16，提示词高108。后续短回执未逐项写明取消原圆角；不能凭“同结构”推定相同 | 现有 owner 证明16属于已审替代范围，或明确保留32/28；无适用回执时只把该局部交用户，不取折中值 | OPEN，基准冲突 |
| D04 标题/字体 | V6 CSS宋体标题为 `clamp(30px, 3.05vw, 44px)`、视口≤760px时32；读取时原生 `NewSessionComposer` 按内容宽<620选32、否则40，两者不是同一公式/坐标。后续方案 §4为Mono/中文回退与小标题16/22；`AeroricFont`已出现，composer尚用宋体路径 | 确认采用范围、公式/断点映射与实际View消费；不把原生固定40追认为V6批准值，不误报token缺失，系统字体例外单列 | OPEN，基准与消费对账 |
| D05 宽度与坐标 | 后续可读列850；`NewSessionGeometry.pageMaximumWidth=930`由composer padding后的容器消费。930与850不在同一层，不能直接判差80 | 记录壳bounds、中央、滚动视口、padding、输入边缘；在相同层验算/核对850目标，不靠修改测试数字结案 | OPEN，尚未实测 |
| D06 Harness/系统效果 | 已审V6要求四图标、当前全名，其余图标；MSR01字符串稿是简化Harness单选，不能证明产品图标/滑块。已有 `NativeHarnessPicker` 系统桥接与资源设置，但没有本轮运行证据 | 保留用户明确图标/名称需求，指明后续适用修订；具名核对四切换、长名、系统选择和辅助设置。确实无法等同交局部方案，不手绘active底 | OPEN，实际效果未验 |

标题/轨道颜色也必须随 D04 的确切版本逐项绑定。当前composer里前后半用
`.dsText`、句点`.dsTextSecondary`只是一条源码观察；若后续已批准标题改版，
不能直接用旧青/琥珀标题宣告违规。能力/项目/强调色仍需沿采用版本核对。

长文交接风险已复核：冻结统一交接46,640 bytes；
`.trellis/scripts/common/config.py`默认单文件32,768，
`common/task_context.py`对超限提示注入将截断。
**不证明某个执行者一定漏读**。本包改用短入口＋按需原文，不改限制或旧包。

## 3. 组件映射与能证明到哪层

下列符号为读取时路径；重构可改名，但执行者须提交替代映射。

| 可见区域 | 现有原生消费位置 | 本轮证据层级 |
| --- | --- | --- |
| 普通新建入口 | `AeroricApp` / `WorkspaceRootView` → `TaskWorkspaceView.composer` → `NewSessionComposer` | 调用源码，不是已运行 |
| 框外项目/Harness | `NewSessionComposer.contextRow/projectMenu/harness` | 源码布局意图；父容器与实际图文未测 |
| 输入边界/内容 | `NewSessionComposer.input`、`NewSessionGeometry.inputRadius/promptHeight` | 纯值与modifier，不是实际高度/IME |
| 配置/权限/模型/强度 | `NewSessionComposer.configurationControls`及四个Menu | 控件与typed注入，不是模型目录/API可用证明 |
| 能力胶囊/资料/配方 | `NewSessionComposer.capabilities/materialReferences/recipe` | View源码；后台能力不因此启动或可用 |
| 选择与品牌资源 | `Design/NativeSelection.swift` 的 `NativeHarnessPicker` | 公共系统控件装配，不是液态滑块/AX已通过 |
| 字体/颜色/几何 | `ThemeTokens`、`NativePanelGeometry`、`NewSessionGeometry` | token及消费者需要一起查；存在不等于整页使用 |
| 项目草稿与往返 | `NewSessionDrafts`、`TaskWorkspaceView` 的draftBook/scope | 状态与typed路径；本轮未测试真实往返 |

此前实施报告中的78项等非窗口结果保留原批次证明范围，本轮不重跑、不转记成
保真通过。只读独立审查覆盖过普通调用链、几何和部分fixture/离屏测试定义；
不作浏览器、原生材质、真实键盘/IME/AX或业务端到端结论。

## 4. 下一执行者最小交回件

先回 G0，而不是再交一个HTML或自动打开App：

1. 当前任务/owner、采用MSR01或V6的确切叠加范围、源指纹与D01–D06逐项依据。
2. 当前实际源patch/分工；尚无产物就明确写无，禁止使用旧运行实例冒充。
3. 接下来只做哪一块、调用哪个既有组件、哪些参数仍待核对。

G0对齐后，实施者按其已有授权推进，逐项交差异；监工先查普通接入与已知
差异，再申请具体原生展示或核对用户自供截图。不要求用户先反复审批同一原则。
未知/受阻与未验证要留下；**“基本一致”“编译通过”“文件齐全”均不能关闭视觉账**。

审批/基准确认后若仍有系统差异，需说明具体位置、原因和替代效果，由用户
决定；不把材质差异当作所有布局问题的通用豁免。最终原生基准只覆盖获确认的
具体包、页面和状态，不追认其他平台/管理页。

本轮无源码修改、产品检查/构建/打包/启动，所有真实UI均
**NOT RUN / DEFERRED**；没有浏览器、VM或真实账号/Agent/API操作。
本台账是初始快照，未来更新另记批次，不重写历史证据或取消暂停。
