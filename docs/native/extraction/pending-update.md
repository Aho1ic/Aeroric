# Pending-update query ownership — P8-05 slice 20

## Boundary and defect

`maintenance.get_pending_release_update` was already registered when a
`MaintenanceHost` was injected and was already forwarded by Tauri. This slice
does not add a method or enable a new capability.

The host serialized the private `PendingReleaseUpdate` record, but the actual
Tauri client decodes `Option<ReleaseUpdatePrepareResult>`. Nonempty replies
lacked the required `readyToRestart`. The former test only checked `tagName`,
so it could not detect this contract error.

The authenticated synthetic UDS regression was run before the fix:
**rc101, 1 passed / 1 failed / 0 ignored / 5 filtered**. Its public DTO decoder
reported `missing field readyToRestart`; the absence/read-error control passed.
Evidence: `/tmp/aeroric-pending-query-red.dOtnU9/host-red.log`.

## Single owner

- `aeroric_core::maintenance::updates::pending_update_result` owns the exact
  optional tag filter and public mapping moved from the original Tauri command.
- Host and Tauri feed it their existing `MaintenanceService::pending_update()`
  result. Tauri keeps the local read inside the existing deferred dispatch.
- The private persisted DTO remains unchanged, including `checksumPath`;
  `readyToRestart` is not added to that file format.
- The existing IO method still folds load/read/decode errors into `None`.

The public reply is `null` or the same seven fields used by the legacy command:
`tagName`, `assetName`, `installerPath`, `readyToRestart`, `checksumVerified`,
`helperStatus`, and `error`. Exact tags are not trimmed or case-folded.

The pending query historically sets `readyToRestart: true` for any available
record, even when checksum/helper/error fields indicate failure. This slice
preserves that query snapshot, **not** a new validation result or permission
to install. Preparation, checksum validation, platform replacement and restart
authorization are unchanged.

## Executable evidence

- Two pure core tests cover all public fields, failed status passthrough,
  legacy defaults, exact/optional tags and no pending record.
- Two authenticated UDS tests handwrite synthetic pending JSON without calling
  prepare. They decode the actual client DTO and assert byte preservation,
  absence of installer/checksum/cache writes and zero calls to all seven fake
  maintenance-port methods. Missing, filtered, corrupt and directory-shaped
  read-error inputs are included.
- The real default CLI has **no** `MaintenanceHost`. Its smoke test checks
  `UnsupportedCapability` and absence of newly created pending/update/cache
  files. Positive CLI maintenance support is not claimed.
- `scripts/native/pending-update-invariants.mjs` supplements those tests with
  a mapping/DTO/deferred-IO/conditional-registration ratchet; it is not a
  substitute for decoding the real UDS response.

Targeted core maintenance **26/0**, host maintenance **7/0** and Tauri check
passed. The first ownership-script run failed because it incorrectly looked
for an `if let` in the dispatcher; the actual conditional registrar uses
`match host` with `None => false`. The script was corrected to check the
existing assembly/registrar chain, not by changing production registration.
Both logs are retained in `/tmp/aeroric-pending-query-targeted.LjJP4i`.
The corrected ownership check, CLI syntax, rebuilt CLI and complete CLI smoke
also passed there, including the explicit unsupported pending query.
Final verification on 2026-10-08:

| Check | Actual result |
| --- | --- |
| maintenance / cutover | 6 gates / 73 tests; 17 / 98 |
| skeleton / tauri-adapter | 24 / 188; 12 / 1280 (Tauri lib1219) |
| all host targets | 11 suites / 90 tests, zero failures or ignored |
| affected React updater tests | 1 file / 9 tests, zero skipped; mocked invoke only |
| typecheck, core/Tauri fmt and clippy | rc0; typecheck includes desktop and mobile |
| pins/floors/contracts/digest/inventory | 7 notification pins, 109 paired floors, 12 unchanged contracts, 16 shards, all rc0 |

Independent static review found no substantiated issue. Logs:
`/tmp/aeroric-pending-query-integration.lFCNry` and
`/tmp/aeroric-pending-query-web.h4sTPk`. Full Swift/Vitest packages, actual
WebView IPC and cross-platform packaging were not rerun. Coverage remains
60/641 and dispatch count remains 30.

## Remaining limits and rollback

- `get_notifications` fetches releases, probes the platform and can write its
  cache; it is not a pure-read substitute and was not changed.
- The host currently treats a non-string `tagName` as no filter via
  `Value::as_str`, unlike Tauri's typed argument rejection. This existing
  invalid-input difference is outside this slice, not silently fixed.
- No real assets, release download, installation, restart, publication, real
  user data or GUI global service root was exercised.
- Embedded remains the default. External errors are not retried locally.
  No writer authority, handover or non-macOS behavior was changed.
- There is no persisted migration to undo. Returning to the compatible
  Embedded placement retains its old query semantics. This is not cutover
  acceptance: P8-05 remains `in_progress` and G43 remains open.
