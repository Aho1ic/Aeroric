# Agent Note: 页面标题并入窗口顶栏；整理历史会话

Status: implemented

## Problem

owner 2026-11-02 试用后提出两件事：

- 每个页面都自己画一行标题（记忆、Wiki、连接、画布、会话头），和窗口顶栏的“切换侧栏”那一行叠成两层，浪费高度，各页样式也不统一。
- 导入的历史会话（Claude、Codex 等）只能逐条看，没有办法用分析模型批量整理成线索和候选记忆。

两项的草图都已确认。

## Decision

**顶栏**

- 新增 `WindowTitle` 和 `.windowTitle(_:)`。页面通过 PreferenceKey 上报标题、副标题、图标或会话状态，`WorkspaceRootView` 把它们画在切换侧栏按钮所在的同一个 `.navigation` ToolbarItem 里，保证紧挨着按钮。
- 会话页的停止、置顶、更多由 `TaskDetailHeader` 自己的 `.toolbar(.primaryAction)` 提供。这些按钮留在页面视图树里，所以确认对话框和环境值照常可用。`TaskDetailHeader` 本身不再占一行。
- 会话头只在会话真正显示在前面时挂载，否则草稿页或工具页会被它的标题盖掉，因为被隐藏的会话视图仍然挂着。
- `ToolSectionScaffold` 的标题和项目名进顶栏；页内那一行只放动作和“返回：…”，两者都没有时整行不画。
- 记忆、Wiki、连接、需要我处理：标题进顶栏；项目选择、开关、搜索、编译留在页内。

**整理历史会话**

- `memory.historyPlan {projectId, sinceMs?}`：只读导入索引和文件大小，不读正文、不调用模型。按文件大小估算 token 和花费，列出已整理过的会话，返回今天剩余次数，以及项目是否开启了记忆、是否选了模型。
- `memory.extractImported {id}`：读这个导入会话的正文，走 `Memory::extract_session`。不强制：已整理的跳过，只说了一句的跳过，沿用每日上限。
- 由 App 逐个调用，最多发今天剩余的次数；遇到“到上限”或“还没有选模型”就停下并说明原因。停止只是不再发下一个。
- 来源是导入会话的整理记录和依据，点开时打开导入会话，不再当成任务打开。

## Alternatives considered

- **标题用页面自己的 `.toolbar(.navigation)`**：SwiftUI 不保证不同视图的 navigation 项的先后顺序，标题可能出现在侧栏按钮前面。
- **页面按钮也走 PreferenceKey，以 AnyView 传上去**：按钮会在根视图的环境里渲染，丢掉页面的环境和确认对话框。
- **在宿主里建后台批任务**：需要保存任务状态、恢复和取消接口。App 逐个调用就够了，代价是关掉弹窗或退出 App 会停，弹窗里写明了这一点。

## Consequences

- 新页面要显示标题，就调用 `.windowTitle(...)`。被隐藏但仍挂载的页面不能调用，否则会抢标题。
- 估算只是粗略值：transcript 文件大约四分之一是正文，正文按约每 2 个字符 1 个 token 计，并截到 `EXTRACT_MAX_CHARS`。界面上标为“约”。

## Verification

- core `a_history_plan_counts_only_what_is_not_extracted`。
- host `a_history_plan_reads_only_the_index`。
- Swift `testHistoryRunSendsWhatIsLeftAndStopsAtTheLimit`。
- 顶栏的实际观感等 owner 目检。
