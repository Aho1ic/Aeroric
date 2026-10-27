# 主壳深重构 V1 — 四屏离线设计审阅

日期：2026-10-22，Asia/Shanghai。Trellis：`10-21-approved-ui-native-integration-01`。

**书面整稿 DESIGN_APPROVED；MSR01–MSR04/V1 资产 PREPARED，视觉均 PENDING。**
用户对完整书面设计及离线制作回复“确认”，随后“继续”；本次只执行该范围。
没有打开浏览器、预览服务、App/Preview、UI/AX/XCUITest 或 VM。
制作和静态通过不是视觉批准，也不是 SwiftUI 原生交接的批准。

设计：[完整书面方案](main-shell-reconstruction-design-v1.md)。
独立入口：[离线主壳稿](../../.artifacts/native/mockups/main-shell-reconstruction-v1/index.html)。
说明：[README](../../.artifacts/native/mockups/main-shell-reconstruction-v1/README.md)。
来源：[source-manifest](../../.artifacts/native/mockups/main-shell-reconstruction-v1/source-manifest.json)。

## 1. 逐屏门禁

| 屏幕/版本 | 画出的内容 | 稿件 | 用户视觉回执 |
|---|---|---|---|
| MSR01/V1 新建任务 | 完整三栏、项目「＋」、V6式输入/配置/配方/能力/更多配置；全局无项目变体 | PREPARED | PENDING，尚未展示 |
| MSR02/V1 当前任务 | 标题、任务描述、完整虚构原始输出片段、显式状态来源、续聊草稿 | PREPARED | PENDING，尚未展示 |
| MSR03/V1 文件预览 | 中央路径、代码/安全Markdown或具名受阻；右侧仅单行树 | PREPARED | PENDING，尚未展示 |
| MSR04/V1 Git diff | 中央带旧新行号及＋/−的单列diff；右侧分组Git清单 | PREPARED | PENDING，尚未展示 |

以下本地链接仅定位资产，不表示已执行浏览器加载或批准其视觉。浏览器展示
须单独限定为本设计目录、审查本地脚本/资源和执行方式并获明确授权，不借机
开启产品 App、自动输入/截图/AX 或其他真实 UI 运行，更不提供替代 VM。

## 2. 每屏视觉核对项

- **MSR01**：与 V6 的整体结构接近，而非只抄颜色；项目范围靠近输入、
  四工具/主配置/模型/权限/强度、配方与组合能力齐全。中央不再巨型占位。
  1000 宽工具依内容换行，表单可滚动；不能通过缩字或藏配置凑高度。
- **MSR02**：13/20 正文与 12/18 原始片段区分清楚，阅读和续聊有完整
  工作面。输出为虚构，不推断审批/工具/完成，不显示假的业务成功。
- **MSR03**：所有文件打开都在中央，同一预览不断替换；代码长行可滚动、
  路径完整可读。右侧只有 12/24 单行树，无路径副行。Markdown 不执行
  HTML/脚本；PDF/空文件/受限链接仍在中央，不擅自外开。
- **MSR04**：中央是一份统一 diff，不套完整 Git 双栏。右侧只负责已暂存/
  未暂存/未跟踪定位；来源明确、旧新行号与符号不靠颜色单独传义。
- **共同**：统一 JetBrains Mono/中文苹方与 V6 明暗角色；不随屏幕放大/
  缩字。项目普通无胶囊、hover加深、常驻裸「＋」不吞折叠/导航；上40下22、
  面板25/留白7/间距6和三栏收缩关系需目视，不能只凭算术批准。

## 3. 状态与现场（不是额外获批屏幕）

同页虚构窗口 A/B 和 Atlas/Orbit，用于检查内存草稿、任务输入/阅读、
右侧清单分区与展开、唯一预览及阅读位置隔离。任务输入与新建草稿分开；
跨任务回到当前任务页、保留预览描述。返回不接管、不发送输入。

资源/清单状态独立覆盖未加载、加载中、ready、empty、failed、cached、
unavailable、unsupported。缓存标来源/版本未知；空是读完确认为空，
错误不冒充零变更。旧读取按窗口/项目/资源种类/来源/代次拦截。
正文只保留每个项目的当前预览，其他资源最多32条轻量位置，不缓存正文。

