# Local project configuration mutations — P8-05 slice 27

## Contract and ownership

- `settings.init_project_config({"projectPath": string})` returns the shared
  ProjectConfig. `settings.write_project_config({"projectPath": string,
  "config": ProjectConfig})` returns JSON **null**, not `{}` or the config.
- These are authenticated **local** RPCs. Paths retain original spelling,
  relative-path/OS symlink semantics and the legacy `.aeroric/config.toml`
  layout. They are not project allowlisting, symlink confinement, SSH/WSL or
  remote/mobile access grants.
- Hosted mutations verify the injected CoreContext writer lease inside one
  process-local write mutex, before any filesystem effects. Init and write
  share that mutex so concurrent initialization cannot overwrite a completed
  explicit save. It does not coordinate other programs or other data roots.
- Bad params fail before filesystem/authority checks with InvalidArgument.
  Operation/serialization and missing/lost writer authority return the existing
  settings-domain IoError; authority denial keeps `no write permission` in its
  diagnostic. The frozen protocol has no PermissionDenied code.
- Explicit `*_legacy` core entrypoints retain the desktop's existing local
  authority, including non-macOS handover policy. Only the deferred Embedded
  future calls them; host handlers must use the checked functions. Missing
  method/session, transport failure and invalid success shapes do not cause a
  local write or automatic retry. A lost reply does not imply a host rollback.

## Preserved filesystem behavior

Init creates `attachments/` first, writes the original template only when
`exists()` is false, then reads UTF-8 and parses TOML or defaults. Existing
malformed TOML is not repaired or replaced. An existing invalid-UTF-8 config
fails after attachments creation, as before. Initialization itself retains the
old `fs::write`, not a new atomic/durability guarantee.

Write creates `.aeroric/` (not attachments), pretty-serializes the full typed
config and atomically replaces the file. Old comments/unknown TOML fields are
not merged; there is no backup. Directory permissions are not tightened;
the new file inherits the original `File::create`/umask behavior. A directory
symlink is followed by filesystem operations, while atomic rename replaces
the **leaf link** rather than its target.

The legacy non-private atomic mechanism and failed-rename cleanup now live in
core `storage::atomic`. Tauri's `storage::{atomic_write,finish_atomic_rename}`
delegate to them, including the latter's existing private-writer caller. Temp
naming, fsync, String OS errors and cleanup match the old executable source;
other core atomic/private-storage APIs are unchanged.

**Do not use `settings::atomic_write` here:** despite its name, it currently
performs a direct `fs::write`. Its other callers are outside this slice.

## Extraction map

| Old location | Single owner / adapter |
| --- | --- |
| Tauri config init/write algorithms | core `settings/project_config.rs` private implementations |
| Tauri config init/write IPC | `config/commands.rs`, exact frozen names/typed replies |
| Legacy non-private atomic writer/rename cleanup | core `storage/atomic.rs`; Tauri delegates |
| Hosted init/write | `settings_services.rs`, typed blocking wrappers over checked core functions |
| Internal project-config reads | unchanged synchronous Tauri reader → shared core read |

Remote/WSL config writers, agent config files, global settings loaders, signing,
OMP roots and production-data cutover are unchanged.

## Evidence and current limits

- Behavioral red after successful compilation: real host **1 passed / 6 failed**
  (unauthenticated control passed); actual command **2 passed / 5 failed**
  (Embedded controls passed). Zero ignored. The earlier host compile error
  from a nonexistent ErrorCode was corrected and is **not** red evidence.
- Green targeted runs: core mutation **7/7**, authenticated host mutation
  **7/7**, real Tauri command suite **13/13** including all six prior read
  cases, zero ignored. Synthetic two-client races, read-only/unauthenticated
  refusal, original-path/symlink/mode behavior and rename cleanup are exercised.
  `/tmp/aeroric-config-mutations-red-basub5hp/`.
- Coverage measured **70/641**, **40** real dispatch sites; floor70. Read and
  mutation source ratchets pass. The extracted atomic bodies match the original
  source after only names/comments/whitespace are normalized.
- Final integration: rebuilt CLI and full scratch headless smoke passed;
  core storage **29/29**, prior config read **3/3**, cutover **39 cases / 172
  tests**, skeleton **24 / 188**, Tauri lib **1260/1260**, and mocked React
  consumers **3 files / 22 tests** passed. No ignored tests in these suites.
  Scoped core/host clippy, Tauri fmt/clippy and TS typecheck passed.
- Inventory and freeze-check passed; **12 contracts unchanged**, **16 digest
  shards** (largest 21842B), both ownership ratchets, tracked/untracked whitespace
  checks and registered runner tests **36/36** passed. The initial mistyped
  runner-test path was corrected using the gate's actual argv; it is not hidden.
  Final evidence: `/tmp/aeroric-config-mutations-integration-6if0tl5d/`.
- The final adapter rerun still exits **1**, with **1321** test cases executed
  and two failed checks: full core fmt and all-target clippy on the inherited
  `validate_agent_launch_services.rs` (unused `Path`/`success_result`).
  Its SHA256 remains
  `a5c2a9447fb92592de48a3618afdf99b48033e8d32bb3ece44107f3f1c037782`.
  This unfinished test and its missing real launch owner were not changed or
  exempted. Full-target verification is **not green**.
- Slice27 is closed as a bounded implementation/verification checkpoint after
  inline source review, not independent subagent or user acceptance. P8-05,
  full cutover and G43 remain open; G43 is the files/search gap. No release,
  production-data switch or launch-validation capability is claimed.

## Slice30 target correction

Init/write also advertise targets in the frozen schema. They now share the
pre-IO target refusal described in `local-definition-targets.md`; a refused
remote initializer cannot create local attachments and a refused remote write
cannot overwrite local config. Existing local checked/legacy algorithms,
Tauri remote/WSL implementations and authority remain unchanged.
