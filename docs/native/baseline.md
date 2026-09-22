# Migration baseline — pre-existing behaviour of the Aeroric repo

> `MIGRATION_PLAN.md` Task P1-05 Action 2. Measured on 2026-09-21 against
> `main` @ `ca69a4b` plus the **additive-only** Phase 1 output
> (`core/`, `macos/`, `scripts/native/`, `docs/native/`, two extra `.gitignore`
> whitelist blocks, eight added `native:*` scripts). Pre-existing failures are
> listed as failures; no assertion was weakened to make the migration look green.
> Companion: `docs/native/environment.md`, `docs/native/rollback.md`.

## Commands run from the repo root

| Command | Exit | Result |
|---|---|---|
| `pnpm lint` (`eslint src packages mobile/app mobile/src --max-warnings 0`) | **0** | clean, 0 warnings tolerated |
| `pnpm typecheck` (`tsc --noEmit && pnpm --dir mobile typecheck`) | **0** | both projects clean |
| `pnpm build` (`tsc && vite build`) | **0** | succeeds; pre-existing chunk-size advisory only (`Some chunks are larger than 500 kB`) |
| `cargo check --manifest-path src-tauri/Cargo.toml --locked` | **0** | `Finished dev profile in 1m 11s` |
| `pnpm test` (`vitest run`, full parallel) — run 1 | **1** | `Test Files 2 failed \| 367 passed (369)` — 3 failed tests, **listed below** |
| `pnpm test` (`vitest run`, full parallel) — run 2, identical inputs | **0** | `Test Files 369 passed (369)` — see §"Second full-run confirmation" |

## Pre-existing failures under the full parallel run

| File | Test | Observed |
|---|---|---|
| `src/test/notebook-panel-core.test.tsx` | `NotebookPanel > normalizes notebook title and markdown content to English punctuation` | `AssertionError: expected '第一行：你好。' to be '第一行:你好.'` |
| `src/test/notebook-panel-core.test.tsx` | `NotebookPanel > reorders quick notes after a long-press pointer drag` | failed in the same full run |
| `src/test/notebook-panel-tags.test.tsx` | `NotebookPanel > 标签 > 跨文件重命名 > 开另一个标签的窗时不带着上一次的报告` | failed after **33.2 s** — the test's own duration dominates |

### Classification — these are load-sensitive, not migration regressions

Evidence, in the order it was obtained:

1. `git diff --stat` for tracked files returns only `.gitignore | 18 +` and `package.json | 10 +-`, and the `package.json` diff is eight **added** `native:*` script lines (verified by reading the diff). Nothing under `src/`, `packages/` or `mobile/` was modified, and vitest's scope is those directories.
2. Re-running just the two failing files in isolation:
   `npx vitest run src/test/notebook-panel-core.test.tsx src/test/notebook-panel-tags.test.tsx` →
   `Test Files 2 passed (2)`, `Tests 110 passed (110)`, `Duration 120.41s` with **`tests 182.00s`** against a 120 s wall clock — i.e. the suite's CPU time exceeds its wall time, which is exactly the signature of timing/interaction tests that fail when the machine is saturated by 369 concurrently running files.
3. Therefore the failures are **pre-existing flakiness under full parallel load**, not behaviour introduced by this migration.

Second full `pnpm test` run (to confirm the same three rather than a moving set): result recorded in §"Second full-run confirmation" below.

### Consequences the migration must respect

- `pnpm test` **is not a reliable one-shot gate** on this machine at default parallelism. Any later phase that reports "tests pass" must state which mode produced that result (full parallel vs targeted), or the claim is not reproducible.
- Do not "fix" these by loosening assertions, and do not disable the notebook panel tests. The punctuation-normalisation test in particular may be genuinely stale (CJK punctuation handling), which is a pre-existing product question, not migration fallout — it is therefore **out of scope** for Phase 1 and left failing with this note.
- Owner for re-triage: whoever next touches `src/components/notebook/` (F15, P7-05) or the vitest parallelism configuration.

## Second full-run confirmation

`pnpm test` re-run unchanged, same machine, same commit:

```
second_full_run_exit=0
 Test Files  369 passed (369)
```

This **settles the classification**: run 1's three failures were
load-sensitive flakes, not deterministic failures and not migration regressions.
The same suite passes completely with no code change between the two runs.

Practical consequence for every later phase: a `pnpm test` failure on this
machine must be **re-run before being treated as a regression**; a single red
full-parallel run is not sufficient evidence. Targeted re-runs (as done above
for the two notebook files) are the fastest disambiguation.


## New-surface baseline (`core/`, `macos/`, `scripts/native/`)

Recorded so a later regression is distinguishable from "never existed".

### As first recorded (P1-04)

