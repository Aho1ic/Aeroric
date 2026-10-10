# Legacy existing-entry guards and rename

Current ownership: P8-05 slices48 and51. The guards and exact legacy rename
business live in `files::legacy_modify`; Tauri retains the rename worker/await
and delegates its body. The actual delete/OS Trash body has not moved.
The slice48 record below is historical, followed by slice51's delta/evidence.

P8-05 slice48, 2026-10-09. Bounded ownership migration, not new host placement
or P8-05 acceptance. Current checkpoint: `MIGRATION_PLAN.md`.

## One owner, unchanged consumers

`files::legacy_modify` contains the old
`validate_existing_path_for_delete`/`validate_existing_path_for_modify`
functions, unchanged except public visibility. The shared
`ensure_not_protected_path` and `PROTECTED_FIRST_SEGMENTS` remain private;
their code and comments moved verbatim. The two guards retain their original
return/action differences rather than introducing a refactored algorithm.

Tauri imports the two guards. The entire actual `delete_path` and `rename_path`
bodies are byte-identical to the pre-slice working tree, including workers and
guard binding/order. No rename/Trash IO moved to core in this slice. Other
fs bodies, previous tests, prior legacy services, native/host/remote/WSL/SFTP,
CLI/wire/lockfiles and inherited launch WIP are protected.

## Exact policy

1. Require an absolute target, not an absolute/directory-typed root.
2. Obtain the raw UTF-8 basename. No trim or new-entry name validation.
3. Canonicalize parent, then the original root spelling.
4. Refuse parent escape before any leaf metadata.
5. Join the raw leaf to the canonical parent without following the leaf.
6. Collapse every `symlink_metadata` failure to `Path does not exist`.
7. Apply the existing root/first-segment protection helper.

Delete fixes action=`delete`, returning the joined path. Modify preserves the
action string and returns `(joined path, canonical root)`.

Protection rejects the root itself, then case-insensitive `.git`/`.aeroric`
only in the first canonical relative component. This does not mean the actual
guard always returns the helper's root-specific error: a project-root target
fails parent containment earlier. Metadata precedes protection, so a missing
protected leaf still produces the metadata error. Nested `sub/.git` and an
ordinary leaf link into a protected directory are distinct from a protected
first component; a canonical-parent alias into that directory is protected.

Observed legacy boundaries, not new authorization promises:

- Relative root spelling, whitespace/Unicode names and canonical parent
  aliases are preserved. A file-root/child reaches the collapsed metadata
  error rather than acquiring native's directory-root check.
- Existing source names are not validated as new names. A Unix backslash
  basename can be renamed even though that is invalid as a new basename.
- Outside/dangling/looping leaf links remain entries; strict read/native
  resolution is not an equivalent replacement.
- Rename still trims/validates its new name before its source guard, constructs
  a sibling destination, calls `exists()`, then returns the collision error or
  calls `fs::rename`. On the tested Unix filesystem, a dangling/looping
  destination link can be replaced because `exists()` follows the link.
  This is preserved old behavior, not silent hardening or a safety guarantee.
- No descriptor-relative, general TOCTOU, ancestor-race, transaction, hard-link
  isolation or new writer/root authority guarantee.

## Safe evidence

Evidence: `/tmp/aeroric-legacy-modify-6x7x827t/`.

Seven guard/actual-rename cases passed on the old production implementation
before extraction: **7 passed,0 failed,0 ignored,1324 filtered**, rc0. Old
production fs bytes were verified unchanged except the test-module declaration.
The successful source is frozen and rerun byte-identically after extraction.
No manufactured behavioral red is needed for a placement-only move.

Each case re-execs exactly itself under env_clear, scratch cwd/HOME/data and
empty PATH, with timeout/reaping and one-nonignored-case checks. Snapshots cover
bytes/modes/links/entries/mtime, permitting only named successful rename changes
and immediate-parent mtimes. Refusal cases permit no changes.

**Actual `delete_path` is never invoked, even for expected refusal.** A
regression in that test could reach real OS Trash. Only its direct guard is
executed; actual command binding/body/worker order are source- and inverse-
reviewed. End-to-end OS Trash and Windows runtime remain unexecuted. Mocked
frontend delete consumers, when run, are not evidence of OS Trash behavior.

