# Module review — skeleton (Phase 1, Task P1-04/P1-05)

**Gate status: `BLOCKED: grill-me entrypoint not loaded in this session` — NOT accepted.**

Per `MIGRATION_PLAN.md` §4.1 and §5.4, this file records the real state instead of
substituting an agent self-review for the tool call. The skeleton may not be
declared accepted, and the Phase 1 gate stays open, until an actual `grill-me`
invocation is recorded in §"Actual invocation" below.

## Tool provenance

| Field | Value |
|---|---|
| Tool | `grill-me` (Agent Skill, **not** a CLI binary) |
| Source | `mattpocock/skills`, MIT licence (confirmed via GitHub API) |
| Pinned revision | `c55ee46073ed923f86ce59a5eb3b6d895095d1b7` (main, 2026-09-18) |
| `grill-me/SKILL.md` last change | `fcf0071560d3` (2026-08-15) |
| Install | `curl -fsS` from raw.githubusercontent at the pinned revision, no hand transcription |
| sha256 (first 16) | `SKILL.md` `caaf8b8de1684f96` (157 B) · `agents/openai.yaml` `c061e39c3e0f9d86` (137 B) · `grilling/SKILL.md` `10ff989e7498b23b` (1987 B) · `grilling/agents/openai.yaml` `1411d7df7d99b7e6` (113 B) |
| Delegation chain (verified complete) | `/grill-me` → `grill-me` (`disable-model-invocation: true`) → `Call the Skill tool with "grilling"` → `grilling` runs the design-tree / frontier-rounds interview |
| Install location | `.claude/skills/{grill-me,grilling}/` — note `.claude` is gitignored here, so this path is machine-local; the durable provenance record is `docs/native/tooling.md` |

## Why the invocation failed

`Skill(skill="grilling")` returned `Skill "grilling" not found`, and the error echoed the
registry of loaded skills. `shadcn` — which also lives in this project's
`.claude/skills/` — **is** in that registry, so the directory is read, but the
registry is snapshotted at session start and does not hot-load newly added
skills. Consequence: the skill exists on disk at the pinned revision and cannot
be called in the session that installed it.

No `grill-me` shell binary exists in the pinned revision (the directory contains
only `SKILL.md` and `agents/openai.yaml`), so there is no CLI fallback to fall
back on, and per §4.1 none may be invented.

## Review subject — what would be interrogated

Commit under review: working tree over `ca69a4b` (uncommitted).

Changed / added files under review (explicit list, not the whole repo):

- `core/Cargo.toml`, `core/Cargo.lock`, `core/crates/{aeroric-protocol,aeroric-core,aeroric-host,aeroric-cli}/Cargo.toml`, their `src/lib.rs` / `src/main.rs`, and `core/crates/aeroric-protocol/tests/skeleton.rs`
- `macos/AeroricKit/Package.swift`, `Sources/{AeroricProtocol,AeroricClient,AeroricUI}/PackageSmoke.swift`, `Tests/{AeroricProtocolTests,AeroricClientTests,AeroricUIUnitTests}/PackageSmokeTests.swift`
- `scripts/native/{doctor,check,inventory,check-patches,inventory.test}.mjs`, `scripts/native/checks.json`, and the five `exit(78)` not-ready scripts
- `docs/native/{environment,tooling,rollback}.md`, `docs/native/adr/001-native-boundary.md`, `docs/native/{command-map,feature-matrix}.json`, `docs/native/command-map.summary.json`
- `.gitignore`, `package.json`, `AGENTS.md`, `.trellis/config.yaml`, four new `.trellis/spec/*/index.md`
- `MIGRATION_PLAN.md` progress annotations

Verification already executed (commands, exit codes, case counts):

| Command | Exit | Cases |
|---|---|---|
| `cargo check --manifest-path core/Cargo.toml` then `--locked` | 0 / 0 | — |
| `cargo test --manifest-path core/Cargo.toml --locked -p aeroric-protocol --test skeleton` | 0 | 5 |
| `swift build --package-path macos/AeroricKit` | 0 | — |
| `swift test --package-path macos/AeroricKit` | 0 | 12 |
| `node scripts/native/check-patches.mjs` | 0 | 9 patches equal |
| `node scripts/native/inventory.mjs --check` | 0 | 660 / 660 mapped, 38 events, 19 panels |
| `node --test --test-reporter=tap scripts/native/inventory.test.mjs` | 0 | 12 (9 negative) |
| `pnpm native:check skeleton` | 0 | 7 cases, 29 test cases |
| `pnpm native:check nonexistent` | 2 | correctly refused |
| `pnpm native:doctor` | 0 | 18 checks |
| `cargo fmt --manifest-path core/Cargo.toml --all --check` | 0 | — |
| `cargo clippy --manifest-path core/Cargo.toml --locked --all-targets -- -D warnings` | 0 | — |
| `pnpm lint` / `pnpm typecheck` / `pnpm build` / `cargo check --manifest-path src-tauri/Cargo.toml --locked` | 0 / 0 / 0 / 0 | see `docs/native/baseline.md` |
| `pnpm test` | 1 then re-measured | see `docs/native/baseline.md` — 3 failures under full parallel load, 110/110 pass in isolation |

