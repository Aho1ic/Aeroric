# Legacy local history — P8-05 slice52

## Boundary

`files::legacy_history` owns the 21 synchronous functions previously in
`src-tauri/src/local_history.rs`. Their bodies, error/IO ordering and wall-clock
call sites are unchanged. Tauri retains its three actual async command workers,
signatures, error mapping and the old four tests; fs save and formatter retain
their snapshot binding. Those callers, actual delete/OS Trash, native history,
host and notebook bodies have not been migrated or replaced in this slice.

Only genuinely equal types and values are shared from `files::local_history`:
`HistoryLayout`, both camelCase DTOs, `CODE_LAYOUT`, `MAX_SNAPSHOT_BYTES` and
`SNAPSHOT_EXTENSION`. There is no legacy-to-native history conversion, read
repair, automatic layout migration or new host method. `prune_history_dir`
is public solely to preserve the existing desktop test binding, not an RPC.
The old root guard already aliases `validate_legacy_project_root`; the core
wall-clock helper has equivalent u64 epoch/default-zero semantics.

The three command-map `core_service` annotations now name the actual legacy
owner, not the different native implementation. Their frozen `files.*` RPC
names/params remain unchanged. Source definition and registration pins are
reanchored; method coverage is not increased by this extraction.
The existing `modules.local_history` rollup follows these three rows. The
inventory caught its initially missed update. Generated method-schema
`coreService` annotations must also follow this owner; regeneration does not
change the method names, params, capabilities or wire formats.

## Differences that must not be erased

| Boundary | Legacy policy | Native policy |
| --- | --- | --- |
| Storage key | hex of `/`-joined relative components | raw relative path |
| List directory | missing→empty, existing errors propagate | read errors suppressed |
| List entries | DirEntry metadata/is_file, metadata size | reads bytes, filters read errors |
| Entry admission | digit/hyphen check, metadata/file, then timestamp | different checks, invalid timestamp→0 |
| Restore backup | skips matching, invalid UTF-8 and oversized current content | forced backup, matching retained, invalid UTF-8 fails |
| Throttle | suffixed timestamps count, clock sampled after scan | different ID scan, supplied timestamp |

Root/source validation and history-path policy are also distinct. A legacy
source must be absolute, canonical, in-root and a file. History directory/entry
symlinks are not confined by that source guard. Tests contain every possible
referent inside scratch; preservation is not a confinement/TOCTOU guarantee.
Listing ignores a symlink entry via DirEntry metadata, while explicitly reading
its numeric ID follows metadata/read. Odd digit/hyphen IDs preserve the old
error precedence. Prune can fail after creating a new snapshot; the caller then
does not write the target, and the new snapshot is not rolled back.

The unchanged strict actual fs writer rejects a missing file with
`Path is outside the allowed directory` before entering history. Directly
calling history instead reports `Cannot resolve file path: …`. The first
baseline exposed a fixture that confused these paths; only the expectation
and explicit contrast were corrected, not production code.

## Scope corrections found by verification

Moving history removed the final production caller of desktop
`clock::now_ms_u64`. Its old implementation and four clock tests remain
unchanged behind `cfg(test)`; the production i64 implementation is unchanged
and links to the live core u64 documentation.

Clippy's test-unwrap heuristic did not recognize the new fixture module's
`cfg(all(test, unix))`. The existing stacked `cfg(test)` / `cfg(unix)` pattern
has the same compilation predicate and is recognized. The frozen eight-case
fixture file is byte-identical; no `allow` suppression or fake caller was added.

The protocol regression run exposed an unrelated, inherited incomplete P8-04
test update. Rust still expected9 mapped/29 pending topics and38 inventory
events, while the unchanged HEAD schemas and existing exporter/Swift/TS checks
already distinguish11/27 and38 legacy channels +3 direct v1 dual-emits.
Only `protocol_roundtrip.rs` was corrected, with exact mapped/direct topic
sets and a legacy-channel bijection. Both failure logs are retained; focused1,
the full protocol gate6/98 and protocol all-target Clippy now pass. No emitter,
event schema, capability or protocol version changed.

## Evidence and limitations

Evidence directory: `/tmp/aeroric-legacy-local-history-zdx97ha8/`.

- Old actual commands/save consumer: corrected baseline12 passed (new8+old4),
  zero ignored; initial11/1 fixture failure retained. Successful source frozen.
- After extraction: seven core suites44 and exact notebook snapshot12 passed.
  Actual history/fs/formatter80 initially passed; final scope recheck includes
  clock,84 passed/0 ignored; all-target Tauri Clippy passed.
- Inline exact inverse:21 old bodies, three workers, old history4/clock4,
  frozen actual8, shared model/layout values and76 protected hashes checked.
- Source controls and real inventory reference controls use scratch copies.
  They are source probes, not executable Rust fault-injection tests.
- Mocked FileViewer history and format-on-save consumers:2 files/2 tests passed.
  These do not execute Tauri formatter, a real shell or OS dialogs.

Current CLI/relevant module results and the final measured floors are recorded
in `MIGRATION_PLAN.md` as they finish. Inherited launch-validation WIP remains
unchanged, including its separate core fmt/Clippy failures. No Windows runtime,
real formatter/history/Trash, independent review, full cutover, release or
P8-05 acceptance is claimed.

Final regression evidence for this slice is under `resume-5hgwmjzc/` in the
same directory: cli5/10, files38/261, editor2/14, cutover93/551 and
skeleton24/188 pass. Adapter executes1406 tests successfully, including
Tauri1345/0/0/0; only the two inherited WIP checks fail. The measured Tauri
floor is1345. Three positive/seventeen negative fresh source controls, five
real-extractor controls, exact map/checks inverse and75 original protected
hashes pass. These counts describe bounded extraction, not full cutover.
