# Legacy text/image queries

P8-05 slice46, 2026-10-09. Bounded extraction complete; not External placement, new
RPC coverage, transport completion, independent review or P8-05 acceptance.
Current execution checkpoint is in `MIGRATION_PLAN.md`.

## Ownership and retained contracts

`core/crates/aeroric-core/src/files/legacy_read.rs` owns the old synchronous
read bodies, `ImagePreviewData`, image limit and private extension-to-MIME
mapping. Tauri keeps the existing command names and parameters, text
local/strict wrappers, pre-worker path validation and blocking workers.
Those workers call the shared functions with the already validated path.
Core does not gain project authority by accepting `&Path`.

- Text: File open, descriptor metadata, `>2*1024*1024` refusal, String capacity
  from metadata, BufReader UTF-8 read. Empty/BOM/CRLF/Unicode/NUL remain exact.
  IO/encoding errors are unwrapped strings; size error remains
  `File too large ({:.1} MB)`.
- Image: extension of the **validated** path, lowercase MIME lookup, unsupported
  format refusal, File open/descriptor metadata, `>10MiB` refusal, capacity from
  metadata and BufReader bytes. PNG/JPG/JPEG/GIF/WebP/BMP/SVG are supported;
  content is not decoded or signature-validated. Use STANDARD Base64, not
  URL-safe encoding. Result has exactly camelCase `dataUrl`, `mimeType` and
  `byteLength`; length is `meta.len()`, not the subsequently read buffer length.
- Preserve legacy symlink behavior: a canonical in-root alias uses the target's
  extension; an allowed external leaf uses the unresolved link's extension.
  Root validation stays before the worker, MIME stays before image IO and size,
  and text size stays before UTF-8 decoding.
- Metadata/path observations remain point-in-time. No new post-read growth
  limit, descriptor-relative confinement, race isolation or image-safety claim.

The core bodies equal the old workers after indentation and three borrow
adaptations: both File-open arguments and the image MIME argument. DTO and
limit changes are visibility only. Removing the new test-module declaration,
undoing the DTO/limit/MIME/Base64 relocation and the two delegates reconstructs
the whole pre-slice `fs.rs` byte-for-byte. All other commands, mutation bodies,
guards, scheduling and earlier tests stay unchanged.

## Not an alias for native or remote behavior

Native `read_write::read_file_content` has different path/stat/error semantics.
`preview::preview_decision` returns a classification enum, not a data URL; it
accepts ICO/TIFF as images and treats SVG differently. Its implementation is
unchanged; only a comment now points to the relocated legacy limit.

The old 2MiB text and 10MiB image limits exceed the v1 1MiB frame. This slice
does **not** lower them, enlarge frames, add aliases/host registrations or
claim a large-response CLI/External roundtrip. Native, POSIX/SSH/WSL/SFTP,
clipboard/UI, mutation and writer/target authority remain separate boundaries.

## Behavioral evidence

Evidence directory: `/tmp/aeroric-legacy-read-3kpaumiv/`.

Seven actual-command cases passed against the old Tauri bodies **before**
moving them: 7 passed,0 failed,0 ignored,1309 filtered. The successful test
source was frozen and remains byte-identical after extraction. Each parent
re-execs only its exact test under `env_clear`, scratch cwd/HOME/data and empty
bin PATH, bounds/reaps the child and asserts one real nonignored test.
Snapshots cover bytes, modes, mtime, links and directory membership.

Observed exact-size cases: 2MiB text and 10MiB image succeed; +1 byte refuses,
without decoding invalid UTF-8 first. The image's encoded portion is
13,981,016 bytes. Tests cover all MIME values, STANDARD `+/` Base64 and exact
wire keys, raw errors/order, file-as-root, canonical versus unresolved-link
MIME and no writes. This is green-before/green-after compatibility evidence,
not a manufactured red/green bug fix.

Post-extraction targeted checks:

- Core `legacy_read`3, `files`13, `legacy_directory`3, `legacy_path`3: all pass.
- Actual Tauri broad `fs::`: 44 passed,0 failed,0 ignored,1272 filtered,
  including all seven unchanged new cases and earlier POSIX/remote/WSL tests.
- Scoped core Clippy and Tauri all-target Clippy: rc0.
- New reader gate and prior directory/path gates pass. Reader gate has
  **6 positive / 51 negative** scratch-source controls for scope, MIME, size,
  metadata/error/read order, wire/Base64, guard/worker/flags and real fixtures.
- Inline inverse review verifies the full fs file and **41** protected hashes
  including host/CLI/protocol/native/remote/Swift/React/lockfiles and launch WIP.

Source controls supplement behavior; their top-level rustfmt function matcher
is not a general Rust parser or a security proof. No independent reviewer or
human acceptance is implied by the inline audit.

## Inventory and gate ownership

Thirteen fs definition pins are reanchored; public names, registrations and
parameters remain frozen. The real extractor excludes `*_tests.rs`. Each new
**production delegate**, not its test file, adds one Rust reference for
`read_file_content` / `read_image_preview` (0→1 each). No other mapping or
reachability metadata changes.

Measured core3/command7/ownership groups are registered in `files` and
`cutover`. Actual final-code runs:

- Existing native preview1/1 (6 filtered); mocked frontend consumers3 files/
  8 tests. No frontend source was changed.
- Rebuilt current CLI, complete headless smoke and `cli`:5 gates/10 tests.
  Smoke exercises existing served methods, not a new large-response transport.
- `files`:25 gates/200 tests; `cutover`:80/490; `skeleton`:24/188.
- Adapter:1377 test cases executed, including full Tauri **1316 passed,
  0 failed,0 ignored,0 filtered**. Its two failing fmt/core-all-target-Clippy
  gates refer only to inherited `validate_agent_launch_services.rs`; both raw
  failures are byte-identical to45. This is not an all-green adapter.
- Only after parsing the full raw run with shared `countCases` was the Tauri
  floor raised1309→1316. Inverse configuration review proves every unrelated
  old case/command/floor unchanged;35 module shapes/169 non-orphan floors pass.
- No skips in these five modules. Raw artifacts were copied at each module's
  completion; obsolete prefix-matched logs are not counted as current cases.

Four scratch copies using the real reachability extractor establish
old-source0/0, old+tests0/0, new delegates1/1, removing only delegates0/0.
The inherited launch WIP remains byte-identical, not formatted/ignored/deleted
to manufacture green. Coverage is still75/641,48 dispatches/floor75, not a
count of extracted bodies. G43 and P8-05 stay open.

Final inverse/floor/source/coverage/inventory/freeze checks pass;12 protocol
documents are unchanged,16 digest shards remain current (largest21842B).
Context15/17, tracked diff and all111 untracked whitespace checks pass.
Per-file32KiB and combined128KiB context limits hold, including automatic
task artifacts. Closure does not authorize Phase9, release or production data.
