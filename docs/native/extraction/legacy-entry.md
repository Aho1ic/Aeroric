# Legacy entry validation and exclusive creation

P8-05 slice47, 2026-10-09. Bounded legacy ownership, not new host placement or
P8-05 acceptance. Current execution checkpoint is in `MIGRATION_PLAN.md`.

## Single owner and retained callsites

`files::legacy_entry` owns:

- `validate_entry_name`: empty, >255 **UTF-8 bytes**, dot/dot-dot, slash/
  backslash/NUL checks in their original order; exact old errors.
- The unchanged Windows-only reserved-stem table and Windows-only extra
  character/control/trailing-dot-space restrictions. Unix does not acquire
  Windows filename restrictions. The table/block and comments moved unchanged.
- `validate_new_path_within`: target absolute, raw UTF-8 basename validation,
  canonical parent **before** canonical root, containment, `(parent, basename)`.
  No leaf existence check, root-directory check, trimming, protected-directory
  policy, recursive initialization, registry lookup or settings IO.
- `create_file` and `create_directory`: final-entry IO at the caller-validated
  `&Path`, with the original exclusive OpenOptions/create_dir and error mapping.
  These helpers are not project authority.

Tauri imports the two validators. Each actual creation command retains its
blocking worker, guard, original `parent.join(&file_name)`, then delegates only
the final IO expression. Both collision paths still return
`A file or folder with that name already exists`; other errors remain raw.
No chmod or truncating/create-if-absent replacement was introduced.

Existing rename still trims `new_name` **at its caller** before name validation.
Existing copy validates and uses its untrimmed source basename. The remainder
of rename/copy/delete/protection/write/read business and all previous tests is
unchanged. Moving a shared validator is not permission to homogenize callers.

The entire pre-slice fs file is reconstructible by reversing the test-module
declaration/import, policy/table relocation and two final IO delegates. Policy
changes are public visibility only; IO changes remove two extra borrows.

## Legacy versus native and safety limits

Native relative-path validation treats backslash as a separator and has
different limits, roots and errors. Native create-file collision is
`file already exists`; directory uses a different exists branch/error. These
are not wire or implementation aliases for the old absolute-target commands.
Native/host/writer/target/remote/WSL/SFTP behavior stays unchanged.

Observed legacy quirks are retained, not advertised as stronger authority:

- Root may be relative; a canonical file can be a parent/root at validation
  time. The later actual create then returns the original raw IO error.
- A trailing slash on the requested target can be stripped by Rust Path's
  parent/basename decomposition; the old create-file command can create a
  regular file from that input. Create/copy names remain untrimmed.
- Existing leaves, including external/dangling/looping symlinks, are not
  resolved by new-path validation. Exclusive creation refuses them without
  replacing/following them. Canonical parent containment remains required.
- Creation does not inherit deletion's protected-first-segment restriction.
  A scratch `.git` directory can be created just as before.
- Exclusivity protects the final entry. No general descriptor-relative
  TOCTOU, ancestor replacement, transaction or permission-isolation guarantee.

Windows cfg code is byte-identical and there are conditional core assertions
for future Windows runs. **No Windows runtime/build pass is claimed here**;
these observations are from the current macOS environment.

## Before/after evidence

Evidence: `/tmp/aeroric-legacy-entry-qcqm1f7s/`.

Eight actual helper/command tests passed on the old business **before** moving
it: 8 passed,0 failed,0 ignored,1316 filtered. Production fs code was verified
byte-identical before that run except the test-module declaration. Successful
test source was frozen; all eight assertions remain byte-identical afterwards.
This is green-before/green-after compatibility, not a manufactured red result.

Each test re-execs only itself under env_clear and scratch cwd/HOME/data/empty
PATH, with a timeout, reaping and one-nonignored-case assertion. No real agent,
shell, Trash, UI or user data is involved. Whole-tree snapshots cover bytes,
modes, link targets, entries and mtime. Success cases permit only named
created/renamed/copied entries and the immediate containing directories'
mtime changes; refusals permit no changes.

Post-extraction checks:

- Core `legacy_entry`4, `files`13, `legacy_directory`3, `legacy_path`3,
  `legacy_read`3 and `file_write_paths`6: all pass.
- Real Tauri broad `fs::`:52 passed,0 failed,0 ignored,1272 filtered,
  including the unchanged eight and prior POSIX/remote/WSL cases.
- Scoped core and all-target Tauri Clippy:rc0.
- New source gate plus old directory/path/read gates pass. New gate has
  **5 positive / 60 negative** scratch-source controls across scope,
  platform/name/parent policy, exclusive IO/errors, worker/join/callers,
  native separation, frozen names/parameters and isolated actual tests.
- Inline whole-file inverse and **46 protected hashes** pass, including
  inherited launch WIP, old legacy/native code, host/CLI/wire/Swift/lockfiles.

Source controls complement real tests; their rustfmt top-level-function matcher
is not a full Rust parser or a security proof. Inline review is not independent
review or human acceptance.

## Integration evidence

Same evidence directory, current rebuilt CLI and the configured raw logs:

- Mocked FileExplorer action/UI consumers:2 files/132 tests pass. No frontend
  production changes or real Tauri/Trash operations.
- `cli`:5 checks/10 tests; `files`:28/212; `cutover`:83/502;
  `skeleton`:24/188. All rc0 with no skipped cases.
- `tauri-adapter`:1385 tests, including full Tauri **1324 passed,0 failed,
  0 ignored,0 filtered**. Tauri fmt/all-target Clippy/typecheck pass.
  Two inherited core fmt/Clippy gates remain red; their raw logs are
  byte-identical to46 and identify only `validate_agent_launch_services.rs`.
  Its protected hash is unchanged. This is not an all-adapter-green result.
- Full Tauri floor raised **1316→1324 only after** parsing that full run.
  The shared gate parser also verifies core4/command8 in both files/cutover.
  Reversing just the six new case entries, four floors and measured full-floor
  increase restores the exact pre-slice config. All35 module shapes and
  173 non-orphan test floors validate.
- 13 fs definition pins reanchored; the two new production create delegates
  account for Rust-reference counts0→1 each. Real extractor scratch controls
  prove old0/0, old+tests0/0, new1/1, new-minus-delegates0/0. No other command
  names/params/metadata/registration/reachability changes.
- Inventory/freeze,12 byte-identical contracts,16 digest shards (max21842B)
  pass. Coverage remains75/641 with48 dispatch sites/floor75.

P8-05 remains in_progress; G43 stays open. No new host service, production
cutover, Windows runtime verification, independent review or release claim.

Final closure: inverse/46 hashes/config-floor/four source gates and inventory
rechecked; context14/18 entries within file/aggregate budgets; tracked and
all116 untracked files whitespace-clean. Completed47 artifacts/manifests are
archived under the active task research directory; the main plan records the
next existing-leaf guard boundary without changing P8-05 acceptance.
