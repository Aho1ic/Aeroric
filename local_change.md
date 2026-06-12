# Local Change Log

> 目的：记录本仓库相对上游 `hanshuaikang/nezha` 的本地个性化改动，供后续合并新版（如 0.4.2）时逐项核对，避免被上游覆盖。
> 安全规则：不要在本文记录 SSH 密码、API key、私钥、账号 token 等敏感值。

## 当前基线（合并 0.4.2 前先读）

- 当前上游基线：`v0.4.1` / `origin/main` 的 `d06e8f4` 已合入。
- 当前本地分支：`codex/ssh-remote-mvp`。
- 本地合并后提交：
  - `b0ec3a8`：合并 0.4.1 前的本地个性化改动检查点。
  - `7eed7b2`：合并上游 `v0.4.1`。
  - `cc7789b`：记录 0.4.1 App 替换结果。
- 已安装应用：`/Applications/NeZha.app`，`Info.plist` 版本为 `0.4.1`。
- 已安装二进制 SHA256：`0a8f6a4d133830575665bed904bec0dc40ccb242056f8d35fdcacdea2b1b79bd`。

## 下次合并新版的固定流程

1. 先读本文件的“当前基线”“本地个性化改动总览”“合并冲突热点”和“读写规范”。
2. 执行 `git status --short --branch`，确认是否有未提交改动；如有，先判断是否属于用户本地改动。
3. 合并前在仓库外保存保护材料：
   - `git diff > ../nezha-merge-backups/local-before-<version>.diff`
   - `git status --short > ../nezha-merge-backups/local-before-<version>-status.txt`
4. 合并前建立本地检查点提交，提交信息建议：`chore(local): checkpoint personalized changes before <version> merge`。
5. 拉取上游：`git fetch --prune --tags origin`。
6. 对比上游新版影响范围：
   - `git log --oneline v0.4.1..v<next>`
   - `git diff --name-status v0.4.1..v<next>`
   - `git diff --name-status v0.4.1..HEAD`
7. 合并新版 tag 或目标提交，优先保留本地个性化功能；遇到行为取舍不明确的冲突，暂停交给用户判断。
8. 合并后至少执行：
   - `pnpm lint`
   - `pnpm test`
   - `cargo test`（在 `src-tauri/`）
   - `pnpm build`
   - `pnpm tauri build`
9. 替换 `/Applications/NeZha.app` 后，记录构建产物和已安装 App 的 SHA256、`Info.plist` 版本、DMG 路径。

## 本地个性化改动总览（合并时必须保留）

### SSH / 远程项目

必须保留的行为：
- SSH 连接支持可选密码字段，密码模式通过 `sshpass -e` 和 `SSHPASS` 环境变量传递。
- SSH 项目支持中心区域自动连接的 SSH 终端。
- SSH 项目右侧文件浏览器支持远程目录、文本、图片预览、创建文件/目录、删除路径。
- SSH 项目继续禁用本地专属能力：本地 shell、Git 面板、文件搜索、项目设置、worktree、附件上传。

关键文件：
- `src-tauri/src/ssh.rs`
- `src-tauri/src/remote_fs.rs`
- `src-tauri/src/remote_git.rs`
- `src-tauri/src/lib.rs`
- `src/components/ssh/*`
- `src/components/project-page/viewMode.ts`
- `src/components/ProjectPage.tsx`
- `src/components/FileExplorer.tsx`
- `src/components/FileViewer.tsx`
- `src/components/RightToolbar.tsx`
- `src/types.ts`
- `src/test/ssh-validation.test.ts`
- `src/test/ssh-session.test.ts`
- `src/test/project-location.test.ts`
- `src/test/project-main-view.test.ts`

### Agent Profiles / Claude GPT55

