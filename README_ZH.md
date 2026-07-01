<p align="center">
  <img src="docs/images/logo.png" alt="Aeroric Logo" width="150" />
</p>

<h1 align="center">Aeroric：面向 AI 编程智能体的桌面工作台</h1>

<p align="center">
  在一个轻量桌面应用里管理 Claude Code、Codex、自定义智能体、多项目任务、实时终端、Git、SSH、SFTP、Docker、数据库、Skill Hub、Markdown 文档、随手记和版本发布流程。
</p>

<p align="center">
  <a href="./README.md">English README</a>
</p>

<p align="center">
  <strong>当前版本：</strong> v1.2.2 · <strong>技术栈：</strong> React 19 / Tauri 2 / Rust · <strong>平台：</strong> macOS / Windows / Linux
</p>

<p align="center">
  <img src="./Aeroric_frame/递归动画.gif" alt="Aeroric 递归智能体任务流" width="86%" />
</p>

## 为什么是 Aeroric

Aeroric 面向 agent-first 的开发方式：多个 AI 编程任务可能同时在本地仓库、远程机器和运维环境中运行。你不需要在终端、编辑器、Git 客户端、Docker 工具、数据库控制台、发布页面和会话日志之间来回切换，Aeroric 把任务下发、终端输出、文件查看、代码与 Markdown 编辑、随手记、脚本运行、代码 Review 和版本发布都放到同一个桌面工作台里。

Aeroric 不替代 Claude Code 或 Codex，而是直接调用本机 CLI，并在外层补齐桌面任务管理能力：多项目导航、权限模式选择、PTY 终端、会话自动发现、文件浏览、LSP 编辑能力、SFTP/SSH 操作、Git 差异查看、Docker 状态查看、数据库工具、本地任务持久化和版本发布协同。

## 可以做什么

- **递归式智能体任务流**：启动任务、查看输出、派生后续操作，并在演进过程中持续跟踪上下文。
- **管理项目工作区**：打开本地或远程项目，让任务、文件、Git 和运行状态围绕项目组织。
- **运行 Claude Code、Codex 和自定义智能体**：创建任务、选择权限模式、查看 PTY 实时输出、交互输入、恢复会话和取消任务。
- **浏览、修改并运行项目文件**：在同一流程里完成仓库文件浏览、源码修改、脚本执行、语言服务辅助和调试迭代。
- **阅读和编辑 Markdown**：在渲染阅读模式与源码编辑模式之间切换，适合维护 README、计划、规格文档、自动生成报告和本地知识笔记。
- **使用 IDE 级项目工具**：搜索替换、诊断查看、符号跳转、测试运行、DAP 调试、运行配置和本地 Web 预览。
- **使用随手记**：快速记录 Markdown 或富文本内容，并在阅读/编辑之间切换。
- **操作开发基础设施**：查看 Docker 容器和镜像、管理端口、使用 SFTP/SSH 工具，并通过 DBX 能力检查 SQLite、MySQL、PostgreSQL、Redis 和 MongoDB 资源。
- **集中管理技能与发布流程**：查看本地 Skill Hub，Review diff、暂存、提交、推送并管理版本发布页面。
- **跟踪用量、会话与通知**：自动发现 Claude Code / Codex 的 JSONL 会话，查看 token 消耗和工具调用指标，让长时间任务可观测。

## 架构概览

| 层级 | 作用 |
| --- | --- |
| React 19 + TypeScript + Vite | 主工作区 UI、项目面板、编辑器界面、任务视图和发布页面。 |
| Tauri 2 + Rust | 桌面壳、原生文件/进程访问、PTY 编排、存储、Git、SSH/SFTP、Docker 和数据库命令。 |
| 智能体运行桥接层 | 以权限模式启动 Claude Code、Codex 和自定义命令，支持 hook 集成、会话发现、恢复和取消。 |
| 项目工具链 | 文件浏览、CodeMirror/Shiki 编辑、LSP 诊断/跳转、DAP 调试、搜索、测试面板、Web 预览和本地历史。 |
| 运维工具链 | Docker、端口、SSH 隧道、SFTP、DBX 驱动的数据库浏览/查询/导入导出、通知和发布资产流程。 |

