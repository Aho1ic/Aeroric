# Settings preferences — P8-05 slice 15

Status: **slice verified**, 2026-10-08. This slice fixes and connects four existing
methods; it is not full settings parity or cutover/grill-me acceptance.

## Ownership and wire

| Method suffix (`settings.*`) | Canonical argument | Stored key / rule |
| --- | --- | --- |
| `save_send_shortcut` | `sendShortcut: String` | `send_shortcut`: `enter` or `mod_enter`, otherwise `mod_enter` (no added trim) |
| `save_shift_enter_newline` | `enabled: bool` | `terminal_shift_enter_newline` |
| `update_auto_cleanup_settings` | `autoCleanupSettings` | `auto_cleanup_settings`: existing typed defaults and clamping |
| `update_weekly_report_settings` | `weeklyReportSettings` | `weekly_report_settings`: existing typed defaults and weekday clamping |

The previous host-only `shiftEnterNewline` spelling remains an explicit
compatibility alias. Two different values are rejected; strings/numbers/null
never stand in for a boolean. The canonical `enabled` spelling was already in
the real React caller and frozen contract; this does not introduce a new RPC.

Core `settings/schema.rs` owns the two schedule DTOs and three normalizers;
Tauri re-exports them. Core `settings/preferences.rs` applies one normalized
field to the original map. It validates an `AppSettings` projection before
committing so a write cannot succeed only to fail decoding its Tauri reply.
The projection is **not** persisted: unknown top-level and nested fields in
unrelated settings remain intact. This is not a legacy settings load, repair,
script refresh, model probe or agent launch.

Numeric wire types are unchanged: values fitting the DTO are clamped;
negative/overflowing/non-numeric values fail decoding. `last_run_at` and
`output_dir` are preserved. Saving a preference does not run cleanup, generate
a report or create its output directory.

## Read-modify-write and placement

- `storage::settings::update_settings` covers read/callback/write with one mutex.
  Direct map saves and both legacy settings loaders use the **same** mutex.
  A lease alone or atomic rename alone does not prevent lost updates between
  requests in the same process. Existing host generic field and effort updates
  now use this same transaction, not a parallel load→save path.
- Writer lease checks still authorize writes; a mutex does not grant authority.
  Failed reads/validation/callbacks create neither a replacement nor a backup.
  Existing private file permissions and atomic backup implementation remain.
  Callbacks must not recursively invoke another settings writer or legacy loader,
  and guards never cross an await. Host disk work uses `spawn_blocking`.
- Tauri's four commands go through `host_dispatch` with the original Embedded
  work in an unpolled future. External failures cannot execute it. Embedded
  legacy/deferred persistence is retained: non-macOS does not acquire new settings
  writer authority or silently get a new ViaHost bridge.

This coordinates the covered API paths, not arbitrary external file editors.
An explicit whole-map save remains a replacement. Other legacy typed full-file
writes can still discard unknown keys; loader repair/cache/script side effects
are not claimed solved by this slice.

## Evidence and limits

The four new authenticated UDS regressions were run **before** implementation:
0 passed / 4 failed (rc101), exposing wrong newline argument, missing clamps,
missing shortcut normalization, and mutation before an unreturnable typed reply.

Regression coverage now includes:

- Core preference tests: unknown fields, schedule rules, no output/agent-home
  side effects, typed corruption and read-only refusal.
- Core storage tests: concurrent updates, failure-before-callback and no-write
  abort, preserving original bytes and backups.
- Host UDS: canonical and compatibility wire, malformed requests, corruption,
  six simultaneous clients mixing preference/generic/effort updates.
- The real CLI smoke exercises all four methods and verifies invalid input does
  not alter the file. Fake profiles include their real DTO-required labels.
- `settings-preference-invariants.mjs` pins four deferred dispatches, one schema/
  normalizer owner and the shared mutex; it is not a behavioral substitute.

Final commands/counts are recorded in MIGRATION_PLAN.md. Real HOME/Keychain/
remote hosts and production data are excluded. The custom-agent context/artifact
candidate pair is deferred because its existing implementation reaches typed
loaders and script/sidecar transactions; registering it blindly would not be
safe progress. G43 and P8-05 remain open.

## Slice 16 extension: notebook embedding configuration

`settings.update_notebook_embedding_settings` is the fifth typed preference.
It takes `notebookEmbeddingSettings` and reuses the core DTO/normalizer:
`provider` is the existing `ollama` / `openAi` enum; `base_url` and `model` are
trimmed and blanks regain the existing defaults. Provider selection is retained;
this layer does not add endpoint validation or make requests.

Inline `api_key` / `apiKey` are not DTO fields and do not enter the replacement
configuration, just as on the old Tauri wire. Credential storage remains a
separate API; this update does not call it or scrub historical files/backups.
The red UDS regression (rc101, 0/1) demonstrated that the previous generic host
path kept whitespace and inline keys. Core now owns this DTO and normalizer;
the desktop re-exports them and defers its original Embedded implementation.

The concurrent host regression now mixes seven clients so a genuinely generic
proxy update remains covered alongside typed embedding/preferences and effort.
Proxy's legacy per-agent migration is not included.

Final slice 16 verification (2026-10-08): core preferences 5, storage settings 5,
host settings 12; cutover 15/80, skeleton 24/188, tauri-adapter 12/1274,
host all-targets 83, Vitest 375 files/5456 tests (zero skipped). Core/Tauri
fmt/clippy, desktop/mobile typecheck and rebuilt CLI smoke passed. Coverage
remains 60/641 while real dispatches increase 26→27. Independent static
embedding review found no substantiated issue; it is not cutover approval.