必须保留的行为：
- `AgentType` 支持 `"claude" | "claude_gpt55" | "codex"`。
- `claude_gpt55` 默认使用 `~/.claude/start-gpt55.sh`，脚本内部依赖独立 Codex 配置。
- `claude_gpt55` 是 Codex-like profile：任务启动、恢复、标题生成、提交信息生成、会话字段选择按 Codex-compatible 路径处理。
- Skill Hub 安装目标只面向真实 `claude` / `codex`，不要把 `claude_gpt55` 当成独立 skill 安装目标。
- Hook 能力判断只对真实 `claude` / `codex` 返回可用，`claude_gpt55` 不复用 Claude/Codex hook profile。
- 0.4.1 上游临时策略已合入：任务命名与提交信息生成优先使用真实 `codex` headless；但任务会话摘要仍要按原任务 agent 判断。

关键文件：
- `src/agents.ts`
- `src/types.ts`
- `src-tauri/src/app_settings.rs`
- `src-tauri/src/config.rs`
- `src-tauri/src/pty.rs`
- `src-tauri/src/agent_assist.rs`
- `src-tauri/src/git.rs`
- `src-tauri/src/hooks.rs`
- `src/components/new-task/AgentPermSelector.tsx`
- `src/components/NewTaskView.tsx`
- `src/components/AppSettingsDialog.tsx`
- `src/components/app-settings/*`
- `src/components/RunningView.tsx`
- `src/components/TodoTaskView.tsx`
- `src/components/task-panel/TaskEditDialog.tsx`
- `src/components/task-panel/TaskListItem.tsx`
- `src/components/SettingsDialog.tsx`
- `src/components/skill-hub/SkillInstallDialog.tsx`
- `src/components/skill-hub/SkillManageDialog.tsx`
- `src/test/agent-options.test.ts`

### `/goal` 模式和任务创建体验

必须保留的行为：
- `+` 菜单中的 plan mode 已改为 `/goal mode` / `/goal 模式`。
- 开启 `/goal` 时，任务提示词追加“先列 plan -> 再修改 -> 完成后审查”的工作流。
- 真实 Claude Code 缺少项目根 `CLAUDE.md` 时不显示初始化提示；Codex-compatible profile 仍保留 `AGENTS.md` 提示。

关键文件：
- `src/components/new-task/goalMode.ts`
- `src/components/NewTaskView.tsx`
- `src/components/new-task/AgentPermSelector.tsx`
- `src/i18n.tsx`
- `src/test/new-task-goal-mode.test.ts`

### 技能库 / Skill Hub

必须保留的行为：
- NeZha 的技能库通过 `~/.nezha/skill_hub.json` 指向外部目录加载，不写入 `.app` 包内部。
- 当前本机技能库路径为 `~/.nezha/skills_hub`，包含从本地 `同步空间` / `LYX` 汇总的 skills，包括 `superpowers` 和 `Trellis`。
- 项目内 Skill Hub 相关安装目标和路径安全校验不能放宽。

关键文件：
- `src-tauri/src/skills.rs`
- `src/components/skill-hub/*`
- `src/components/app-settings/SkillsPanel.tsx`
- `src/styles/skill-hub.ts`
- `src/i18n.tsx`

### 其他本地文档 / 项目文件

- `local_change.md`：本地改动记录和合并规范，必须保留。
- `plan.md`、`docs/project-overview.md`：合并前已存在，除非用户明确要求，不要删除。

## 合并冲突热点

下次合并 0.4.2 时重点检查这些文件：
- `src/components/ProjectPage.tsx`：上游 Git 面板、Diff 交互常与本地 SSH 中心终端/远程文件能力重叠。
- `src/components/AppSettingsDialog.tsx`、`src/components/app-settings/types.ts`：上游设置页导航常与本地 `claude_gpt55` 面板重叠。
- `src-tauri/src/agent_assist.rs`、`src-tauri/src/git.rs`、`src-tauri/src/app_settings.rs`：上游 Codex/Claude 策略常与本地 GPT55 profile 重叠。
- `src/i18n.tsx`：上游新增文案常与本地 SSH/GPT55/goal 文案重叠。
- `src/styles/panels.ts`：上游新增面板样式常与本地 SSH 面板样式重叠；处理方式通常是两边都保留，避免删除 `ssh*` 和 `thanks*` 样式。
- `src-tauri/src/lib.rs`：新增 Tauri 命令注册容易互相覆盖，必须同时保留上游命令和本地远程命令。
- `src/types.ts`：数据模型变化必须兼容本地 `SshConnection.password`、`AgentType` 三 profile、项目 `location` 等字段。

