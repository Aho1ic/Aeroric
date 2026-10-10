# Agent-config service boundary — P8-05 slices39–43

## Existing surface and defect

The host already registered `settings.get_agent_config_file_path`,
`settings.read_agent_config_file` and `settings.write_agent_config_file`.
The private `load_typed_settings` used only by these three handlers discarded
any `AppSettings` deserialization error into defaults. A malformed field could
therefore redirect path selection or masquerade as an empty path/null result.
The write handler additionally performed settings/path/file IO without checking
the current CoreContext writer.

The runtime change is deliberately two blocks in `settings_services.rs`:

1. Keep the raw settings reader and absent-map defaults, but propagate a typed
   decode error as `invalid stored app settings: ...`, mapped by the existing
   callers to nonretryable IoError.
2. After validating both request strings, check `context.require_write()`
   before loading settings or resolving a config path. Reuse the established
   nonretryable IoError response; no cached writer flag.

Both queries remain read-only-capable. Configured-path trimming, exact agent
and raw content strings, missing-config null, unconfigured-path empty string,
unknown-agent/unconfigured-write errors, all existing IO and reply bodies
remain unchanged. The operations never repair or rewrite `settings.json`.

## Behavioral evidence

`/tmp/aeroric-agent-config-authority-red-mah757mq/`:

- The new actual authenticated UDS suite compiled against the old behavior:
  **6 passed/6 failed/0 ignored/0 filtered, rc101**.
- Three unauthorized-write cases changed existing bytes, created parents or
  wrote after replacing the real lease inode. All **24** malformed-typed-field
  × method pairs ran, returning defaults/null or a misleading path error.
  Same-connection corruption and writer-before-settings ordering also failed.
- After the two-block fix, **12/12** plus unchanged settings17 and project
  config4+7+5 suites pass: **45 tests**, zero ignored. Host library and all five
  named test targets pass scoped clippy with `-D warnings`.
- Positive controls retain read-only queries, raw content including NUL/Unicode,
  current direct-write modes, absent settings/blank paths, custom path trimming,
  invalid UTF-8/directory IO errors, malformed request precedence and recovery.

All red/green cases use `claude_gpt55` (whose unconfigured path is None) or a
fully configured scratch custom profile. Even the buggy default branch cannot
read a real HOME config. Snapshots cover settings and the separate target tree's
bytes, modes and entries. No credentials, agents, Keychain, SSH or Trash run.

## Integration evidence

`/tmp/aeroric-agent-config-authority-integration-61bvlpnq/`:

- Current CLI rebuilt; full headless smoke passes with nine typed-corruption
  refusals, three parameter refusals, raw/empty config wire, no-repair checks
  and recovery. Scratch settings bytes are restored in `finally`.
- `cutover` **63 gates/406 tests** and `skeleton` **24/188** pass. Only two
  new gate entries and the measured12 floor are added; prior argv/counts/floors
  and the original settings suite remain unchanged.
- Inline review (not independent/user approval) proved that reversing only
  the typed-decode and writer blocks restores the entire pre-slice source.
  Excluded core/path/schema/legacy/WIP SHA256 values match.
- The actual source ratchet on scratch layouts passes its positive control
  and rejects all13 negative controls, including removed/misordered/unchecked
  writer admission, decode fallback and mismatched registrations.
- Adapter **1345 tests**, including Tauri **1284**, pass; module exit1 is solely
  the inherited launch-validation file's fmt and two clippy errors. Its SHA256
  remains `a5c2a9447fb92592de48a3618afdf99b48033e8d32bb3ece44107f3f1c037782`.
- Inventory/freeze-check, contracts12, digest16/max21842B, context28/24 within
  per-file/aggregate bounds and tracked/all86 untracked whitespace checks pass.
  See `final-artifacts.json`; slice39 is bounded complete.

## Remaining migration work at the slice39 checkpoint

This is prerequisite safety work, **not** completed Tauri placement or shared
agent-config extraction. At that checkpoint the two path-selection copies still
existed (consolidated in slice40 below); defaults still call process-global
home/data helpers, and the host calls
`settings::atomic_write`, whose actual body creates parents then `fs::write`s.
The Tauri command instead uses its storage temp/sync/rename writer. Do not
claim equivalence from those helper names or forward commands before designing
the shared path/IO boundary. Do not globally replace that helper for unrelated
settings/credential/wrapper callers during this slice.

