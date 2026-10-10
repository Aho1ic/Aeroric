# Write/create leaf paths — P8-05 slice35

## Demonstrated boundary

The already-hosted `files.write_file` and `files.create_file` use core
`files::read_write`. Parent canonical containment alone did not authorize the
leaf: `fs::write` followed an out-of-root symlink, and `exists()` missed a
dangling create destination. This was demonstrated through real authenticated
UDS, with every outside referent confined to a sibling scratch directory.

This is a security correction to existing methods, not new RPC coverage or
a migration of Tauri's distinct legacy absolute-path file IO. The host
handlers, current-writer/root ordering and reply/error framing are unchanged.
Coverage remains75/641; G43 and P8-05 remain open.

## Policy and compatibility

Core `path_policy::resolve_file_write_destination` preserves
`resolve_parent_within`'s path/parent behavior. It inspects the leaf with
`symlink_metadata`, resolves an existing leaf canonically, and requires its
resolved path to remain inside the canonical project root. The actual write
uses that returned path. Only an ordinary NotFound permits a new direct file.
Inspection or resolution errors are not converted to “missing”.

An unresolved/dangling leaf is refused, including an in-root dangling link.
That is an explicit tightening of the former follow-and-create behavior;
it is not claimed as byte-for-byte preservation of unsafe behavior.
Valid existing in-root leaf/parent aliases remain supported and are retained
as links. Normal new files, content bytes including BOM/CRLF/Unicode, modes,
empty overwrites, separator normalization and size-before-path ordering stay.

`read_write::create_file` now uses `OpenOptions.write(true).create_new(true)`,
not an `exists()`/truncating-write pair. All existing directory entries,
including dangling links, retain the original `file already exists` refusal.
Other create errors retain their diagnostic prefix. Exclusive creation
protects the final entry; it is not descriptor-relative protection of mutable
parent components or a general filesystem transaction/TOCTOU guarantee.

Copy, directory-create, rename/delete, history, formatting, target routing,
Tauri/frontend code and protocol schemas are outside this change.

## Targeted evidence

`/tmp/aeroric-file-leaf-red-dwvdq2w1/`:

- Pre-slice source snapshots isolate the core change from inherited work.
- `host-red.log`: compiled **3 passed/4 failed/0 ignored/0 filtered, rc101**.
  Existing/dangling outside writes, an unresolved inside write and dangling
  create incorrectly returned success. Valid alias/wire/mode, parent-refusal
  and read-only pre-root controls passed.
- `core-green.log`: new `file_write_paths` **6/6**, old files **13/13** and
  project copy **4/4**. The >2MiB size guard is tested directly here, not
  falsely claimed to cross v1's smaller RPC frame limit.
- `host-green.log`: new `file_write_paths_services` **7/7**, old copy **6/6**,
  file authority **14/14**, search authority **9/9** and batch2 **4/4**.
  Refusals preserve outside bytes/modes and links; the same authenticated
  connection successfully writes an ordinary file after a refusal.
- Scoped core/host libraries and all named suites pass clippy `-D warnings`.

The existing file ownership ratchet and current-CLI smoke cover this policy.
Final relevant gates and artifact checks must be recorded before considering
this slice bounded complete. The unrelated launch-validation WIP is neither
deleted nor weakened to manufacture a full-green result.


## Final runtime integration (2026-10-09)

`/tmp/aeroric-file-leaf-integration-ut_ncu_h/`:

- Rebuilt CLI and full synthetic smoke passed, including leaf write/create
  refusals and the allowed in-root alias/exclusive-create wire.
- files15/125, cutover60/348 and skeleton24/188 passed, no skips.
- Full Tauri1284/1284 plus Tauri fmt/clippy/typecheck passed. Adapter remains
  rc1/1345 tests only at inherited launch WIP core fmt/clippy (unused Path and
  success_result); its original SHA256 is unchanged. Full raw logs retained.
- Inline exact-reversal review in source-equivalence.log restores both whole
  core files by removing only the new resolver and replacing write/create.
  Host services, old gate argv/floors, copy and all other algorithms are
  byte-identical to pre-slice source. This is not independent/user approval.
- Scratch ownership controls: original rc0; removing either copy guard or
  write-leaf resolution, or replacing create_new with create, each returns rc1.

Coverage75/641,45 dispatches/floor75 is unchanged. No remote capability,
release or real-data cutover is claimed. Non-search file target routing is
still a separate open investigation; G43/P8-05 remain open.

Final artifact checks passed: inventory/freeze-check, 12 unchanged generated
contracts, 16 digest shards/max21842B, context25+21 within byte caps, and
tracked plus79 untracked file whitespace. No old gate argv/floor was weakened.
