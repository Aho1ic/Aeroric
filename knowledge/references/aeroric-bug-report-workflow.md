# Aeroric Bug Report Workflow

This project often receives Chinese multi-item bug prompts. Treat them as executable work orders.

## Intent Mapping

| User phrase | Meaning |
|---|---|
| `逐个修改以下问题` | Keep an ordered checklist. Do not merge unrelated bugs into one vague task. |
| `先操作我的电脑进行复现确认问题根源` | Use local app state, screenshots, tests, and code search to reproduce or trace the root cause before patching. |
| `参考截图: '/path/file.jpg'` | Inspect the referenced screenshot when it affects UI/layout/IME/layering behavior. |
| `检查整个项目中...` | Search globally with `rg`; fix the shared pattern, not only the named screen. |
| `修改完成后进行功能测试和代码审查` | Run targeted and full verification, then review changed files for regressions and missing tests before reporting completion. |
| `最后打包为 Aeroric.app 并替换 /Applications/Aeroric.app` | After verification, run `pnpm tauri build`, copy the built app into `/Applications`, and verify `Info.plist`. |

## Required Flow

1. Read `AGENTS.md`, `local_change.md`, and this file.
2. Capture `git status --short`; assume existing dirty changes are user work unless proven otherwise.
3. For each numbered bug, record: reproduction evidence, root-cause component, exact files touched, regression test, verification command.
4. Prefer local patterns and existing helpers. Avoid adding unrelated features while fixing bugs.
5. For destructive operations, confirm user intent in UI code before invoking backend deletion/mutation commands.
6. For cross-layer bugs, trace UI -> Tauri command -> Rust backend -> shell/database/file system and verify type names and payload casing.
7. Before completion, run at minimum: targeted tests for changed behavior, `pnpm lint`, `pnpm build`, `pnpm test`, and relevant `cargo test --manifest-path src-tauri/Cargo.toml`.
8. If packaging is requested, run `pnpm tauri build`, replace `/Applications/Aeroric.app`, and verify bundle name/version.

## Review Checklist

- Deletion/replace flows wait for explicit confirmation before mutation.
- Modals that must be topmost are portaled or have a z-index above terminal/canvas layers.
- IME/input fixes cover composition, delayed replay, autocorrect, autocapitalize, and terminal textarea attributes.
- SFTP/SSH/Docker/database changes include both frontend tests and Rust tests when backend behavior changes.
- New files are either intentionally tracked or explicitly ignored; do not leave accidental generated files untracked.
- Large mixed changes are reported as separate logical groups so the next commit can be split safely.