## `local_change.md` 读写规范

### 读取规范

- 合并上游、打包替换、修改 SSH/GPT55/goal/Skill Hub 相关逻辑前，必须先读本文件。
- 不只看最新流水记录；必须优先看置顶的“当前基线”“本地个性化改动总览”“合并冲突热点”。
- 如果本文与实际代码不一致，以实际代码为准，同时更新本文说明差异。

### 写入规范

- 新记录写在“历史记录”顶部，按时间倒序追加，标题格式：`## YYYY-MM-DD 主题`。
- 每次合并新版后，必须更新“当前基线”：
  - 上游 tag / commit。
  - 本地合并提交。
  - `/Applications/NeZha.app` 版本。
  - 已安装二进制 SHA256。
- 如果新增或修改本地个性化功能，必须同步更新“本地个性化改动总览”的行为说明和关键文件。
- 如果发生冲突，必须记录：
  - 冲突文件。
  - 选择保留哪边。
  - 是否需要用户判断。
  - 最终验证命令。
- 不要记录敏感值。涉及 SSH 密码、token、私钥时，只记录“字段/能力存在”，不要记录具体内容。
- 保留历史记录，不要为了精简删除旧版本验证结果；旧记录用于回溯当时的打包和安装状态。
- 命令验证要写实际执行结果，不写“应该可用”“预计通过”。

## 历史记录

## 2026-06-12 合并上游 v0.4.1

### 合并策略

- 上游来源：`https://github.com/hanshuaikang/nezha.git`
- 目标版本：`v0.4.1`
- 本地基线：`v0.4.0` 之后的个性化改动已先提交为本地检查点，避免合并时覆盖 SSH、GPT55、远程文件浏览器、技能库等本地功能。
- 合并冲突仅出现在 `src/styles/panels.ts`：
  - 保留本地 SSH 连接/终端/中心面板样式。
  - 追加上游 `ThanksPanel` 相关样式与 `thanksNameBase` 共用常量。

### 上游 v0.4.1 已合入内容

- Git 文件列表树形视图与批量 Git 操作命令。
- Git 首次提交前取消暂存修复。
- 任务命名和提交信息生成在 Claude headless 计费变动期间优先使用真实 Codex。
- 通知轮询。
- App 设置中的社区入口、感谢页和相关资源。
- 版本号更新到 `0.4.1`。

### 本地个性化改动保留点

- SSH 连接密码、远程任务、远程文件浏览器相关前后端代码保留。
- `claude_gpt55` Agent profile、App 设置面板、Codex-like 路径处理保留。
- `/goal` 模式提示词、Claude 缺少 `CLAUDE.md` 时不提示初始化、SSH 项目中心终端保留。
- 技能库相关本地功能保留。

### 合并后验证

- `pnpm lint` 通过。
- `pnpm test` 通过：9 个 test files，63 个 tests。
- `cargo test` 通过：77 个 Rust tests。
- `pnpm build` 通过；仍保留既有 chunk size warning。
- `pnpm tauri build` 通过，生成：
  - `src-tauri/target/release/bundle/macos/NeZha.app`
  - `src-tauri/target/release/bundle/dmg/NeZha_0.4.1_aarch64.dmg`
- 已用 `rsync -aE --delete src-tauri/target/release/bundle/macos/NeZha.app/ /Applications/NeZha.app/` 替换应用。
- 构建产物与 `/Applications/NeZha.app/Contents/MacOS/nezha` 的 SHA256 均为 `0a8f6a4d133830575665bed904bec0dc40ccb242056f8d35fdcacdea2b1b79bd`。
- `/Applications/NeZha.app/Contents/Info.plist` 显示 `CFBundleShortVersionString` / `CFBundleVersion` 均为 `0.4.1`。

