# Project configuration reads — P8-05 slice 26

The subsequent local init/write migration is recorded separately in
[`project-config-mutations.md`](project-config-mutations.md); the read contract
and its synchronous internal callers remain unchanged.

## Frozen behavior and scope

`settings.read_project_config` accepts `{"projectPath": string}` and returns the
existing `ProjectConfig`, with snake_case fields inside `agent`, `git`, `editor`.
The CLI uses its real generic `rpc` command. This is distinct from global
`settings.load_app_settings` and agent credential/config-file methods.

The existing local command:

- Joins the original string with `.aeroric/config.toml`; no trimming or project
  registration lookup. Relative paths and symlinks keep their old filesystem
  semantics. This migration does **not** claim symlink confinement, project
  allowlisting or new cross-client path normalization.
- Returns `ProjectConfig::default()` if `exists()` is false or TOML decoding
  fails, including missing required sections. Optional fields use their existing
  serde defaults; user strings are not normalized.
- Propagates filesystem/read-to-UTF-8 errors instead of repairing the file.
- Creates no project/config/attachments directory, backup, settings, mode change
  or credential file. `init_project_config` and `write_project_config` are
  distinct mutations and are not hosted by this read-only slice.

The host requires a string parameter, returning InvalidArgument for absent or
wrong types, IoError for read errors. Empty strings retain the old relative-path
behavior. The authenticated local endpoint does not become a remote RPC method.
No new policy is inferred from an existing on-disk path being readable.

## Extraction map

| Source | Destination | Retained caller / boundary |
| --- | --- | --- |
| Tauri `config.rs` project DTOs/defaults/template | core `settings/project_config.rs` | re-exports for existing local/SSH/WSL callers |
| Tauri `config.rs::read_project_config` business | core `settings/project_config.rs::read_project_config` | unchanged synchronous helper for PTY and Git |
| Tauri command attribute/registration | `config/commands.rs` + `lib.rs` | generic mockable Runtime, existing IPC name |
| frozen host method | `settings_services.rs` blocking handler | one shared read, no loader/writer |

All local work in the real IPC stays inside the deferred Embedded future.
External returns the host's typed result or an error; missing method, absent
session, transport failure and malformed payload never cause a local read.

## Validation record

- Before implementation: authenticated host tests **0 passed / 4 failed /
  0 ignored**, rc101, all at the missing frozen method, not compilation.
  `/tmp/aeroric-project-config-red.cFaMSv/host-red.log`.
- Actual Tauri command red: **1 passed / 5 failed / 0 ignored**, rc101;
  after wiring, **6 passed / 0 failed / 0 ignored**. Core **3/3** and real
  authenticated host **4/4**. The handoff successor reran all three unchanged
  suites and the three mocked React consumers (**3 files / 22 tests**, no skips):
  `/tmp/aeroric-config-handoff-check-8i9oejmr/`.
- The rebuilt current CLI passed the complete headless smoke, including config
  read/defaults/refusals and file-preservation checks. Inventory/freeze,
  12 unchanged contracts and 16 digest shards passed. Coverage measured
  **68/641**, **38** real dispatch sites; floor **68**.
- Recovered completed gate logs at
  `/tmp/aeroric-project-config-integration-l02wy_n_/`: cutover **36 cases /
  151 tests**, skeleton **24 / 188**, both rc0. Tauri-adapter returned rc1:
  its Tauri lib **1253 tests**, Tauri fmt/clippy and TS typecheck passed;
  core fmt and all-target clippy failed only on the inherited unfinished
  `validate_agent_launch_services.rs` (formatting, unused `Path` and unused
  `success_result`). That file remains unchanged, sha256
  `a5c2a9447fb92592de48a3618afdf99b48033e8d32bb3ece44107f3f1c037782`.
- Handoff successor static review compared the moved DTO/defaults/read with
  the original `config.rs`, then traced the real command, blocking host
  registration, strict decoding and scratch-only tests. No actionable
  regression found in this slice. Prior subagent review results were
  unavailable and are not claimed; this is not the final cutover/user review.
  P8-05 and G43 remain open; full-target verification is **not** all green.

## Separate deferred candidate

`terminal.validate_agent_launch` cannot be registered as a project-directory
check: current Tauri code loads/repairs settings, resolves family/wrappers and
chmod, merges shell/credential/proxy/router environment, then actually launches
`--version`. Codex deliberately uses a stable home/temp cwd rather than the
project path, and timeout kills/reaps then returns success for interactive
wrappers. Its injected, headless launch owner is still P8-05 terminal-domain work. The existing
untracked test for that future method is preserved unchanged and may keep a
full-target test/fmt/clippy run red; it is not this slice's acceptance evidence.

## Slice30 target correction

The earlier host reader ignored ADR-006 target; read/init/write now call the
single private host local-target guard before core IO. Omitted and matching
local targets retain the old path/default/IO behavior. Unsupported SSH/WSL,
malformed/null and conflicting local targets refuse, never fall back locally.
Real red→green evidence: `docs/native/extraction/local-definition-targets.md`.
This corrects the boundary without claiming remote configuration support.
