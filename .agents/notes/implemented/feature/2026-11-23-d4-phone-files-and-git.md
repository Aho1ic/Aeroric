# Agent Note: D4 后续：手机的“文件与 Git”页接上 Mac

Status: implemented

## Problem

手机有一个“文件与 Git”分区，列文件和 Git 改动，但从来没有数据：
- store 里的 `files`、`gitChanges` 只有测试里的 `updateFilesAndGit` 填过；
- 连接器也没读过宿主的这几个接口。

宿主上这几个接口早就有了（`remote_headless.rs`），动手前（`main@d6010b6c`）核对过：
- **`project.files {projectId, path?}`：** 列一个目录，路径必须在项目里，文件夹排前面。
- **`git.changes {projectId, taskId?}`：** `git status`，给了任务时用任务的 worktree。
- **`git.diff {projectId, path, staged, taskId?}`：** 一个文件的差异，大小由 `bounded_file_diff` 限住。
- **`project.readFile {projectId, path}`：** 读一个项目内的文件，最多 200 KB，超出的标 `truncated`。
- **SSH 和 WSL 项目：** 这几个接口都回 `available: false` 和原因。

宿主还有 `project.writeFile`。backlog §4.4-8 定的是“远程写文件默认关闭”，所以手机不接它。

## Decision

**手机端只读，宿主不改。**

- **读取的路子：** 和上一步任务页读会话一样。
  - 页面要读什么，store 发出一个 `CompanionRead`（`files`、`changes`、`diff`、`file`）给 `onRead`；
  - 连接器调对应接口，回答交给 `applyRead`，没收到回答交给 `readFailed`；
  - store 自己不碰网络。
- **页面状态：** `CompanionBrowse` 记下当前项目、从根目录打开的文件夹、目录内容、Git 改动、打开的文件或差异。每一项都是 `CompanionLoad`：读取中、已读到、Mac 说不能看（写原因）、没收到回答（写原因）。
- **晚到的回答：** 只有页面还显示着它读的那样东西时才用。
  - 例如已经进了子文件夹，根目录晚到的列表丢掉；
  - 已暂存和未暂存的同一个文件算两份差异。
- **读失败时：** 已经显示的内容保留。
- **何时重读：** 每次快照（重连、推送触发的重读、判旧后的重读），把页面正显示的改动、目录和打开的文件或差异都再读一次。Agent 改了文件，Mac 一发 `task-changed`，页面就跟上。项目在快照里没了，写“这个项目在 Mac 上已经没有了”。
- **界面：**
  - 顶上选项目；
  - “Git 改动”一节：每行写状态、路径、已暂存，点开看差异，加的行绿、删的行红、`@@` 蓝；
  - “文件”一节：点文件夹进去，有“上一级”；点文件看内容，太大时写“只显示了前面一部分”。
  - 文件和差异在弹出页里看，可以横向滚动，可以选中复制。
- **退出时：** 取消配对、设备被撤销时清掉。

## Alternatives considered

**改成按任务看改动（用任务的 worktree）。** 这次没做：
- 宿主的 `git.changes`、`git.diff` 已经支持 `taskId`；
- 手机端先按项目看。在任务页加“这个任务的改动”，可以作为下一步。

**手机也能改文件。** 没做，按 backlog 的决定，远程写文件默认关闭。

**推送带上改了哪些文件，手机按增量改。** 没选：和任务页一样，快照是权威，推送只是提示，重读几次请求就够。

## Consequences

- **收益：** 不用回到 Mac，就能看 Agent 改了哪些文件、改成什么样、项目里有什么。
- **局限：**
  - 只按项目看，不按任务的 worktree 看；
  - 不能改文件，不能暂存或提交；
  - 每次快照都会让 Mac 跑一次 `git status` 并列一次目录，只在这一页打开时才会；
  - 宿主的 `project.writeFile` 仍对已配对设备开放，和“远程写文件默认关闭”不一致。手机不调用它。同日已在宿主上默认关掉，见 [远程写文件默认关闭](../bug-fix/2026-11-23-remote-file-writes-off-by-default.md)；
  - 真机、真实 Mac、真实 Relay 上 NOT RUN，界面没有目检。
- **验证：**
  - 手机新增 5 个测试：
    - 每种读取的方法名和参数；
    - Mac 的回答变成目录、改动、文本，不能看的原因，读不懂的回答；
    - store 打开项目、进出文件夹、开差异、晚到的回答丢掉、读失败时保留、文件夹按文件夹打开、离开页面后不收；
    - 快照触发重读，项目没了，取消配对后清掉；
    - 经假 Mac：读出改动和目录，打开差异，被拒的读取写明原因，推送 `task-changed` 后改动重读，从没调用 `project.writeFile`。
  - MacBook 上：
    - iOS 包 67 个测试都过，连接相关四套连跑 5 次都过；
    - iOS App 在模拟器上编过。