## 2026-06-11 CLAUDE Banner, SSH Center Terminal, Goal Mode

### 本轮 Plan

1. 定位 `CLAUDE.md` 缺失提示、SSH 项目中心区渲染、`+` 菜单 plan/goal 模式的现有实现。
2. 先补 RED 测试，覆盖：
   - Claude Code 缺少项目根 `CLAUDE.md` 时不显示初始化提示。
   - `+` 菜单的 `/goal` 模式会把“先列 plan、再修改、完成后审查”的工作流写入任务提示词。
   - SSH 项目在有连接信息时中心区域显示 SSH 终端，而不是普通新任务输入区。
3. 实现最小代码改动：抽出可测试 helper，组件改为调用 helper；SSH 终端面板支持远程项目中心区自动连接与隐藏连接列表。
4. 运行定向测试、全量前端测试/构建、Rust 测试、Tauri 打包。
5. 做 inline self-review，确认范围只覆盖本目标。
6. 用新构建的 `NeZha.app` 替换 `/Applications/NeZha.app`，并把验证结果追加到本文件。

### 实现记录

- 新增 `src/components/new-task/goalMode.ts`
  - `shouldShowInstructionsBanner()`：对真实 `claude` 直接隐藏缺失 `CLAUDE.md` 初始化提示；Codex-compatible profile 仍保留 `AGENTS.md` 提示。
  - `buildPromptWithGoalMode()`：`/goal` 模式开启时，将“先列出 plan -> 再进行修改 -> 修改完成后进行审查”追加到任务提示词。
- 新增 `src/components/project-page/viewMode.ts`
  - `shouldShowRemoteSshTerminal()`：SSH 项目且能解析到连接时，中心区切到 SSH 终端。
- 修改 `src/components/NewTaskView.tsx`
  - 缺失指令文件提示改为调用 `shouldShowInstructionsBanner()`，因此 Claude Code 不再显示“此项目中未找到 CLAUDE.md / 一键初始化”提示。
  - 提交时改用 `buildPromptWithGoalMode()`，`+` 菜单中的模式从普通 plan mode 语义升级为 `/goal` 工作流。
- 修改 `src/components/new-task/AgentPermSelector.tsx` 与 `src/i18n.tsx`
  - `+` 菜单显示 `/goal mode` / `/goal 模式`。
- 修改 `src/components/ProjectPage.tsx`
  - SSH 项目中心区域优先渲染 `SshTerminalPanel`，不再显示不可用的新任务输入区。
  - 传入 `initialConnectionId`、`autoConnect`、`hideConnectionList`，让中心区终端自动使用该远程项目绑定的 SSH 连接。
- 修改 `src/components/ssh/SshTerminalPanel.tsx`
  - 支持 `width` 为字符串，便于中心区使用 `100%`。
  - 支持自动连接和隐藏连接列表，右侧 SSH 管理面板原有行为保持不变。
- 修改 `src/styles/panels.ts`
  - 新增 `sshCenterPanel`，中心区复用 SSH 终端时移除右栏边框并保持铺满布局。
- 新增测试：
  - `src/test/new-task-goal-mode.test.ts`
  - `src/test/project-main-view.test.ts`

### 审查记录

- 已做 inline self-review：
  - 未发现旧的 `Please use plan mode` 残留。
  - `/goal` 提示词包含用户指定的 plan、修改、审查三步。
  - Claude Code 缺失 `CLAUDE.md` 提示已在 helper 层直接关闭。
  - SSH 中心终端只在 SSH 项目且连接可解析时启用；右侧 SSH 面板仍保留为连接管理入口。

### 验证结果

- RED 验证：
  - `pnpm test -- --run src/test/new-task-goal-mode.test.ts src/test/project-main-view.test.ts` 首次失败，原因是目标 helper 文件尚不存在。