The first skeleton run exposed the existing digest 80% headroom limit:
26,404 B in one gap shard. Applying the documented 9→10 gap partition policy
produced 16 total shards, maximum 21,842 B; the unchanged 17-test floor,
80% bound, reconstruction and orphan tests all pass. A separate read-only review
also reconstructed all 42 gaps from the indexed shards without omissions.

## Slice 17 extension: built-in DSH effort

`settings.update_dsh_reasoning_effort` keeps `{agent, effort}`. Core's opaque
`DshReasoningEffort` accepts only exact built-in `dsh`, then trim/ASCII-lowercases
against `DSH_REASONING_EFFORTS = off/high/max`. Both legacy helpers retain their
existing family-specific error check but reuse this final validator. Host routes
the request to the same preference transaction as the other five methods.

This is a patch of `dsh_reasoning_efforts.dsh`, not the legacy whole-map cleaner:
other valid string entries (including the already-tested `other: low`) and
unknown settings remain. A malformed map is not silently replaced with `{}`;
other incompatible typed fields cause rejection before commit. The exact target
entry may be repaired. Embedded's legacy loader and unrelated cleanup remain
unchanged behind a deferred dispatch future; External never polls that future.

The authenticated UDS regressions first failed 0/2 (rc101): the old handler
overwrote a non-object map and committed an unreturnable `custom_agents: false`.
The Swift regression also first failed: DSH incorrectly reused low/medium/high
and rejected off/max. Native capabilities now have separate lists and the
existing Picker/action share the capability function, refusing stale selections
after family changes. Claude/Codex/OMP choices remain unchanged. This corrects
the callback view; no new native shell or SettingsClient integration is claimed.

Proxy is deliberately separate: its legacy loader consumes per-agent overrides,
enabled flags and old custom-agent credentials **before** assigning the requested
replacement. Applying a generic normalizer to the old raw map after replacement
could revive values the user explicitly cleared. This is not solved by trimming
four proxy strings or by calling the side-effecting legacy loader.

Final slice 17 verification (2026-10-08): preferences 8, storage 5, host settings
14; settings-ui 1/13, cutover 15/85, skeleton 24/188, tauri-adapter 12/1274,
host all 85, Swift all 271, React targeted 3 files/49 tests (zero skipped).
Both fmt/clippy, typecheck, rebuilt CLI smoke and ledgers passed. Coverage
60/641; 28 real dispatches. Independent review found no substantiated issue.
Native family-change/click behavior was not dynamically UI-tested, and the
targeted React run logged one controlled-input warning not yet attributed.

## Slice 18 extension: proxy replacement and legacy-field consumption

`settings.update_proxy_settings` is the seventh typed preference, with canonical
`proxySettings`. Core owns the `ProxySettings` and `LegacyAgentProxyConfig` DTOs
and the existing pure normalizers; Tauri re-exports them. All four request
strings default to empty and replace the previous proxy configuration together.
URL trim/prefix, comma-separated no-proxy trim (without deduplication), and
username/password trim retain the legacy rules. This is not endpoint/scheme
validation or a proxy connectivity test. Unlike embedding's inline keys, the
proxy request's username/password are existing persisted DTO fields.

The old command loads and consumes legacy state **before** replacement. The
headless transaction therefore substitutes the new target in a temporary typed
projection, validates the old migration inputs and unrelated fields, derives
enabled flags, consumes deprecated overrides/custom credentials, and writes the
complete normalized replacement. It never invokes the legacy settings loader.
The original raw map is persisted, not the projection: custom-agent order,
unrelated/unknown fields and nested fields called username/password remain.
Only each custom object's top-level legacy username/password are removed.
Non-object custom records are rejected, not silently filtered or left partly
migrated. A corrupt target proxy field can be repaired; corrupt migration inputs
or unrelated typed settings fail before any write.

Both legacy AppSettings wrappers now call core field helpers. Their original
semantics remain: inspect raw trim-presence before normalization (a no-proxy
value of `" , , "` counts as present), do not sort legacy HashMap values or
require an enabled address, and use the first nonempty whole credential pair
only when both current credential fields are empty. Enabled flags retain their
existing normalized-key/legacy-true precedence. The headless replacement uses
the enabled helper, **not** the old proxy resolver: calling the resolver after
replacement would revive explicitly cleared addresses or credentials.

The three new UDS regressions first failed 0/3 (rc101): the old host retained
whitespace, failed to consume legacy flags, and committed unreturnable custom
data. The obsolete generic field writer has now been removed; seven concurrent
clients exercise one preference transaction path. Embedded loader behavior and
platform authority stay unchanged behind the deferred Tauri future. Swift proxy
form/RPC discrepancies remain outside this slice; no native proxy integration
or full legacy-loader migration is claimed.

Final slice 18 verification (2026-10-08): preferences 12, storage 5, host settings
17; cutover 15/92, skeleton 24/188, tauri-adapter 12/1274, host all 88,
settings-ui 1/13, targeted React 3 files/52 tests with zero skipped. Both
fmt/clippy, desktop/mobile typecheck, rebuilt CLI smoke and all ledgers passed.
The independent review found no substantiated implementation issue, but identified
a direct-first-clear test gap. Core and UDS now restore original legacy fixtures
and clear with the first request; a fresh preferences/host/cutover/CLI run passed
after that addition, without inflating test counts. Coverage remains 60/641;
29 real dispatches. Full Vitest/Swift packages and real proxy/native proxy UI
were not rerun in this slice. The targeted Web run had only the color-environment
warning, not the previous slice's unclassified controlled-input warning.
