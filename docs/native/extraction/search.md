# Search — P8-05 slice 14 / G43

Status: **local slice verified; G43 still open**, 2026-10-08. This is implementation/verification evidence,
not a replacement for the cutover grill-me review or a claim that G43 is closed.

## One business implementation

| Legacy source | Core owner | Desktop remainder |
| --- | --- | --- |
| `src-tauri/src/search.rs` | `files/search.rs` | eight command adapters + `LegacySearchTransport` |
| `src-tauri/src/fs.rs` filename list/search | `files/project_files.rs` | two command adapters; unrelated fs unchanged |
| old core P6 parser/offset-only apply | replaced by the original root-aware engine | no second parser |
| host `services.rs` native replacement scanner | `files::search::replace_text_preview_for_file` / `apply_text_replacements_for_root` | `search_services.rs` wire adapters only |

The 18 original search tests move with the code. Additional core tests cover
actual rg records, fallback, UTF-8 offsets/captures, truncation, symlink loops,
overlap/stale rejection and injected remote transport. Filename tests use isolated
Git indexes and scratch HOME/config, no commits or remote repositories.

Deliberate safety fixes while consolidating:

- `rg --no-config` prevents ambient rg configuration from injecting `--follow` or
  changing its output protocol. Project ignore/glob behavior otherwise remains.
- Fallback, structured and preview walks skip symlinks, matching rg's no-follow
  behavior and avoiding root escape/cycles.
- Parsed rg-relative paths reject traversal/absolute names and invalid UTF-8 offsets.
- Local apply keeps original text/range/boundary guards, rejects overlapping
  intervals and serializes concurrent callers of this engine. This is not an OS
  compare-and-swap against arbitrary external editors; filesystem TOCTOU remains
  the existing path-policy boundary.

## Protocol adapters

Canonical legacy methods already belong to v1:
`files.search_text`, `files.search_structured`, `files.replace_text_preview`,
`files.apply_text_replacements`, `files.list_project_files`,
`files.search_project_files`.

Legacy payloads and return DTOs stay intact (`projectPath`, absolute match paths,
line/column/lineText/matchText; preview replacementText and byte ranges).
The host accepts a `projectPath` only as a selector into host-owned project
records, recorded task worktrees or the **same** registered notebook vault
allowlist. It does not auto-register roots. Native clients can use `projectId`.
`projectId` plus a conflicting path is rejected.

`remote_*` commands remain in Tauri, but External translates them according to
ADR-006 to their parent method + `target: {kind:"ssh",connectionId,remotePath}`.
No password/connection object is transmitted. The current host **rejects SSH and
WSL with UnsupportedCapability** before local path lookup. Embedded still invokes
the original SSH policy via the injected transport. There is no local fallback
after an External failure, no new remote method registration and no credit for
remote support in coverage.

### Native-only replacement wire (existing method names)

`files.replace_preview` still takes `projectId,path,query,replacement`; the
single-file result now contains **objects**, not the old incompatible tuples:

```json
{
  "query": "title",
  "replacement": "name",
  "files": [{
    "path": "code.txt",
    "name": "code.txt",
    "matches": [{
      "path": "code.txt", "name": "code.txt",
      "line": 1, "column": 4, "lineText": "İ title",
      "matchText": "title", "replacementText": "name", "start": 3, "end": 8
    }]
  }],
  "totalMatches": 1,
  "truncated": false
}
```

`files.apply_replacements` takes `projectId,path,replacement,matches`, where every
match must contain `start,end,matchText`. Missing text/invalid types reject the
request rather than silently dropping records or applying unchecked offsets.
This native pair is **literal-only**: preview explicitly rejects `options.regex`
when true. Use `files.replace_text_preview` / `files.apply_text_replacements`
for regex capture expansion and per-match `replacementText`. A capture-expanded
preview cannot be paired with native apply's uniform literal replacement.
Well-formed but stale/out-of-range/invalid-boundary records are skipped by core.
Swift keeps those byte offsets/text verbatim and rejects malformed replies.
Older tuple-only/offset-only peers fail explicitly and need coordinated upgrade.

