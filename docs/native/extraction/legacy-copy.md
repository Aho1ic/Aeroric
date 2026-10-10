# Legacy clipboard-copy loop and helpers

P8-05 slices49–50, 2026-10-09. Bounded legacy ownership, not new host placement
or P8-05 acceptance. Current execution checkpoint is in `MIGRATION_PLAN.md`.

Slice49's evidence below is historical: its entire caller was unchanged.
Slice50 subsequently moved the target-kind/source loop, preserving the actual
worker/empty/strict-target guard and old helper bodies; see the final section.

## Slice49: single helper owner and unchanged caller

`files::legacy_copy` owns `copy_path_recursive` and
`ensure_copy_target_not_inside_source`. The helper bodies, errors and Windows
cfg branch moved verbatim except public visibility. Core imports Path; Tauri
imports the helpers and drops its now-unused Path import, retaining PathBuf.

The entire actual `copy_paths_to_directory` command remains byte-identical:
worker → empty-list success → strict target guard → target.is_dir → source loop.
The loop retains absolute/source.exists/raw UTF-8 name validation/destination
exists/self-copy guard/recursive IO, preserving source order and partial success.
Its old direct-helper tests and all other fs bodies are unchanged.

These low-level helpers are not root/source/destination admission:

- Recursive copy first symlink_metadata. A symlink is copied as read_link
  text; Unix symlink and Windows source.is_dir-selected symlink_dir/symlink_file
  branches are unchanged. There is no recursive traversal through the link.
- Directory uses create_dir, then unsorted read_dir recursion. File uses
  fs::copy and maps its byte count to unit. Directory/file AlreadyExists map
  to the legacy collision message; link-creation errors remain raw.
- File permissions follow the original fs::copy behavior; directory permissions
  come from ordinary create_dir, not copying the source directory's mode.
- Self-copy guard returns immediately for !source.is_dir; otherwise it
  canonicalizes source before target and rejects target.starts_with(source).
  It is not a general existence or absolute-path guard.

## Explicit legacy versus native boundary

The actual legacy caller permits absolute outside-project source paths, has a
raw-name policy and recursive directory/link behavior, and returns unit. Native
project-scoped copy has different source/target admission and a result list.
No new alias/dispatch/RPC registration connects these as equivalent contracts.
Native/host/writer/search/history/remote/WSL/SFTP/CLI/protocol behavior is unchanged.

Observed old behaviors were retained, not silently repaired:

- Top-level dangling/loop sources can fail source.exists before recursive IO;
  nested links use symlink_metadata and are copied as link entries.
- A directory source link participates in canonical self-copy containment
  before its link entry is copied. Alias target paths still resolve through the
  original strict target guard.
- The low-level regular-file helper can overwrite an existing file. Its
  caller's destination.exists check is a separate policy.
- **On the tested Unix filesystem, regular-file copy through a dangling
  destination link can create its outside-project referent.** The old command
  permits that because exists follows the link. The fixture contains the
  referent entirely inside scratch; no external user data was involved.
  Directory copy instead reports the legacy collision, symlink copy a raw OS
  collision, and a looping file destination a raw IO failure.
- Native project-copy refuses occupied/dangling destinations via
  symlink_metadata. Core regression tests exercise this contrast before the
  legacy low-level copy, proving these helpers are not admission equivalents.

This is an inherited legacy limitation, not a fix or authorization to expose
that behavior through hosted/native authority. Any future forwarding boundary
must account for the difference explicitly. No general descriptor-relative,
ancestor/TOCTOU, hard-link, transaction, cleanup or rollback guarantee is added.

## Before/after evidence

Evidence: `/tmp/aeroric-legacy-copy-l6lxka0e/`.

Six tests ran on the old production helpers/actual command before extraction:
**6 passed,0 failed,0 ignored,1331 filtered**, rc0. Original production fs
bytes were unchanged except the test declaration. Successful source is frozen
and replayed unchanged, not edited to manufacture a red/green story.

Each test re-execs only itself under env_clear and scratch cwd/HOME/data/empty
PATH, with timeout/reaping and one-real-nonignored-child-case assertions.
Only fixture files/links are used; every possible referent is within scratch.
No real clipboard/Trash, agent, FIFO/device/socket or Windows runtime is used.

Whole-tree snapshots cover bytes/modes/link targets/entries/mtime. Success allows
only named new/replaced entries and containing-parent mtimes. New copied mtimes
are not assumed to equal source mtimes; a synthetic create_dir oracle supplies
the default directory mode. Source-list partial progress is deterministic;
read_dir enumeration order is not asserted.

Post-extraction:

- Core `legacy_copy`4, legacy_modify4, files13, directory3, entry4, path3,
  read3, write-paths6 and project-copy4: **44 passed**, no failures/ignored.
- Actual Tauri broad `fs::`: **65 passed,0 failed,0 ignored,1272 filtered**,
  including the unchanged six and previous legacy/POSIX/remote/WSL cases.
- Scoped core and all-target Tauri Clippy pass.
- New ownership gate plus five prior legacy gates pass. New scratch-source
  controls: **5 positive/63 negative**, every expected outcome verified.
- Exact whole-fs inverse, two visibility-only helper moves and **54 protected
  hashes** pass, including all prior legacy/native/host/CLI/wire/locks/launch WIP.

## Integration evidence

Current CLI was rebuilt before running the five configured modules. Raw logs
and summaries were copied only for each module's configured cases, with fresh
mtime checks, into the evidence directory above.

- Mocked FileExplorer actions/UI: **2 files/132 tests**, no real file service,
  clipboard or Trash calls.
