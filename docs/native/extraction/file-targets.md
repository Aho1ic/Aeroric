# Explicit project-scoped file targets — P8-05 slice36

## Proven defect and scope

Seven non-search methods in `services.rs` and five in `file_services2.rs`
ignored `params.target`. A remote or malformed target could return local file
data, mutate local files or create history snapshots. The former formatter
returned InvalidArgument for an unsupported extension **after** snapshotting,
so checking only its error code would have missed the side effect.

This slice corrects explicit targets on these twelve existing methods:

- read_dir, read_file, write_file, create_file, create_directory, delete_path,
  rename_path;
- format_file, list_local_history, read_local_history_entry,
  restore_local_history_entry, copy_paths_to_directory.

No new RPC, alias, remote capability or Tauri forwarding site is added.
Coverage75/641,45 dispatches/floor75 stays unchanged. Missing target retains the
old projectId/local branch; this is not stored remote-project inference.
Target-only requests, remote transports and other domains are not implemented
or implicitly approved here. G43/P8-05 remain open.

## Admission and local-root policy

Host-private `file_access::FileTarget` uses the existing protocol
`ProjectTarget`, not a second DTO:

| Stage | Behavior |
| --- | --- |
| `decode` | Pure: omitted target carries no assertion; Local carries its path; null/malformed is InvalidArgument; typed SSH/WSL is UnsupportedCapability with a reason |
| Existing writer/root checks | Eight mutations still check current write authority before registry/root IO; four queries do not acquire a writer |
| `root_refusal` | Only after authoritative projectId root canonicalization, validate any explicit Local path and require canonical equality |

All nine handlers decode after their original pre-root parameter checks,
before writer admission or lookup. Local root assertion precedes core/port
calls. No delayed `entryId`/`projectId` parsing was hoisted just to regularize
the existing batch differences. Omitted-target error/argument behavior remains.

Equivalent Local aliases are accepted, but a second root, worktree or vault
cannot replace the selected projectId. This differs intentionally from
definition services' exact-original-projectPath policy and search's broader
registered/worktree/vault resolver. Those implementations remain unchanged.
Core file algorithms, `write_refusal`, default Trash port, schemas and
Tauri/Swift/frontend code are not changed by this slice.

## Behavioral evidence

`/tmp/aeroric-file-target-red-8upxywck/`:

- `host-red.log`: compiled **3 passed/14 failed/0 ignored/0 filtered, rc101**.
  Each of twelve operations ran all eight target variants on fresh fixtures
  rather than aborting at the first red: SSH, WSL, null, mistyped Local,
  incomplete SSH, conflicting Local, missing Local root and relative Local.
  Read results were local, mutations changed the fixture, delete called the
  injected quarantine, and format snapshotting was observed despite its error.
- `host-green.log`: new suite **17/17** and nine prior suites **73/73** pass.
  Includes all96 operation/target refusal combinations and48 poison-registry/
  read-only combinations, plus omitted/matching/canonical-alias controls for
  all operations, read-only queries, authentication and same-connection recovery.
- Recursive bytes/modes/entries and zero Trash calls prove refused operations
  leave scratch fixtures unchanged. Production OS Trash is never exercised.
  Absolute `.txt` format controls prove ordering without spawning a formatter.
- `host-clippy.log`: host library plus all ten named suites pass `-D warnings`.

## Integration checkpoint (2026-10-09)

`/tmp/aeroric-file-target-integration-gcbdvc6w/`:

- Explicit CLI rebuild and full scratch headless smoke pass, including the new
  remote/null/conflicting-Local query/write/format/copy refusals, no snapshots,
  and matching/canonical-alias Local controls.
- `files` **16 gates/142 tests**, `cutover` **61/365**, `skeleton` **24/188**
  pass. Measured host-suite floor17 is registered in files and cutover; previous
  commands/floors are unchanged. Coverage is **75/641,45 dispatches/floor75**.
- Adapter **1345 tests**, including Tauri **1284**, pass; the module exits1
  only for the inherited launch-validation WIP's core fmt and two clippy
  warnings. Raw logs are preserved. That file's SHA256 remains
  `a5c2a9447fb92592de48a3618afdf99b48033e8d32bb3ece44107f3f1c037782`.
- Inline review (not independent or user approval) checked every new handler,
  test matrix and smoke delta. Reversing only the target/import/adjacent-comment
  edits recovers all three pre-slice runtime files exactly. Core read/write/path
  policy, search and definition target helpers remain byte-identical.
- Actual ownership script on scratch source copies: positive exit0; all18
  individually removed handler decode/root guards, missing canonical equality
  and decode-after-writer controls exit1. No repository mutation for negatives.
- Inventory/freeze-check, contracts12, regenerated digest16 (largest21842B),
  context26/22 (each file<32768B, aggregate<131072B), tracked and81 untracked
  whitespace checks pass. The first no-index whitespace harness incorrectly
  treated clean exit1 as failure; isolated clean/dirty controls proved exit1/
  exit3 with diagnostics, and the corrected scan found no whitespace issues.

Slice36 is bounded complete, not full P8-05/G43 acceptance or remote support.
Stored project `location` admission is a distinct next boundary: the old root
resolvers still look only at the record's raw `path`.

The subsequent slice37 work on that separate boundary is tracked in
[Stored file-project admission](./file-project-records.md); it does not alter
this slice's explicit-target policy or historical evidence.
