# Aeroric 项目概述

Aeroric 是一款面向 AI 编程智能体的桌面任务管理器，核心目标是把多项目工作区、任务生命周期管理、原生终端、会话回放、Git 操作、文件浏览和技能管理集中到同一个界面里。它服务的不是单次编辑，而是围绕 Claude Code、Codex 这类 agent 的持续协作流程。

## 项目定位

这个仓库不是传统 IDE 的替代品，而是一个面向 agent workflow 的控制台。它把“发任务、看输出、等确认、回看会话、处理 Git 变更、继续任务”串成一条闭环，尽量减少在终端、编辑器、Git 客户端和日志文件之间切换的成本。

从现有代码看，Aeroric 的重点不是复杂编辑器功能，而是围绕任务和会话的可见性、可恢复性，以及多项目并发管理能力。

## 技术栈

- 前端：React 19、TypeScript、Vite
- 桌面壳：Tauri 2
- 后端：Rust
- 终端：xterm.js
- 代码高亮：Shiki、CodeMirror
- UI 组件：Radix UI、lucide-react
- 其它：marked、DOMPurify、reqwest、portable-pty、notify、parking_lot

开发脚本来自 `package.json`：

- `pnpm dev`：启动 Vite 开发服务器，端口 `1420`
- `pnpm build`：TypeScript 类型检查 + Vite 打包
- `pnpm tauri dev`：启动完整桌面应用
- `pnpm tauri build`：构建桌面安装包

Tauri 配置中，前端开发地址固定为 `http://localhost:1420`，窗口默认尺寸为 `1100 x 720`，打包目标覆盖桌面平台。

## 前端结构

前端入口是 `src/main.tsx`，它挂载了：

- `I18nProvider`
- `ToastProvider`
- `NotificationsProvider`
- 根组件 `App`

`src/App.tsx` 是前端的状态中枢，保存了项目、任务、当前视图、主题、字体、技能中心状态，并负责：

- 监听 Tauri 事件
- 调用 Rust 命令持久化项目和任务
- 协调任务面板、项目切换、右侧工具面板和终端区域
- 在启动时处理任务状态恢复与中断任务归一化

从组件命名可以看出，前端围绕几个稳定区域组织：

- `WelcomePage` / `TimelineView`：项目入口与跨项目时间线
- `ProjectPage`：单项目主视图
- `ProjectRail`：左侧项目切换栏
- `TaskPanel` / `TaskList` / `TaskListItem`：任务列表与分支操作
- `NewTaskView` / `TodoTaskView` / `RunningView` / `SessionView`：任务创建、待办编辑、运行态、会话查看
- `FileExplorer` / `FileViewer` / `ImagePreviewPane`：文件浏览与预览
- `GitChanges` / `GitHistory` / `GitDiffViewer`： Git 变更与历史查看
- `ShellTerminalPanel` / `TerminalView`：嵌入式终端与 agent 输出
- `SettingsDialog` / `AppSettingsDialog`：项目级和应用级配置
- `SkillHubView`：技能中心

## 后端结构

Rust 入口在 `src-tauri/src/lib.rs`。这里负责：

- 初始化 `TaskManager`
- 注册所有 Tauri 命令
- 在启动阶段预热 shell、安装 hook、启动事件 watcher
- 处理 macOS 下窗口隐藏与重新唤回逻辑

后端模块按职责拆分得比较清楚：

- `pty.rs`：任务和 shell 的 PTY 创建、读写、恢复、取消
- `session.rs`：会话文件监听、消息读取、导出 markdown
- `storage.rs`：项目和任务的文件持久化
- `fs.rs`：目录、文件、图片预览、写入、创建、删除、搜索
- `git.rs`： Git 状态、日志、分支、暂存、提交、推送、拉取、工作树操作
- `analytics.rs`：会话指标解析，提供 token 和工具调用统计
- `config.rs`：项目级 `.aeroric/config.toml` 读写与初始化
- `app_settings.rs`：应用级智能体路径、版本、字体等设置
- `hooks.rs` / `event_watcher.rs`： hook 安装、探测与事件订阅
- `skills.rs`： skill hub 管理、安装、卸载、清理
- `usage.rs`： Claude / Codex 用量快照
- `notification.rs`：通知读取与标记已读

`TaskManager` 由 `parking_lot::Mutex` 管理多个 PTY、写入器、子进程句柄和会话映射，说明后端的核心任务是把 agent 进程生命周期和 UI 状态稳定关联起来。

## 数据模型

共享类型定义在 `src/types.ts`。最核心的对象是 `Project` 和 `Task`。

`Project` 包含：

- `id`
- `name`
- `path`
- `branch`
- `lastOpenedAt`
- `hiddenFromRail`

`Task` 包含：

- `id`
- `projectId`
- `prompt`
- `agent`：`claude` / `codex`
- `permissionMode`：`ask` / `auto_edit` / `full_access`
- `status`：`todo`、`pending`、`running`、`input_required`、`detached`、`interrupted`、`done`、`failed`、`cancelled`
- `createdAt`
- 会话信息：`claudeSessionId`、`codexSessionId` 及对应路径
- worktree 信息：`worktreePath`、`worktreeBranch`、`baseBranch`
- 结果统计：`additions`、`deletions`

存储方式不是 localStorage，而是文件化持久化：

- `~/.aeroric/projects.json`
- `~/.aeroric/projects/<projectId>/tasks.json`

主题、字体、显示窗口等 UI 偏好仍保存在 localStorage。

## 事件与通信

前后端通过 Tauri 命令和事件协作。

常见的状态通道包括：

- `task-status`
- `task-session`
- `shell-output`

agent 输出不走全局广播，而是通过 `tauri::ipc::Channel<String>` 直接进入终端批处理流程，这样更适合高频输出场景。

从代码结构看，系统重点处理的是三类同步问题：

1. 任务状态和 PTY 状态同步
2. 会话文件与 UI 中会话视图的同步
3. Git / worktree 变更与任务完成状态的同步

## 项目配置与扩展能力

每个项目会自动生成 `.aeroric/config.toml`，主要包含：

- 默认智能体
- 默认权限模式
- 任务提示词前缀
- Git 提交提示词

此外，仓库还支持：

- 智能体路径与版本探测
- hook 安装与状态检测
- skills 安装与清理
- 任务工作树的创建、合并和移除
- 用量统计和通知中心

## 知识库与文档

`knowledge/` 目录是项目的结构化知识库，偏向记录 WHY、契约和踩坑结论，而不是重复代码本身的 WHAT。当前已有内容主要集中在：

- xterm 终端渲染问题
- Claude Code / Codex hook 支持对照

`README.md` 和 `README_ZH.md` 提供了产品级介绍，适合对外展示；`docs/project-overview.md` 更适合作为仓库内部的结构索引。

## 一句话总结

Aeroric 是一个以 agent 为中心的桌面工作台：它把多项目任务管理、终端执行、会话追踪、Git 工作流和技能管理放在一起，让 AI 编程过程能被持续观察、恢复和接续，而不是只停留在“发起一个命令”的层面。