- GREEN / 完成验证：
  - `pnpm test -- --run src/test/new-task-goal-mode.test.ts src/test/project-main-view.test.ts` 通过：9 个 test files，63 个 tests。
  - `pnpm lint` 通过。
  - `pnpm build` 通过；仅保留既有 chunk size warning。
  - `pnpm test` 通过：9 个 test files，63 个 tests。
  - `cargo test` 通过：77 个 tests。
  - `pnpm tauri build` 通过，生成：
    - `src-tauri/target/release/bundle/macos/NeZha.app`
    - `src-tauri/target/release/bundle/dmg/NeZha_0.4.0_aarch64.dmg`
- 安装替换：
  - 使用 `rsync -aE --delete src-tauri/target/release/bundle/macos/NeZha.app/ /Applications/NeZHa.app/` 的同等大小写路径完成覆盖；实际执行目标为 `/Applications/NeZha.app/`。
  - 新构建二进制与 `/Applications/NeZha.app/Contents/MacOS/nezha` 的 SHA256 均为 `24a9d048b315531ad538b96b6a2baacd3f043625a1b5582dc8b1af318b48b399`，确认已替换。

### 最终复验记录

- 重新执行 `pnpm test -- --run src/test/new-task-goal-mode.test.ts src/test/project-main-view.test.ts` 通过：9 个 test files，63 个 tests。
- 重新执行 `pnpm lint` 通过。
- 重新执行 `pnpm build` 通过；仅保留既有 chunk size warning。
- 重新执行 `pnpm test` 通过：9 个 test files，63 个 tests。
- 重新执行 `cargo test` 通过：77 个 Rust tests。
- 重新执行 `pnpm tauri build` 通过，生成：
  - `src-tauri/target/release/bundle/macos/NeZha.app`
  - `src-tauri/target/release/bundle/dmg/NeZha_0.4.0_aarch64.dmg`
- 重新执行覆盖安装：
  - `rsync -aE --delete src-tauri/target/release/bundle/macos/NeZha.app/ /Applications/NeZha.app/`
  - 构建产物与 `/Applications/NeZha.app/Contents/MacOS/nezha` 的 SHA256 均为 `24a9d048b315531ad538b96b6a2baacd3f043625a1b5582dc8b1af318b48b399`。

## 2026-06-11 SSH Password, Remote File Explorer, Agent Profiles

本文件记录本地改动范围，方便远端仓库更新后做合并核对。不要在这里记录 SSH 密码、API key、私钥等敏感值。

### 功能概览

- SSH 连接配置支持直接填写密码。
- 后端 SSH 命令在有密码时通过 `sshpass -e` 启动，并用 `SSHPASS` 环境变量传递密码；无密码时仍直接使用系统 `ssh`。
- SSH 项目打开后，右侧“文件浏览器”可浏览远程目录、打开文本/图片、自动保存文本修改、创建文件/文件夹、删除路径。
- SSH 项目继续禁用本地专属能力：本地 shell、Git 面板、文件搜索、项目设置、worktree、附件上传。
- 新任务 Agent 下拉扩展为三项：
  - `claude`：终端直接运行 Claude Code，配置文件为 `~/.claude/settings.json`。
  - `claude_gpt55`：运行 `~/.claude/start-gpt55.sh`，脚本内部使用独立 `CODEX_HOME=~/.codex-gpt55`。
  - `codex`：终端直接运行 `codex`，配置文件为 `~/.codex/config.toml`。
- 已检查本机 Claude 默认模型配置，`~/.claude/settings.json` 中 `ANTHROPIC_DEFAULT_OPUS_MODEL` 已是 `mimo-v2.5-pro`，无需改写。
- App 设置页新增 “Claude GPT55” 配置面板，可查看/编辑 `~/.claude/start-gpt55.sh`，并可保存该脚本路径。
- GPT55 因脚本最终执行 Codex CLI，在任务启动、恢复、标题生成、提交信息生成、会话字段选择上按 Codex-compatible 路径处理。

