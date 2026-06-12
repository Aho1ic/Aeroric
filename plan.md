# NeZha SSH Remote MVP Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use `superpowers:subagent-driven-development` if subagents are explicitly authorized, otherwise use `superpowers:executing-plans` to implement this plan task-by-task. Every stage must end with review before moving to the next stage.

**Goal:** Add SSH remote-server support to NeZha so the packaged `NeZha.app` can connect to remote machines, open interactive remote terminals, and later grow toward a VS Code Remote-like workflow.

**First delivery scope:** SSH connection management, interactive remote terminal, and optional remote directory startup. This covers the first usable version of "connect to a remote server from NeZha".

**Architecture:** Reuse the existing `portable-pty` and xterm pipeline instead of embedding a Rust SSH implementation. NeZha launches the system `ssh` command inside a PTY, stores non-secret connection metadata under `~/.nezha/ssh-connections.json`, and routes SSH terminal I/O through the same PTY maps used by local shells.

**Tech Stack:** React 19, TypeScript, Tauri 2, Rust, `portable-pty`, xterm.js, Radix UI, lucide-react.

---

## Requirement Confirmation

This plan satisfies the requested direction:

- Add SSH connection to remote servers: covered by Stages 1-5.
- Similar to VS Code Remote: MVP provides remote terminal first; project binding, remote agent tasks, remote file/Git support are planned as follow-up stages.
- Final packaged app: Stage 8 builds `NeZha.app` with `pnpm tauri build`.
- Review after every stage: every stage includes a required review gate.

First round intentionally does not save SSH passwords and does not use `russh`. Authentication is delegated to system SSH, `ssh-agent`, key files, `~/.ssh/config`, and interactive terminal prompts.

## Current Codebase Fit

Confirmed integration points:

- `src-tauri/src/pty.rs` already owns PTY maps, `open_shell`, `send_input`, `resize_pty`, `kill_shell`, and `shell-output` events.
- `src/components/ShellTerminalPanel.tsx` already wraps xterm.js and invokes `open_shell`, `send_input`, `resize_pty`, and `kill_shell`.
- `src/components/RightToolbar.tsx` is the right-side entry point for panels and terminal controls.
- `src/hooks/useProjectPanels.ts` currently defines `RightPanel = "files" | "git-changes" | "git-history" | null`.
- `src/types.ts` and `src-tauri/src/storage.rs` are the schema authority for persisted frontend/backend models.
- New persisted app-level SSH data should live outside `Project` and `Task` storage until remote projects are introduced.

## Execution Rules

- Do not implement on `main` unless the user explicitly confirms this is acceptable. Prefer an isolated worktree before code changes.
- Use TDD for behavior changes: write the failing test, confirm it fails for the expected reason, implement the minimal fix, then confirm it passes.
- Preserve existing local project behavior and existing shell behavior.
- Do not store passwords, passphrases, or private key contents.
- Do not expose arbitrary SSH arguments in the MVP. Add only explicit fields: host, port, username, identity file, remote path.
- All blocking filesystem/process work in new Tauri async commands must use `tokio::task::spawn_blocking`.
- Every stage must finish with: targeted tests, code review, and stage review notes.

## Review Gate Template

Use this after every stage:

```markdown
### Stage N Review

**Spec compliance**
- [ ] Stage requirements implemented exactly.
- [ ] No unrelated behavior added.
- [ ] Existing local shell/project/task workflows preserved.

**Code quality**
- [ ] TypeScript strict types, no new `any`.
- [ ] Rust structs mirror persisted TypeScript models where applicable.
- [ ] Blocking work is not run directly on the Tauri async runtime.
- [ ] No secrets are persisted or logged.
- [ ] Shell/SSH arguments avoid injection risks.

**Verification**
- [ ] Targeted tests passed.
- [ ] `pnpm lint` passed or failure documented.
- [ ] `pnpm test` passed or failure documented.
- [ ] `pnpm build` passed or failure documented.
- [ ] `cargo test` passed or failure documented.

**Reviewer result**
- [ ] Approved to proceed.
- [ ] Required fixes completed before next stage.
```

If subagents are explicitly authorized, run two reviews after each stage: spec compliance review first, then code quality review. If not, perform an inline review using the same checklist and include findings in the stage notes.

---

## Stage 1: SSH Connection Model and Persistence

**Goal:** Add a non-secret SSH connection model shared by frontend and backend, persisted in `~/.nezha/ssh-connections.json`.

**Files:**

- Modify: `src/types.ts`
- Create: `src-tauri/src/ssh.rs`
- Modify: `src-tauri/src/lib.rs`
- Test: Rust unit tests in `src-tauri/src/ssh.rs`