Coverage remains **75/641,45 dispatches/floor75**. P8-05 remains in progress;
G43 remains the independent open files/search gap, not an agent-config owner.
Full launch validation and its inherited WIP remain untouched.

## Slice40: one pure path selector

`settings::agent_config` now owns the selector, with an `AgentConfigPaths`
borrowed view of four configured strings and custom id/path/family/codex_like.
No credentials, full-settings serialization or new wire DTO is involved.
Core AppSettings has a field projection; Tauri's temporary schema has a small
field-only projection. Both old resolver wrappers call `resolve_path` and pass
their existing default/custom-home helpers as lazy callbacks.

Exact IDs/first custom match, builtin precedence, trim/blank and original
path spelling remain. OMP/custom OMP always use builtin managed config rather
than an executable. Custom DSH uses its home plus settings.yaml; other families
keep their configured path. The shared AgentFamily parser retains trim and
codex_like fallback. Failed selected home callbacks still become None, not a
second path/port. No tilde expansion, canonicalization or ID sanitization is
introduced in the pure selector.

This is a placement refactor with a prior green compatibility baseline, not
a claimed behavior fix: no missing-new-API compiler error is called a red test.
The selector itself acquires no environment or IO ownership. Legacy synchronous
reasoning callers, host typed/current-writer admission, IO and IPC placement
remain unchanged. Default-root injection and shared file read/atomic write
are still pending.

Evidence `/tmp/aeroric-agent-config-paths-qgy_0nf1/`:

- New core **9** tests cover counted fake callbacks and the compatibility rules;
  existing host12+17 **29** tests pass unchanged.
- Tauri `config::` filter actually matches config **25** and run_config **23**:
  **48 passed/0 ignored**, including two new projection/adapter controls.
  Both core and host scoped clippy pass. New tests never call real HOME helpers.

Integration `/tmp/aeroric-agent-config-paths-integration-_8oyww4y/`:

- Current rebuilt CLI/full smoke, `cutover` **65 gates/417 tests** and
  `skeleton` **24/188** pass. Two measured9/2 gate entries are added; existing
  test floors remain intact. Three moved config command definition pins were
  reanchored without altering frozen names/parameters/results.
- Inline review (not independent/user approval): reverse the host wrapper to
  recover its full pre-slice file; reverse the Tauri projection/wrapper and
  remove only its two new tests to recover that entire original file exactly.
  Prior commands, IO, reasoning callers, tests, default helpers/schema and
  inherited WIP match their pre-slice hashes.
- The actual combined source ratchet passes its positive control and rejects
  all24 negative controls covering39 admission plus duplicate/wrong selector,
  eager/global defaults, lookup/family and projection regressions.
- Adapter **1347 tests/Tauri1286** pass, with module exit1 solely for the same
  inherited launch WIP fmt/clippy failures. No new diagnostic or WIP change.
- Inventory/freeze-check, contracts12, digest16/max21842B, source ownership,
  context28/24 (117217/109509B) and tracked/all88 untracked whitespace checks
  pass. Each context file remains below32768B and each manifest below131072B.
  `final-artifacts.json` records the final evidence.

Slice40 is bounded complete. Coverage stays **75/641,45 dispatches/floor75**;
P8-05/G43 remain open. The next boundary is explicit-path file IO and real
legacy atomic-write parity, not default-root relocation or IPC placement.

## Slice41: explicit-path IO and legacy leaf replacement

`settings::agent_config_files` owns the exists/None/raw UTF-8 reader, the
CoreContext-checked writer and an explicitly named legacy authority seam.
Both writers create parents then reuse `storage::atomic::write_legacy_atomic`;
they do not introduce another atomic implementation. Tauri keeps its loader,
selector, synchronous command signatures and original authority, delegating
only IO. The host retains string decoding, current-writer-before-settings
admission and strict typed settings, then rechecks authority in the shared writer.

