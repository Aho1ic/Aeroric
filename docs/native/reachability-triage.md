# Command reachability triage (G16 / G17)

> Purpose: explain **why** 93 of the 660 registered Tauri commands have no caller in
> live code, so that P2-01 freezes only real methods into the local protocol and P3-05
> ports the DSH capability once rather than 42 times.
> This is **evidence, not a deletion list**. Nothing here may be removed on the strength
> of this file alone; each bucket names the task and the decision that closes it.
> Measured 2026-09-21 from `main` @ `ca69a4b` by `scripts/native/inventory.mjs`
> (`--freeze-reachability` writes the table, `--check` re-derives and compares it).

## Method

A command counts as **reachable** when its name appears as a `"string literal"` in `src/`
(the React client) or in `mobile/` / `packages/`, **or** is referenced at least once inside
`src-tauri/src/` on a line that is neither its own `fn` declaration nor a comment
(`src-tauri/src/lib.rs`, the registration site, is excluded). Counting unit is *referencing lines*.

Test sources (`*.test.ts(x)`, `tests/`, `*_tests.rs`) never make a command reachable — a mock
asserting a call is not a production call site — but they are now scanned and recorded as
`test_only_literal`, because discarding them entirely hid wired functionality (see the
revision-2 note below). Prefix derivation is likewise preserved per command as
`prefix_derived_of` / `prefix_derived_parent_reachable` rather than being inferred by hand.

Directories excluded as non-evidence: `node_modules`, `.git`, **`.worktrees`**, `.build`,
`.artifacts`, `target`, `dist`, `build`. The `.worktrees/` exclusion is not cosmetic:
`review-fixes` and `arch-debt` hold complete stale copies of the source tree and make dead
commands look referenced — the first pass over-counted reachability until they were excluded.
`meta.reachability.excluded_dirs` records which of them actually exist, so the exclusion is auditable.

Frontend literal surface, for contrast: **422** `invoke("literal")` call sites, all 422
resolving to registered commands — the direct call surface is clean, and `--check` now
enforces that.

## Buckets

> **Revision 2 (2026-09-21).** Revision 1 claimed the `tests::*` cluster was "an unwired
> feature" and put 42 commands in a "zero evidence anywhere" bucket. Both were wrong, and the
> reason matters: the scanner **discarded test sources entirely**, so a command exercised only
> from a test file — including one whose test asserts `invoke` was called with the exact
> literal `"run_tests"` — was reported as unwired. Reading that as "dead" would have justified
> dropping live functionality. Test files are now scanned as a **weak** signal
> (`test_only_literal`) that never makes a command reachable on its own but is preserved and
> reported, and `prefixCommand` derivation is recorded per command
> (`prefix_derived_of`, `prefix_derived_parent_reachable`). Partition below is disjoint and
> sums to 93.

| # | Bucket | Count | Verdict | Owner / closing decision |
|---|---|---|---|---|
| A | `remote_*` / `wsl_*` derived from a **reachable** local command, no other evidence | 28 | **Not dead.** Produced at runtime by `prefixCommand` (`src/lib/invokeFacade.ts:93-97`); the mirror table only spells out a subset, so a literal scan cannot see them. | P2-01 must model these as **one RPC method + a `target` parameter**, not as 28 separate methods. |
| A′ | Same, **and** also named in a test file | 6 | All `remote_lsp_*` (`definition`, `references`, `rename`, `apply_workspace_edit`, `document_symbols`, `workspace_symbols`): derivation plus direct test evidence of the remote call path. | Same collapse as A. P6-03 must confirm the SSH LSP path is genuinely exercised, not merely asserted in a mock. |
| B′ | Named only in test files, not prefix-derived | 6 | `discover_tests`, `run_tests`, `remote_discover_tests`, `remote_run_tests` — **the run/test feature is wired** (`src/test/test-explorer-panel.test.tsx:65,170` assert `invoke` receives `"run_tests"`); the production call site does not use a bare literal, which is why the literal scan missed it. Plus `list_dsh_agent_presets`, `upgrade_agent_versions`. | P6-03 finds the real call site and records how the name is built; the other two belong to P5-05/P8-02 triage. **None of these may be treated as removable.** |
| C | `dsh_webui::*` typed commands with **no evidence of any kind** | 41 | **Structural duplication, not rot.** The React DSH layer reaches dsh-web over the **`/api/remote.mux` WebSocket downlink** (`src/dshProtocol.ts:16-21`), so the parallel typed Tauri surface is bypassed. The authoritative capability list is `src-tauri/src/dsh_webui/protocol_inventory.rs`: **198 live pinned names across 10 lists** (RPC_METHODS 62, REMOTE_METHODS 87, REMOTE_EVENTS 19, REMOTE_EVENT_MODES 19, stream frames 3/4/4; plus 48 entries explicitly marked RETIRED and excluded from the capability surface) (`SOURCE_COMMIT c291e796…`, `PACKAGE_VERSION 0.1.5-rc.2`, `PROTOCOL_VERSION 2`). | **G17** — P3-05 decides once: 42 typed core methods, or one gateway/mux-shaped core service plus the pinned inventory. |
| D | Everything else, no evidence of any kind | 12 | Itemised below. | Each named task. |