模型不是 host 权威输入票据或业务 owner；这是测试设计契约，不是产品接线。
选择失败/空等变体是在强制查看虚构状态；不读取真实磁盘/项目/Git。

## 4. 网页与 Apple 原生差异

- 旁邻互斥选择使用真实 HTML radio/select，选择标记交浏览器，不画
  自制 active 托盘/下划线/滑块。左导航有系统 radio，文件行无 radio；
  最终原生仍须 Picker/List(selection:) 与 NativeSelection 包装。
- **网页没有实现 Apple 原生 Liquid Glass**，不使用 CSS blur 冒充。
  系统窗框位置只是示意；阅读面板用稳定色，原生玻璃和系统工具栏另验。
- 可控文字字体本地附带，字体/许可哈希可静态验证，但实际字体、系统菜单
  例外、中文回退、原生/浏览器渲染均 NOT RUN / DEFERRED。
- 紧凑600pt约束保留在组合 envelope：534+6+240=780；产品源码未改。
  字符串检查不能证明真实 SwiftUI 容器或534pt中央已可用。
- 焦点恢复/不抢输入、长名单行、滚动与高对比等规则已经编写；未运行
  浏览器或原生键盘/IME/AX，不能称其交互已通过。

## 5. 验证与后续

本轮仅隔离 Node 纯值/字符串/静态、脚本语法、本地资源/字体、文档门禁与
保护输入核对。实际命令和失败/修复记录：
`.artifacts/native/approved-ui-integration/20261022-000052-main-shell-v1/`。
最终数量与退出码以该目录 report 为准；不把旧78项产品检查移作本稿结果。
没有 Swift/Rust/TS 产品变更，不跑其构建来冒充新屏可用性。
当前实际81项Node纯值/字符串/静态通过（0失败/0跳过），6份脚本语法通过。
其中24项只是屏幕/主题/宽度入口的字符串结构核对，不是24次视觉渲染。

视觉回执必须明确屏幕、版本与展示范围；共享原则、文字确认、准备资产、
静态通过均不填视觉批准。实质改版退回审阅。获指定屏批准后，才交该屏最小
原生接入审查；真实 UI/VM 仍受独立政策门禁，不因本稿获批恢复。
原任务未完成，不归档；四 Harness、输入票据、freshness、B02、品牌许可和
真实 UI 验收继续留账。历史 PI V1/V2、V6 与报告保持原文，不追认旧V2完成。

## 6. 24 个可定位样本

以下尺寸是壳逻辑尺寸，不是13英寸或27英寸显示器物理分辨率承诺。
所有组合仍为 **PENDING**；未实际打开或视觉检查。