The host's former `settings::atomic_write` was a direct write. Unlike Tauri's
temp/write/sync/rename, it changed linked targets and retained existing target
modes. Hosted writes now match the shipping legacy leaf-replacement policy:
file/directory/dangling symlinks are replaced, hardlink aliases are not modified,
and real directories still fail with no leaked temporary config copy. Contents
remain raw; new-file permissions follow the existing inherited-mode policy.
This is not private-mode hardening, root confinement, a settings transaction,
or a guarantee against mid-operation lease revocation or path races.

The generic settings helper and its unrelated consumers, the pure selector,
default-home helpers, all loader behavior, reasoning readers and IPC placement
are unchanged. Default-root injection and three Tauri dispatches remain pending.

Behavioral evidence `/tmp/aeroric-agent-config-files-red-rik4kn3n/`:

- Against the original host, both suites compiled and ran to completion:
  authority **11 passed/1 failed**, files **4 passed/4 failed**; total
  **15 passed/5 failed/0 ignored/0 filtered, rc101**.
- The one old positive write assertion deliberately compares the actual legacy
  atomic mode control, rather than asserting preservation of the old direct-write
  target mode (0640 versus 0644 on this machine). All prior refusal assertions
  remain. The four new red cases exposed file/hardlink target writes, dangling
  target creation and directory-symlink rejection.
- After delegation, new core7 plus selector9 **16**, host files8 plus authority12
  plus settings17 **37**, and Tauri `config::` **48** tests pass, zero ignored.
  Scoped core/host clippy pass. Fixtures are scratch-only, including red paths.
- The source gate initially over-read the following `pub(crate)` reasoning
  function because its boundary regex only recognized `pub`. Only that boundary
  was corrected; the IO assertions were not removed. The actual gate and smoke
  syntax check then passed; scratch negative controls cover this instrument fix.

Integration `/tmp/aeroric-agent-config-files-integration-njwn653g/`:

- Rebuilt current CLI/full smoke passes, including all four link-leaf controls.
  `cutover` **67 gates/432 tests** and `skeleton` **24/188** pass.
- Adapter **1347 tests/Tauri1286** pass; module exit1 still comes only from the
  inherited launch WIP's fmt and unused Path/success_result diagnostics. Its
  SHA256 remains the same as above. No full-suite green claim.
- Inline review reverses just the two IO delegates/import to recover the full
  Tauri source, and just the host IO delegates to recover its full source.
  Reversing the single named positive-mode assertion recovers the entire prior
  authority suite. Other tests, selector, helpers and launch WIP hashes match.
- Only measured7/8 gate entries/floors and the existing ownership note change.
  Seven moved definition pins are reanchored; frozen wire/registration is intact.
- The actual source gate passes two positive controls (including an unrelated
  scoped async reader) and rejects28 mutations with assertion failures.
  An initial scratch mutation-anchor setup error is preserved separately;
  it ran no controls and is not counted as product red evidence.
- Inventory/freeze-check, contracts12, digest16/max21842B, context28/24 within
  per-file/aggregate bounds, tracked and all91 untracked whitespace checks pass.
  `final-artifacts.json` records the final evidence.

Slice41 is bounded complete; default-root ownership and the three Tauri IPC
placements are next. Coverage remains **75/641,45 dispatches/floor75**;
P8-05/G43 remain open, with no production cutover or commit/release authorization.

## Slice42: injected defaults without expanding unrelated history

`settings::agent_config_roots` composes the existing selector with injected
external and managed roots. Its builtin filenames and custom-home
normalization/containment are pure and shared with legacy wrappers. Preserve
the actual DSH directory **local_dsh** (the sanitizer reserves builtin IDs),
not the misleading old comment's `dsh`; OMP uses literal **omp** even for a
custom OMP profile. Configured-path, exact-ID/first-match, trim, error-to-None,
typed admission, current writer and raw/atomic IO semantics remain.

An initial plan to replace CLI's common `user_home="/"` with HOME was rejected
before runtime changes. Tracing `MetricsPaths::from_context` to
`metrics::analytics::usage_roots_for_home` exposed external `.omp/agent/sessions`
discovery; rebinding all services would widen unrelated roots (ADR-001 §5).
Instead `HostWiring.agent_config_external_home` is a trusted composition-only
port. Settings registration captures immutable config AppPaths from that home
or the original context home, always using **context.data_dir** for managed
data. The three handlers retain the original Arc<CoreContext> and blocking
worker. No request field controls these roots. Common CLI context, metrics and
session discovery stay unchanged; their larger boundary remains separate work.

