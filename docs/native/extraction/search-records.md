# Search saved-record root admission — P8-05 slice38

## Defect and bounded change

Search already rejected unavailable explicit targets and checked current writer
authority for both applies. Its root resolver nevertheless trusted a selected
project's raw `path`, and path-only candidate scans also trusted every project's
task worktrees, without admitting the saved `location`.

`search_services::root_for` now reuses the host-private
`file_access::require_local_record` introduced in slice37:

- A `projectId` selects that record. Reject an unsupported saved SSH/WSL location
  or unaddressable location with its existing typed error and closed
  `detail.field="location"` before accessing its raw root.
- For path/Local-target-only selection, skip inadmissible project candidates
  **before** inspecting their raw roots or loading their tasks. Do not turn an
  unrelated bad record into a global error. Without an independent eligible
  project/worktree/vault root, retain the existing NotFound response.
- Independently registered notebook vaults remain authoritative; unregistering
  one removes that authority. Supplying `projectId` still selects that record
  and is not overridden by a separate vault registration.

The helper reuses core addressability and location policy, not a second identity
parser. Local raw-path selection, worktree/vault matching, all eight operation
bodies, six read-only queries and both current-writer applies are unchanged.
Existing explicit `projectPath`/Local-target canonical-conflict checking remains
in its original position; this slice does not move all request-path validation
behind saved-record admission. Admission is request-time, not an atomic
revocation or TOCTOU guarantee.

No remote transport, schemas, new methods, Tauri/Swift/frontend behavior, core
algorithm or coverage increase is claimed. The entire runtime delta is one
import and two guard blocks in the existing search resolver.

## Behavioral evidence

`/tmp/aeroric-search-record-red-n1ug6f67/`:

- Expanded real authenticated UDS suite compiled against the old resolver:
  **10 passed/12 failed/0 ignored/0 filtered, rc101**. The original9 tests and
  valid Local/worktree control passed.
- Eight operations × four saved locations (SSH/WSL/null/unknown) × five
  selectors (id/id+Local/path/target-only/worktree) ran all **160 pairs** on
  fresh fixtures. Old behavior produced **40 apply mutations** and **40
  fake-Git invocations**; reads returned local results.
- The two guards make the suite **22/22**, with nine previous suites **97/97**,
  zero ignored. Host library and all ten scoped test targets pass clippy with
  `-D warnings`.
- Additional cases cover read-only/current-writer precedence, an unrelated
  ineligible record with poison tasks, vault registration and revocation, and
  same-connection record change/refusal/recovery. Fake-Git positive controls
  prove its invocation marker detects an allowed operation.

Scratch roots, bytes/modes/entry snapshots, missing rg and injected fake Git
isolate the tests. No real HOME, Git configuration, credentials, SSH or agents
are accessed. The old9 test/fixture source is preserved as a byte-identical
prefix; thirteen tests were appended.

## Integration (2026-10-09)

`/tmp/aeroric-search-record-integration-889y8nq6/`:

- Rebuilt CLI and full headless smoke pass. The new smoke exercises **48**
  refusal calls, valid Local worktree and restored-record controls, and checks
  unchanged scratch project/task/file bytes and directory entries.
- `files` **16 gates/171 tests**, `cutover` **61/394**, `skeleton` **24/188**
  pass. Only the measured search-write-authority floors change **9→22** in
  files/cutover; no other gate command, count or floor changes.
- Adapter executes **1345 tests**, including **1284 Tauri tests**, successfully.
  Its module still exits1 solely for inherited launch-validation WIP fmt and
  two clippy diagnostics. The WIP SHA256 remains
  `a5c2a9447fb92592de48a3618afdf99b48033e8d32bb3ece44107f3f1c037782`.
- Inline review, **not independent or user approval**, verified that reversing
  the import/two guards restores the complete pre-slice runtime file exactly.
  Excluded core/file/identity/definition/WIP hashes and old test prefix match.
- The actual source ratchet on scratch copies passes its positive control and
  rejects **all12 negative controls**, including missing/late/inverted record
  guards, swallowed typed errors, lost vault authority, writer ordering and a
  duplicate parser.
- Inventory/freeze-check, contracts12 and digest16 (largest21842B) pass.
  Context27/23 is within each32768B/aggregate131072B bound; tracked and
  all83 untracked file whitespace checks pass (`final-artifacts.json`).

Slice38 is bounded complete. Coverage remains **75/641,45 dispatches/floor75**.
P8-05 and G43 remain open;
actual remote search, remaining file methods/image transport and full launch
validation are outside this bounded slice.
