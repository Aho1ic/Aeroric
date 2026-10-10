# Legacy write composition — P8-05 slice54

## Boundary

`core/crates/aeroric-core/src/files/legacy_write.rs` owns exactly the legacy
desktop write composition that used to live inline in the registered
`fs::write_file_content` command:

```text
validate_path_within(file_path, project_path, false)   // strict: canonical in-root file required
→ to_string_lossy() as the validated text
→ legacy_history::record_snapshot_before_write(project_path /* original string */, text, content)
→ std::fs::write(validated_path, content)
```

The strict guard requires an existing target that fully canonicalizes inside
the root, so a missing file refuses as `Path is outside the allowed
directory` before any history or write; an existing in-root directory passes
the guard and is then refused by the history validator as
`File path is not a file`. There is no size cap, no destination resolution,
no atomic rename and no create-new — those belong to other policies.

`src-tauri/src/fs.rs` keeps the registered command name/signature and its
`spawn_blocking` worker, delegating the three steps to the core owner.
`src-tauri/src/remote/files_rpc.rs` (`RPC project.writeFile`, mobile) keeps
its outer existing-regular-file/canonical-root checks, response-size bound
and audit logging, and still calls `crate::fs::write_file_content` unchanged.
The snapshot helper, root guard and clock placement are byte-identical to
slice52's owners. `core_service` stays `files`; the wire name/params do not
change.

## Distinct policies (not aliases)

| Boundary | Legacy desktop writer | Native/host writer |
| --- | --- | --- |
| Target form | absolute path, strict canonical-in-root guard | projectId + relative path |
| Size cap | none | 2 MiB (`validate_content_size`) |
| Leaf resolution | full canonicalize; missing files refused | `resolve_file_write_destination`; new direct files allowed |
| History | hex-layout snapshot of prior content before `fs::write` | caller hook; raw layout for host force snapshot |
| Authority | command registration only | CoreContext write lease; mobile RPC audit log |

`create_file` keeps `create_new(true)`; WSL/SSH write commands, notebook/host
snapshots, formatter and login-shell code are untouched. Mobile RPC must not
be rerouted into `files.write_file` or the native writer.

## Verification and limits

Fresh snapshots: `/tmp/aeroric-legacy-write-qng5xy3d/` (97 source snapshots
and 216 inherited dirty hashes predate implementation).

- New five actual cases ran against the old composition before the move:
  **5 passed, 0 failed/ignored, 1353 filtered**; `fs::` **70/0** (65 old + 5
  new), `local_history::` **12/0**. The fixture source is frozen as
  `legacy_write_tests.baseline.rs`; after the move the same five replay green
  plus direct core `legacy_write` **5/5** with file_history 7 / files 13 /
  legacy_history 8 / legacy_rename 4.
- Exact inverse: `src-tauri/src/fs.rs` differs only by the test-module
  declaration and the command-body swap; `files/mod.rs` +1 module line; the
  map re-anchors `write_file_content` declaration 119→123, registration
  499→526 and `rust_internal_refs` 1→2 (the new delegate line). All other 94
  snapshot sources byte-identical; `inventory --check` consistent 660/660.
- Ownership gate `scripts/native/legacy-write-invariants.mjs` compares the
  exact core body, the thin desktop command, the unchanged mobile call/outer
  checks, the unchanged native writer and the frozen actual5/direct5.
  Controls: **5 positive / 11 negative** scratch source probes (owner,
  order, snapshot argument, wire, size cap, atomic rename, native alias,
  deleted test, removed registration, mobile reroute, broken env isolation).
  These are source probes, not executable Rust mutants.
- `legacy-write-core-rust` (min 5), `legacy-write-command-rust` (min 5) and
  `legacy-write-ownership` are registered in files/cutover with matching
  runner floors (35 modules / 197 floors). Actual cases use env-cleared
  exact children with owned HOME/PATH/cwd, bounded kill/reap and
  one-executed-case assertions; every symlink referent is scratch-contained.
  No real formatter, Trash, clipboard, HOME or settings were touched; no
  External capability is inferred.

P8-05 remains in progress/G43 open; the unrelated launch-validation WIP and
its core fmt/Clippy failures were neither edited nor suppressed.
