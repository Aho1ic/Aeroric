# Agent Note: 新建会话记住每个项目的选择，切换 Harness 时换算过去

Status: implemented

## Problem

用户 2026-11-23 的要求：
- **随时记住：** 同一个项目里，任何时候选的 Agent 配置、模型、权限、思考强度，下次新建会话默认就是它们；
- **切 Harness 不丢：** 选好以后切换 Harness，之前的选择要保留，并自动换到兼容的格式，比如从 Codex 换到 Claude，接口从 `/v1/responses` 换成 `/v1/messages`；
- **离开或重启后不丢：** 点开别的页面再回到新建会话，或者退出 App 再打开，选择都还在；
- **新建项目：** 选完文件夹后显示新建会话，不显示任务列表。

核对的事实（`main@ac179f98`）：
- **只在发出成功后才记：** `NewSessionDraftBook` 只在会话发出成功（`acknowledge`、`reconcileAccepted`）后，才把选择记成这个项目的偏好。选了没发，下一个新建会话还是上次发出时的选择。
- **切 Harness 会重选：** `NewSessionHome.selectHarness` 一律选新 Harness 的第一个可用配置，模型、思考强度换成那个配置的默认值。权限要求“为当前 Agent 明确选择”，切过去以后算没选。
- **同一个连接有两个配置：** 连接在每个 Harness 下各派生一个配置，id 是 `agent-config:via-<harness>-<连接哈希>`，同一个连接的哈希相同。网关负责把 Codex 的 Responses 和 Claude 的 Messages 转到连接的协议上。所以“换到兼容格式”就是换到同一个连接在另一个 Harness 下的那个配置。
- **重启时可能被空白页盖掉：**
  - 重启后，页面选中（`onChange(of: selection, initial: true)`）可能先于草稿恢复（`onAppear`）执行，先开出一个空白草稿；
  - `restore` 只填还没有的草稿，于是保存的那份被丢掉。
- **配置版本：** 配置改过以后版本变了，草稿记的旧版本对不上，会显示“选的配置已更新到新版本，请重新选一次”，之前的选择要重做。
- **打开项目以后：** `openProjectPanel` 和打开远程项目以后，选中的是项目这一行，显示的是项目的任务列表。

## Decision

**每次选择都记成这个项目的偏好。**
- `NewSessionDraftBook.edit` 改动了一个项目的草稿、而且草稿已有配置时，把配置、Harness、模型、思考强度、权限（以及配方、能力）记为这个项目的偏好；
- 偏好照旧随 `memos.preferences` 存进 `UserDefaults`；
- 提示词、附件、提交身份不记。

**切换 Harness：`NewSessionDraft.switchHarness`。** 按顺序找新 Harness 下对应的配置：
1. 同一个连接：`agent-config:via-codex-<哈希>` 对 `agent-config:via-claude-<哈希>`，`-login`、`-all` 也一样；
2. 原来用的是 Harness 自己的登录，就换成新 Harness 自己的登录；
3. 名字相同的 Agent 配置。比较时去掉名字里写的 Harness，比如“GPT 5.5 · Codex”和“GPT 5.5 · Claude”算同一个；
4. 都没有，才用新 Harness 的第一个可用配置（原来的做法）。

选好配置后：
- **模型、思考强度：** 新配置提供原来的，就保留；不提供，就用配置的默认值；
- **权限：** 用户明确选过、而且新配置提供同一档，就保留，并记为新 Agent 的选择。

**以前权限要求“为当前 Agent 明确选择”，现在这样做是用户要的：**
- 档位名在各 Harness 间是同一套（每次询问、自动编辑、完全访问）；
- 用户刚在同一页选过；
- 新配置不提供这一档时，仍然要重新选。

**重启时保存的一份优先：** 草稿簿记下哪些草稿是打开后还没碰过的（`untouched`）。
- `restore` 用保存的那份换掉这些草稿；
- 没有保存草稿的项目，用恢复回来的偏好填上。

**配置更新了版本：** `NewSessionDraft.refresh` 在配置清单读到同一个 id 的新版本时，悄悄换到新版本，模型、思考强度仍提供的就保留。`NewSessionHome.adoptDefaults` 在草稿已有配置时调用它。

**打开项目以后：** 本机项目和远程项目打开以后都选中这个项目的新建会话页（`SessionNavigation.draft(projectID:)`）。

## Alternatives considered

**全局记一份，不分项目。** 没选：用户说的是“同一个项目中”，不同项目常用不同的 Harness 和模型。

**切 Harness 时保留原配置，让宿主去转。** 没选：配置本身就绑定了 Harness（id 里写着），宿主按配置启动。换到同一个连接的另一个配置，是现有结构里正确的做法。

**权限一律不带过去。** 没选：用户明确要保留，而且权限档位各 Harness 通用。

## Consequences

- **收益：**
  - 在一个项目里选过一次，以后新建会话、离开再回来、重启，都是这套选择；
  - 切 Harness 不用从头选，同一个连接、模型、思考强度、权限都会跟过去；
  - 新建项目直接进入新建会话。
- **局限：**
  - 全局新建会话页（没选项目）不记偏好，和原来一样；
  - 名字匹配只认去掉 Harness 名后完全相同的名字；
  - App 界面上没有目检。
- **验证：**
  - Swift 新增 3 个测试：
    - Codex 切 Claude 换到同一个连接，模型、思考强度、权限保留，切回来也对；按名字换；新配置不提供的思考强度不保留；登录对登录；
    - 选了没发就记住，重启后没碰过的页面用保存的那份，发出后下一个新建会话沿用选择、提示词清空；
    - 配置出了新版本时悄悄换过去，模型保留；
  - MacBook 上新建会话相关的测试都过；App 层的改动（`AppCommands`、`TaskWorkspaceView`）要在完整构建里编译。