### Data Model

Add to `src/types.ts`:

```ts
export interface SshConnection {
  id: string;
  name: string;
  host: string;
  port: number;
  username: string;
  identityFile?: string;
  remotePath?: string;
  createdAt: number;
  lastConnectedAt?: number;
}
```

Add the mirrored Rust struct in `src-tauri/src/ssh.rs`:

```rust
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct SshConnection {
    pub id: String,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    #[serde(rename = "identityFile", skip_serializing_if = "Option::is_none")]
    pub identity_file: Option<String>,
    #[serde(rename = "remotePath", skip_serializing_if = "Option::is_none")]
    pub remote_path: Option<String>,
    #[serde(rename = "createdAt")]
    pub created_at: i64,
    #[serde(rename = "lastConnectedAt", skip_serializing_if = "Option::is_none")]
    pub last_connected_at: Option<i64>,
}
```

### Commands

Implement Tauri commands:

- `load_ssh_connections() -> Result<Vec<SshConnection>, String>`
- `save_ssh_connections(connections: Vec<SshConnection>) -> Result<(), String>`

Storage path:

```text
~/.nezha/ssh-connections.json
```

Use existing `storage::nezha_dir()` and `storage::atomic_write()` rather than duplicating app data path logic.

### Tests

Write Rust tests first:

```rust
#[test]
fn ssh_connection_deserializes_without_optional_fields() {
    let raw = r#"{
      "id":"conn-1",
      "name":"prod",
      "host":"prod.example.com",
      "port":22,
      "username":"deploy",
      "createdAt":1700000000000
    }"#;

    let connection: SshConnection = serde_json::from_str(raw).unwrap();

    assert_eq!(connection.identity_file, None);
    assert_eq!(connection.remote_path, None);
    assert_eq!(connection.last_connected_at, None);
}
```

Run targeted verification:

```bash
cd src-tauri
cargo test ssh_connection_deserializes_without_optional_fields
```

### Stage 1 Review

Run the review gate template before Stage 2. Confirm no password/passphrase field exists in TypeScript, Rust, or persisted JSON.

---

## Stage 2: SSH Command Builder and PTY Opening

**Goal:** Launch system `ssh` in a PTY and route output through existing terminal infrastructure.

**Files:**

- Modify: `src-tauri/src/ssh.rs`
- Modify: `src-tauri/src/pty.rs` if shared PTY helpers need `pub(crate)` visibility
- Modify: `src-tauri/src/lib.rs`
- Test: Rust unit tests in `src-tauri/src/ssh.rs`

### Backend Commands

Add:

```rust
#[tauri::command]
pub async fn open_ssh_shell(
    app: tauri::AppHandle,
    task_manager: tauri::State<'_, crate::TaskManager>,
    shell_id: String,
    connection: SshConnection,
    cols: Option<u16>,
    rows: Option<u16>,
) -> Result<(), String>
```

Add:

```rust
#[tauri::command]
pub async fn kill_ssh_shell(
    task_manager: tauri::State<'_, crate::TaskManager>,
    shell_id: String,
) -> Result<(), String>
```

`send_input` and `resize_pty` should continue to work because SSH shells use the same `TaskManager` PTY maps.

### Command Construction

Build arguments as a vector, not by string concatenation:

```text
ssh -tt -p <port> [-i <identityFile>] <username>@<host>
```

When `remotePath` exists, append one remote command string:

```text
cd -- '<quoted remote path>' && exec "${SHELL:-/bin/sh}" -l
```

Implement a focused helper:

```rust
fn shell_quote_posix(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}
```

Do not add `extraArgs` in the MVP. If the TypeScript proposal keeps `extraArgs`, leave it out of the first implementation or keep it ignored until a whitelist is designed.

### Tests

Write Rust tests first for:

- default port is preserved as `22`
- identity file adds `-i`
- remote path is quoted
- single quotes in remote path are escaped
- host and username are not interpolated into a local shell command

Example:

```rust
#[test]
fn shell_quote_posix_escapes_single_quotes() {
    assert_eq!(
        shell_quote_posix("/srv/app's repo"),
        "'/srv/app'\\''s repo'"
    );
}
```

Run targeted verification:

```bash
cd src-tauri
cargo test ssh
```

### Stage 2 Review

Run the review gate template before Stage 3. Pay special attention to shell injection, PTY cleanup, and child process kill behavior.

---

## Stage 3: SSH Connection Management UI

**Goal:** Let users create, edit, delete, and select SSH connections inside NeZha.

