# Local definition targets — P8-05 slice30

## Finding and scope

The frozen schema gives all three project-config methods `takesTarget: true`.
Read/write SSH twins map to the parent methods under ADR-006. Their earlier
host handlers ignored target and performed local IO. Real authenticated UDS
tests reproduced this: read returned the local config, init returned a newly
created local config, write returned null, and malformed/conflicting targets
also succeeded. The local/auth/writer control passed (1 green/4 red).

This is a boundary correction, not a restart of slices26/27 or new remote IO.
No Tauri or core business code changes. Method coverage stays **73/641** with
**43** real dispatch call sites; no new method or SSH/WSL capability is claimed.

## Shared policy

Private `aeroric-host::local_targets::local_target_error` is the sole decoder for
five definition handlers: project-config read/init/write and run-config
read/write. It takes the already decoded original projectPath and the caller's
unsupported-target message. Run-config retains its earlier diagnostic text.

- No target: continue the old local behavior.
- Present target: decode protocol `project::ProjectTarget`, not the IDE UI
  model. Null, malformed shape/kind and a conflicting local path string return
  InvalidArgument.
- Matching Local: exact original string equality, without trimming or
  canonicalization; the required projectPath is still used by core.
- SSH/WSL: UnsupportedCapability, never fall through to local IO.

All required typed fields are decoded first. Target refusal happens before
core, so a refused initializer cannot create attachments and a refused writer
cannot overwrite or acquire authority. Omitted/matching targets retain the
existing checked writer lease and every legacy path/default/error/IO rule.
Authentication remains the dispatcher's earlier gate.

The helper has no filesystem, environment, registry, credentials, lease or
process access. It is not a universal target resolver: search's canonical-root
and registered-project policy stays distinct. Target-only calls, remote
configuration access, retries and protocol-size changes remain out of scope.

## Evidence

`/tmp/aeroric-config-target-red-vr_y_5sx/`:

- `host-red.log`: successful compilation,1 passed/4 failed,0 ignored,rc101.
- `host-green.log`: target5, original project-config read4/mutations7 and
  run-config8 all passed (24 total), zero ignored.
- `host-clippy.log`: scoped library and these four suites passed with warnings
  denied.
- `source-equivalence.log`: moved guard is token-identical after substituting
  the preserved run-config message; each project-config body is unchanged
  apart from its new pre-IO guard.

`local-target-invariants.mjs` pins the single decoder and five typed call sites;
it is not a Rust parser or replacement for behavior tests. Full current CLI/
cutover/adapter evidence is recorded in the master plan. The untouched
launch-validation WIP remains a separate full-target fmt/clippy failure.

### Final bounded checkpoint — 2026-10-09

`/tmp/aeroric-config-target-integration-yyxmybyt/` holds the final commands,
exits and copied per-case logs (including the failures, not only summaries):

- Current CLI rebuilt, then the full synthetic headless smoke passed.
- cutover:48 cases/247 tests; skeleton:24/188; no skips.
- Tauri library:1276 passed, zero ignored. Tauri fmt/clippy/typecheck passed.
- tauri-adapter:rc1,1337 tests executed, no skips. Its only two failed cases
  are core fmt and all-target core clippy on the inherited unfinished
  validate_agent_launch_services.rs. Its SHA256 remains
  a5c2a9447fb92592de48a3618afdf99b48033e8d32bb3ece44107f3f1c037782.
- Inventory/freeze,12 unchanged contracts,16 digest shards (max21842B),
  four ownership checks and runner36 passed. Coverage73/641,43 dispatches.
- Tracked diff and62 untracked files passed whitespace checks; Trellis19/15
  validated, all context files below32768B and each manifest below131072B.

Inline review compared the two pre-slice service snapshots, the new shared
guard, actual UDS tests and CLI assertions. No additional actionable issue
was found within that scope. This was not an independent review, a user
approval, remote execution verification or full P8-05 acceptance.

Slice31 reuses the same pure guard for debug-config read/write, bringing its
call sites to seven without changing the five earlier policies. See
`debug-config.md`; adding those parent handlers is not remote DAP support.