- `cli`: **5 checks/10 tests**; `files`: **34/233**;
  `cutover`: **89/523**; `skeleton`: **24/188**. All exit0.
- `tauri-adapter`: **1398 tests**; full Tauri **1337 passed,0 failed,0 ignored,
  0 filtered**. Tauri fmt/Clippy and TS typecheck pass. The module exits1 only
  for inherited core fmt/Clippy failures in
  `validate_agent_launch_services.rs`. Both raw failing logs are byte-identical
  to slice48; the untouched WIP hash is
  `a5c2a9447fb92592de48a3618afdf99b48033e8d32bb3ece44107f3f1c037782`.
- Shared `countCases` parsed the full raw result before raising both Tauri
  minCases/floor **1331→1337**. New core4/command6 results are checked in both
  modules; reversing only six new case entries/four floors and that measured
  full-floor change restores the exact pre-slice checks config.
  **35 module shapes/181 non-orphan floors** validate.
- Exactly **13 fs definition pins** changed; command names, parameters,
  registration, reachability, reference counts and other map metadata remain
  unchanged. Contracts **12 unchanged**; digest **16 shards/max21842B**.

Source controls are not runtime mutation tests or a complete Rust parser.
Inline review is not independent review/human acceptance. Coverage remains
**75/641,48 dispatches/floor75**, P8-05 in progress and G43 open. This is not an
all-adapter-green, production-cutover, Windows or completed-migration claim.

## Slice50: ordered loop behind unchanged target admission

Evidence: `/tmp/aeroric-legacy-copy-loop-1z4523fj/`.
`files::legacy_copy::copy_paths_to_directory(Vec<String>, PathBuf)` now owns
the old target.is_dir, sequential source checks/copies and final unit result.
The body is a dedent-only move. Both old helpers, including Windows branches,
remain byte-identical. Tauri retains worker → empty-list success → strict
target guard, then the qualified core delegate and original await/error path.
Every other fs body and old test is preserved by exact whole-file inverse.

Core receives an admitted target, not a project/root/writer capability. It
checks target kind even for an empty source list, whereas the actual command
succeeds before admission in that case. No native/host alias, new RPC, root
lookup, cleanup, sorting, history or settings IO is introduced. All preceding
legacy/native, link, raw-name, mode, error and partial-progress distinctions
remain in force, including the explicitly documented dangling-destination
limitation.

Fresh pre-move replay of the unchanged six actual-command/helper fixtures:
**6 passed,0 failed,0 ignored,1331 filtered**, rc0; **67** snapshot hashes
unchanged and the successful source frozen again. Four new direct core cases
append to the unchanged old four-case prefix. They cover target/source/name/
collision/self-copy order, empty-list boundaries, raw external sources and
target aliases, recursive link entries, deterministic partial progress and
native/dangling-destination contrasts. Full-tree snapshots allow only named
copies and parent mtimes; every possible referent remains inside scratch.

Post-move targeted results:

- Core copy8 plus the previous related suites: **48 tests passed**.
- Actual broad Tauri `fs::`: **65 passed,0 failed,0 ignored,1272 filtered**,
  with all six frozen cases unchanged. Scoped core and Tauri all-target
  Clippy pass.
- Inline review caught that the recursive helper has only Unix test users
  after moving the loop. A compile-only absent-Unix-test use-graph probe
  reproduced the unused import (**rc1→rc0**). Its import is now
  `cfg(all(test, unix))`; self-copy/PathBuf remain `cfg(test)`. Fresh Tauri
  tests/Clippy passed after that correction. This is not a Windows target build.
- Both changed source gates retain the old helper/fixture/name assertions and
  pin the new loop/delegate/import bindings. Copy controls **7 positive/
  93 negative**, entry controls **7/72**, all expected outcomes verified.
  These are scanner controls, not executed runtime mutants.
- Whole-fs/core inverse and **60 protected hashes** pass. Only **13 fs
  definition pins** and one Rust reference change. Real extractor controls
  measured old0, old-plus-tests/core0, new1, new-minus-delegate0; full repo
  **0→1** is the production delegate, not a test-count artifact.
- Mocked FileExplorer consumers: **2 files/132 tests**. Contracts **12
  unchanged**, digest **16/max21842B**. Both core minCases/floors are raised
  **4→8** only after measurement; command6/full Tauri1337 are unchanged.

Fresh integration: rebuilt current CLI; cli **5 checks/10 tests**, files
**34/237**, cutover **89/527**, skeleton **24/188**, all exit0. Adapter executed
**1398 tests**, including full Tauri **1337 passed,0 failed,0 ignored,0 filtered**;
Tauri fmt/Clippy/typecheck pass. Only inherited core fmt/Clippy fail; both raw
failure logs are byte-identical to49 and the launch-WIP hash above is unchanged.
Configured raw logs and summaries were copied with freshness checks, not
recovered from stale prefix-matched artifacts.

Final gate audit parses core8/command6 in both modules and full Tauri1337 from
those raw logs. Reversing only the two measured core minCases/floors and six
slice notes restores the pre-slice config exactly; **35 modules/181 non-orphan
floors** validate. No Tauri case or full-floor increase was claimed.

Inline review is not independent acceptance; P8-05/G43, default Embedded,
inherited launch WIP and coverage75/641 with48 dispatches stay as-is.

Bounded closure rechecked inverse/protected hashes, all six ownership gates,
inventory/freeze, contracts/digest and context17/20 within per-file/total
budgets. Tracked and all126 untracked files passed whitespace checks.
Slice51's researched rename boundary is separate, not part of this acceptance.