**Files:**

- Create: `src/components/ssh/SshConnectionDialog.tsx`
- Create: `src/components/ssh/SshConnectionList.tsx`
- Create: `src/components/ssh/SshTerminalPanel.tsx` only if needed in this stage; otherwise Stage 4
- Modify: `src/App.tsx` or extract a focused hook if state growth becomes too large
- Modify: `src/i18n.ts` or the current i18n resource file used by the project
- Modify: `src/styles/panels.ts` or the closest existing style module
- Test: frontend tests for validation/normalization where practical

### UI Behavior

Connection fields:

- Name
- Host or `~/.ssh/config` alias
- Port, default `22`
- Username
- Identity file path, optional
- Remote path, optional

Validation:

- Name is required.
- Host is required.
- Port must be `1..=65535`.
- Username is required unless the host field intentionally supports a full SSH alias workflow. First implementation should keep username required for clarity.
- Password is not available as a saved field.

Use existing UI patterns:

- lucide `Server` or `Network` icon.
- Radix UI for dialogs/select-like controls where applicable.
- No new CSS framework.
- Styles go into `src/styles/`, not inline styles for new UI.

### Frontend State

Load on app startup:

```ts
const connections = await invoke<SshConnection[]>("load_ssh_connections");
```

Save after changes:

```ts
await invoke<void>("save_ssh_connections", { connections: nextConnections });
```

### Tests

Add focused tests for pure validation/normalization helpers:

```ts
expect(normalizeSshPort("")).toBe(22);
expect(normalizeSshPort("0")).toBeNull();
expect(normalizeSshPort("65536")).toBeNull();
expect(normalizeSshPort("2200")).toBe(2200);
```

Run targeted verification:

```bash
pnpm test -- --run
pnpm lint
```

### Stage 3 Review

Run the review gate template before Stage 4. Confirm UI does not imply password persistence and does not expose arbitrary SSH flags.

---

## Stage 4: SSH Terminal Panel and Right Toolbar Entry

**Goal:** Add a right-side SSH panel that opens an interactive remote terminal using `open_ssh_shell`.

**Files:**

- Modify: `src/hooks/useProjectPanels.ts`
- Modify: `src/components/RightToolbar.tsx`
- Create or complete: `src/components/ssh/SshTerminalPanel.tsx`
- Consider extracting shared terminal instance logic from `src/components/ShellTerminalPanel.tsx` only if duplication becomes risky
- Modify: `src/components/ProjectPage.tsx`
- Modify: i18n resources
- Modify: style modules under `src/styles/`

### Panel Integration

Extend:

```ts
type RightPanel = "files" | "git-changes" | "git-history" | "ssh" | null;
```

Add a toolbar button with lucide `Server` or `Network`:

```tsx
{ key: "ssh", icon: <Server size={17} />, title: t("ssh.title") }
```

`SshTerminalPanel` should:

- List saved connections.
- Open an xterm terminal for the selected connection.
- Call `open_ssh_shell`.
- Send keyboard input through existing `send_input`.
- Resize through existing `resize_pty`.
- Kill through `kill_ssh_shell` on terminal close.
- Display SSH output by listening to the chosen event route.

Recommended event route for MVP:

- Reuse `shell-output` with `shell_id` if the backend registers SSH sessions using IDs like `ssh:<connectionId>:<timestamp>`.
- This preserves current frontend event plumbing and avoids a second output event type.

### Manual Test Requirements

Run these manually before approving the stage:

- Connect using a `~/.ssh/config` alias.
- Connect using host + username + port.
- Connect using an identity file.
- Interact with password/key passphrase prompts in terminal.
- Run `vim`, `top`, `sudo`, and `tmux` in the SSH terminal.
- Close the panel and confirm the remote SSH child process is killed.

### Stage 4 Review

Run the review gate template before Stage 5. Confirm local terminal behavior remains unchanged.

---

## Stage 5: Remote Directory Startup

**Goal:** When a connection has `remotePath`, SSH should start the login shell in that remote directory.

**Files:**

- Modify: `src-tauri/src/ssh.rs`
- Modify: `src/components/ssh/SshConnectionDialog.tsx`
- Modify: `src/components/ssh/SshTerminalPanel.tsx`
- Test: Rust tests for remote command construction

### Behavior

If `remotePath` is set:

```text
ssh -tt user@host 'cd -- <quoted-remote-path> && exec "${SHELL:-/bin/sh}" -l'
```

If `remotePath` is empty:

```text
ssh -tt user@host
```

