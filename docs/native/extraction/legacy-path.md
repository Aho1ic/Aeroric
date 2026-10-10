# Legacy absolute-target guard — P8-05 slice45

## Placement, not a policy change

`core::files::legacy_path::validate_path_within` is the single owner of the old
Tauri helper. Tauri imports its unchanged name. All six production policy
expressions and their worker ordering are byte-identical:

| Caller | Policy | Guard location |
| --- | --- | --- |
| Reveal | true | inside worker; platform effect not tested here |
| Directory listing | false | inside worker |
| Text reader | variable: local true / strict wrapper false | before worker |
| Image preview | true | before worker |
| File write | false | inside worker, before history |
| Copy destination | false | inside worker |

The helper is not a project ACL, RPC endpoint, writer lease or replacement for
native `path_policy`. No native/host/remote capability or External routing is
added. Other new-entry/rename/delete guard bodies remain where they were.

## Measured legacy semantics

The new seven-case compatibility suite first passed against the **old private
helper and real callers**, before moving code:

- Only the target must be absolute. Its failure precedes root resolution.
  The original root string is canonicalized without trimming, an absolute
  check or directory-type check. Relative and file roots are accepted.
- A fully canonical target inside the canonical root succeeds in both modes.
  In-root dot segments and an outside spelling resolving back inside succeed.
- Strict mode otherwise returns the existing generic outside-directory error.
- Loose read mode canonicalizes the parent and leaves the final UTF-8 name
  unresolved. It accepts absent/dangling/looping/NUL leaves; existence or
  symlink status is not a precondition. Intermediate-parent escape refuses.
  All original error strings and their precedence remain unchanged.
- Actual local text/image reads accept the existing scratch external-link
  cases. Strict text, directory, write and copy refuse with no fixture changes.
  No OS reveal, real remote, production file, HOME loader, shell/git/agent,
  history initialization or Trash operation was invoked.

This corrects the old comment claiming all lexical `../` paths are rejected.
Only comments and public visibility differ in the moved helper; its code is
otherwise exact. This remains point-in-time validation, not descriptor-relative
IO, general symlink-race isolation or permission for an RPC caller to choose
the loose policy. A returned NUL/missing leaf is not a successful file read.

## Evidence and limits

`/tmp/aeroric-legacy-path-3ffxasdn/`:

- `command-before.log`: **7/7,0 ignored,1302 filtered**, first run passed.
  `legacy_path_tests.baseline.rs` freezes the successful assertions.
- Every new outer case re-execs only itself with env_clear, scratch cwd/HOME/
  data and an empty bin path. It bounds/reaps the child and asserts exactly
  one nonignored pass. Byte/mode/mtime/link/directory snapshots stay unchanged.
- `command-after.log`: the **same source bytes** pass; broad `fs::` filter
  **37/37** includes all30 prior matching cases.
- `core-targeted.log`: new core **3/3**, existing files **13/13** and legacy
  directory **3/3** pass. Core and all-target Tauri Clippy pass.
- `delta-review.log`: reversing the extraction reconstructs complete previous
  fs.rs, including all old tests and other guards. Core business is identical
  excluding comments/visibility; protected native/host/registry/CLI/wire/
  consumer/launch hashes are unchanged.
- `ratchet-results.json`: **5 positive/45 negative** controls. Ownership,
  root/leaf/error/order changes, all six call policies, both read wrappers,
  worker placement and test isolation are checked. Unrelated adjacent generic
  helpers and comments are accepted; no whole-module policy conflation.
- Only13 command source-location pins change. All reachability metadata stays
  unchanged. An initial audit incorrectly expected new test calls to raise
  reference counts; the actual extractor's scratch control disproved that
  assumption. It also corrects slice44's earlier attribution: that +1 was its
  production delegate, not its excluded test file. No extractor or frozen
  field was changed to accommodate the mistaken audit.

## Integration

Current CLI rebuild and its full headless smoke pass. Module results:
`cli` **5 gates/10 tests**, `files` **22/190**, `cutover` **77/480** and
`skeleton` **24/188** all pass. `tauri-adapter` executes **1370 tests**, including
full Tauri **1309 passed,0 failed,0 ignored,0 filtered**; Tauri fmt/Clippy,
TypeScript and adapter contracts pass.

The adapter module as a whole remains red in exactly two inherited gates:
core fmt and all-target core Clippy. Both raw logs are identical to the prior
slice and point only to the unfinished `validate_agent_launch_services.rs`
(formatting, unused `Path` and `success_result`). Its SHA256 stays
`a5c2a9447fb92592de48a3618afdf99b48033e8d32bb3ece44107f3f1c037782`.
No test, formatting exemption or ignore was added to conceal these failures.
Only after the full-suite run does the Tauri floor move1302→1309.

Review is inline, not independent or human acceptance. Runtime coverage stays
75/641 with48 dispatches/floor75. The shared parser confirms1309; reversing
only new cases/floors recovers all prior gate configuration. Final inventory/
freeze,12 contracts,16 digest shards (largest21842B), context14/15 (including
auto artifacts <128KiB, each file <32KiB), source/inverse and tracked/all106
untracked whitespace checks pass. Evidence: `floor-final.log`, `final-*`.
Slice45 is bounded complete, not a green full adapter gate. P8-05/G43 stay open;
full migration, legacy file wire/frame boundaries, cutover and release remain.
