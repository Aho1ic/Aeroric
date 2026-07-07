# Agent Terminal History And Project Click Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Preserve completed Claude/Codex terminal history after marking tasks done and make a single left click on another project open that project's conversation.

**Architecture:** Keep terminal buffers for completed tasks until explicit deletion or rerun/resume, and pass their restore state to the completed-session view as a fallback. Project rail clicks will switch project and select the best target task in the clicked project.

**Tech Stack:** React 19, Vitest, Testing Library, Tauri IPC, xterm restore snapshots.

---

### Task 1: Regression Tests

**Files:**

- Create: `src/test/running-view-history-fallback.test.tsx`
- Modify: `src/test/project-rail-drag.test.tsx`

- [ ] Add a RunningView test where `read_session_messages` returns `[]` for a completed task with a session path and `getRestoreState()` returns terminal data. Expected: the terminal history fallback is rendered.
- [ ] Add a ProjectRail test where clicking an inactive project name calls `onSwitch(project)` and `onSelectTask(recentTask.id)` once.
- [ ] Run both tests and confirm they fail before production changes.

### Task 2: Terminal History Lifecycle

**Files:**

- Modify: `src/App.tsx`

- [ ] Stop clearing task terminal buffers on non-active `task-status` events.
- [ ] Stop clearing terminal buffers in `handleMarkTaskDone`.
- [ ] Keep existing buffer cleanup for delete, failed worktree creation rollback, rerun, and resume.

### Task 3: Completed Session Fallback

**Files:**

- Modify: `src/components/RunningView.tsx`
- Modify: `src/components/SessionView.tsx`

- [ ] Pass terminal restore state from RunningView to SessionView when a task is complete/interrupted/detached.
- [ ] In SessionView, render a read-only TerminalView fallback when session parsing returns no messages or errors and fallback restore state has snapshot/data.
- [ ] Keep the normal parsed conversation view when messages exist.

### Task 4: Single-Click Project Conversation Switch

**Files:**

- Modify: `src/components/ProjectRail.tsx`

- [ ] Add a helper to select the preferred task for a project, preserving the selected task if it already belongs to that project, otherwise choosing the newest task.
- [ ] Update project name click to switch project and select that task in the same click.

### Task 5: Verification, Packaging, And Git

**Files:**

- Verify only unless fixes are required.

- [ ] Run targeted Vitest tests.
- [ ] Run `pnpm test`, `pnpm lint`, `pnpm build`, and Rust tests.
- [ ] Review `git diff` for scope and regressions.
- [ ] Build the Tauri app bundle.
- [ ] Replace `/Applications/Aeroric.app`.
- [ ] Commit on `main` and push.
