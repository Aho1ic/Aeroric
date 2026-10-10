# Terminal-history filenames and deletion — P8-05 slice 28

## Discovered parity defect

Slice24's hosted `project.read_task_terminal_history` used
`AppPaths::terminal_history_file`, which validated trimmed ID characters but
joined the **original** ID. The legacy `storage::safe_task_history_name` trims
before appending `.log`. With `task.log` and a decoy ` task .log`, the host
returned the decoy while Embedded returned the canonical history.

Behavioral red: the core path regression and authenticated host read each
failed **0/1** after successful compilation. Both now use the single pure
`AppPaths::terminal_history_name` rule. Original strings still travel over RPC;
only the history filename trims. General project/task/event paths deliberately
remain unchanged. This is a correction to slice24, not a new global ID policy.

## Frozen deletion contract

`project.delete_task_terminal_histories({"taskIds": string[]})` returns JSON
**null**. It is not `terminal.*`, a task deletion, an active-task policy, a
process cancellation, or a replay/lease reset.

The host authenticates normally and decodes the complete params object and
array element types before any mutation. Malformed shape/type → InvalidArgument.
Then the checked core entrypoint verifies the injected context's current writer
lease (also for an empty request); absent/lost authority → IoError with
`no write permission`. The frozen protocol has no new permission code.

The common core algorithm is deliberately **sequential**:

1. Resolve the history directory for the next ID.
2. Validate that ID and derive the trimmed history filename.
3. If `path.exists()`, `remove_file(path)`; otherwise continue.

A later invalid string (InvalidArgument) or IO failure (IoError) stops the loop.
Earlier removals remain applied; later IDs are untouched. This differs from
rejecting a non-string array element before the loop starts. Never replace it
with all-ID prevalidation or an implied atomic transaction.

## Filesystem and authority boundaries

- Empty/missing/duplicate IDs do not initialize directories, repair files,
  tighten modes, read task registries or create backups/trash.
- Existing leaf links are unlinked without deleting targets; dangling links
  remain because `exists()` is false. A directory at the log filename errors,
  not recursively deleted. Directory symlinks retain OS resolution.
  This is legacy local behavior, not symlink confinement.
- There is no new lock/retry guarantee. Concurrent deletes can race between
  exists/remove and return an IO error; a live appender can recreate a log.
  A failed/lost host reply does not promise rollback.
- Hosted deletion runs the checked core entrypoint on a blocking worker.
  It never invokes the legacy authority bypass.
- The explicit core legacy entrypoint receives a directory resolver from
  Tauri, called once per reached ID **before** validation and never for an
  empty batch. Thus core has no ambient-home dependency and Embedded preserves
  its old root-resolution/error order and authority on every platform.
- Tauri retains its synchronous delete helper for the LAN roundtrip test.
  The actual generic Runtime IPC is in `storage/commands.rs`: its entire local
  call is deferred until Embedded selection. External never locally deletes,
  validates, retries or falls back on session/transport/host/decoding failure.
  Only a null reply decodes as successful unit.

Append, compaction, total-byte tail reads and terminal replay/control ownership
are unchanged. Core `terminal/history.rs` owns policy plus path-injected read
and delete; this does not claim all terminal persistence has been extracted.

## Extraction map

| Boundary | Single owner / adapter |
| --- | --- |
| History filename validation/trim | core `paths::AppPaths::terminal_history_name` |
| Legacy Tauri filename helper | String-error adapter over the shared rule |
| Delete loop | core `terminal/history.rs::delete_histories_with_directory` |
| Hosted deletion authority | checked core entrypoint, real `services.rs` handler |
| Embedded compatibility/root lookup | explicit legacy entrypoint, Tauri sync delegate |
| Actual IPC and frozen mapping | `storage::commands::delete_task_terminal_histories` |

## Evidence and limits

- Mutation behavioral red: authenticated host **1 passed / 6 failed**;
  actual command suite **6 passed / 5 failed** (read4 + Embedded deletion2
  passed; External deletion5 failed). Zero ignored, successful compilation.
  An initial mistyped command filter ran zero tests; that is non-evidence and
  was corrected to the existing gate's actual module filter.
- Green: core paths **7/7**, core mutations **7/7**, host read **3/3** and
  mutations **7/7**, actual command **11/11**, zero ignored.
  `/tmp/aeroric-history-red-yrmftgqm/`.
- Rebuilt current CLI full headless smoke passes canonical/padded read,
  deletion/null wire, strict type refusal, partial invalid-ID/IO errors and
  prior fake-task lifecycle. Scoped core/host clippy passed. Contracts **12**
  remain byte-identical; digest **16** shards, largest21842B.
  `/tmp/aeroric-history-integration-wocg19vx/`.
- Mocked cleanup consumers **1 file / 20 tests** passed, no skips:
  `/tmp/aeroric-history-web-20zz9o1u/`.
- Coverage measured **71/641**, **41** real dispatch sites; floor71. Source
  ownership checks pass. Final cutover **42 cases / 202 tests**, skeleton
  **24 / 188**, terminal-core **13 / 108**, and Tauri lib **1268/1268** passed.
  Tauri fmt/clippy/typecheck, inventory/freeze-check, runner **36/36** and
  tracked plus **51** untracked-file whitespace checks passed.
  Paired Tauri-lib/terminal-unit floors were raised to the measured1268/66
  and those gates rerun on the final configuration.
- The final adapter still exits **1**, **1329** test cases executed: full core
  fmt and all-target clippy fail only on the inherited launch-validation test
  (formatting, unused `Path` and `success_result`). Its SHA256 is unchanged:
  `a5c2a9447fb92592de48a3618afdf99b48033e8d32bb3ece44107f3f1c037782`.
  It is neither deleted nor exempted; full-target verification is not green.
- Slice28 is closed after inline source/contract review and the above
  verification, not independent subagent/user approval. G43 remains files/search,
  P8-05 and full cutover remain open. No default placement switch, release or
  real-data migration is authorized.
