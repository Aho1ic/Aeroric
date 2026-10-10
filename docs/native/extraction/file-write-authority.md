# Hosted file writer admission — P8-05 slice32

## Demonstrated boundary

Authentication in `dispatcher::dispatch` is not writer authority. Before this
slice, eight already-registered file mutations used CoreContext only to look
up a project root. Their path-only core functions, or host `copy_into`, had
no lease to check. A read-only business context could therefore mutate files,
including history snapshots; an inode-replaced writer could keep writing.

This is a correction to existing hosted methods, not eight new methods or a
claim that all legacy file commands now forward to the host. Coverage stays
75/641. G43 and P8-05 remain open.

## Placement and preserved behavior

`aeroric-host::file_access::write_refusal` checks
`CoreContext::require_write` on each admitted mutation. A missing or lost
lease returns IoError with the original permission diagnostic, no retry.
The helper does not acquire authority, cache an earlier lease check or
consult client-supplied authority flags.

Admission is after the existing pure pre-root argument decoding and before
project-record lookup, canonicalization and file-side effects:

| Handler | Hosted methods |
| --- | --- |
| `services::files_write_file` | `files.write_file` |
| `services::files_create` | `files.create_file`, `files.create_directory` |
| `services::files_delete` | `files.delete_path` |
| `services::files_rename` | `files.rename_path` |
| `file_services2::files_format_file` | `files.format_file` |
| `file_services2::files_copy_paths` | `files.copy_paths_to_directory` |
| `file_services2::files_local_history`, Restore only | `files.restore_local_history_entry` |

The file/directory/history query paths do not require a writer. For an
authorized writer, decoding, path/error ordering, result shapes, snapshots,
copy non-overwrite behavior and IO algorithms are unchanged. In particular,
the existing formatter takes an absolute file path and snapshots before
extension selection; an unsupported `.txt` can still create a snapshot for
an authorized writer. This slice does not pretend it is a pure query.

`HostWiring::files` is a trusted, in-process `FilePorts` injection.
Its deletion default is exactly core `files::read_write::delete_path`
(OS Trash, no permanent-delete fallback). Tests use the existing
`delete_path_with_trash` path policy and an isolated quarantine; RPC cannot
provide a deletion callback. No core/Tauri/frontend/protocol changes or
default-placement changes are part of this slice.

This is request-time writer admission, not a per-project transaction,
filesystem race cure, ACL or mid-flight revocation guarantee. ADR-006 target
routing, core path-only callers, search apply and other mutation families
still need their own evidence; do not extend this checkpoint into those claims.

## Targeted evidence

`/tmp/aeroric-file-authority-red-5d1pn1yf/`:

- Pre-edit source snapshots and `runtime-delta.patch` isolate this slice from
  the extensive inherited workspace.
- `host-red.log`: after only the isolated-trash seam, compiled behavior
  **4 passed/10 failed/0 ignored,rc101**. Eight missing-lease mutation tests,
  a same-connection replaced-lock matrix and pre-root refusal ordering failed.
  Writer/query/unauthenticated/invalid-argument controls passed.
- `host-green.log`: authority14, original batch2 four and search12 passed.
  The authority tests assert recursive bytes/modes/entries, current-inode
  refusal, continued queries, and zero trash calls after refusal.
- `host-clippy.log`: host lib and those three test suites passed with
  `-D warnings`. No test touches OS Trash or runs a formatter.

## Final integration (2026-10-09, bounded completion)

`/tmp/aeroric-file-authority-integration-_na_dbj_/`:

- Current CLI rebuilt and full synthetic headless smoke passed, including
  writer file/history/copy controls and unsupported `.txt` formatting.
- files10/93, cutover54/316 and skeleton24/188 passed without skips.
- Full Tauri lib1284/1284, fmt/clippy/typecheck passed. Final adapter remains
  **rc1/1345 tests**, with only the two inherited core fmt/clippy failures
  in untouched `validate_agent_launch_services.rs`. Full per-case logs are
  copied into the `*-raw/` directories, not reduced to an exit-code summary.
- Inline review of the pre-slice delta found only the seven guarded
  function sites (eight methods), the default-preserving deletion injection
  and the read-only test-harness seam. No core algorithm or Tauri source
  changed. This is not independent or user approval.
- Coverage75/641,45 dispatches/floor75; inventory/freeze-check,12 unchanged
  contracts,16 digest shards/max21842B, runner36 and context22+18 passed.
  Tracked diff and72 untracked files were whitespace-clean.

G43 remains open, including its unserved/remote files boundaries. The
separate search-apply authority branch is the next investigation, not an
implicit guarantee from this slice. Source ownership checks supplement,
not replace, real UDS tests. No release or production cutover was performed.
