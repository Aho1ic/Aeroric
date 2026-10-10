# Local debug-config documents — P8-05 slice31

## Boundary and discovered mismatch

The frozen methods are `ide.read_debug_configs` / `ide.write_debug_configs`.
They read/write `.aeroric/debug-configs.json`, not `debug.json`, and return
`DebugConfigDocument` (write does not return null). Both take ADR-006 targets.

The old Tauri definition models used lowercase node/python. The earlier core
copy omitted that serde annotation and emitted Node/Python; an independent
wire expectation reproduced this, whereas a self-roundtrip had hidden it.
The annotation is now shared with the original model defaults. Core's
`validate_document` is still a distinct native editor policy (version1,
unique IDs, nonempty names), not the actual legacy IO validator.

This slice does not implement DAP execution, attach networking, adapter
discovery, remote IO or a new permission model. It does not reopen prior
user-approved tasks or treat their old “verbatim” wording as proof.

## Single owner and compatibility

`aeroric-core::ide::dap` owns the definition DTOs, default values and IO:

- The legacy root guard uses the original projectPath: absolute, then
  canonicalize, then directory check, with the original String diagnostics.
- Missing file returns defaults. UTF-8/JSON/semantic failures are errors,
  never read repair. Read version0 becomes1 only in the reply; nonzero
  versions, duplicate IDs, `{}` and serde's sequence form remain accepted.
- Definition validation preserves its order: ID/name, lexical cwd,
  launch/attach program or endpoint, then breakpoints. Launch requires
  program; attach may omit its contents but still requires the field, a
  nonblank host without internal whitespace/slashes and a nonzero port.
  Breakpoint line/column must be positive; present condition/logMessage must
  not be blank. These checks do not mutate the supplied strings.
- Lexical candidates may be missing or refer through a symlink. This is not
  live debugger containment. Tauri retains canonical cwd/program resolution,
  remote path validation and debugger process/connection ownership.
- Writes force version1, validate before mkdir and pretty-print through
  `storage::atomic::write_legacy_atomic`, not the settings writer. Preserve
  inherited creation modes, existing directory modes, no attachments,
  directory/root links, leaf-link replacement and temp cleanup on failure.
- Checked and explicit legacy writes share a process-local mutex. Checked
  writes call the current `CoreContext::require_write` inside that mutex and
  before filesystem IO. This is not revision merging or an external-program
  lock. Readers observe complete old/new documents without acquiring the lane.

Tauri re-exports only the definition models; its session DTOs stay unchanged.
`dap/paths.rs` imports shared lexical/endpoint validators under the old names
for retained callers, and keeps live resolution and remote helpers. The five
redundant clones of already-Copy shared enums are the only mechanical changes
inside retained execution functions.

## Host and actual IPC

Host `ide_services` uses the existing blocking adapter and shared typed
parameter decoder, preserving its String diagnostics for run-config callers.
The debug handlers decode all required fields before `local_target_error`.
Omitted target or an exactly matching Local string follows the legacy path.
Null/malformed/conflicting targets are InvalidArgument; SSH/WSL is
UnsupportedCapability. Required projectPath/document remain required.

The actual registered Tauri functions are in `dap/commands.rs`; all local
work, including root validation, is inside the deferred Embedded future.
External returns a typed host result or an error. A missing session, bad host data, unsupported method, disconnection or refusal
must never trigger a retry or local configuration IO. Core parse/filesystem/lease failures map to IoError in the
host; argument/legacy validation failures map to InvalidArgument.

No new remote capability is claimed. Parent registration alone does not
implement the SSH twins. Default placement stays Embedded.

## Evidence (bounded completion, 2026-10-09)

`/tmp/aeroric-debug-config-red-fg0bf6qj/`:

- `serde-red.log`: compiled,0 passed/1 failed, Node versus node.
- `host-red.log`: compiled,1 passed/7 failed; unauthenticated control passed.
- `command-red.log`: compiled, command3/8 plus original DAP29/29; five
  External failures demonstrated the old local execution. All zero ignored.
- `core-green.log`: definition IO10 and DAP protocol3 passed.
- `host-green.log`: debug8, existing run-config8 and project targets5 passed.
- `command-green.log`: actual command8 plus all original29 passed; a removed
  IO import then produced a warning, addressed before final scoped/broad runs.

Final integration: `/tmp/aeroric-debug-config-integration-n6_l7mm0/`.

- Current CLI rebuilt, then the full synthetic headless smoke passed,
  including debug definition wire, versions, targets and validation.
- cutover52/302, skeleton24/188 and IDE3/17 passed without skips.
- Full Tauri lib1284/1284 passed, zero ignored; fmt, all-target clippy and
  TypeScript check passed. The paired test floor was then raised to1284.
- Final adapter is **rc1/1345 tests**, not fully green: only core fmt and
  core all-target clippy fail on the inherited, untouched
  `validate_agent_launch_services.rs`. Raw logs are in `adapter-final-raw/`.
- Source-token comparison covered six moved helpers,22 retained path
  functions/tests,79 retained DAP functions/tests (only five Copy-enum clone
  removals) and seven pre-existing core DAP bodies. Tauri remote/protocol
  files have no diff. This was an inline review, not independent approval.
- The three mocked React debug consumer files passed13 tests:
  `/tmp/aeroric-debug-config-web-p7snwv7n/vitest.log`.
- Coverage75/641,45 dispatches/floor75; inventory/freeze/reachability,
 12 unchanged contracts,16 digest shards (max21842B), runner36 and
  context20+16 passed. Tracked diff and68 untracked files were whitespace-clean.

Source ratchets check ownership/call order; they are not Rust parsers or
substitutes for behavioral tests. This completes slice31 only. The inherited
launch-validation WIP, P8-05 in_progress and G43 open remain unchanged; no
production cutover, independent/user approval or release is inferred.
