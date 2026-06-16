<p align="center">
  <img src="docs/images/logo.png" alt="Aeroric Logo" width="150" />
</p>

<h1 align="center">Aeroric: Desktop Workspace for AI Coding Agents</h1>

<p align="center">
Run Claude Code, Codex, and custom agents across projects with live terminals, task tracking, Git, SSH, Skill Hub, and usage analytics in one lightweight desktop app.
</p>

<p align="center">
  <img src="docs/images/index.gif" alt="Aeroric current workspace" width="86%" />
</p>

[中文文档](./README_ZH.md)

## Why Aeroric

Aeroric is built for agent-first development, where several AI coding tasks may run at the same time across different repositories or remote machines. Instead of switching between terminal tabs, editor windows, Git clients, and session logs, Aeroric keeps the full workflow in one place: start work, watch terminal output, review generated changes, inspect files, resume sessions, and commit code.

The app uses native Claude Code and Codex CLIs rather than replacing them. It adds a desktop control layer around those tools: multi-project navigation, permission-aware task launch, PTY-backed terminals, automatic session discovery, local task persistence, and a focused code review surface.

## Installation

Install Claude Code and/or Codex before using Aeroric. On macOS, if the unsigned app is blocked by Gatekeeper, run:

```bash
xattr -rd com.apple.quarantine /Applications/Aeroric.app
```

## Current Feature Set

- **Multi-project workspace**: switch between local and SSH projects while active tasks keep running in the background.
- **Agent task lifecycle**: create todo tasks, launch Claude Code/Codex/custom agents, resume sessions, cancel work, and track pending, running, input-required, done, failed, and cancelled states.
- **Terminal-first execution**: xterm.js terminals stream real PTY output, support interactive input, smart copy, configurable newline shortcuts, font controls, and IME-safe text entry.
- **Session discovery and replay**: Claude Code and Codex JSONL sessions are detected automatically and shown in the UI for review and recovery.
- **Native Git workflow**: inspect unstaged/staged changes, review diffs, generate commit messages, commit, push, pull, and browse history without leaving the app.
- **Code and file tools**: browse the project tree, preview images, edit source and Markdown with syntax highlighting, and search project files for prompt mentions.
- **SSH and SFTP support**: open remote projects, run remote shells, browse remote files, and manage local connection settings.
- **Skill Hub**: register a local skills folder, edit skills as a project, and keep Superpowers/Trellis-style skill libraries available to agents.
- **Usage analytics**: read agent session metrics for token usage and tool calls so long-running work remains visible.
- **App settings**: configure agent paths, custom agents, language, theme, font family, terminal font size, and task display preferences.

## Screenshots

<p align="center">
  <img src="docs/images/workspace.png" alt="Aeroric workspace" width="86%" />
</p>

<p align="center">
  <img src="docs/images/dark.png" alt="Aeroric dark mode" width="86%" />
</p>

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