Post-extraction:

- Core `legacy_modify`4 plus files13/directory3/entry4/path3/read3/write-paths6:
  **36 passed**, no failures/ignored.
- Real Tauri broad `fs::`: **59 passed,0 failed,0 ignored,1272 filtered**,
  including the seven frozen cases and previous POSIX/remote/WSL regressions.
- Scoped core and all-target Tauri Clippy pass.
- New ownership gate and four previous legacy gates pass. New source controls:
  **5 positive/63 negative**, all expected outcomes. One initial negative
  probe replaced only one of three real calls, so the presence gate correctly
  stayed green; the scratch probe was corrected to remove all three. No
  product/test/gate assertions changed to accommodate it.
- Entire fs inverse, exact visibility-only core move and **50 protected
  hashes** pass. This includes unchanged actual delete/rename bodies, private
  protection helper/table/comments and all inherited legacy/native boundaries.

The source matcher is scoped to rustfmt top-level functions, not a full Rust
parser or security proof. Source controls complement runtime tests. Inline
review is not independent review or human acceptance; broader module/floor
results below were recorded after their actual runs.

## Integration

Same evidence directory, current rebuilt CLI and fresh configured raw logs:

- Mocked FileExplorer action/UI consumers:2 files/132 tests pass. Only mocked
  invoke/dialog/virtual files run; no frontend production source changed.
- `cli`:5 checks/10 tests; `files`:31/223; `cutover`:86/513;
  `skeleton`:24/188. All rc0 with no skipped cases.
- `tauri-adapter`:1392 tests including full Tauri **1331 passed,0 failed,
  0 ignored,0 filtered**. Tauri fmt/all-target Clippy/typecheck pass.
  The two inherited core fmt/Clippy failure logs are byte-identical to47;
  their only source is the protected, unchanged launch-validation WIP.
  This is not an all-adapter-green result.
- Full Tauri floor **1324→1331** raised only after the shared parser measured
  that full run. Current core4/command7 results in both files/cutover were also
  parsed. Reversing only six new case entries, four floors and this measured
  full-floor increase restores the exact pre-slice config. All35 module shapes
  and 177 non-orphan test floors validate.
- Only 13 fs definition pins changed. Every command name/parameter/registration,
  reachability/refcount and other command-map field is unchanged.
- Inventory/freeze,12 unchanged contracts,16 digest shards (max21842B) pass.
  Coverage remains75/641 with48 dispatches/floor75; default Embedded unchanged.

P8-05 in_progress/G43 open. No OS Trash/Windows runtime, new host capability,
production cutover, independent acceptance, commit or release claim.

Final closure rechecked inverse/50 protected hashes, five ownership gates,
measured floor and inventory artifacts. Context16/19 entries remain within
file/aggregate budgets; tracked and all121 untracked files whitespace-clean.
The main plan continues to the researched legacy recursive-copy helper boundary.

## Slice51 — rename business extraction

2026-10-09, evidence `/tmp/aeroric-legacy-rename-zqjj1kzz/`. Review baseline is
this slice's67 `.before` snapshots, not HEAD or a previous slice's inverse.

The worker's original body moved, dedented only, to
`files::legacy_modify::rename_path(String, String, String) -> Result<(), String>`.
Its name validator is the existing shared `legacy_entry::validate_entry_name`.
The two guards/private protection helper/table are unchanged. Tauri retains
its exact public signature, `spawn_blocking`, await and join-error mapping.
Its only body change is the fully qualified delegate. The name/modify-guard
imports, now used only by the unchanged Unix test module, are
`cfg(all(test, unix))`. Existing copy imports and actual delete/Trash are
byte-identical, as are every other fs body and old test.

Preserved order: new-name trim/validation → existing-entry guard with action
`rename` → parent/sibling destination → `destination.exists()` refusal →
`std::fs::rename` and its raw error string. Source names remain raw and
unvalidated; source leaf links remain entries. Live destinations refuse, but
dangling/looping destinations may be replaced as entries. This is **not**
legacy copy's destination-link copy-through behavior or native rename's
different admission/overwrite policy. No new history, settings IO, writer,
root authority, host method, dispatch, fallback or atomicity guarantee.

