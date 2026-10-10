# Project-scoped copy destinations — P8-05 slice34

## Scope and demonstrated defect

The existing hosted `files.copy_paths_to_directory` accepts `projectId`,
relative `targetDir` and relative `paths`; its result is `{copied: [paths]}`.
It is not the legacy absolute/external-source clipboard-copy contract.
No new method, capability or Tauri forwarding site is added by this slice.

The former host `file_services2::copy_into` only lexically validated the
target before `create_dir_all`. A destination directory symlink, including
one above a missing suffix, could direct writes outside the project.
The leaf `exists()` check missed dangling symlinks and allowed copying
through them. All three failures were reproduced through authenticated UDS
using scratch sibling directories; no real outside files were touched.

## Single owner and preserved contracts

The copy algorithm now lives in core
`files::read_write::copy_project_paths_to_directory`. The host retains typed
arguments, current-writer admission before project lookup, canonical project
root resolution, and the existing result/error framing. `copy_into` is gone.

Before mkdir, `path_policy::validate_directory_destination` finds the nearest
existing ancestor with `symlink_metadata`, canonicalizes it and the project
root, and requires containment. A dangling directory link is an existing
entry whose resolution fails, not a nonexistent ancestor to skip. Validation
does not write and does not rewrite the supplied destination spelling.

Before each copy, any destination entry reported by `symlink_metadata`
gets the original non-overwrite refusal, including a dangling leaf link.
Only NotFound permits copying; other inspection failures are explicit errors.

The original source resolution, source order, `fs::copy` binary bytes and
permissions, in-root directory aliases and returned path spelling remain.
Missing nested destinations still work. Directory creation precedes source
checks, and a later bad source preserves earlier successful copies. This is
not an atomic batch, descriptor-relative race fix or mid-flight writer
revocation guarantee. Other file mutation helpers and target-routing
boundaries are not implicitly covered.

## Targeted evidence

`/tmp/aeroric-project-copy-red-he56r3q8/`:

- `before.json` and source snapshots isolate this slice from inherited WIP.
- `host-red.log`: compiled **3 passed/3 failed/0 ignored, rc101**. The directory,
  ancestor and dangling-leaf cases incorrectly returned copied success;
  in-root, nesting/partial-progress and read-only controls passed.
- `core-green.log`: new `project_copy` **4/4**, original `files` **13/13**.
  Includes dangling ancestors, read-only validation and source-link refusal.
- `host-green.log`: new `project_copy_services` **6/6**, original batch2
  **4/4**, file authority **14/14** and search authority **9/9**.
- `core-clippy.log` / `host-clippy.log`: scoped libraries and those suites
  passed with `-D warnings`.

`file-write-invariants.mjs` checks the single copy owner and both guards.
The current-CLI smoke adds scratch-only directory/ancestor/leaf refusals and
an allowed in-root alias with a missing suffix. CLI rebuild and full relevant
gates are required before the final checkpoint; targeted results alone are
not a full-green or Phase8 acceptance claim.

## Final runtime integration (2026-10-09)

`/tmp/aeroric-project-copy-integration-u4n83f7w/`:

- Current CLI rebuilt; the entire synthetic headless smoke passed, including
  the three unsafe destination cases and the allowed alias/missing suffix.
- files **13/112**, cutover **58/335**, skeleton **24/188** passed, no skips.
- Full Tauri library **1284/1284**, Tauri fmt/clippy and TypeScript typecheck
  passed. Adapter remains **rc1, 1345 tests** solely because inherited
  `validate_agent_launch_services.rs` fails core fmt and all-target clippy
  (unused `Path` and `success_result`). Its SHA256 remains
  `a5c2a9447fb92592de48a3618afdf99b48033e8d32bb3ece44107f3f1c037782`.
  Per-case raw logs and summaries are preserved in `*-raw/`.
- Inline review of `runtime-delta.patch` checked the three-file source move,
  writer/root/error ordering and retained copy behavior. The old gate argv
  and floors are unchanged; two measured cases were added to each module.
  This is not independent review or user approval.
- Ownership positive/negative controls ran on scratch source copies:
  original **rc0**, missing directory guard **rc1**, missing leaf guard **rc1**.
  The order assertion explicitly rejects a missing guard instead of accepting
  `indexOf(...) == -1`.

Coverage stays **75/641**, with **45** dispatches and floor75. G43 remains
open: this correction does not serve the missing image/remote capabilities,
change legacy clipboard-copy behavior or authorize External/production
cutover. Separate path-only write/create leaf handling and non-search file
target routing still require investigation.

Final artifact checks: inventory/freeze-check, 12 unchanged generated
contracts, 16 digest shards (max21842B), context24+20 and tracked/76 untracked
file whitespace checks passed. No existing gate floor or argv was weakened.
