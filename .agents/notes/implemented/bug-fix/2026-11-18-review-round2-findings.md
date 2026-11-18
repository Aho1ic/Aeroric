# Agent Note: 第二到四轮目检发现的问题——待办连点建两条、胶囊输入框里有小圆角框、参考代码库页不见、新会话页的配置说法、报错露出调试文字

Status: implemented

## Problem

2026-11-18 10:22–10:48 第二轮目检（Codex Computer Use，`~/Documents/computer-use/2026-11-18-1022-aeroric-review-round2/report.md`）。A1、A2 两处回归已符合预期，另有四处不符合：

1. **待办连点"保存"建了两条。**
   - B2-7 的防重只在保存进行中有效（`TodoStore.writing`）；
   - 宿主保存加重读不到 100 毫秒，第一次保存已经结束、表单还在关闭时，双击的第二下落到按钮上，又存了一条。新待办的 id 由宿主生成，两次保存是两条不同的记录。
2. **两处输入框不是胶囊。**
   - Git 侧栏的提交说明是 `.plain` 文本框，外面和"生成""提交"一起包在一个圆角矩形卡片里；
   - `CapsuleTextFieldStyle` 的文本框聚焦时，系统焦点环在胶囊里面画出一圈小圆角矩形（画布"添加会话卡"的搜索框打开就自动聚焦，所以最明显）。
3. **设置里没有"参考代码库"。**
   - 宿主的 `references.list` 正常，能列出已有的 mmyolo；
   - 这一页只在连接宿主那一刻读一次列表，读成功了才显示；那一次失败就再也不读，整页一直藏着。
4. **新会话页切到 Aeroric 项目后，拦截原因写"宿主尚未提供匹配的 Agent 配置及版本"。**
   - 实际是这个项目的草稿还没选过配置（各项目草稿互相独立，不自动选）；
   - 这句话读起来像宿主坏了，目检因此把它记成不符合。

5. **报错露出调试文字**（第四轮，`2026-11-18-1303-aeroric-review-round4`）。只读成员会话改权限被拒时，提示是"修改失败：refused(AeroricProtocol.FailureFrame(id: …, message: "只读会话…不能改权限" …))"。
   - 界面有 72 处写成 `"…失败：\(error)"`；
   - 字符串插值用的是 `description`，而 `ConnectionError` 只给了 `errorDescription`（`localizedDescription` 用的那个），插值就落到枚举的调试输出。

另外两处不是代码问题：B1 的 Claude Code 成员用了官方订阅配置，网关返回 403，主持人总结也因此失败；D3 的悬停和 C2 的滑动过渡，Computer Use 截不到。

## Decision

1. **一个编辑框只成功保存一次。**
   - `TodoStore.save(_:from:)` 多收一个编辑框的标识，保存成功后记住它，同一个编辑框再点就什么也不发；
   - 保存失败不记，可以改了再存；
   - `TodoEditorSheet` 每次打开生成一个新的标识。
2. **胶囊**：
   - Git 提交说明改用 `.textFieldStyle(.capsule)`，去掉外面的圆角矩形卡片；
   - `CapsuleTextFieldStyle` 里先把文本框本身设成 `.plain`，再加 `.focusEffectDisabled()`。
     - 第一次只加了 `.focusEffectDisabled()`，第三轮目检（`2026-11-18-1142-aeroric-review-round3`）里 Git、画布、参考代码库三处仍有内框；
     - 在 Mac 上离屏渲染对比才确认：自定义 `TextFieldStyle` 的 `configuration` 照样画 AppKit 的边框，不是焦点环；
     - 所有用 `.capsule` 的文本框（22 处）一起受益。
3. **设置窗口每次打开时**，如果参考代码库还没读到，再读一次。
4. **拦截原因按实际情况说**：
   - 宿主还没给配置：宿主尚未提供 Agent 配置；
   - 没选过：请先选一个 Agent 配置；
   - 选过的配置换了版本：请重新选一次；
   - 选过的配置已被删除：请重新选择。
   - 第三轮目检看到 Aeroric 项目的草稿属于"换了版本"，按钮却写"Agent"，和下面的原因对不上；
     - 配置按钮现在仍显示原配置名，后面加"（已更新，需重选）"（`ComposerPickerOptions.configurationTitle`）；
     - 换版本仍然要人重选，不自动沿用（`testProfileVersionAndUnsupportedModelNeverSilentlyFallback` 钉着）。

5. **`ConnectionError` 加 `CustomStringConvertible`**，`description` 与 `errorDescription` 同一套话：被拒时就是宿主给的原因。72 处一处不改，以后新写的也对。

## Alternatives considered

**报错：72 处逐个改成 `AgentConfigStore.describe(error)`。** 最强的理由是每处显式。没选：改动面大，以后新写的插值还会再犯；在错误类型上给出说法，一处就够。


**待办：宿主按客户端草稿 id 去重。** 最强的理由是任何客户端连点都只建一条。没选：要给待办协议加一个"客户端请求 id"字段并改宿主存储；问题只出在这一个表单，客户端记住"已保存"就够了。

**待办：成功后不把 id 移出 `writing`。** 最强的理由是不加新字段。没选：新待办的草稿 id 都是空串，留在 `writing` 里会挡住之后所有新建。

**参考代码库：读失败时也显示这一页，只显示错误。** 最强的理由是不会藏起来。没选：老宿主没有这组方法时也会失败，那时整页应该藏起来；打开设置时重试，既不误显示，也不会永久藏住。

**胶囊：把 22 处 `.textFieldStyle(.capsule)` 改成一个视图修饰符。** 最强的理由是不依赖 `TextFieldStyle` 的私有 `_body`。没选：在样式里把内部文本框设成 `.plain` 就够了，离屏渲染与手写的"plain 加胶囊"逐像素相同，22 处调用不用动。

**配置换版本后自动沿用新版本。** 最强的理由是少点一下。没选：版本变了可能是模型、权限或连接变了，沿用就是悄悄换了用户选的东西；只把按钮说清楚。

**拦截原因：自动给没选过配置的项目选上一个。** 最强的理由是少点一下。没选：B2 的规则是"不自动选第一项、不假设默认值"，改的是说法，不是规则。

## Consequences

- **收益**：连点保存只建一条；提交说明和自动聚焦的搜索框都是胶囊；参考代码库页能找到；新会话页的拦截原因告诉人该做什么。
- **代价**：用胶囊样式的文本框聚焦时没有系统焦点环，只能靠光标看出焦点。
- **验证**：
  - Swift 新增 `testAnEditorThatSavedSavesNothingMore`、`testAnUnmatchedConfigurationSaysWhatToDo`、`testALaterLookShowsThePaneAfterAFailedFirstOne`；
  - `CapsuleTextFieldStyleTests.testTheCapsuleStyleDrawsNoBezel`：离屏渲染 `.capsule` 文本框和手写的 plain 加胶囊，要求 PNG 完全相同；去掉 `.plain` 时这条失败（10297 字节对 9407 字节）；
  - `ConnectionErrorWordsTests.testAnInterpolatedRefusalIsTheHostsMessage`：被拒的错误插进句子里，就是宿主的原话；
  - 第四轮目检已过：Git、画布、参考代码库、待办、讨论要求的输入框只有一层胶囊；新会话页按钮写"Magpie（已更新，需重选）"；只读成员改权限被宿主拒绝、权限仍是 plan；
  - 第三轮目检已过：待办双击只建一条，设置里有参考代码库、取消后列表不变；
  - 界面改动等第三轮目检，记在路线文档 §8。