| Command | Exit | Cases |
|---|---|---|
| `pnpm native:doctor` | 0 | 18 checks satisfied |
| `pnpm native:check skeleton` | 0 | 7 cases run, **29 test cases executed** |
| `pnpm native:check nonexistent` | 2 | correctly refused |
| `pnpm native:inventory --check` | 0 | registered 660 / mapped 660, 26 features, 14 gaps, **38 events, 19 panels** |
| `node --test --test-reporter=tap scripts/native/inventory.test.mjs` | 0 | 12 (9 of them negative tests) |
| `cargo test --manifest-path core/Cargo.toml --locked -p aeroric-protocol --test skeleton` | 0 | 5 |
| `swift test --package-path macos/AeroricKit` | 0 | 12 |
| `cargo fmt --manifest-path core/Cargo.toml --all --check` | 0 | — |
| `cargo clippy --manifest-path core/Cargo.toml --locked --all-targets -- -D warnings` | 0 | — |
| `node scripts/native/check-patches.mjs --min-entries 9` | 0 | 9 patch entries equal to `src-tauri`; an **empty** table now fails instead of printing "0 identical" |

### Refreshed (P1-05 follow-up rounds, same day)

The rows above are kept as the original snapshot rather than overwritten; this is
what the same commands report now.

| Command | Exit | Cases |
|---|---|---|
| `pnpm native:check skeleton` | 0 | 20 cases, **100 test cases**, ~8 s wall |
| `pnpm native:inventory --check` | 0 | 660 / 660 mapped, 26 features, **17 gaps**, 38 events, 19 panels, 660 definitions resolved, 5 dynamic sites, 422 frontend invoke literals, 93 unreachable |
| `node --test --test-reporter=tap scripts/native/inventory.test.mjs` | 0 | 33 (25 negative: 4 freeze-merge, 5 category-derivation, 6 method-surface, rest inventory) |
| `node scripts/native/extraction.mjs --module=database --module=notebook --check …` | 0 | 75 rows / 168 wrappers reproducible; re-deriving corrected two long-standing columns (test items 914→782, pub symbols 586→650) |
| `node scripts/native/extraction.mjs --module=remote --check …` | 0 | 22 rows / 9 wrappers — the §5.1 expansion for `remote/` |
| `node scripts/native/extraction.mjs --module=dsh_webui --check …` | 0 | 9 rows / 66 wrappers — the §5.1 expansion for `dsh_webui` (dir + sibling file)
| `node --test --test-reporter=tap scripts/native/extraction.test.mjs` | 0 | 9, incl. counting-semantics negatives (`#[cfg(test)]` on its own line, field-level `cfg(test)`, parameterised `#[tokio::test(…)]`, impl methods) |
| `node scripts/native/inventory.mjs --freeze-check` | 0 | frozen file is byte-identical to what `--freeze` would write |
| `node --test --test-reporter=tap scripts/native/digest.test.mjs` | 0 | 10 (9 negative, incl. the catch-all head shard) |
| `node --test --test-reporter=tap scripts/native/check-runner.test.mjs` | 0 | 18, incl. five subprocess assertions (empty module, red child, unreadable corpus, genuinely clean corpus, empty purity listing) |
| `node scripts/native/digest.mjs --check` | 0 | 7 shards + index, largest **16 769 B of a 32 768 B injection cap** |
| whitespace sweep over untracked text | 0 problems | 52 files, zero trailing whitespace / EOF blank / CRLF |
| `python3 .trellis/scripts/task.py validate <task>` | 0 | 11/11 native tasks, no truncation warning |

### Two baseline facts this surface depends on

- The Trellis context loader truncates any injected file at
  `context_injection.max_file_bytes = 32768` **silently**. Both frozen digests had
  reached 96% and 94% of that, so they are now cut into shards by
  `scripts/native/digest.mjs`; `--check` refuses a shard spec that would drop a
  canonical key, a shard that no longer matches its source, and a shard over the
  cap. Sharding is a cut, not a summary.
- `docs/native/` is whitelisted out of the repo's `docs/*` ignore rule **by
  extension at any depth**. It used to enumerate subdirectories, which silently
  swallowed `docs/native/extraction/database-notebook.md` (28 496 B) — an
  artifact that existed on one machine only. `git ls-files --others` is the check
  that catches this class; `git status` absence does not.


## CI coverage gap this baseline exposes

`.github/workflows/checks.yml` scopes every Rust job by explicit
`--manifest-path` (`src-tauri`, `remote-protocol`, `remote-relay`) and every JS
lint/format glob to `src`, `packages`, `mobile`. There is **no root
`Cargo.toml`**, so `core/` and `macos/` are not built, tested, linted or audited
by CI at all until `native-macos-checks.yml` exists (P10-04), and the Windows /
Linux core checks are owed by P7-02. Until then `core/` is guarded only by the
local `pnpm native:check` above. Tracked as `docs/native/feature-matrix.json` **G14**.

## Not run in this baseline

- `pnpm format:check`, `pnpm check:design-tokens`, `pnpm check:version`,
  `pnpm check:csp-hazards`, `pnpm test:coverage`, `pnpm audit` — CI-scoped and
  untouched by Phase 1 output; first required at P10-01.
- `cargo test --manifest-path src-tauri/Cargo.toml --locked --lib` — required by
  P2-05's acceptance; `cargo check` was used here to establish compile parity only.
- Any Swift/Xcode build of the app target — the Xcode project does not exist yet
  (`macos/project.yml` is created in P4-01).