### 后端修改

- `src-tauri/src/ssh.rs`
  - `SshConnection` 新增可选 `password` 字段。
  - 新增 SSH command spec，区分普通 `ssh` 与密码模式 `sshpass -e ssh`。
  - 新增可复用的 `std_ssh_command_for_remote_command`，供远程文件/Git命令复用密码连接。
  - 远程 Agent 命令支持 `claude_gpt55`，远端默认执行 `~/.claude/start-gpt55.sh`，参数按 Codex-compatible 规则构建。
  - 增加 SSH 密码与 GPT55 远程命令单元测试。

- `src-tauri/src/remote_fs.rs`
  - 新增/完善远程文件命令：列目录、读文本、写文本、读图片预览、创建文件、创建目录、删除路径。
  - 对远程路径做项目根约束，禁止修改远程项目根、`.git`、`.nezha`。
  - 远程目录条目保持与本地文件树一致的 `is_dir/is_gitignored` 字段格式。
  - 文本读取限制 2 MB，图片预览限制 10 MB。

- `src-tauri/src/app_settings.rs`
  - `AppSettings` 新增 `claude_gpt55_path`。
  - `AgentVersions` 新增 `claude_gpt55_version`。
  - `get_agent_launch_spec("claude_gpt55")` 默认解析到 `~/.claude/start-gpt55.sh`。
  - 新增 `is_codex_like_agent()`，统一判断 `codex` 与 `claude_gpt55`。
  - `save_agent_paths` 扩展为保存三路 Agent 路径。

- `src-tauri/src/config.rs`
  - Agent 配置文件映射新增 `claude_gpt55 -> ~/.claude/start-gpt55.sh`。

- `src-tauri/src/pty.rs`
  - 本地任务启动/恢复用 `is_codex_like_agent()` 处理 GPT55。
  - GPT55 不复用 Claude/Codex 全局 hook 判定，回退现有轮询/会话发现路径。

- `src-tauri/src/agent_assist.rs`、`src-tauri/src/git.rs`
  - 任务标题生成与提交信息生成中，GPT55 按 Codex CLI 参数和输出解析处理。

- `src-tauri/src/hooks.rs`
  - `usable_for()` 仅对真实 `claude` / `codex` 返回 hook 可用；未知 profile 返回 false。

- `src-tauri/src/lib.rs`
  - 注册远程文件新增命令：`remote_read_image_preview`、`remote_create_file`、`remote_create_directory`、`remote_delete_path`。

### 前端修改

- `src/types.ts`
  - `SshConnection` 新增 `password?: string`。
  - `AgentType` 扩展为 `"claude" | "claude_gpt55" | "codex"`。
  - Codex-like 的 `auto_edit` 权限标签显示为 `Auto Mode`。

- `src/agents.ts`
  - 新增 Agent profile 的单一来源：`AGENT_OPTIONS`、`agentDisplayLabel()`、`isCodexLikeAgent()`。

- `src/components/ssh/SshConnectionDialog.tsx`、`src/components/ssh/validation.ts`
  - SSH 表单新增密码输入。
  - 空密码不会写入连接对象；非空密码保存在本地 SSH 连接配置中。
  - 文案更新为密码存储提示。

- `src/components/FileExplorer.tsx`
  - 新增 `remote` 上下文，按远程/本地选择 Tauri 命令。
  - 远程模式支持列目录、创建文件/目录、删除路径。
  - 远程模式隐藏“在系统文件夹打开”菜单项。

- `src/components/FileViewer.tsx`
  - 新增 `remote` 上下文。
  - 远程模式支持文本读取、文本自动保存、图片预览。

- `src/components/ProjectPage.tsx`
  - SSH 项目允许打开文件浏览器。
  - 按工具类型细分禁用状态：文件浏览器、Git、本地终端、搜索、设置分别控制。
  - 将远程连接与远程项目根路径传给 FileExplorer/FileViewer。