CLI serve captures HOME/USERPROFILE once for that port. Missing, empty or relative
home returns exit3 before startup, lock or data-directory creation.
`--data-dir` remains mandatory, never inferred from HOME; doctor/client-only
commands remain data-only. Legacy helpers still acquire their original roots
and preserve eager builtin HOME errors and lazy custom-root error order.
`agent_config::dsh_config_path` centralizes settings.yaml while retaining both
existing `dsh_settings_path` interfaces. Tauri config IPC/loaders are unchanged.

Behavioral evidence `/tmp/aeroric-agent-config-roots-hy4rqxc5/`:

- Compiled red: host **2 passed/5 failed**, CLI **1 passed/2 failed**, zero ignored.
  Global decoys were selected/written instead of injected targets; two hosts
  shared defaults, and four invalid HOME inputs started writers. Tests boundedly
  reaped those processes. No real HOME/config was accessed.
- Final targeted core **24**, host **45**, CLI **3**, Tauri config filter **48**
  pass, zero ignored; scoped core/host/CLI clippy pass. Host includes one eighth
  trusted-port-isolation test added after red, not claimed as part of red.
- The first helper change orphaned a Tauri DSH helper; retaining its existing
  config-path callback and delegating the filename join removed the warning
  without deleting/allowing it. Final targeted compilation is warning-free.

Integration `/tmp/aeroric-agent-config-roots-integration-fou5vnnz/`:

- Current rebuilt CLI/full smoke and `cli` **5 gates/10 tests**, `cutover`
  **70/451**, `skeleton` **24/188** pass. Three measured8/8/3 gates are added;
  prior gates/floors are unchanged. New tests were rustfmt-formatted before
  these final runs. No frozen method/wire/registration or command-pin change.
- Initial full smoke failed the old preference no-agent-home-creation check:
  the new root stage had left its own synthetic directories behind. It now
  refuses preexisting fixture roots and removes only its own directories in
  finally. The existing preference assertion and runtime were not weakened;
  the complete rebuilt-CLI smoke passes, including fake task lifecycle/replay.
- Adapter **1347 tests/Tauri1286** pass; module exit1 is solely inherited launch
  WIP fmt/clippy (unused Path/success_result), verified from complete raw logs
  and the unchanged WIP hash. No full-green, production or release claim.
- Inline source/delta review, not independent approval: old CLI tests recover
  exactly after removing two HOME fixture lines; the selector recovers after
  only Path import/helper/DSH delegation inverses; all text outside six legacy
  helper bodies matches its snapshot. Protected IO/config IPC, paths/atomic,
  authority/files/settings tests, metrics and WIP snapshots are unchanged.
- Actual source-ratchet **3 positive/33 negative controls** pass on scratch
  layouts, including same-context/managed-root/CLI-port, no common-history-root
  expansion, writer/admission/selector/IO and DSH callee/argument guards. Its
  regex was narrowed to accept rustfmt whitespace/trailing commas, not alternate
  calls. A missing-symbol setup error ran no controls and is preserved separately.
- Final inventory/freeze-check, contracts12, digest16/max21842B, context limits
  and tracked/untracked whitespace evidence live in `final-artifacts.json`.

Slice42 is bounded complete. Coverage remains **75/641,45 dispatches/floor75**;
P8-05/G43 stay open. Continue the three actual Tauri agent-config IPC placements,
keeping the legacy settings loader deferred and External strictly no-fallback.

## Slice43: the three actual Tauri IPCs

Only the three commands move from `config.rs` into existing `config/commands.rs`;
`lib.rs` registers their new Rust paths. Their names, JSON agent/content fields
and String/Option<String>/unit replies stay frozen. The new `AppHandle<R>` is
framework injection (`!` in annotated Rust params), not a wire parameter.
The **complete** old operation runs in a blocking worker created only inside
the unpolled Embedded future. Loader fingerprint/chmod/repair, legacy root
callbacks, selector and read/legacy atomic write are preserved. Parent
projection/selector and reasoning reads remain; no duplicate sync IPC is kept.

