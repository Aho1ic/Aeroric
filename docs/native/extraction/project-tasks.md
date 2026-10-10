# Exact project task-list IPC — P8-05 slice 22

## Existing owner and selected boundary

`core/.../storage/tasks.rs::load_tasks` already reads the injected
`projects/<projectId>/tasks.json` as raw records. It needs no project registry
entry or lease, makes no directory/permission changes, returns an empty array
for an absent file, and refuses corrupt containers or non-object records.
There is no reason to introduce another storage reader.

ADR-002 freezes `project.load_project_tasks`. Its missing host registration is
an implementation gap. The existing `task.list(projectPath)` is not an alias:
it scans registered projects, reads stamped task lists and rewrites projectId.
Keep that method unchanged.

The new real host handler extracts a string projectId, invokes the existing
core reader and returns the raw bare array. Missing/non-string parameters and
InvalidId errors become InvalidArgument; other read errors become IoError.
Original order, duplicate IDs, unknown fields and stored projectId are preserved.

ID validation is the existing lexical rule: validate trimmed characters, then
join the **original string**. A padded valid ID selects its padded directory,
not the trimmed directory. This is not new canonicalization, symlink confinement
or per-project authorization.

The actual Tauri IPC moves to `storage::commands::load_project_tasks<R>`, while
the original synchronous helper remains for four internal callers. Only the
deferred Embedded branch evaluates it. Its old common-directory creation,
permission tightening and invalid-ID ordering stay unchanged; External must
not perform those effects, even when the host is unavailable or refuses.
The client decodes the existing Task type, with no default/partial/local fallback.

## Validation plan and evidence

Three real-host tests cover orphan/raw/padded-path behavior, absent/empty/
corrupt-file recovery and invalid parameter/ID refusal with unchanged fixtures.
Eight actual-command tests cover Embedded compatibility, exact host parameters,
typed data and duplicate order, empty results and every refusal branch.
All roots are synthetic, Tauri uses the existing scratch-home guard and runs
single-threaded with an isolated fallback AERORIC_HOME.

The current CLI smoke reads its already-settled fake task, compares raw records
and on-disk bytes, rejects an invalid ID and leaves an absent project directory
absent. Ownership checks supplement the runtime evidence.

Actual red/green and broad gate results are recorded in MIGRATION_PLAN.md
slice 22 as each step completes; planned cases are not claimed as executed.

## Exclusions

- No changes to the core reader, task mutation/lease authority, internal
  synchronous consumers, intrinsic aggregation or default placement.
- No actual project probes, HOME/session imports, SSH, agents, credentials,
  production tasks, installation or release.
- The separate `read_task_terminal_history` candidate is deferred: its old
  disk-tail IO has not moved into core, filename trimming differs from AppPaths,
  and its approximately 8 MiB string (possibly expanded by lossy UTF-8/JSON
  escaping) conflicts with the 1 MiB full-frame limit. `task.output` and
  `terminal.plan_replay` have different live-stream/base64/cursor semantics.
  Do not fake-register it, truncate it or widen the protocol in this slice.
- P8-05 remains in progress and G43 open; this is not production cutover approval.