## 产品功能截图

### 递归智能体任务流

任务可以启动、输出、派生后续操作，并在 Aeroric 中持续可见，适合多步骤调试、代码生成和发布准备。

<p align="center">
  <img src="./Aeroric_frame/递归动画.gif" alt="递归智能体任务动画" width="86%" />
</p>

### 项目首页

项目页把任务列表、智能体控制、文件工具、Git 上下文和工作区操作放在一起，让每个仓库都能独立管理并保留状态。

<p align="center">
  <img src="./Aeroric_frame/项目首页.png" alt="Aeroric 项目首页" width="86%" />
</p>

<p align="center">
  <img src="./Aeroric_frame/深色模式项目首页.jpg" alt="深色模式项目首页" width="86%" />
</p>

### Claude 终端与 IDE 工作区

Claude Code 和其他智能体运行在 PTY 终端中，支持实时输出、交互输入、会话控制、复制、字体设置、文件上下文和输入法安全输入。

<p align="center">
  <img src="./Aeroric_frame/Claude终端.png" alt="Aeroric Claude 终端" width="86%" />
</p>

<p align="center">
  <img src="./Aeroric_frame/深色模式IDE.jpg" alt="深色模式 IDE 工作区" width="86%" />
</p>

### 浏览、修改、运行脚本

Aeroric 把文件浏览、源码编辑和命令执行放在同一工作流里，适合智能体辅助调试、脚本迭代和仓库维护。

<p align="center">
  <img src="./Aeroric_frame/浏览-修改-运行脚本.png" alt="浏览、修改、运行脚本" width="86%" />
</p>

<p align="center">
  <img src="./Aeroric_frame/深色模式文件浏览器.jpg" alt="深色模式文件浏览器" width="86%" />
</p>

### 阅读 Markdown

内置 Markdown 渲染预览，便于在提交前检查 README、计划文档、规格说明和自动生成报告。

<p align="center">
  <img src="./Aeroric_frame/阅读模式查看markdown文件.png" alt="阅读模式查看 Markdown 文件" width="86%" />
</p>

### 编辑 Markdown

Markdown 文件也可以直接进入源码编辑模式，文档维护不需要离开当前项目工作台。

<p align="center">
  <img src="./Aeroric_frame/编辑模式查看markdown文件.png" alt="编辑模式查看 Markdown 文件" width="86%" />
</p>

### 随手记

随手记支持 Markdown 和富文本内容，用于记录任务线索、命令片段、发布检查项和临时想法。

<p align="center">
  <img src="./Aeroric_frame/随手记.jpg" alt="随手记" width="86%" />
</p>

<p align="center">
  <img src="./Aeroric_frame/深色模式-阅读视图-markdown随手记.jpg" alt="深色模式 Markdown 随手记阅读视图" width="86%" />
</p>

### Docker

Docker 页面展示容器和镜像列表、状态、运行时长、端口映射和刷新控制，便于检查本地开发与部署环境。

<p align="center">
  <img src="./Aeroric_frame/docker.jpg" alt="Docker 容器与镜像视图" width="86%" />
</p>

<p align="center">
  <img src="./Aeroric_frame/深色模式Docker.jpg" alt="深色模式 Docker 容器与镜像视图" width="86%" />
</p>

### SFTP

SFTP 工具把远程文件传输和远程项目查看放在本地项目工作流旁边，适合部署、排障和服务器侧文件维护。

<p align="center">
  <img src="./Aeroric_frame/SFTP.jpg" alt="SFTP 文件传输视图" width="86%" />
</p>

<p align="center">
  <img src="./Aeroric_frame/深色模式SFTP.jpg" alt="深色模式 SFTP 文件传输视图" width="86%" />
</p>

### SSH

SSH 连接可以在 Aeroric 中集中管理，远程 Shell、项目操作和智能体辅助终端任务都能留在同一个桌面环境里。