| 屏幕/版本 | 主题 | 尺寸 | 本地样本 | 视觉 |
|---|---|---|---|---|
| MSR01/V1 | 明亮 | 1000×640 | [定位](../../.artifacts/native/mockups/main-shell-reconstruction-v1/index.html#screen=MSR01&appearance=light&width=1000) | PENDING |
| MSR01/V1 | 明亮 | 1260×800 | [定位](../../.artifacts/native/mockups/main-shell-reconstruction-v1/index.html#screen=MSR01&appearance=light&width=1260) | PENDING |
| MSR01/V1 | 明亮 | 1728×960 | [定位](../../.artifacts/native/mockups/main-shell-reconstruction-v1/index.html#screen=MSR01&appearance=light&width=1728) | PENDING |
| MSR01/V1 | 石墨深色 | 1000×640 | [定位](../../.artifacts/native/mockups/main-shell-reconstruction-v1/index.html#screen=MSR01&appearance=dark&width=1000) | PENDING |
| MSR01/V1 | 石墨深色 | 1260×800 | [定位](../../.artifacts/native/mockups/main-shell-reconstruction-v1/index.html#screen=MSR01&appearance=dark&width=1260) | PENDING |
| MSR01/V1 | 石墨深色 | 1728×960 | [定位](../../.artifacts/native/mockups/main-shell-reconstruction-v1/index.html#screen=MSR01&appearance=dark&width=1728) | PENDING |
| MSR02/V1 | 明亮 | 1000×640 | [定位](../../.artifacts/native/mockups/main-shell-reconstruction-v1/index.html#screen=MSR02&appearance=light&width=1000) | PENDING |
| MSR02/V1 | 明亮 | 1260×800 | [定位](../../.artifacts/native/mockups/main-shell-reconstruction-v1/index.html#screen=MSR02&appearance=light&width=1260) | PENDING |
| MSR02/V1 | 明亮 | 1728×960 | [定位](../../.artifacts/native/mockups/main-shell-reconstruction-v1/index.html#screen=MSR02&appearance=light&width=1728) | PENDING |
| MSR02/V1 | 石墨深色 | 1000×640 | [定位](../../.artifacts/native/mockups/main-shell-reconstruction-v1/index.html#screen=MSR02&appearance=dark&width=1000) | PENDING |
| MSR02/V1 | 石墨深色 | 1260×800 | [定位](../../.artifacts/native/mockups/main-shell-reconstruction-v1/index.html#screen=MSR02&appearance=dark&width=1260) | PENDING |
| MSR02/V1 | 石墨深色 | 1728×960 | [定位](../../.artifacts/native/mockups/main-shell-reconstruction-v1/index.html#screen=MSR02&appearance=dark&width=1728) | PENDING |
| MSR03/V1 | 明亮 | 1000×640 | [定位](../../.artifacts/native/mockups/main-shell-reconstruction-v1/index.html#screen=MSR03&appearance=light&width=1000) | PENDING |
| MSR03/V1 | 明亮 | 1260×800 | [定位](../../.artifacts/native/mockups/main-shell-reconstruction-v1/index.html#screen=MSR03&appearance=light&width=1260) | PENDING |
| MSR03/V1 | 明亮 | 1728×960 | [定位](../../.artifacts/native/mockups/main-shell-reconstruction-v1/index.html#screen=MSR03&appearance=light&width=1728) | PENDING |
| MSR03/V1 | 石墨深色 | 1000×640 | [定位](../../.artifacts/native/mockups/main-shell-reconstruction-v1/index.html#screen=MSR03&appearance=dark&width=1000) | PENDING |
| MSR03/V1 | 石墨深色 | 1260×800 | [定位](../../.artifacts/native/mockups/main-shell-reconstruction-v1/index.html#screen=MSR03&appearance=dark&width=1260) | PENDING |
| MSR03/V1 | 石墨深色 | 1728×960 | [定位](../../.artifacts/native/mockups/main-shell-reconstruction-v1/index.html#screen=MSR03&appearance=dark&width=1728) | PENDING |
| MSR04/V1 | 明亮 | 1000×640 | [定位](../../.artifacts/native/mockups/main-shell-reconstruction-v1/index.html#screen=MSR04&appearance=light&width=1000) | PENDING |
| MSR04/V1 | 明亮 | 1260×800 | [定位](../../.artifacts/native/mockups/main-shell-reconstruction-v1/index.html#screen=MSR04&appearance=light&width=1260) | PENDING |
| MSR04/V1 | 明亮 | 1728×960 | [定位](../../.artifacts/native/mockups/main-shell-reconstruction-v1/index.html#screen=MSR04&appearance=light&width=1728) | PENDING |
| MSR04/V1 | 石墨深色 | 1000×640 | [定位](../../.artifacts/native/mockups/main-shell-reconstruction-v1/index.html#screen=MSR04&appearance=dark&width=1000) | PENDING |
| MSR04/V1 | 石墨深色 | 1260×800 | [定位](../../.artifacts/native/mockups/main-shell-reconstruction-v1/index.html#screen=MSR04&appearance=dark&width=1260) | PENDING |
| MSR04/V1 | 石墨深色 | 1728×960 | [定位](../../.artifacts/native/mockups/main-shell-reconstruction-v1/index.html#screen=MSR04&appearance=dark&width=1728) | PENDING |
