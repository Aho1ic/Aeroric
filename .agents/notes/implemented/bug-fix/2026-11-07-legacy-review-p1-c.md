# Agent Note: 回溯评审 P1 C 批——归档会话的项目、标签与导入错误可见、远程安装与 Codex、长会话首页

Status: implemented

## Problem

2026-11-06 回溯评审在侧栏、导入、远程主机和会话历史里确认了八个问题：

- **B1-4**：侧栏靠“行在哪个项目下面”反推项目。归档行单独渲染，算出 nil，本项目标签不可选，“仅当前项目”被禁用。
- **B1-7**：标签写入失败只写进 `TagBook.errorText`，界面不读；“创建并贴上”不等贴标完成就关闭对话框。
- **B2-1**：导入时按根逐个 `grant`，而每次 `grant` 都先删掉同一家的全部旧授权，多根来源（Cursor CLI 与编辑器、Qoder 发行变体）只剩最后一个根。
- **B2-5**：导入操作的错误被随后成功的 `refresh` 清掉，选择器也照常关闭。
- **B2-7**：Aeroric 远程运行时 `~/.local/bin` 总在 PATH 最前，npm 升级被离线装的旧二进制遮蔽；完成后的复探只要求找到版本，照样报成功。
- **B2-10**：SSH 主机页只有一个全局安装轮询，第二台主机的安装永远停在“安装中”。
- **B2-14**：远程项目的内置 Codex 用本机模型查询：本机查询失败时，原因写进 `blockedReasons`，远程探测通过也不清，入口被禁用；提交校验同样被它拦住。
- **B4-1**：会话历史首页按约 640 KiB 分页后又追加整份统计，没算进 1 MiB 帧；超限时界面的 `try?` 吞掉错误，整页空白。

## Decision

- **B1-4**：`SidebarItem` 新增 `projectID`，`taskItem` 从任务记录填入，导入行取 `ImportedSession.projectId`。`projectID(of:)` 优先读它，再回退到按位置反推。
- **B1-7**：
  - `TagBook.assign` 先乐观更新再写入，返回失败原因并重读标签库。菜单勾选失败写入 `errorText`，侧栏“项目”标题下显示可关闭的胶囊错误条，重读会撤回乐观勾选。
  - `create` 等贴标完成；贴标失败返回“标签已创建，但没能贴上：<原因>”，对话框不关。
  - 新增通用组件 `CapsuleErrorLine`（与 Git 面板的错误行同款），标签、导入、会话历史共用。
- **B2-1**：`apply` 用 `grants_by_agent` 把选中会话所属各家的根聚合起来，每家调用一次 `grant_roots`。取消勾选即撤销授权的语义不变。
- **B2-5**：`ImportBook` 拆成 `actionError` 和 `readError`，`lastError` 取二者之一。`refresh` 只改 `readError`；`actionError` 留到用户关闭或下一次操作。`applyPick` 返回是否导入成功：失败时选择器不关，并在顶部显示错误；部分目录没能加为项目时照常关闭，错误留在设置页。
- **B2-7**：
  - 直接安装的命令末尾追加 `printf 'AERORIC_RESOLVED %s\n' "$(command -v <agent>)"`。`push_job_line` 把这条记进作业的 `resolved`，不进日志。
  - `shadowed_upgrade`：安装命令是 `npm install -g`，而解析到的路径在 `~/.local/bin/<agent>` 时，判失败，提示“删除它，或改用本机下载后传输升级”。
  - 不自动删除：Claude 官方安装脚本也装在 `~/.local/bin`，只有 npm 路径才算遮蔽。
  - `install_result` 比较升级前后的探测版本，没变时消息写成“已可用：X（版本未变化）”。
- **B2-10**：`installPolls` 改为按主机的字典；`refresh` 时，仍在安装中的主机重新接上轮询。
- **B2-14**：
  - 在 core 的 `session_catalog::for_project` 里，对 SSH 项目调用 `remote_builtin_codex`：清掉内置 Codex 的 `blockedReasons`（只来自本机模型查询），宿主之后照常追加远程探测原因。
  - 本机没有模型列表时，只提供 `REMOTE_DEFAULT_MODEL`（“默认模型”）。`chosen_model` 把它和空字符串都当作“不传 model”，远端 Codex 自己选默认。
  - 放在 core 而不是宿主的 `mark_remote_availability`，这样新会话选择器和提交校验读到的是同一个目录。
- **B4-1**：
  - 宿主先算统计的序列化大小，`message_budget` 从首页预算里扣掉。统计超过半页时不再随首页发送，保证整帧不超限。
  - `LiveSessionStore.reloadHistory` 不再 `try?`，失败写入 `historyError`；会话页顶部显示胶囊错误条和“重试”，空态在出错时不显示。

## Alternatives considered

- **B1-4 打开菜单时再查一次任务记录**：只修一个入口，筛选、拖放等其他按项目判断的地方还会错。
- **B2-1 让 `grant` 改为追加**：撤销单个根的语义会变，所有调用方都得重查。
- **B2-7 直接删除 `~/.local/bin` 里的旧文件**：可能删掉官方安装器装的 Claude，也替用户做了不可逆操作。
- **B2-14 现在就经 SSH 在远端跑 `codex app-server` 读模型列表**：工作量大，另作后续。
- **B4-1 统计拆成单独的 RPC**：要新增协议和客户端调用，扣预算已经能保证不超帧。

## Consequences

- 远程内置 Codex 在本机能列出模型时，沿用的是本机账号的列表，远端账号不支持的模型要到运行时才报错。
- 极长会话（统计超过半页）的首页不带统计，统计栏为空，直到后续另做按需读取。
- B2-10 和 B1-7 的流程依赖具体的宿主客户端，这次没有替身可测，靠代码审查和目检。

## Verification

- Rust：
  - `every_root_of_a_picked_agent_is_granted_together`
  - `an_npm_upgrade_hidden_by_an_offline_copy_is_a_failure`
  - `a_remote_builtin_codex_ignores_the_local_model_query`
  - `the_first_page_leaves_room_for_the_stats`
- Swift：`testAFailedHistoryReadIsShownAndCanBeRetried`。
- 归档会话的标签范围、标签错误条，靠 Codex Computer Use 目检验证。