- `src/components/RightToolbar.tsx`
  - `localToolsDisabled` 拆分为 `filesDisabled`、`gitDisabled`、`terminalDisabled`、`searchDisabled`、`settingsDisabled`。

- `src/components/new-task/AgentPermSelector.tsx`、`src/components/NewTaskView.tsx`
  - Agent 下拉使用 `AGENT_OPTIONS`。
  - GPT55 作为 Codex-like profile 参与 AGENTS/CLAUDE 指令文件判断。

- `src/components/AppSettingsDialog.tsx`
  - App 设置侧栏新增 “Claude GPT55”。

- `src/components/app-settings/*`
  - App 设置类型、路径提示、版本检测、保存逻辑扩展到 `claude_gpt55`。
  - Agent 配置文件高亮语言增加 `shellscript`。

- `src/components/RunningView.tsx`、`src/components/TodoTaskView.tsx`、`src/components/task-panel/TaskEditDialog.tsx`、`src/components/task-panel/TaskListItem.tsx`、`src/components/SettingsDialog.tsx`
  - 统一使用共享 Agent 标签/判断，避免各处硬编码 Claude/Codex 两项。

- `src/components/skill-hub/SkillInstallDialog.tsx`、`src/components/skill-hub/SkillManageDialog.tsx`
  - Skill Hub 安装目标显式收窄为真实 `claude` / `codex`，避免 GPT55 profile 造成重复安装或类型错误。

- `src/i18n.tsx`
  - 新增 SSH 密码提示、GPT55 路径提示、相关中文/英文文案。

### 测试与验证

- `pnpm test -- --run src/test/ssh-validation.test.ts src/test/agent-options.test.ts` 通过。
- `cargo test --lib` 通过。
- `cargo fmt` 已执行。
- `pnpm build` 通过。
- `pnpm lint` 通过。
- `pnpm test` 通过：7 个 test files，56 个 tests。
- `cargo test` 通过：77 个 tests。
- SSH 实机验证通过：
  - 使用用户提供的 SSH 连接信息连接到 `192.168.10.100:22`。
  - 可进入远程 `/home` 并列目录。
  - 可在远程用户目录创建临时文件、读取内容、删除临时文件。
- Agent 启动入口验证通过：
  - `/opt/homebrew/bin/claude --version` 返回 Claude Code 版本。
  - `/opt/homebrew/bin/codex --version` 返回 Codex CLI 版本。
  - `/Users/macbook/.claude/start-gpt55.sh --version` 返回 Codex CLI 版本。
- `pnpm tauri build` 通过。
  - App bundle: `src-tauri/target/release/bundle/macos/NeZha.app`
  - DMG: `src-tauri/target/release/bundle/dmg/NeZha_0.4.0_aarch64.dmg`
- `/Applications/NeZha.app` 已被新构建的 bundle 替换。
- 已验证 `/Applications/NeZha.app/Contents/MacOS/nezha` 中包含新增命令/字段：`remote_read_image_preview`、`remote_create_file`、`save_ssh_connections`、`claude_gpt55`。

### 尚需注意

- SSH 密码会随连接对象保存在本地 `~/.nezha/ssh-connections.json`，当前没有系统钥匙串加密层。
- 远程目录列举使用 GNU `find -printf`，适合常见 Linux 服务器；如果远端是 macOS/BSD，需要后续兼容实现。
- 远程文件浏览器已可读写文件，但远程 Git 面板仍未接入前端，SSH 项目中保持禁用。
- 远程任务附件上传/同步仍未实现，因此 SSH 项目继续禁止本地图片/文本附件。
- GPT55 profile 使用 `~/.claude/start-gpt55.sh` 启动，并依赖脚本内的独立 Codex 配置；不要把它当作普通 Claude Code hook profile 处理。

### Pre-existing Untracked Files

- `plan.md`
- `docs/project-overview.md`

这两个文件在本轮继续执行前已经存在，本次没有把它们纳入变更说明主体。