If the remote directory does not exist, SSH should show the remote shell error in the terminal. Do not hide or reinterpret this error in the MVP.

### Tests

Write Rust tests first:

```rust
#[test]
fn remote_command_changes_directory_before_login_shell() {
    let command = build_remote_start_command("/srv/nezha app");
    assert_eq!(
        command,
        "cd -- '/srv/nezha app' && exec \"${SHELL:-/bin/sh}\" -l"
    );
}
```

Run:

```bash
cd src-tauri
cargo test remote_command
```

### Stage 5 Review

Run the review gate template. This is the MVP completion review. Include a requirement mapping:

- SSH connection management works.
- Remote terminal works.
- Remote directory startup works.
- No passwords are stored.
- `NeZha.app` build path remains supported.

---

## Stage 6: Remote Project Location (Follow-up)

**Goal:** Extend `Project` to support local and SSH locations without breaking existing `projects.json`.

**Files:**

- Modify: `src/types.ts`
- Modify: `src-tauri/src/storage.rs`
- Modify: project creation/opening flows in `src/App.tsx` and related components
- Test: frontend and Rust compatibility tests

### Proposed Model

```ts
export type ProjectLocation =
  | { kind: "local"; path: string }
  | { kind: "ssh"; connectionId: string; remotePath: string };

export interface Project {
  id: string;
  name: string;
  path: string;
  location?: ProjectLocation;
  branch?: string;
  lastOpenedAt: number;
  hiddenFromRail?: boolean;
}
```

Compatibility rule:

- If `location` is missing, treat the project as `{ kind: "local", path: project.path }`.
- Do not rename or remove `path` until there is a migration plan.

### Stage 6 Review

Run full schema review. Confirm old `~/.nezha/projects.json` files deserialize unchanged.

---

## Stage 7: Remote Agent Tasks (Follow-up)

**Goal:** Run Claude Code or Codex on a remote server through SSH.

**Backend commands:**

- `run_remote_task(...)`
- `resume_remote_task(...)`
- `cancel_remote_task(...)`

Remote command shape:

```text
ssh -tt user@host 'cd -- <remote-project> && claude --permission-mode default'
```

or:

```text
ssh -tt user@host 'cd -- <remote-project> && codex'
```

Rules:

- Remote machines must install Claude Code or Codex themselves.
- NeZha does not upload agent binaries.
- First version may skip remote JSONL session discovery.
- Task status may be based on SSH child process exit code for the first version.
- Terminal output still goes through xterm.

### Stage 7 Review

Review remote command quoting, permission mode mapping, cancellation, and user-facing failure reasons.

---

## Stage 8: Remote Files, Git, and Packaging

**Goal:** Move toward VS Code Remote-like project workflows and package the app.

### Remote Files and Git

Recommended first route: lightweight SSH commands.

Potential modules:

- `src-tauri/src/remote_fs.rs`
- `src-tauri/src/remote_git.rs`

Potential commands:

- `remote_read_dir_entries`
- `remote_read_file_content`
- `remote_write_file_content`
- `remote_git_status`
- `remote_git_show_diff`

Rules:

- Avoid shell injection in every remote command.
- Keep file size limits aligned with local `read_file_content`.
- Consider SFTP only after the command-based MVP proves useful.
- Consider a remote helper server only for a later high-fidelity VS Code Remote experience.

### Packaging

Run:

```bash
pnpm install
pnpm lint
pnpm test
pnpm build
pnpm tauri build
```

Expected local macOS app path:

```text
src-tauri/target/release/bundle/macos/NeZha.app
```

For distribution beyond local use, add a separate release plan for:

- code signing
- notarization
- DMG packaging

### Stage 8 Review

Run final release review:

- local workflows still work
- SSH terminal workflows work
- build artifacts exist
- packaging command output is recorded
- remaining limitations are documented

---

## MVP Acceptance Checklist

The first implementation round is complete only when all of these are true:

- [ ] `SshConnection` exists in TypeScript and Rust.
- [ ] SSH connections load/save to `~/.nezha/ssh-connections.json`.
- [ ] No password/passphrase field exists.
- [ ] SSH command construction is covered by Rust tests.
- [ ] Remote path shell quoting is covered by Rust tests.
- [ ] A right-side SSH UI entry exists.
- [ ] A user can open an interactive SSH terminal in NeZha.
- [ ] xterm input, resize, and process cleanup work for SSH terminals.
- [ ] Optional remote directory startup works.
- [ ] `pnpm lint`, `pnpm test`, `pnpm build`, and `cargo test` have been run.
- [ ] Each stage has review notes and no unresolved Critical or Important findings.