### Targeted evidence

- Before extraction the same seven actual guard/rename cases passed:
  **7 passed,0 failed,0 ignored,1330 filtered**, rc0. All67 snapshot hashes
  were unchanged and the successful test source was frozen again.
- New, separate `tests/legacy_rename.rs`:4 direct cases. The old4 guard tests
  and their no-rename header remain byte-identical. Coverage includes
  new-name-before-source errors/protection/collisions; trim/raw source names,
  modes, children and parent/root aliases; unfollowed inside/outside/dangling/
  looping sources; destination-link replacement and native-policy contrasts.
- Core10 suites: **52 passed**, including rename4/guard4/copy8. Actual broad
  Tauri `fs::`: **65 passed,0 failed,0 ignored,1272 filtered**. Scoped core
  Clippy and Tauri all-target Clippy both pass.
- Full-tree snapshots include bytes/modes/link text/entries/mtime. Only named
  renames and immediate-parent mtimes may change. Valid entries and every
  possible link referent are inside scratch. The relative-refusal input has
  a NUL so a regression cannot accidentally address a real cwd entry.
- Actual delete/OS Trash is **never invoked**, even for refusal. Native
  overwrite/outside-source refusal contrasts execute only against scratch
  fixtures; Unix cases do not claim Windows runtime execution.
- The modify gate now checks the exact core body plus the thin actual worker
  and test imports. Old guard/delete/fixture assertions stay. The entry gate
  follows only rename name/trim ownership and its bindings; copy/name/create
  assertions stay. Controls pass: modify **7 positive/92 negative**, entry
  **9 positive/82 negative**. These are source-scanner controls, not executed
  runtime mutants.
- Whole-fs reconstruction, dedent-only core inverse, frozen7/old guard4 and
  **61 protected hashes** pass. The source matchers are bounded checks, not
  general Rust parsers. Review is inline, not independent/human acceptance.
- Real extractor controls: old0; old with core/test sources0; new1; removing
  only the new delegate0; full-repo rename reference0→1. Exactly13 fs
  definition pins change; every other map field is preserved.
- Only the measured new rename4 suite/floor is added in files and cutover.
  Existing guard4/command7/full Tauri1337 are unchanged. Reversing two cases/
  floors and six slice notes restores the exact pre-slice config;35 modules
  and183 non-orphan floors validate. Broad integration is recorded separately
  after running it, not inferred from these targeted results.

### Slice51 integration

Fresh rebuilt CLI and configured raw logs in the same evidence directory:

- Mocked FileExplorer action/UI consumers: **2 files/132 tests**, rc0; no
  production frontend changes or real OS operation.
- `cli`: **5 checks/10 tests**; `files`: **35/241**; `cutover`: **90/531**;
  `skeleton`: **24/188**. All rc0, no skipped cases.
- `tauri-adapter`: **1398 tests**, including full Tauri **1337 passed,0 failed,
  0 ignored,0 filtered**. Tauri fmt/all-target Clippy/typecheck pass. Only the
  inherited core fmt/Clippy failures remain; both complete raw logs are
  byte-identical to50, and the protected launch WIP hash is unchanged.
  Adapter is **not** all green.
- Final floor audit reads both modules' fresh rename4/guard4/command7/ownership
  logs and full nonfiltered Tauri1337; config inverse and35 modules/183
  non-orphan floors pass. No existing floor decreased or increased.
- Coverage remains **75/641,48 dispatches/floor75**, default Embedded.
  Inventory/freeze/contracts/digest/context and final whitespace are checked
  before the plan's bounded-completion marker.

No Windows app/OS Trash run, new host method, independent/human acceptance,
production cutover or release. Next research concerns the distinct legacy
hex-layout local-history owner; existing native history is not an alias.

Slice51 final closure: exact inverse/61 protected hashes, six legacy ownership
gates, measured floors, coverage and inventory/freeze pass;12 contracts remain
unchanged and16 digest shards stay within32768B (largest21842B).
Context17/19 entries meet per-file/aggregate budgets. Tracked changes and
**all127 untracked files** are whitespace-clean. P8-05 in_progress/G43 open.