<p align="center">
  <img src="./Aeroric_frame/SSH.jpg" alt="SSH 连接视图" width="86%" />
</p>

<p align="center">
  <img src="./Aeroric_frame/深色模式SSH.jpg" alt="深色模式 SSH 连接视图" width="86%" />
</p>

### 技能库

技能库用于查看和编辑本地 skills，让智能体复用团队工作流、代码规范和专用处理流程。

<p align="center">
  <img src="./Aeroric_frame/技能库.jpg" alt="Aeroric 技能库" width="86%" />
</p>

### 数据库

数据库相关工具集中在侧边栏中，方便在不离开工作台的情况下查看应用状态和配套资源。

<p align="center">
  <img src="./Aeroric_frame/数据库.jpg" alt="数据库工具视图" width="86%" />
</p>

<p align="center">
  <img src="./Aeroric_frame/深色模式数据库.jpg" alt="深色模式数据库工具视图" width="86%" />
</p>

### 版本发布页面

版本发布页面用于汇总版本上下文、检查发布状态，并把发布动作和对应代码变更放在同一个桌面流程里。

<p align="center">
  <img src="./Aeroric_frame/版本发布页面.png" alt="版本发布页面" width="86%" />
</p>

## 安装

从 GitHub Releases 页面下载对应平台安装包。每个桌面版本应包含 macOS DMG、Windows NSIS/MSI、Linux DEB/RPM 以及 `SHA256SUMS.txt` 校验文件。

使用 Aeroric 的智能体任务前，请先安装 Claude Code 和/或 Codex。macOS 首次打开未签名应用时，如果系统提示应用已损坏或无法打开，执行：

```bash
xattr -rd com.apple.quarantine /Applications/Aeroric.app
```

## 开发

本地构建需要 Node.js 24、pnpm 9、Rust stable、当前系统对应的 Tauri 平台依赖，以及满足 `src-tauri/Cargo.toml` 路径依赖的同级 DBX 仓库：

```bash
git clone https://github.com/Aho1ic/dbx.git ../dbx
```

```bash
pnpm dev            # 启动 Vite 开发服务器，端口 1420
pnpm build          # 类型检查并构建前端
pnpm lint           # 运行 ESLint
pnpm test           # 运行 Vitest
pnpm tauri dev      # 启动桌面应用
pnpm tauri build    # 构建生产桌面包
```

前端使用 React 19 + TypeScript + Vite，桌面壳使用 Tauri 2 + Rust。后端命令位于 `src-tauri/src/`，核心应用状态由 `src/App.tsx` 管理，并通过 Tauri 存储命令持久化。

## 发布检查

发布 `v1.2.2` 这类 tag 前，需要确保 `package.json`、`src-tauri/tauri.conf.json` 和 `src-tauri/Cargo.toml` 版本一致。桌面构建工作流完成后，确认 Release 中包含：

- `Aeroric-X.Y.Z-1.x86_64.rpm`
- `Aeroric_X.Y.Z_aarch64.dmg`
- `Aeroric_X.Y.Z_amd64.deb`
- `Aeroric_X.Y.Z_arm64-setup.exe`
- `Aeroric_X.Y.Z_arm64_en-US.msi`
- `Aeroric_X.Y.Z_x64-setup.exe`
- `Aeroric_X.Y.Z_x64.dmg`
- `Aeroric_X.Y.Z_x64_en-US.msi`
- `SHA256SUMS.txt`

## 致谢

Aeroric 基于 [Tauri](https://github.com/tauri-apps/tauri)、[React](https://github.com/facebook/react)、[xterm.js](https://github.com/xtermjs/xterm.js)、[CodeMirror](https://codemirror.net/) 和 [Shiki](https://shiki.style/) 等优秀开源项目构建。

特别感谢 [hanshuaikang/nezha](https://github.com/hanshuaikang/nezha) 和 [t8y2/dbx](https://github.com/t8y2/dbx)，这些开源工作为 Aeroric 的智能体工作台和数据库工具提供了启发与参考。

链接认可 LINUX DO 社区：[linux.do](https://linux.do)。