Structural fact worth recording: **all 660 command names are globally unique** (verified; the
repo's own registration guard also fails on a duplicate definition name). That is why
"method name = legacy command name" needs no namespace to stay collision-free — and it is an
invariant P2-01 may rely on, so renaming a legacy command later is a protocol change, not a refactor.

### Bucket D itemised

| Command | Category / check module | Note |
|---|---|---|
| `app_settings::save_agent_paths` | domain_rpc / settings | panel writes paths via `update_agent_path_settings` (1 real frontend hit); this looks like a near-duplicate entry point |
| `app_settings::save_custom_agent_profile` | domain_rpc / settings | panel uses `setup_agent_profile` + custom-agent commands; possibly superseded |
| `dsh_plugins::install_dsh_plugin` | domain_rpc / extensions | F17. Frontend uses only `list_dsh_plugins` and `save_dsh_plugin_settings` — so install/uninstall/toggle are **the intended capability with no UI yet**, not rot |
| `dsh_plugins::uninstall_dsh_plugin` | domain_rpc / extensions | as above |
| `dsh_plugins::toggle_dsh_plugin` | domain_rpc / extensions | as above; P8-01 requires enable/disable semantics |
| `skills::get_marketplace_skill_details` | domain_rpc / extensions | Skill Hub detail view (F17) — likely a native-side gap |
| `notification::install_release_update` | domain_rpc / maintenance | update install (F19); still unexplained, verify in P8-04 |
| `notebook::notebook_git_sync` | domain_rpc / notebook | F16. Referenced only by definition, registration and Rust comments. **Correction:** revision 1 said "React uses `note_sync_*` names" — that was never checked; `grep` finds no `note_sync_*` at all, so there is no substitute name, only an unwired one |
| `notebook::notebook_sync_bind` | domain_rpc / notebook | as above |
| `notebook::notebook_sync_unbind` | domain_rpc / notebook | as above |
| `config::write_wsl_project_config` | domain_rpc / settings | WSL counterpart of a reachable `write_project_config`; probably bucket A in substance |
| `wsl::get_wsl_agent_status` | platform_specific / platforms | F12; macOS cannot exercise it — keep capability-gated, do not delete |


## Consequences the plan must respect

1. **P2-01 must not promote unreachable commands into `methods.schema.json` unexamined.**
   `meta.counts.unreachable = 93`; of those, **81 carry a `rpc_method` proposal** that
   `command-map.json` generated before this triage existed. **40 of those 81 now have an
   explanation** (prefix-derived, or exercised from tests), so they are live surface that must
   be modelled — usually as one method plus a `target` — rather than dropped. The remaining
   **41 have no evidence of any kind** and must be individually adjudicated before entering the
   contract. The proposals stay in the file as history but are **not** an approved method list.
2. **Bucket A collapses to parameterised methods.** `remote_lsp_hover` + `wsl_…` etc. become
   `lsp.hover` with a `target` argument. This is a direct consequence of the already-frozen
   `ProjectTarget` design and shrinks the contract by ~34 entries.
3. **Bucket C is a decision, not a port.** Recorded as **G17** so it cannot be closed by
   silence. F06's acceptance stays "each DSH capability has one owner", and
   `protocol_inventory.rs` is the checklist, and it is 10 pinned lists rather than one number: `RPC_METHODS` 62, `REMOTE_METHODS` 87, `REMOTE_EVENTS` 19, `REMOTE_EVENT_MODES` 19, stream frames 3/4/4, plus 48 explicitly retired entries (10/10/28) that are not a capability surface — 198 live names. (An earlier revision wrote "235 names"; that figure was never derived from the file and is withdrawn.) Not the 41 zero-evidence Tauri commands.
4. **Bucket D items keep their commands until triaged.** Several are the *intended* capability
   for F16/F17/F19 with a missing UI, and per INV-10 removing cross-platform or
   mobile-facing surface because "macOS React doesn't call it" is explicitly out of bounds.
5. **The table is machine-enforced.** `--check` compares every command's four reachability
   fields and the `unreachable` total against a recomputation, and fails on drift in either
   direction, so a later phase cannot quietly re-classify a command to make a gate pass.

## What is still unknown (not pretending otherwise)

- Whether `mobile/` reaches any of bucket C/D through the remote protocol rather than
  Tauri invoke. `mobile/` was scanned for string literals and identifiers and produced no
  hits, but the phone may address dsh-web through the relay with method names that only
  exist in `packages/remote-contracts` — that needs checking during P8-03.
- Whether `notification::install_release_update` is genuinely unreferenced or reached via a
  Rust-side call path my line-based heuristic treats as its own declaration.
- Upstream truth for bucket C: `protocol_inventory.rs` pins dsh-web at commit
  `c291e796…` / `0.1.5-rc.2`; whether all 62 `RPC_METHODS` entries are still current was not verified
  against the upstream package (out of scope, and reference material is read-only).

## Revision 3 @ 2026-09-21 — the remote/ half of G16 is closed by measurement

`docs/native/extraction/database-notebook.md` deferred `remote/` on one unresolved fact: does mobile
reach these commands through the relay rather than Tauri `invoke`? Measured:

- `grep -rl "@tauri-apps" mobile --exclude-dir=node_modules` → **0 files**. Mobile never imports the
  Tauri API; its transport is `mobile/src/transport/{e2ee,orca-e2ee-v2,pairing-offer,remote-connection}.ts`.
- Server side confirms the path: `src-tauri/src/remote/tasks_rpc.rs:483` does
  `app.emit("remote-task-request", payload)` and awaits the React answer (`src/App.tsx:498`,
  `src/test/app-event-wiring.test.tsx:124`, `src/test/app-remote-task-request.test.tsx`).

Consequence: `remote/` was expanded in §5.1 (`docs/native/extraction/remote.md`, 22 rows / 9 wrappers)
without waiting on any user decision, and the remaining "no caller in live code" explanation for the
`remote_*` commands is **transport-level dispatch, not dead surface**. Only `dsh_webui/` still waits on
Q7/G17. Unchanged and still open: 53 rows with no evidence at all, 41 of them `dsh_webui`.