External strictly decodes host replies and never executes local work, including
on bad result types, host error/missing method, disconnect, no session or no
installed gateway. Host/core business, roots, schemas and loaders are unchanged.
This is three real command placements, not all settings/launch ownership or
authorization to enable External by default.

Evidence `/tmp/aeroric-agent-config-ipc-fu493ht4/`:

- Actual command red compiles: **2 passed/8 failed/0 ignored/1286 filtered**,
  rc101. Both Embedded controls pass; path/read return local rather than host
  data, write never reaches host, and refusal/transport/placement cases continue
  locally. This is not a route-helper test or a compilation failure.
- New tests re-exec their own named case under env_clear with scratch roots,
  fake shell/which and no inherited credentials. The loader really probes the
  login shell even for absolute paths; a platform scratch slot alone would
  still read real rc. Probe markers and byte/mode/mtime/entry snapshots detect
  any External loader activity. Child execution must report exactly one
  successful, nonignored case, not silent zero-test success.
- The same ten cases pass after the call helper switches to actual async IPCs.
  All test assertions/helper matrices are unchanged; only the parent child-count
  guard is strengthened. Final `config::` filter **58/58**, zero ignored, and
  Tauri all-target Clippy pass; mocked frontend consumers **2 files/45 tests**.
- First Clippy reported30 helper unwrap/expect/panic diagnostics under
  cfg(all(test, unix)). Splitting to equivalent cfg(test)+cfg(unix) lets the
  existing test policy recognize the module. No lint config/allow or assertion
  suppression was introduced; this sole change made Clippy green.
- Inline exact inverses recover the complete parent, prior command module and
  lib.rs. Each worker body matches its old business body modulo indentation/
  module qualification, with string literals compared exactly. Protected
  loader/core/host/roots/IO/schema/prior suites/smoke/launch-WIP snapshots match.
  Annotated Rust params gain only the three injected handles; contracts12
  remain current. This is not independent review/user acceptance.
- Actual source guard **4 positive/65 negative controls** pass on scratch:
  admission/selector/IO/roots retained, generic function boundaries, wrong
  registration/method/params, eager/duplicate loader, missing worker, result
  coercion, wrong legacy authority and unsafe/no-case tests are rejected.
- RPC coverage stays **75/641**; actual Tauri dispatches rise **45→48**, all
  targeting real host methods. No RPC floor inflation; measured10 command gate
  and source/registration pins are added/updated.
- The first cutover ran all461 tests successfully but failed the old
  project-config source gate: it scanned this entire IPC module, so the new
  neighbouring legacy loader triggered the reader's no-loader rule. The fix
  scopes only its `adapter` variable to the unique real reader; every previous
  assertion and other scan is byte-unchanged. Actual scratch **4 positive/16
  negative** controls prove adjacency is accepted while missing/duplicate
  readers and internal loader/IO/coercion/eager reads still fail. The original
  failing gate log is retained; this is not suppression of a runtime failure.

## Slice43 integration checkpoint (2026-10-09)

The same evidence root retains original failures and separate `*-final` logs:

- Current CLI explicitly rebuilt; full scratch smoke and cli **5/10** pass.
- After the narrow reader-gate repair, cutover **71 gates/461 tests** and
  skeleton **24/188** pass, no skips. Adapter **1357 tests**, including Tauri
  **1296/1296,0 ignored**, pass. Only inherited launch-validation WIP core fmt
  and the unused Path/success_result clippy diagnostics fail the adapter gate.
  Its SHA256 remains a5c2a9447fb92592de48a3618afdf99b48033e8d32bb3ece44107f3f1c037782.
- After that measured full-suite result, the Tauri floor rises1284→1296. No
  other old floor, test command or assertion changes. Coverage **75/641**, all
  **48 dispatches** real, floor75. This does not claim all legacy file/native
  aliases have equivalent wire behavior or that production cutover is ready.
- Inline scoped inverses and protected-file hashes still pass after the source
  gate fix; this is not independent review or user acceptance. Final artifact,
  context-budget and whitespace checks are recorded in `final-artifacts.json`.

Slice43 is bounded complete. P8-05/G43 remain open. Default Embedded and all
production/release boundaries remain unchanged; the next uncovered boundary
is planned separately.
