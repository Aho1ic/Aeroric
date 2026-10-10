# Local run-config documents — P8-05 slice 29

## Single owner and retained boundaries

`core::ide::run_config` owns the existing document/config/breakpoint/process
snapshot models and local definition IO. Tauri reuses the models and local
validation helpers; `run_config/commands.rs` contains the two actual generic
Runtime commands. The retained remote IO, shell/debug launch decisions, output
readers and process lifecycle are **not** hosted by this slice.

The earlier P6-03 “verbatim” description was inaccurate: core serialized debug
types as `Node`/`Python`, and its `validate_document` was stricter about
version/duplicate IDs but did not validate local paths or commands. A real
serde regression failed before the lowercase fix. The existing native editor
policy and its tests remain separate; legacy IO never calls that validator.
No historical user acceptance is inferred or rewritten by this correction.

## Compatibility contract

- Root: original string, absolute, canonicalize, then require directory. Shared
  `files::path_policy::validate_legacy_project_root` is the exact old Tauri guard;
  `path_guard.rs` re-exports it under the old name for all existing callers.
  Native file-policy `validate_project_root` retains its different early
  existence error. Neither is a project-registry ACL.
- File: `<canonical-root>/.aeroric/run-configs.json`. Missing (including a
  dangling leaf link or a non-directory `.aeroric` parent hidden by `exists`)
  returns defaults. Present invalid UTF-8/JSON or invalid configs error.
  Reads never repair, mkdir, chmod, back up or rewrite.
- Read version 0 becomes 1 only in the reply; other versions and duplicate IDs
  remain accepted. `debugType` is `node`/`python`, missing debugType is node,
  and existing env/args/breakpoint/document defaults remain. `{}` is valid;
  serde also accepts its existing sequence form (`[]`, `[0, []]`). No
  blanket “malformed means defaults” fallback is added.
- Legacy local validation is lexical: nonempty identity/name/command or
  program/cwd, in-root cwd/program/breakpoint paths, positive line/column.
  Missing in-root paths remain valid. Launch-time canonical cwd validation
  remains separate. Local environment validation is not replaced with the
  stricter remote environment policy.
- Write returns the **normalized document**, not null. It forces version 1,
  validates before mkdir, pretty-serializes a full replacement and uses
  `storage::atomic::write_legacy_atomic` (temp, sync, rename, cleanup).
  Inherited new-file permissions and existing directory modes remain; no
  attachments or private-mode change. Root/directory links are followed;
  atomic rename replaces a leaf link without modifying its target.

## Authority and wire

Frozen methods: `ide.read_run_configs({projectPath})`,
`ide.write_run_configs({projectPath, document})`. Fully decode a params object
and its required fields before IO. Root/semantic/type errors are
InvalidArgument; filesystem/JSON/persistence/current-lease errors are IoError.
Embedded keeps the old String diagnostics.

`write_run_configs` checks the current writer lease **inside** the common
process-local mutex. `write_run_configs_legacy` shares that mutex/body but
retains the old Embedded authority. Reads are lock-free. This is not an
optimistic revision transaction or coordination with arbitrary external tools.
Legacy durability promises do not add directory fsync or cross-platform
atomic replacement guarantees beyond the existing mechanism.

ADR-006 `params.target` cannot be ignored: omitted means legacy local,
malformed/null means InvalidArgument, SSH/WSL means UnsupportedCapability
before local IO. An explicit local target must match the required original
projectPath string; conflicting strings are rejected without reading either
root. Target-only calls and remote definition IO are not implemented here.
Use the protocol `project::ProjectTarget`, not the different IDE UI model.

Actual Tauri IPC defers **all** local root validation and IO. It forwards the
original path/document, decodes the existing DTO and never retries locally
after host failure, disconnect, malformed reply or missing session.

## Evidence and limits

`/tmp/aeroric-runconfig-red-09krbuc2/`:

- Behavioral red after successful compilation: serde 0/1; real host 1/7;
  actual command 3/8, with all 15 old run-config tests green.
- Core documents 9 plus lsp_protocol 9 passed. Host initial 7 passed.
- Inline schema review added a real target red: 0/1, because an SSH-target
  request returned local data. After the refusal fix, host 8/8 passed.
- Actual commands 8 + old run-config 15 + root guard 10 + incidental git
  guard 4 passed (37 total). All runs above had zero ignored tests.
- Initial test/non-test unused-import warnings were removed; final broad
  checks and current CLI evidence are recorded in MIGRATION_PLAN.md.

The ownership script is a source ratchet, not a parser or runtime proof.
Verification rebuilds the current CLI before smoke, which checks definitions
without executing their shell command. P8-05/G43, remote ownership, launch-validation WIP and
production-data cutover remain open. No default placement change or release.

Final bounded verification: `/tmp/aeroric-runconfig-integration-gfduudpp/`:
CLI rebuilt/full smoke, cutover46/242, skeleton24/188, IDE3/16, Tauri1276,
scoped core/host clippy and Tauri fmt/clippy/typecheck passed. React6:
`/tmp/aeroric-runconfig-web-0xz88ba9/`. Final adapter rc1/1337 tests retains
only the untouched launch-validation WIP fmt/clippy failures. Inventory,
12 unchanged contracts,16 digest shards, runner36 and whitespace58 passed.
The existing bounded v1 frame transport is unchanged; this slice does not
promise an unbounded JSON response or introduce a chunking protocol.

Slice30 moves the already verified target decoder into private host
`local_targets.rs`, shared by project-config and run-config handlers. The
run-config diagnostic and call order remain unchanged; see
`local-definition-targets.md` for new refusal regressions.
