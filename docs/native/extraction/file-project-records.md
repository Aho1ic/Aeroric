# Stored file-project location admission — P8-05 slice37

## Defect and authority

After explicit target admission (slice36), both projectId file-root scans still
selected `record.path` without checking `record.location`. A saved SSH/WSL or
malformed location with a coincidentally valid local raw path therefore read,
wrote, snapshotted or deleted local files. Supplying an explicit Local target
also did not make that stored remote identity local.

This slice covers the same twelve non-search file operations, not new methods
or remote support. The established stored identity policy is
`projects::mutations::is_addressable` **then** `resolve_location`: using only the
latter would fall back on malformed locations. ADR-013 Q32 and the existing
task launch planner already require this pairing.

Host-private `file_access::require_local_record` reuses these core functions.
It is pure and runs on the selected record before raw-path extraction:

- typed SSH/WSL → UnsupportedCapability;
- null/malformed/unknown `location` → InvalidArgument;
- new refusals carry nonretryable reasons and closed `ErrorDetail.field=location`;
- absent location or typed Local → retain the existing raw-record-path contract.

This is **transport/addressability admission**, not local path selection
migration. It does not select `location.path` instead of `path`, invent a schema
from opaque stored `target` metadata, merge projects, change core identity, or
grant worktree/vault/target-only access. Search uses a different resolver and is
not claimed fixed here.

## Adapter changes and ordering

The pure explicit-request target decode still precedes current-writer admission.
Mutations then check the writer before loading projects; queries remain
read-only-capable. Stored admission follows that record lookup, before root
canonicalization or business IO. No second record read is inserted.

The first scan returns typed errors separately from its old `None` outcome.
Unknown/missing-path/corrupt-registry requests retain its NotFound behavior and
message. The second batch retains the public root-resolver port, now returning
`FileProjectError` rather than String so location errors are not all remapped to
NotFound. Existing storage/missing-record/missing-path messages and invalid-root
handling are unchanged. Every invocation reads the current saved record; this
is not a transaction or mid-operation revocation guarantee.

Core file algorithms and identity helpers, FileTarget, writer admission,
production Trash, search/definition helpers, Tauri, Swift/frontend, schemas and
capability coverage are unchanged.

## Targeted evidence

`/tmp/aeroric-file-record-red-6ae09w5c/`:

- Before runtime edits, the expanded real authenticated UDS suite compiled:
  **18 passed/15 failed/0 ignored/0 filtered, rc101**. All17 prior tests and
  new Local controls passed. Twelve operations each ran all eight bad/remote
  saved shapes with omitted and explicit Local request targets: **192 pairs**,
  fresh fixtures without early exit. Reads leaked local results, mutations
  changed fixtures, counted quarantine was called and formatting snapshotted.
- After admission/error wiring, **33/33** plus nine prior suites **73/73**
  passed; scoped host library and all ten suites clippy passed. Original17
  test source/fixtures remain byte-preserved, with16 tests appended.
- Read-only queries/mutation precedence, missing/invalid raw paths, closed
  location diagnostics, Local/alias/legacy/opaque-metadata controls and
  same-connection record change/refusal/recovery are included.
- Intermediate `host-incomplete-wiring.log` is a mechanical replacement/build
  failure, not the behavioral-red evidence: the old diagnostic string was
  initially mismatched, leaving six callers unwired. It was corrected while
  retaining the actual old message; tests were not weakened.

All fixtures use scratch directories and recursive bytes/modes/entry snapshots.
Deletion uses counted injected scratch quarantine, never OS Trash. Absolute
unsupported `.txt` formatting cannot start a formatter even before the fix.

## Integration checkpoint (2026-10-09)

`/tmp/aeroric-file-record-integration-goxt82y8/`:

- Current CLI rebuilt; full smoke passes including40 safe stored-location
  refusal calls, no snapshots/byte changes, Local and restored-record recovery.
  Only scratch project records are changed and restored by the fixture.
- `files` **16 gates/158 tests**, `cutover` **61/381**, `skeleton` **24/188**
  pass. The measured existing suite floor rises17→33 in files and cutover,
  with no other gate command/count/floor change.
- Adapter **1345 tests** (Tauri **1284**) pass; module exit1 remains solely the
  inherited launch WIP's core fmt and two clippy errors. Raw output is retained.
  Its SHA256 is still
  `a5c2a9447fb92592de48a3618afdf99b48033e8d32bb3ece44107f3f1c037782`.
- Inline review (not independent/user approval): old FileTarget/writer/Trash
  and all six first-batch business bodies recover exactly after removing the
  new framing; second-batch handlers are byte-identical. Only reviewed record
  resolver/error blocks differ. Core/identity/protocol/search/definition helpers
  are unchanged. Original17 tests are preserved verbatim before the additions.
- Actual source ratchet on scratch copies: positive exit0, all32 negative
  controls exit1, covering explicit guard deletion/order, both stored guards,
  addressability-before-resolution and typed-error propagation.
- Inventory/freeze-check, contracts12, digest16 (largest21842B), context27/23
  with per-file/aggregate bounds, and tracked plus82 untracked whitespace checks
  pass. Coverage remains **75/641,45 dispatches/floor75**.

Slice37 is bounded complete. P8-05/G43 remain open; search stored-record
admission, real remote transport/image boundaries and full launch validation
are not covered by this checkpoint.
