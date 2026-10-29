# Agent Note: 关联文件夹

Status: implemented

## Problem

一个项目常要用到项目外的文件夹（共享协议、设计素材、笔记）。以前只能把它们复制进来或建符号链接，前者会过时，后者会改动用户的仓库（`git status` 出现新条目）。

## Decision

- **存储**（L1）：项目记录的 `linkedFolders`（规范化后的绝对路径数组），经 `ProjectService::link_folder` / `unlink_folder` 写入；不往用户仓库写任何东西，不建符号链接。只支持本地项目（L2）。
- **拒绝规则**（`projects::linked::validate`）：远程项目、不存在或不是文件夹、项目本身、包含项目的上级目录、已在项目里面的目录、重复关联。
- **启动**：`launch_plan` 把存在的关联文件夹放进 `LaunchRequest` / `ResumeRequest` 的 `linked_folders`（远程项目为空），经 `CodexSessionSpec.extra_dirs` 传到三家：
  - Claude：每个文件夹一个 `--add-dir`；
  - OMP：`--add-dir=<路径>`；
  - Codex：`auto_edit`（workspaceWrite）时写进每轮 `sandboxPolicy.writableRoots`；完全访问不需要，只读仍只读；Codex 本来就能读整个磁盘。
  - DSH：`dsh` 与其 tui / web 都没有额外目录参数（2026-10-29 核实），会话页提示「看不到」。
  新建和恢复（App 重启后接着聊等）都按当时的列表带上（Q2）；失效的文件夹启动时跳过，记录保留（Q4）。
- **接口**：`project.linked_folders`（各项目的列表与是否存在）、`project.link_folder`、`project.unlink_folder`。
- **文件树**：关联文件夹作为项目根下的链接节点（`@linked:<路径>|<相对路径>`）；展开与预览时以 `<项目id>#linked:<路径>` 请求宿主，`find_project_root` 只放行该项目确实列出的路径，其余文件访问边界不变。
- **界面**（设计稿经用户确认，Q1–Q4 均按推荐）：项目菜单「关联文件夹（N）…」打开窗口：名字、路径、失效状态，移除；「添加文件夹…」用系统面板，可多选，拒绝理由就地显示。会话信息栏列出「关联文件夹 N 个」及这个 Harness 能否看到（Q3-a）。权限跟随会话的权限模式（Q1-a）。

## Alternatives considered

- **在项目里建符号链接**：不选（L1）。会改用户仓库；仅在某个 Harness 不支持额外目录时，再作为需要用户明确选择的选项。
- **为文件树另起一套树**：不选。复用同一棵树，节点路径带前缀即可，展开、预览、只读规则都不用重写。
- **Codex 用 `-c sandbox_workspace_write.writable_roots`**：不选。一个 app-server 供多个项目的会话共用，按轮传 `writableRoots` 才能按项目区分。

## Consequences

- 新增项目字段 `linkedFolders`；旧版客户端保存项目时可能丢掉它（与 `color` 等同类字段一样）。
- 测试：核心拒绝规则与失效保留 2 个、Codex 可写目录 1 个、宿主 Claude 参数 1 个、App 端路径映射与提示 2 个。
- 未验证：真实 Claude / Codex 会话里 Agent 读到关联目录的内容（需要用户在 App 中试一次）。
