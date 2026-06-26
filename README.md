<p align="center">
  <img src="docs/images/logo.png" alt="Aeroric Logo" width="150" />
</p>

<h1 align="center">Aeroric: Desktop Workspace for AI Coding Agents</h1>

<p align="center">
Run Claude Code, Codex, and custom agents across projects with live terminals, task tracking, Git, SSH, SFTP, Docker, database tools, Skill Hub, Markdown docs, quick notes, and release workflows in one lightweight desktop app.
</p>

<p align="center">
  <a href="./README_ZH.md">中文文档</a>
</p>

<p align="center">
  <img src="./Aeroric_frame/递归动画.gif" alt="Aeroric recursive agent workflow" width="86%" />
</p>

## Why Aeroric

Aeroric is built for agent-first development, where multiple AI coding tasks may run at the same time across local repositories, remote machines, and operational environments. Instead of switching between terminal tabs, editors, Git clients, Docker tools, release pages, and session logs, Aeroric keeps the workflow in one desktop workspace: start work, watch terminal output, inspect files, edit Markdown, keep quick notes, run scripts, review changes, and publish releases.

Aeroric does not replace Claude Code or Codex. It calls the native CLIs and adds a desktop control layer around them: multi-project navigation, permission-aware task launch, PTY-backed terminals, automatic session discovery, file browsing, SFTP/SSH operations, Git review, Docker visibility, database utilities, and local task persistence.

## What You Can Do

- **Run recursive agent workflows**: start tasks, inspect output, branch into follow-up actions, and keep the evolving context visible.
- **Manage project workspaces**: open local or remote projects and organize tasks, files, Git state, and runtime status around each repository.
- **Run Claude Code, Codex, and custom agents**: create tasks, choose permission modes, stream PTY output, provide interactive input, resume sessions, and cancel work when needed.
- **Browse, edit, and execute project files**: inspect repository files, modify scripts, run commands, and keep file operations close to the agent conversation.
- **Read and edit Markdown**: switch between rendered reading mode and source editing mode for README files, plans, specs, and generated reports.
- **Keep quick notes**: write Markdown or rich text notes for task clues, command snippets, release checks, and temporary ideas.
- **Operate development infrastructure**: view Docker containers and images, use SFTP/SSH tools, and inspect database-related resources.
- **Keep skills and release work close**: browse local Skill Hub content, review diffs, stage files, commit, push, and manage release pages.
- **Track usage and sessions**: discover Claude Code/Codex JSONL sessions and inspect token/tool-call metrics for long-running work.

## Product Tour

### Recursive Agent Workflow

Tasks can launch, stream output, branch into follow-up actions, and remain visible as they evolve, which fits multi-step debugging, code generation, and release preparation.

<p align="center">
  <img src="./Aeroric_frame/递归动画.gif" alt="Recursive agent workflow animation" width="86%" />
</p>

### Project Workspace

The project page keeps task lists, agent controls, file tools, Git context, and workspace actions together so each repository can be managed without losing its state.

<p align="center">
  <img src="./Aeroric_frame/项目首页.png" alt="Aeroric project workspace" width="86%" />
</p>

<p align="center">
  <img src="./Aeroric_frame/深色模式项目首页.jpg" alt="Dark project workspace" width="86%" />
</p>

### Claude Terminal and IDE Workspace

Claude Code and other agents run in PTY-backed terminals with live output, interactive input, session controls, copy behavior, font controls, file context, and IME-safe text entry.

<p align="center">
  <img src="./Aeroric_frame/Claude终端.png" alt="Claude terminal inside Aeroric" width="86%" />
</p>

<p align="center">
  <img src="./Aeroric_frame/深色模式IDE.jpg" alt="Dark IDE workspace" width="86%" />
</p>

### Browse, Modify, and Run Scripts

Aeroric keeps file browsing, source edits, and command execution in the same workflow, which is useful for agent-assisted debugging, script iteration, and repository maintenance.

<p align="center">
  <img src="./Aeroric_frame/浏览-修改-运行脚本.png" alt="Browse, modify, and run scripts" width="86%" />
</p>

<p align="center">
  <img src="./Aeroric_frame/深色模式文件浏览器.jpg" alt="Dark file browser" width="86%" />
