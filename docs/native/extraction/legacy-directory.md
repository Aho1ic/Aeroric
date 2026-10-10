# Legacy directory-listing extraction — P8-05 slice44

## Distinct contracts, not aliases

The old Tauri `fs::read_dir_entries` still owned the complete directory-listing
business despite a native implementation already existing in core:

| Property | Legacy `FsEntry` | Native `FileEntry` |
| --- | --- | --- |
| Wire | seven snake_case keys, including extension | six camelCase keys, no extension |
| Entry path | absolute, from the validated canonical directory | project-relative |
| Filtering | case-sensitive ignored names, directories only | no ignored-name filter |
| Sorting | directories first, case-insensitive names | directories first, case-sensitive names |
| Directory kind | `Path::is_dir` follows the entry link | existing DirEntry metadata policy |
| Git ignore | best-effort `git check-ignore --stdin` | false, no git subprocess |

The native model's old “wire identical to Tauri” comment was false. Only that
comment is corrected; native model/implementation, host handlers, aliases,
authority/target policy and Swift consumers are byte-unchanged. Name coverage
in `host-coverage` does **not** certify these wires are interchangeable.

## Placement and preserved behavior

`core::files::legacy_directory` owns the legacy model, ignored-name list and
synchronous listing algorithm. Tauri re-exports the model and delegates from
its **existing** blocking worker, after its unchanged
`validate_path_within(path, project_path, false)` guard.

The target must be absolute and resolve inside the canonical root. The old
guard accepts a relative project root; do not silently require that root to be
absolute during extraction. The core helper accepts an already validated
directory plus the original project string for git cwd; it is not an RPC or
a source of root authorization.

Preserved exactly: lossy path/name conversion, lowercased extensions,
DirEntry metadata/time conversion, followed directory kind and unfollowed link
flag, per-entry `flatten` behavior, sort/filter rules, original git argv/cwd/
stdio, newline input and stdout-based ignore flags. Empty listings never
spawn; spawn/wait failures leave ignore flags false. Nonzero git exit status
does not discard stdout. `configure_background_command` now names its same
core owner directly, rather than through Tauri's existing re-export.

The old metadata comment incorrectly said it follows links. The original
command's dangling-link fixture returns a modification time despite a missing
target; the moved comment now describes the actual operations. No metadata
operation changed. No timeout, stronger TOCTOU protection, frame-size policy,
read-file/image behavior or new External routing is introduced by this slice.

## Compatibility and targeted evidence

`/tmp/aeroric-legacy-directory-ibnn2dq5/`:

- The six new actual-command fixtures first passed **6/6,0 ignored,1296
  filtered** against the old business body (`command-before.log`). This is a
  behavior-preserving extraction, not a fictitious behavioral-red fix.
- Two earlier fixture-setup failures were independently reproduced and retained:
  `.git/.Git` share an inode on this case-insensitive filesystem; APFS refuses a
  non-UTF8 filename with errno92. Both occurred before invoking the command.
  Distinct names and a Unicode fixture preserve the intended assertions; no
  runtime non-UTF8 filename coverage is claimed on this filesystem.
- Every new outer test re-execs only its exact name under env_clear with
  scratch roots and fake git. Parent bounds/reaps its child and requires one
  passed, nonignored case. The fake records argv/cwd/stdin outside the project
  fixture; no real git configuration, rc, agents, credentials or remote IO.
- After extraction, the **same test source bytes** pass; the broader `fs::`
  filter passes **30/30**, including all24 prior matching file/posix/remote/WSL
  cases. Byte/mode/mtime/entry snapshots, exact JSON, sorted paths, Unicode/
  spaces, directory-only filtering, links, no-process refusals/empty results,
  missing git and nonzero git output are covered.
- Direct new core tests **3/3** preserve both distinct models and empty/error
  no-write boundaries. Existing core files **13/13**, scoped core Clippy and
  all-target Tauri Clippy pass. Mocked UI consumers: **3 files/137 tests**.
- Inline exact inverse recovers the complete old fs.rs. The new business body
  matches the old closure after two borrow adaptations, equivalent subprocess
  qualification and the metadata-comment correction. All17 ignore literals,
  DTO semantics, prior tests and protected files remain unchanged.
- Actual source ratchet: **4 positive/34 negative** scratch controls pass.
  It scopes assertions to the real function, accepts adjacent generic helpers,
  and rejects wire/filter/sort/link/time/git/cwd/worker/guard/test-safety changes.
  This review is inline, not independent review or user acceptance.

RPC coverage remains **75/641**, **48 dispatches**, floor75. The13 fs command
source-location pins are reanchored; reachability's Rust-reference count rises
4→5 solely for the new production delegation line in `fs.rs`. The first
post-freeze inverse audit caught this derived field; its allowance is exact,
not a blanket metadata exclusion. Method names/params/registrations do not change.

Attribution correction during slice45: this note originally credited the new
test call. The real inventory extractor excludes `*_tests.rs` files. A scratch
control using that extractor observes3→3 when adding the two test files,3→4
with the production delegate, then4→3 when removing only that delegate name.
The measured original4→5 count was correct; its earlier causal description was
not. Evidence: `/tmp/aeroric-legacy-path-3ffxasdn/reachability-control.log`.

## Integration evidence

The current CLI was rebuilt before checking its real headless smoke. Under
isolated data roots, `cli` passes **5 gates/10 tests**, `files` **19/180**,
`cutover` **74/470** and `skeleton` **24/188**. Raw module outputs and machine
summaries are retained in the same evidence directory.

`tauri-adapter` executes **1363 tests**, including the full Tauri library
**1302 passed,0 failed,0 ignored,0 filtered**. Its Tauri fmt/Clippy, TypeScript,
contracts and adapter checks pass. Two inherited gates remain red:
core fmt and all-target core Clippy, solely in the untouched unfinished
`validate_agent_launch_services.rs` (formatting, unused `Path`, unused
`success_result`). Its SHA256 remains
`a5c2a9447fb92592de48a3618afdf99b48033e8d32bb3ece44107f3f1c037782`.
The module is **not** reported green. No assertion/ignore/formatting bypass was
used. After observing the full Tauri result, its floor moves1296→1302.
The shared `countCases` parser confirms1302 from the retained raw output;
inverting the two new case groups and this floor update recovers every other
pre-slice44 gate, floor and command exactly (`floor-final.log`).

Final inventory/freeze,12 contracts,16 digest shards (largest21842B), source,
coverage and context checks pass. Both context manifests (29/25 entries) and
their auto-loaded artifacts stay under128KiB total and32KiB per file. Tracked
and all101 untracked files pass whitespace checks. Evidence is in `final-*`;
the slice is bounded complete, not a passing full adapter gate or P8-05
acceptance.

P8-05/G43 stay open; no default-placement, production-data or release change.
