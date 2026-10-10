# Shared formatter policy — P8-05 slice53

## Boundary

`core/crates/aeroric-core/src/files/format.rs` owns the shared
`FormatterCommand`, `FormatFileResult`, `formatter_for_extension` and
`validate_file_path`. The two function definitions were byte-identical to the
old desktop copies apart from validator visibility. DTO fields, derives and
camelCase serialization were equal too.

`src-tauri/src/formatter.rs` imports the validator and reexports both types.
Its typed one-line selector forwarder preserves the existing Rust binding
without an unused type import. The actual async worker and original three
tests are unchanged. This is shared policy, **not shared execution**:

| Boundary | Legacy desktop worker | Existing native/host runner |
| --- | --- | --- |
| Root | Original String and legacy root guard | Caller-validated root, host projectId/current-writer authority |
| Snapshot | Legacy hex layout with `next_content=""` and ordinary skips | Caller hook; host uses raw-layout force snapshot |
| Process | Background configuration and late cached login-shell environment | Synchronous command using its inherited environment |
| Failure | Error returned; no automatic restore | Error returned; no automatic restore |

The selector is pure and preserves all 16 extensions, ASCII case conversion,
leading-dot removal and literal argv. It neither parses a path's extension nor
quotes/splits the supplied filename. The validator performs read-only
canonicalization/metadata IO, assumes a validated canonical root and keeps
absolute → resolve → containment → file-kind error order. It is not a root ACL,
lexical-only validation or a TOCTOU guarantee.

Neither runner implements timeout/cancellation or rollback of an external
program's writes. A failed formatter may already have modified its input.
Misleading core and host call-site comments were corrected; every other byte
in those files is unchanged. An older invalid-source test is not proof of a
general external-formatter rollback guarantee.

No history, writer, delete/Trash, notebook, platform/login-shell, remote/WSL,
host authority, method, parameter, capability or default-placement change.
The map's existing `files::format` owner remains; only `format_file` source
citations move to declaration11 and unchanged registration527 (old pin500 was
stale). The real extractor still reports zero Rust production references;
five source controls distinguish excluded tests/core from a positive
production reference. All 12 generated contract documents remain unchanged.

## Verification and limits

Fresh snapshots: `/tmp/aeroric-legacy-formatter-hxz5togj/`.
The 95 source snapshots and 224 inherited dirty hashes predate implementation.
They are the comparison baseline, not a reset of the inherited working tree.

- Before sharing, new eight isolated cases plus old three passed against the
  actual old worker: **11 passed, 0 failed/ignored, 1342 filtered**.
  Successful actual fixture and desktop source are frozen as `*.baseline.rs`.
- After sharing, six core suites **42 passed** (including direct policy5),
  Tauri formatter/history/fs/clock **92 passed, 0 failed/ignored,
  1261 filtered**, both package all-target Clippy and scoped rustfmt passed.
- Exact desktop prefix/suffix, actual worker, old3, frozen8, DTO and
  core/host comment-only inverse passed. At that stage **92/95** protected
  source hashes and **222/224** inherited dirty hashes were unchanged.
- The ownership gate has **5 positive /44 negative** scratch source controls.
  These are source probes, not compiled Rust mutants or independent review.
- Actual cases use exact env-cleared children, owned HOME/PATH/cwd,
  kill/reap deadlines and one-executed-case assertions. Every symlink
  referent is scratch-contained. Full-tree assertions check bytes, modes,
  link text, entries and mtimes, permitting only named snapshot additions
  and their parent-directory mtimes.
- Actual commands stop before spawning: invalid root/file, canonical target
  extension, unsupported/no-extension after a hex snapshot, history
  empty/binary/oversize/self-history skips, and history failure before
  selection. Direct native cases verify callback order without a process.
  Neither these cases nor the mocked FileViewer consumer prove a successful
  formatter run, production cutover, real shell configuration or OS Trash.

`formatter-policy-core-rust` (min 5), `formatter-policy-command-rust` (min 11)
and `formatter-policy-ownership` are registered in files/editor/cutover with
matching runner floors (35 modules / 193 non-orphan floors here).

Current modules after the rebuilt CLI: `cli` 5/10, `protocol` 6/98, `files`
41/277, `editor` 5/30, `cutover` 96/567, `skeleton` 24/188, all rc 0; mocked
FileViewer consumers (`file-viewer-local-history`, `file-viewer-format-on-save`)
2 files / 2 tests rc 0. `tauri-adapter` executes 1414 cases with src-tauri lib
tests 1353 ≥ floor 1345 and keeps exactly the two inherited launch-validation
reds: fresh `core-cargo-fmt` stdout and `core-clippy` diagnostics are
byte-identical to the slice-52 baseline, WIP SHA256
`a5c2a944…c037782` unchanged. Light gates: doctor 18/18, inventory consistent
(660/660 definitions), freeze byte-noop, contracts 12 byte-identical, digest
16 shards (max 21 842 B), coverage 75/641 with 48/48 dispatch.

P8-05 remains in progress/G43 open; the unrelated launch-validation WIP and its
core fmt/Clippy failures were neither edited nor suppressed to green this slice.