These are native-only existing adapters, not entries to add to the frozen
Tauri-derived methods schema (ADR-038 precedent). The two false legacy→native
name-only aliases are removed; legacy methods now have their own real handlers.

## Evidence and remaining work

- Before the fix, three authenticated UDS regression tests failed: tuple `[4,9]`
  instead of a rich match at bytes 3..8, same-length stale overwrite, and unchecked
  apply without matchText. After consolidation these three pass.
- `tests/search_services.rs` exercises host authority, native/legacy roundtrip,
  notebook registration/revocation, recorded worktrees, malformed input,
  authentication, unsupported targets and filename methods.
- Swift `FilesClientTests` uses the existing scripted UDS/real codec.
- Independent code review found a native regex preview/apply mismatch (not a
  grill-me/cutover review). The added UDS regression first failed because native
  preview accepted `(title)([0-9])` → `${1}_${2}`, displaying `title_2` even though
  native apply would write the unexpanded template. The native preview guard
  rejects this unsupported option; the same regression exercises capture-aware
  legacy preview/apply, preserving that existing capability.
- `scripts/native/smoke-headless.mjs` exercises the **real CLI rpc** search,
  preview/apply and refusal paths. Build the CLI before running it; an old cached
  executable is not evidence.

Final command counts/results live in MIGRATION_PLAN.md slice 14 and checks.json.
Remaining G43 work: headless SSH credential/executor ownership and real remote
target support, then the other missing files-family methods. Remote lexical
path validation alone is not proof against remote symlink races. Do not mark
G43, full host parity, or P8-05 accepted from local tests.

## Slice33 — current writer for both hosted apply wires (2026-10-09)

The core apply mutex above serializes engine callers; it is not a
data-directory writer lease. Real UDS regressions demonstrated that the
separately registered legacy/native apply handlers still wrote with a
read-only CoreContext or after the held `.write.lock` inode was replaced.

`root_for` now receives the registered Operation. After its existing pure
target decoder and SSH/WSL refusal, it calls the shared host
`file_access::write_refusal` only for Apply/NativeApply, before any project
registry, worktree, notebook-vault or path IO. Missing/lost authority is
IoError, including an empty apply; unsupported/malformed targets keep their
prior error precedence. The six search/preview/filename operations do not
require a writer. Authentication still precedes all of these checks.

Search's canonical/registered-root policy, rich/literal wires, parsing,
UTF-8/stale/overlap behavior and core apply mutex remain unchanged. This
does not add target capability, a new ACL or mid-flight revocation. The
eight other file mutation guards in slice32 are a distinct audited boundary,
not the source of this guarantee.

Targeted evidence: `/tmp/aeroric-search-authority-red-njgr10ax/`.
`host-red.log` compiled4 passed/5 failed/0 ignored (rc101); `host-green.log`
passed new authority9, original search12 and slice32 file14. Scoped
host/lib/three-suite clippy passed. Tests use synthetic Unicode files,
missing rg and a fake Git executable, never real indexes/configs/remotes.
Final integration: `/tmp/aeroric-search-authority-integration-b_mfn3o4/`.
Current CLI was rebuilt and its full smoke passed both legacy and native
preview/apply roundtrips. files11/102, cutover56/325 and skeleton24/188 passed
without skips. Tauri1284/1284, fmt/clippy/typecheck passed. Adapter remains
rc1/1345 solely at the unchanged launch-validation WIP core fmt/clippy;
per-case raw logs are retained.

Inline source comparison reverses only the guard import, Operation parameter/
call and five-line admission to recover the entire pre-slice host source
byte-for-byte; core search is unchanged. Inventory/freeze-check,12 unchanged
contracts,16 digest shards/max21842B, runner36/context23+19, ownership and
tracked+73untracked whitespace checks passed. Coverage75/641,45 dispatches/
floor75 is unchanged. No independent/user acceptance or G43 closure is inferred.