The counts above are the state the review was actually run against, kept verbatim.
The gate has since grown to **20 cases / 92 test cases** (`inv02-core-source-scan`,
`log-redaction-selftest`, `digest-shards-current`, `digest-unit`,
`inventory-freeze-is-noop`, `gate-runner-unit`) and the inventory checker grew from 12
to 22 tests; both snapshots are in `docs/native/baseline.md`. Nothing that was reviewed was removed —
every later case only adds a guard, which is the direction Q2's floor rule is meant
to keep auditable.


Questions the §4.2 prompt would put to the tool, with what local evidence already answers:

1. **Unique owner of data / task / PTY?** Nothing in the skeleton owns any data, task or PTY; `aeroric-core` is empty by design and `aeroric-cli` exits 2. Ownership questions are deferred to P2-02/P3-01 and are **not** claimed as settled here.
2. **Which counterexample overturns the current implementation?** Nine negative inventory tests fail loudly: drop a command mapping, invent a mapping, unknown check module, unknown feature id, unfrozen event channel, orphan event left untriaged, missing settings panel, panel with no native target, drift in the recorded dynamic emit sites. The zero-executed-cases rule also fired once for real and caught a bad test-output parser assumption.
   **One counterexample was found that none of the above catch, and it overturned a claim this review had accepted:** running `--freeze` once rewrote the frozen inventory — deleting all 14 human event `note` annotations and resetting `disposition`/`gap_id` triage to defaults — while `--check` still reported *consistent* in both failing states. The merge preserved a hand-picked whitelist of human keys, so anything else was overwritten by the extractor's own output (which seeds `disposition: null`). Fixed by declaring which keys are *derived from source* and preserving everything else, plus `--freeze-check` (byte-level "the frozen file is exactly what `--freeze` would write", now gate case `inventory-freeze-is-noop`) and 4 merge tests. Two of those tests were initially **false green** because their stub omitted the null placeholders the real extractor emits; a mutation run (restore the whitelist merge) now shows 3 failures, which is what makes them count. Recovery cost was zero only because the artifact had been `cp`-backed up before the destructive run, and the restore was verified by sha256.
3. **Permission / path / token / log / untrusted-data boundaries?** Not yet implemented — protocol auth is P3-06. Only the forward-compat and UTF-8 boundaries are exercised, by the Swift/Rust envelope and frame-splitter tests.
4. **Regression to old Tauri / CLI / four families / storage-remote protocols?** `pnpm lint`, `pnpm typecheck`, `pnpm build` and `cargo check --manifest-path src-tauri/Cargo.toml --locked` all still pass; the command-registration guard is untouched and `inventory.mjs` re-derives 660/660.
5. **Would rollback lose data, double-start a task or change user permissions?** No: §"What is currently safe to abandon" in `docs/native/rollback.md`. P1–P4 output is additive and inert; no runtime path is wired and no user directory was written.

**Unresolved risk that the review must be pointed at**: the 9 `[patch.crates-io]`
entries in `core/Cargo.toml` are all reported `not used in the crate graph`.
`check-patches.mjs` proves path and rev parity but **not** that they resolve, so a
revision drift in `../dbx` or `src-tauri/vendor/*` would stay invisible until
P3-02/P7-04 pull those crates into the graph. Whether that is acceptable for
Phase 1, or a resolution probe should be added now, is a judgement the review
should make rather than this file.

## Actual invocation

**Performed 2026-09-21 — protocol executed; conclusions pending the user.**

| Field | Value |
|---|---|
| Entry point used | `Skill(grilling)` — the delegation target of `grill-me` |
| Why not `/grill-me` | `grill-me/SKILL.md` carries `disable-model-invocation: true`; it is a slash alias whose entire body is `Call the Skill tool with "grilling"`. The pinned chain was therefore entered one hop in, at the skill that actually holds the protocol. This is recorded rather than glossed: it was **not** a self-invented substitute. |
| Making it loadable | The two pinned skill directories were copied from `<project>/.claude/skills/` into `~/.claude/skills/` → symlink → `~/.mirasim/skills/`, the only skill root this CLI reads. **User explicitly approved this global write** after being shown that the project directory is not a load path. Post-copy sha256 of all four files matched §2 exactly; the six pre-existing skills were untouched (verified by mtime + listing). No session restart was needed — the registry hot-loaded `grilling`, which further disproves the earlier "snapshot at session start" theory. |
| Review subject commit | working tree over `ca69a4b`, uncommitted |
| Protocol executed | design-tree / frontier rounds: facts resolved by the agent from source, decisions deferred to the user, whole frontier asked in one round, no action until the user confirms shared understanding |

### Facts the protocol resolved from source (not asked of the user)

