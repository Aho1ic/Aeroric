# Project-list IPC — P8-05 slice 21

## Existing behavior and selected boundary

The real host already registers `project.list` unconditionally. It reads the
injected data directory through `ProjectService::list` and returns
`{"projects":[Record...]}` without creating or probing project directories.
The command inventory freezes `project.load_projects` under accepted ADR-002.
Its missing registration is an implementation gap, not a misspelling.

Tauri's registered `storage::load_projects()` read its own local file. It also
created the data directory and tightened file/directory permissions. That
synchronous helper has internal skills/remote callers, so it cannot simply
become async without changing unrelated behavior.

`storage::commands::load_projects<R>` is the new **IPC boundary**, registered
under the same frontend command name. It dispatches literal `project.load_projects`
with `{}`, serializes the old bare array only in the deferred Embedded branch,
and decodes the selected owner's result as the existing Project array. The original
synchronous helper and its body remain for internal Rust callers.

This is not another Project business type: the adapter uses the
existing desktop `storage::Project`. The host keeps raw records; the desktop
keeps its original typed output. Order/duplicates are not normalized here.
Missing/wrong data is an error, not an empty list or local fallback.

The host adds a real `project.load_projects` bare-array handler. Both literal
registrations call one `ProjectService::list()` read/error path; a private
reply-shape enum only selects array versus object serialization. The existing
`project.list` object wire, its CLI/Swift callers and core reader stay unchanged.
The two methods are not aliases returning an incompatible shared shape.

## Red/control evidence

- Direct old Tauri command, mock Runtime and real authenticated scripted UDS:
  **rc101, 1 passed / 6 failed / 0 ignored / 1219 filtered**. Embedded passed;
  External returned local-only/empty values and never reached project.list.
  Log: `/tmp/aeroric-project-list-red.LE4501/command-red.log`.
- Real host UDS control: **2/0, rc0**. Raw fields, unknown values, `b,a,b`
  order/duplicates, bytes and modes survive. Missing/empty lists and corrupt
  file/container/record errors are covered, followed by successful recovery
  on the same connection.
  Log: `/tmp/aeroric-project-list-control.h2JZcw/host-query.log`.

The seven command tests include actual Embedded permission effects, different
local/host data, empty host list without local directory creation, bad wrapper
and Project fields, method/IO failure, disconnect and no session. Object
wrappers and arrays with non-Project entries are rejected. Source invariants supplement
these behavioral tests rather than replacing them.

The rejected first implementation's targeted green: direct command **7/0**, real host **2/0**, Tauri check,
ownership, CLI syntax and rebuilt complete CLI smoke all **rc0** in
`/tmp/aeroric-project-list-targeted.usQuC2`. That implementation incorrectly
renamed the frozen map leaf to `project.list`. The broader contracts gate
returned **rc1** (`rpc_method "project.list" != derived "project.load_projects"`)
in `/tmp/aeroric-project-list-integration.94M02V/contracts-write.log`.
The temporary coverage61/641 reading from that rename is not accepted evidence.
Keep both independent naming checks and ADR-002 unchanged.

The corrected tests exercise both real host wires, require the frozen method
to fail before its registration exists, and check each CLI read leaves
projects.json unchanged. Post-correction results and independent review are
recorded in `MIGRATION_PLAN.md` slice 21 as they actually run.

Corrected red: real host **0/2 passed, rc101** (UnsupportedCapability on the
missing frozen method), and real command **2 passed / 5 failed, rc101** on the
first adapter. Logs: `/tmp/aeroric-project-list-dualwire-red.GsqbQO`.
Corrected targeted green: host **2/0**, command **7/0**, Tauri check, ownership,
scoped fmt and rebuilt full CLI smoke all **rc0** in
`/tmp/aeroric-project-list-dualwire-targeted.GC00mU`. The actual new registration
now measures **61/641**, with **31** valid dispatch sites; the independent
freeze and naming checks remain intact.

Final acceptance: projects **5/21**, cutover **20/107**, skeleton **24/188**,
tauri-adapter **12/1287** (lib1226), host **12 suites/92 tests**, React startup
**3/55**, typecheck and both fmt/clippy all pass. Independent review found no
substantiated issue. Twelve contracts are unchanged; 111 paired floors, five
storage pins, 16 digest shards and context13/10 pass. Logs:
`/tmp/aeroric-project-list-dualwire-final.M056zV`,
`/tmp/aeroric-project-list-dualwire-web.JCF3aF`. No full Swift/Vitest, real
WebView IPC, cross-platform packaging or production-data cutover was tested.

## Exclusions and rollback

- Internal synchronous helper callers and `save_projects` are not routed in
  this slice; the full project storage/cutover surface is not declared done.
- `load_project_tasks(projectId)` directly reads a task directory, including
  orphans; host `task.list` scans registered projects by optional projectPath
  and overwrites returned projectId. These are not interchangeable.
- No remote credentials, project probes, real HOME, GUI, production data,
  mutation/writer handover, installation or release is exercised.
- Embedded remains the default. A failed External query is an error, never
  automatic local execution. The new read path writes no migration data;
  compatible Embedded rollback keeps the same synchronous reader.
- P8-05 stays in progress; G43 remains open. This does not authorize production
  switching, commit/push or cutover acceptance.
