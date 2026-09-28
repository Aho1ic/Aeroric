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


## Full-parallel `cargo test --lib` (src-tauri) — first recorded at P2-02

A Rust-side flake was observed in the same pattern as the TS one, and was **classified** as
load-sensitivity on the same evidence shape (fails under a busy sweep, passes on re-run). The
mechanism offered for it has since been disproved — see the correction below the table. Treat
"classified as load-sensitive" in this section as *undiagnosed*, not as *explained*.

| Test | Fails when | Passes when | Status now |
|---|---|---|---|
| `remote::network_exposure_tests::failed_invite_after_widening_restores_loopback_listener` | full parallel run, 4 added tests, machine busy with concurrent cargo builds (2 consecutive runs) | same code, idle machine (2 consecutive runs, 2412 passed); `--test-threads=1` (2412 passed); run alone or with its own group (`network_exposure`, 9 passed) | cause **unknown**; 47 later runs (alone / group @16 threads / full parallel `--lib`) produced 0 failures; gate case now single-threaded, so it is contained either way |

Mechanism, as originally written here (2026-09-22): `ensure_started` asks
`handle.is_listening()` **once** after restoring the loopback scope, and under scheduling
pressure that read was claimed to land "before the rebound listener reports itself", so the
`else` branch tears the server down and the test's
`expect("server remains available locally after failed invite")` fires. It went on to assert
a product consequence: "a failed pairing-QR request can stop the local remote server on a
loaded machine".

**That mechanism is wrong, and the product claim with it. Disproved by reading the
implementation, 2026-09-25:**

- `is_listening()` is `self.bound_addr.is_some()` (`remote/server.rs:225-226`) — a plain field
  read, not a socket or task-state probe. There is nothing that can "not have reported yet".
- `replace_listener` sets `self.bound_addr = Some(bound_addr)` (`remote/server.rs:320-321`)
  before returning `Ok(())` (`:325`).
- `ensure_listener_scope` (`remote/server.rs:260-290`) can only return `Ok(())` from two
  places: the early return at `:265-267`, whose guard **already requires** `is_listening()`,
  or `replace_listener`'s `Ok` path, which guarantees `bound_addr == Some(_)`. Every failure
  path returns `Err`, and the caller (`remote/mod.rs:859-871`) shuts down and returns before
  reaching the check.

So on the failed-invite path, by the time `remote/mod.rs:874` runs, `is_listening()` is
**necessarily true** — the `else` branch there is unreachable on that path, and the
"loaded machine stops the local remote server" consequence has no support. (The `else` is
still reachable on the invite-*success* path, where no scope restore happens; that is the
"both binds genuinely failed" case its comment describes, and it behaves correctly.)

What is therefore still open: the two failures recorded below have **no established cause**.
The observation itself stands (they happened, and went away on re-run), but "load-sensitive"
was an inference from that pattern, not a diagnosis, and the diagnosis offered here was
falsifiable and turned out false. Since 2026-09-25 the same test has been re-run 20 times
alone, 12 times with its whole `remote::` group at `--test-threads=16` (139 tests per run),
and 15 times inside full default-parallel `--lib` runs — 0 failures. The `--lib` gate case
now runs single-threaded anyway (MIGRATION_PLAN 补记五十六/六十二), so this cannot resurface
as gate noise; if it ever fails again it should be treated as an undiagnosed defect, not
dismissed as load.

Owner note, corrected: **do not take "fix the `is_listening()` race" into P7-01/P7-03 as an
input** — there is no such race. What P7 may legitimately look at is whether a
`running: Option<_>` teardown decision that depends on a field the compiler can already prove
`Some` is worth simplifying, and whether the `else` branch deserves a debug log, since it is
unreachable on one of its two paths.

Attribution evidence kept deliberately boring: the three touched `src-tauri` files were
restored to HEAD content by `cp` from backups and the suite re-run (2408 passed), then put
back (`diff` confirmed byte-identical). No `git checkout`/`stash`/`reset` was used.

Consequence, same rule as the TS section above: a red **full-parallel** `cargo test --lib`
on this machine gets re-run (and, if it persists, narrowed to a group) before anyone may
call it a regression.

## Data-directory file inventory (`~/.aeroric`) — required by ADR-012

`~/.aeroric` on this machine holds 15 GB across ~412k files. The categories below matter
because the migration adds a file the shipping client has never seen, and because
`docs/native/rollback.md` requires backing this directory up before any handover.

| Path | Who writes today | Who writes after P2-05 | May it be deleted? |
|---|---|---|---|
| `projects.json`, `projects/<id>/tasks.json`, `settings.json` | Tauri client (`src-tauri/src/storage.rs`) | core, via `storage/{projects,tasks,settings}.rs` | no — user data |
| `terminal-history/<id>.log`, `events/<id>/events.jsonl`, `agent-homes/**`, `agent-credentials/<id>` | Tauri client | unchanged until their own phase (P3-02/P3-03/P3-04) | no |
| `.write.lock` | nobody today (the client has no cross-process lock) | core only, and only while a writer lives | yes — deleting it does **not** release the lock and must not be treated as a lock |
| `runtime.json` | nobody yet | core only. **Core-exclusive and disposable**: absent or deleted ⇒ rebuilt at `revision: 0`; the shipping client never reads it (asserted by the `legacy-client-ignores-runtime-metadata` gate case) | yes, deliberately |
| `project-groups.json` | nobody yet | core only, and only while a writer lives. **Core-exclusive and disposable**: absent means "no configured groups", which is not an error. It cannot live in `settings.json` because `AppSettings` is a typed struct whose next save drops unknown keys (asserted by the `legacy-client-ignores-core-exclusive-files` gate case) | yes, deliberately |
| `task-operations.json` | nobody yet | core only, and only while a writer lives. **Core-exclusive**: it holds the operation claims (one spawn per attempt, ADR-017), each task's generation counter, and the cut-off-but-not-yet-acknowledged entries (ADR-018). None of the three can live in the task record, because the legacy client's whole-list save of `tasks.json` drops fields it does not model — and a lost claim is a lost spawn-once guarantee | yes, deliberately |
| `<file>.bak` | nobody today | core, one generation per replaced file | user's call; the core keeps exactly the previous version and no more (ADR-011 decision 3) |
| `.<name>.<pid>-<ns>.tmp` | both writers (same spelling on purpose) | swept only by a lease holder, older than 24 h (ADR-010 decision 8) | swept, not by hand |
| `backups/` | **no code writes or reads it** (verified: `rg` over `src-tauri/src` finds no reference) | unchanged | out of scope; a user-created directory, not a product feature |