| Fact | Method | Result |
|---|---|---|
| Do captured logs hold secrets today? | regex sweep of `.artifacts/native/` (8 files) for password/secret/token/api_key/BEGIN-RSA | none; dir is ignored at `.gitignore:67` |
| Is the purity gate load-bearing yet? | `cargo tree --manifest-path core/Cargo.toml --locked` size | 57 crates resolve; **no** tauri/wry/webview — but nothing in the graph could introduce one either, so the check is currently near-vacuous |
| Are `minCases` floors protected? | grep of `inventory.mjs` for `minCases` | **unguarded** — a floor could be lowered to 1 and the gate would re-green silently |
| How ugly is the proposed RPC surface? | prefix-vs-namespace comparison over 579 proposals | 118 (20%) carry a redundant domain prefix (`git.git_status`, 73 of them `notebook.*`) |
| Can a dynamic command name escape the frozen set? | expand `prefixCommand` (local/remote_/wsl_) over `GIT_MIRRORS`/`LSP_MIRRORS` locals, diff against the 660 registry | **4 generated names are not registered** — see below |

### Finding raised by the review itself (severity: medium, pre-existing, now gated)

`src/lib/api/git.ts:25-34` declares `ssh`/`wsl` mirror entries named `remote_git_checkout_branch`, `wsl_git_checkout_branch`, `remote_git_create_branch`, `wsl_git_create_branch`, but `src-tauri/src/lib.rs` registers none of them. `prefixGitCommand` can therefore produce a command name that does not exist. It is latent, not yet user-visible, because `BranchBar.tsx:64/391` only ever uses `.local`. Registered as **G15** and pinned in `command-map.json → dynamic_invoke_sites[].known_unregistered`, so `inventory --check` now fails if that set changes in either direction (a new phantom name *or* a silently deleted disclosure).

### Frontier put to the user (answers pending — gate stays open)

1. **Q1** RPC naming. **Correction to how this question was first put to the user:** it was
   framed as "keep verbatim, or strip prefixes and **risk collisions**". Verified 2026-09-21:
   **collisions are zero under all three options** (verbatim 0, stripped 0, no-namespace 0),
   because **all 660 legacy command names are already globally unique**. So the collision
   argument was wrong and must not carry any weight in the choice. What actually differs:
   (A) `git.git_status` keeps 1:1 grep-ability with the legacy surface at the cost of 20%
   redundant prefixes; (B) `git.status` reads better and is collision-free **today** but
   guarantees nothing for names added later; (C) `git_status` is collision-free today and
   loses the namespace entirely. Recommendation stands at **(A) plus a frozen
   `legacy_command` field**, now on the honest grounds of traceability rather than safety.
2. **Q2** `minCases` guard — **implemented 2026-09-21** (floors, orphan detection); proved by
   tampering with a floor and watching the gate fail.
3. **Q3** INV-02 source + manifest guards — **implemented**; the `cargo tree` assertion was
   confirmed near-vacuous at skeleton stage. **Hardened after this review, on evidence:** the
   source guard was itself bypassable — with `tauri::AppHandle` inside `core/crates/aeroric-core/src/`,
   one line in a `.gitignore` in that same directory made it exit 1 with empty output, i.e. report
   *no forbidden matches* while the violation was on disk (an emptied scan target does the same).
   Both `rg` guards now pass `--no-ignore`, and every `absent` case must declare a `controls` probe
   per scanned path that has to find content; `forbid` cases must declare `mustContain`. Re-running
   the same attack sample now yields `FAIL (forbidden matches present)` and gate exit 1.
4. **Q4** Log redaction — **implemented** with a self-test that found two leaks on first run.
5. **Q5** Commit to `main` — **still open**, and now backed by an incident: an untracked
   `checks.json` was truncated to 0 bytes and recovered only by reconstruction
   (`docs/native/rollback.md` › Self-inflicted loss).
6. **Q6** Remote/WSL git check-module ownership — **still open**.
7. **Q7 / G17** DSH typed-vs-mux shape — **still open**; note the triage revision moved 42 →
   **41** genuinely unevidenced `dsh_webui::*` commands, since `list_dsh_agent_presets` turned
   out to have test evidence.

**Gate remains `BLOCKED` until the user answers and the resulting decisions are implemented and re-verified.** Recording the questions is not the same as passing the review; §4.2 forbids writing unanswered questions as passes.

Rounds 11–13 added, beyond the cases listed above: `extraction-docs-current` ×3
(database+notebook / remote / dsh_webui — §5.1 row lists must be reproducible from source and agree with
`command-map.json`), `extraction-unit` (9), `invariants-unit` (13, covering INV-09 and patch parity),
and 5 category-derivation negatives inside `inventory-unit`. Round 13 also made this review's own
tooling claims honest in `adr/001-native-boundary.md`: its "`grill-me` unavailable → blocks Phase 1"
and "AGENTS.md native rules deferred" rows were stale and are corrected there.