</p>

### Markdown Reading Mode

Rendered Markdown preview is built into the file viewer, making README files, plans, specs, and generated reports easy to review before committing them.

<p align="center">
  <img src="./Aeroric_frame/阅读模式查看markdown文件.png" alt="Markdown reading mode" width="86%" />
</p>

### Markdown Editing Mode

Markdown files can also be edited directly with syntax-aware source mode, so documentation updates stay inside the same project workspace.

<p align="center">
  <img src="./Aeroric_frame/编辑模式查看markdown文件.png" alt="Markdown editing mode" width="86%" />
</p>

### Quick Notes

Quick notes support Markdown and rich text content for task clues, command snippets, release checks, and temporary ideas.

<p align="center">
  <img src="./Aeroric_frame/随手记.jpg" alt="Quick notes" width="86%" />
</p>

<p align="center">
  <img src="./Aeroric_frame/深色模式-阅读视图-markdown随手记.jpg" alt="Dark Markdown quick note reading view" width="86%" />
</p>

### Docker View

The Docker page lists containers and images with status, runtime, port mappings, and refresh controls for local development and deployment checks.

<p align="center">
  <img src="./Aeroric_frame/docker.jpg" alt="Docker containers and images view" width="86%" />
</p>

<p align="center">
  <img src="./Aeroric_frame/深色模式Docker.jpg" alt="Dark Docker containers and images view" width="86%" />
</p>

### SFTP

SFTP tools keep remote file transfer and remote project inspection available next to local project work, which reduces context switching during deployment or server-side fixes.

<p align="center">
  <img src="./Aeroric_frame/SFTP.jpg" alt="SFTP file transfer view" width="86%" />
</p>

<p align="center">
  <img src="./Aeroric_frame/深色模式SFTP.jpg" alt="Dark SFTP file transfer view" width="86%" />
</p>

### SSH

SSH connections can be managed from Aeroric so remote shells, project operations, and agent-assisted terminal work stay in the same desktop environment.

<p align="center">
  <img src="./Aeroric_frame/SSH.jpg" alt="SSH connection view" width="86%" />
</p>

<p align="center">
  <img src="./Aeroric_frame/深色模式SSH.jpg" alt="Dark SSH connection view" width="86%" />
</p>

### Skill Hub

The Skill Hub makes local skill libraries visible and editable, helping agents reuse team workflows, coding conventions, and specialized procedures.

<p align="center">
  <img src="./Aeroric_frame/技能库.jpg" alt="Aeroric Skill Hub" width="86%" />
</p>

### Database Tools

Database-oriented project utilities are grouped in the sidebar so application state and supporting resources can be inspected without leaving the workspace.

<p align="center">
  <img src="./Aeroric_frame/数据库.jpg" alt="Database tools view" width="86%" />
</p>

<p align="center">
  <img src="./Aeroric_frame/深色模式数据库.jpg" alt="Dark database tools view" width="86%" />
</p>

### Release Page

The release workflow helps collect version context, review publish state, and keep release work close to the code changes that produced it.

<p align="center">
  <img src="./Aeroric_frame/版本发布页面.png" alt="Aeroric release page" width="86%" />
</p>

## Installation

Install Claude Code and/or Codex before using Aeroric. On macOS, if the unsigned app is blocked by Gatekeeper, run:

```bash
xattr -rd com.apple.quarantine /Applications/Aeroric.app
```

## Development

```bash
pnpm dev            # Start Vite dev server on port 1420
pnpm build          # Type-check and build frontend
pnpm lint           # Run ESLint
pnpm test           # Run Vitest
pnpm tauri dev      # Start the desktop app
pnpm tauri build    # Build production desktop bundles
```

The frontend is React 19 + TypeScript + Vite. The desktop shell is Tauri 2 + Rust. Backend commands live in `src-tauri/src/`, and most application state is owned by `src/App.tsx` and persisted through Tauri storage commands.

## Acknowledgments

Aeroric builds on excellent open-source projects including [Tauri](https://github.com/tauri-apps/tauri), [React](https://github.com/facebook/react), [xterm.js](https://github.com/xtermjs/xterm.js), [CodeMirror](https://codemirror.net/), and [Shiki](https://shiki.style/).