`docs/native/reviews/storage.md` records what was measured for each row.

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

## P2-04 复核记录（2026-09-23）

| 项 | 实测 |
| --- | --- |
| 门禁 | `tasks` **7 case／105 用例**；回归 `storage` 5／183、`projects` 4／18、`protocol` 5／88、`skeleton` 20／113；`doctor` 18/18 |
| 全量 | `pnpm test` **373 files 全绿，exit 0**；`cargo test --manifest-path src-tauri/Cargo.toml --locked --lib` **2412 passed**；`cargo test -p aeroric-core` = 114 单测 + 16 + 8 + 4 + 10 + 27 + 4 |
| 静态 | `clippy --all-targets --workspace -- -D warnings` 0 错；`cargo fmt --check` 0 diff；`pnpm typecheck` 0 错；`git diff --check` 干净 |
| parity | `contracts/native/v1/fixtures/task-transitions.json` **283 条**，期望由旧客户端 TypeScript 亲自算出（`src/test/task-core-parity.test.ts`），Rust `tests/task_parity.rs` 逐条重放，**0 条声明分歧**；产物与知识表内容一致由专门断言守着（`contracts.mjs --check` 只比字节，跨渲染会误报，故内容断言在 Rust 侧） |
| 真实 `~/.aeroric` | 目录树 411,846 个文件里 **`runtime.json` / `project-groups.json` / `task-operations.json` 一个都不存在** —— 这才是可复查的口径：core 从未写过真实数据目录。2 秒窗口内另有两处漂移（`usage-statistics.sqlite3` 与一个 terminal-history 日志），是产品自身在写；证据 `.artifacts/native/real-home-p204-window.json`。**不写"真实目录零漂移"**：产品在本机常在写，任何零漂移说法都必须先说明是"core 没写"而不是"目录没动" |
| 反例 | 9 次（M1–M9）：改坏实现 → 门禁必须红 → 还原并逐文件 `cmp` 确认字节一致。M7/M8 各自钉住一条设计选择，见 `reviews/tasks.md` §5 |
| 自曝 | M4 首次跑时忘了备份 `operations.rs`，改坏后无备份可还原，只能逐字符反向替换 + 当场验证 8/8 转绿。教训：**反例脚本必须先 cp 备份、后改文件**，备份清单要和被改文件清单对得上 |

## Native gate baseline — measured 2026-09-28 (MIGRATION_PLAN 补记一百十五)

All nine `native:check` modules were run on this machine the same day, and the case counts below
are what the runner printed (`ok (N cases, min M)`), not prose carried forward. **Every floor now
equals its measured count** — five had been sitting below (`protocol-rust` 50/49,
`protocol-swift-golden` 20/19, `protocol-ts-golden` 21/20, `tasks-idempotency-rust` 10/8,
`adapter-acceptance-real-kernel` 19/18), i.e. deleting one test at those five sites would not have
gone red. Each raise was preceded by a platform-stability check: `task_idempotency.rs` has zero
`cfg(`, `aeroric-protocol` has zero platform `cfg`, and `core_adapter_tests.rs`' only cfg is a
`#[cfg(unix)] { chmod }` **inside a helper body**, not on a `#[test]`, so the counts are identical
on every target. If a genuinely platform-gated test ever appears, raise per-platform floors or assert
test-name sets (P7-01) — never lower a floor to match.

| Module | cases / executed |
|---|---|
| `skeleton` | 22 / 142 |
| `protocol` | 6 / 98 |
| `storage` | 5 / 361 |
| `projects` | 5 / 21 |
| `tasks` | 7 / 151 |
| `tauri-adapter` | 12 / 2526 (incl. `src-tauri-lib-tests` 2465, serial) |
| `runtime` | 1 / 5 |
| `agents` | 1 / 2 |
| `terminal-core` | 13 / 107 |

Static and non-gated surfaces, same day: `pnpm typecheck` exit 0; `cargo fmt --check` exit 0 for
both `core` (with `--all`) and `src-tauri` (without `--all`, per the gate's own argv);
`cargo clippy --all-targets --locked -- -D warnings` exit 0 for both workspaces;
`pnpm native:inventory --check` consistent (`taskStates=51/12accepted`), `--freeze-check` a byte
noop; `pnpm native:contracts --check` 12 documents byte-exact; `pnpm native:digest --check` 7 shards;
`pnpm native:doctor` 18/18; `git diff --check` clean.

**Full `pnpm test` (376 files / 5483 tests, exit 0, 355.87 s wall)** was run deliberately here
because **no gate case executes it** (finding F2, owned by P10-05): a TS test that no module names is
invisible to every `native:check`. This is the fourth full-suite run recorded in this file, and it
matches the P2-05-era numbers (376 files / 5483 tests) — the migration's host-side reader and gate
changes have not moved the frontend suite.
