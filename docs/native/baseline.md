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

> **Refuted 2026-09-29 (ledger `G35`) — row 14 above no longer holds.** Re-running the same command today
> gives **rc 1**: one warning, `src/test/native-host-protocol.test.ts:36` `type Row = Record<string, any>`
> (`@typescript-eslint/no-explicit-any`), and `pnpm format:check` is also **rc 1** across **17 files**.
> Attribution was done per file against `origin/main` (`ca69a4b`): 11 of the 17 do not exist there, 6 were
> Prettier-clean there, so **all 17 came from this initiative's 119 unpushed commits** — i.e. a genuine
> post-baseline regression, not inherited debt, and this table's row 14 is now stale evidence that must not
> be quoted as "lint is clean". `pnpm build` (row 16) was re-run today and is still 0. The rows are kept
> verbatim because they record what P1-05 accepted; this note is the correction. No native gate runs either
> command (`grep -c "pnpm lint\|format:check" scripts/native/checks.json` = 0), so gate green says nothing here.

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
noop; `pnpm native:contracts --check` 12 documents byte-exact; `pnpm native:digest --check` 7 shards —— **就地更正（补记一百二十三）**：这个别名在 `package.json` 里已经带了 `--write`，所以它展开成 `digest.mjs --write --check`，**一直在写而不在查**，`wrote 7 shard(s)` + exit 0 不是验证证据。当天真正的验证是直接调用 `node scripts/native/digest.mjs --check`；CLI 现在拒绝同时给两个模式（exit 2）。
`pnpm native:doctor` 18/18; `git diff --check` clean.

### Digest shards re-cut the same day (补记一百二十三)

`feature-matrix-gaps.json` reached **31 516 B = 96%** of the 32 768 B injection cap, so `digest.mjs`
grew array chunking: the gaps array now ships as `feature-matrix-gaps-1.json` (16.25 KB) and
`-2.json` (14.76 KB), 8 + 9 = 17 rows, order and content byte-identical to the canonical array.
Largest shard is back to **20 827 B (64%)** across 8 shards.

| command | exit | result |
|---|---|---|
| `node scripts/native/digest.mjs --check` | 0 | `verified 8 shard(s), largest 20827 B` |
| `node --test scripts/native/digest.test.mjs` | 0 | 17 pass / 0 fail (was 10) |
| `pnpm native:check skeleton` | 0 | 22 cases / 149 test cases, with `digest-unit` floor raised to 17 in the same batch |
| `pnpm native:digest --check` | 2 | refuses `--write --check` and prints the non-writing command |

Two guards were added because the chunking made them load-bearing: a shard the plan no longer
emits is reported (an injection list points at files **by path**, so an orphan shard keeps feeding
stale content forever), and the reconstruct-the-canonical assertion now reads the **shard files on
disk** instead of the planned payloads — the previous version was blind to a hand-edited shard, which
the new negative test caught on its first run. `.trellis/.../check.jsonl` was repointed at both
halves, and `task.py validate` was confirmed to fail on a missing referenced file.


**Full `pnpm test` (376 files / 5483 tests, exit 0, 355.87 s wall)** was run deliberately here
because **no gate case executes it** (finding F2, owned by P10-05): a TS test that no module names is
invisible to every `native:check`. This is the fourth full-suite run recorded in this file, and it
matches the P2-05-era numbers (376 files / 5483 tests) — the migration's host-side reader and gate
changes have not moved the frontend suite.

### Host tests write the real `~/.aeroric` (G19 / G20, measured 2026-09-28)

§3.1 #7 says tests must not modify the real `~/.aeroric`. One paired before/after around a single
gate run (`cargo test --manifest-path src-tauri/Cargo.toml --lib`, rc=0, 2465 passed, 87.38 s) shows
it does, in two places:

| carrier | before | after | note |
|---|---|---|---|
| `~/.aeroric/remote-audit.log` `invite-created` count | 1960 | **1963** | three lines at `05:43:32.740/.744/.770`, file +177 B |
| `~/.aeroric/agent-credentials/liwan` mtime | 13:37:48 | **13:43:12** | `muyuan` and `agentrouter` rewritten in the same window, all 51 B |

Three idle controls (90 s, 25 s, 7 min — the desktop app pid 50882 running the whole time) moved
**nothing**, so the writer is the test process, not the app. Scale of the audit-log pollution: 1963
`invite-created` rows versus 6 `device-paired`, 4 `device-revoked`, 114 `auth-ok`, and **641**
clusters of ≥3 rows within 0.5 s going back to 08-04 — roughly 641 test runs. That file rotates at
1 MB (`remote/audit.rs:11-20`), so real events get pushed out.

Mechanism, read from code rather than guessed: `load_settings_unlocked()` calls
`refresh_stale_codex_agent_scripts` / `refresh_stale_claude_agent_scripts`
(`app_settings/load_save.rs:449-450`), which call `sync_agent_credentials(&profile.id, …)`
(`agent_scripts.rs:2026` / `:2091`) against `agent_api_key_path()` → `aeroric_dir()`
(`load_save.rs:48-53`). **Loading settings therefore has a write side effect**, so any test that
reaches that path without a scratch home can rewrite real key files. **The post-run content is not the user's key.** Three tests in `app_settings/agent_scripts.rs` use this machine's *real*
profile ids as fixtures — `refreshes_stale_codex_agent_scripts_to_chat_bridge` (:3087, id `liwan`),
`resets_codex_chat_bridge_wrappers_when_setting_is_off` (:3130, id `muyuan`), `refreshes_stale_claude_agent_scripts`
(:3222, id `agentrouter`) — and put only the *script* in `std::env::temp_dir()`, while the sidecar path is derived from
the id. Each gate run therefore overwrites three real API-key files with the literal `sk-test` (51 B → 7 B); something
later in the same session writes the real key back (both sampled at 13:52 and 14:15 as real values), but that recovery
depends on someone loading the real settings again afterwards.

Bisection evidence, all measured the same day against the already-built test binary: `--lib app_settings` (161 tests,
4.36 s) moved the sidecars, `--lib app_settings::agent_scripts` (47, 2.35 s) moved them, the other five sub-modules did
nothing, and one test alone — `refreshes_stale_codex_agent_scripts_to_chat_bridge` — turned `liwan` into `sk-test`.
`stat` showed **inode birth == mtime** with `agent-credentials/` itself touched at the same second, i.e. a real
`atomic_write_private` replacement, not a metadata touch. The `invite-created` half has its own minimal repro too:
`--lib network_exposure` (9 tests, **0.07 s**) appends **+3** rows, while the same session's `runtime`/`agents`/
`terminal-core` suites appended none.

Recovery: `~/.aeroric/settings.json` (mtime 09:59, never written by the tests) still holds all three 51 B keys, so the
clobber is repairable byte-exactly — and was, twice: once by whatever restores it inside a run, and once by this session
after the single-test repro (`sha256` prefixes back to `964fe3db…`/`6032c79e…`/`5038c677…`, mode 0600, no trailing newline).

| command | exit | result |
|---|---|---|
| `node scripts/native/inventory.mjs --check` | 0 | consistent, `gaps=19` (was 17) |
| `node scripts/native/digest.mjs --check` before `--write` | 1 | 8 PROBLEM incl. stale shards + disk reconstruction |
| `node scripts/native/digest.mjs --check` after `--write` | 0 | verified 8 shards, largest 21199 B (65%) — 19 gaps fit without touching the cap (re-measured after `G21`: 20 gaps, largest 21926 B = 67%) |
| `pnpm native:check skeleton` | 0 | 22 cases / **160** test cases |

Adoption figure for the fix: `install_scratch_home` appears **17** times in `src-tauri/src` against
**2479** `#[test]` / `#[tokio::test]` attributes. Do not "fix" this by exporting `HOME=/tmp/...`:
measured the same day, `HOME=/tmp/fakehome cargo test` exits 1 with `error: rustup could not choose a
version of cargo to run, because one wasn't specified explicitly, and no default is configured` — the
fake home gets a fresh empty `~/.rustup/settings.toml`, so the run is invalid rather than clean (it
also proves the control cannot be salvaged by checking real-home mtimes afterwards).

### Census of the whole host suite (measured 2026-09-28)

Method, recorded so nobody needs a throwaway script: split the 2465 `--lib` tests by top-level module
(82 groups), run each group as a filtered run against the already-built binary, and take a
`(mtime_ns, size)` snapshot of `~/.aeroric` depth-1 files/dirs plus `agent-credentials/*` and `agents/*`
immediately before and after each group. A **20 s idle control run first (desktop app live, pid 50882)
produced 0 changes**, so every recorded change is test-attributable.

Result: **13 of 82 groups write the real `~/.aeroric`**, across four carriers.

**就地更正（同日晚些，纯静态复核）**：上面这张表的**模块归属**不可信，因为 `cargo test --lib -- <name>`
的过滤是**整条测试路径的子串匹配**，不是顶层模块选择。用 `-- --list --format terse` 取出全部 2465 条真名后逐组比对
「自有条数」与「过滤器实际命中数」：82 组里 **28 组过匹配，合计 3 583 次多余执行**。最夸张的是 `tests` 组：自有 15 条，
但 `-- tests` 命中 **2465 条（＝整套套件）**，因为几乎每条路径都含 `::tests::`——所以「`tests` 只有 15 条测试却写满四类载体」
这个说法的真正含义是「**跑完整 `--lib` 会同时改这四类载体**」，而它本来就是 `src-tauri-lib-tests` 这条门禁 case 每个门禁周期都在做的事。
受影响行：`tests`(15→2465)、`remote`(98→309)、`config`(10→94)、`ssh`(64→82)、`pty`(45→111)。**归因仍然成立的七组**（自有 == 命中）：
agent_assist(13)、custom_themes(10)、node_runtime(1)、notification(11)、remote_git(9)、remote_os(18)、wsl_git(11)；
凭据那两条独立证据不受影响——`app_settings`(161==161) 是干净组，且单条测试 `refreshes_stale_codex_agent_scripts_to_chat_bridge`
的 `sk-test` 现场与 inode birth==mtime 都是当场抓到的。审计那条也独立成立（`remote/mod.rs:831` 的调用链 + 每轮恰好 +3）。
**规则**：按模块过滤宿主测试一律写 `-- <module>::`，并把 cargo 报的实际执行条数与「该模块自有条数」一起记进证据——
两数不等就是归因失效信号。今天已注册的两条 `--lib` 过滤 case 检查过了：`terminal::` 命中 65 == floor 65，
`terminal::history` 命中 7 == floor 7，都是精确的（尾缀 `::` 正是它们不过匹配的原因）。

| carrier | groups that wrote it | note |
|---|---|---|
| `usage-statistics.sqlite3` (+ `-wal`/`-shm`) | agent_assist, custom_themes, node_runtime, notification, remote_git, remote_os, ssh, wsl_git, tests | 79 122 432 B real DB; mtime advanced, size held |
| `agent-credentials/{liwan,muyuan,agentrouter}` | app_settings, config, tests | caught **at 7 B `sk-test` mid-census (14:39:14)**, back to 51 B real values after the run |
| `remote-audit.log` | remote, tests | +3 `invite-created` each (1969→1972→1975) |
| `terminal-history/` (151 entries) | remote, tests | new carrier, registered as `G21` |

`pty` touched only the `agent-credentials/` **directory** mtime with no depth-1 file appearing or
disappearing — unexplained, noted rather than guessed. The self-restoration seen across the census is
the strongest form of the `G20` risk: the clobber really happens, and its repair depends on some later
group happening to load the real settings, not on any guarantee.

### How large is the real-home write surface (measured 2026-09-28)

The census above answers "what did the suite actually touch". The separate, larger question — "what *could*
it touch" — was measured statically (no test run, no write): `storage::aeroric_dir()` has **51 call sites in
23 files** under `src-tauri/src`, expanding to **28 depth-1 literal targets**, at least five of which hold
secrets (`ssh-passwords.json`, `storage-secrets.json`, `remote-keypair.json`, `agent-credentials/*`,
`settings.json`'s per-profile api keys). One line-continuation case (`remote/audit.rs:15-16` →
`remote-audit.log` plus its `.1` rotation) is invisible to same-line extraction, so the true count is higher.

Because the census snapshotted **all** depth-1 entries, the other 24 targets can positively be reported as
untouched by the suite today; the blind spot is depth ≥ 2 inside directories, and only `agent-credentials/*`
and `agents/*` were drilled into (`terminal-history/` surfaced as its entry count changing). Registered as
**G24** (`P3-06, P8-03`), whose point is that the fix is **one injection point, not 51 patches**: give
`aeroric_dir()` an app-owned override (`HOME=` redirection is invalid — rustup then exits 1), and reuse the
existing `platform::install_scratch_home` (already referenced 17 times). `local_router_commands.rs:40`
already takes an explicit `data_dir`, which proves the shape is available rather than blocked by design.


### Injection payload budget (measured 2026-09-28)

`.trellis/scripts/common/config.py` defaults `context_injection` to `max_file_bytes = 32768`,
`max_artifact_bytes = 65536`, `max_total_bytes = 131072`, and `.trellis/config.yaml` keeps the whole
block **commented out**, so the defaults are what apply. Two different kinds of fact follow:

- `max_file_bytes` is the only limit anything in this repo reads, and only to print a **non-blocking
  warning** in `task.py validate` ("injection will truncate it"); `validate` still exits 0. The hard
  per-file enforcement here is therefore our own `digest.mjs`, which refuses red above 32768 B per
  shard. Every phase-3 referenced file stays under it by construction — read current sizes from
  `docs/native/digest/index.json` or `node scripts/native/digest.mjs --check` rather than from prose,
  and note the chunk boundaries are **size-driven**, so which gap ids land in which shard changes
  whenever the ledger grows (that is why `check.jsonl` must be re-checked after every re-cut).
- `max_total_bytes` is **not consumed anywhere under `.trellis/scripts`** (grep finds only its
  definition and the config docstring). Yet the payload the phase-3 lists point at already exceeds that
  documented total: summing the sizes of the files *referenced by* the two lists gives
  `implement.jsonl` 67 118 B (8 entries) + `check.jsonl` 106 343 B (6 entries) = **173 461 B = 132.3%** of
  131 072 B, measured 2026-09-28 — up from 117.7% the same day, because 补记一百四十二 added a third gap
  shard to `check.jsonl`. Note the two `.jsonl` files themselves are tiny (2 453 B together: they hold
  *paths*, not bodies), so `wc -c` on them is the wrong measurement — resolve each `file` entry and sum.
  Whether the external injector
  enforces it is **unverifiable from inside this repo** — if it does, overflow "degrades to index
  lines", i.e. a sub-agent could receive file names instead of the gap register it must close. The first
  phase-3 sub-agent run (P3-04) should record whether it got bodies or index lines before anyone adds
  more injected files.

**Re-measured 2026-09-29 (补记二百一十五 / 二百一十八; ledger `G36`) — the total above is now wrong in the
other direction, and the mechanism claim in this section held up.** Same instrument (resolve each `file`
entry, sum real sizes): `implement.jsonl` **79 830 B (8 entries)** + `check.jsonl` **49 496 B (4 entries)**
= **129 326 B = 98.7%** of 131 072 B. It fell back under the cap because `check.jsonl` went from 6 entries
to 4, not because anything was fixed — headroom is now only **1 746 B**, so one more injected file of any
real size crosses it. Unique referenced files today = **12**, the two lists are disjoint, and the largest
single item is `docs/native/extraction/sessions.md` at 25 883 B (still under `max_file_bytes`, so even the
non-blocking warning does not fire; `task.py validate` today printed no warnings).
The grep was redone at line level and confirms what this section already asserted: `max_total_bytes` appears
only in `config.py:286/297/306` (definition + docstring); the sole consumer of `get_context_injection_limits()`
is `task_context.py:252`, which reads **`max_file_bytes` only**, and the oversize branch at
`task_context.py:299-307` prints a warning without incrementing `errors` — so `validate` exits 0 regardless.
`max_artifact_bytes` likewise has no consumer. **Consequence unchanged and sharpened:** whether overflow
actually "degrades to index lines" is not decidable from this repo, therefore the P3-04 prerequisite
(record bodies vs index lines on the first sub-agent run) is the only available observation, and no
`native:check` green status speaks to it.

**Re-measured again 2026-09-29 after the 5th and 6th gaps re-cuts (5 → 6, then 6 → 7 shards; ledger `G37` and the full-ledger re-verification in 补记二百二十六).** Same instrument
(resolve each entry's `file` field, sum real sizes): `implement.jsonl` **79 830 B (8 entries)** — byte-identical
to the line above — plus `check.jsonl` **49 722 B (4 entries)** = **129 552 B = 98.8%**, headroom **1 520 B**.
The whole movement across both re-cuts is **+226 B**, and it came from one file: `docs/native/digest/index.json`
grew a row for `feature-matrix-gaps-6.json` and then one for `feature-matrix-gaps-7.json`. **Measured reason the
re-cuts cost almost nothing here: neither list references any `feature-matrix-gaps-*.json`** (the only ledger shard
injected is `feature-matrix-features.json`), so `baseline.md`'s own instruction that "`check.jsonl` must be
re-checked after every re-cut" is satisfied by *verifying* that — the gaps shards only enter the payload if a
future list names one. Also measured: injecting digest files by resolved `file` entries gives **12 unique
referenced files**, lists still disjoint.

### Instruction-surface drift witness (measured 2026-09-28)

`AGENTS.md` and `.trellis/spec/**` are gitignored, so `git status` cannot witness them (already a known trap —
`G22`/`G23` live there). Today that became concrete rather than theoretical: session notices arrived claiming
the user had rewritten `AGENTS.md:31` (to "does not exist yet; P4-03 creates it") and four
`.trellis/spec/*/…/index.md` files (to a "status + pointer" form). **Neither edit exists in this workspace** —
the links are still there verbatim at line 24 of all four indexes with three still labelled `Defined`, and
`AGENTS.md:31` still names `NativeSelection.swift` in the present tense. The only witness available for
gitignored text is content + mtime, so it is recorded here:

| file | mtime (local) | sha256 (first 16) |
|---|---|---|
| `AGENTS.md` | 2026-09-21 16:01:40 | `540d5a67b5991165` |
| `.trellis/spec/aeroric-core/backend/index.md` | 2026-09-21 15:58:16 | `54b34cb039c18f40` |
| `.trellis/spec/aeroric-desktop/backend/index.md` | 2026-09-21 15:58:16 | `505964c8e8dfb265` |
| `.trellis/spec/aeroric-macos/frontend/index.md` | 2026-09-21 15:58:16 | `997f4d3f7d5b7148` |
| `.trellis/spec/aeroric-web/frontend/index.md` | 2026-09-21 15:58:16 | `fff4270755d5f356` |

Re-check with: `for f in AGENTS.md .trellis/spec/*/*/index.md; do stat -f "%Sm %N" -t "%F %T" "$f"; shasum -a 256 "$f"; done`
(9 spec indexes exist; diff the five hashes above against this table). A hash change with no record in
`MIGRATION_PLAN.md` means something
else edited the instruction surfaces. This repo sits under `~/Downloads/同步空间/`, an iCloud-backed sync folder
(`com.apple.CloudDocs.iCloudDriveFileProvider` is running), so a second writer on another device is a live
possibility rather than an exotic one — it is **not proven**, but any claim of the form "the workspace is the
only writer" has to survive this table. Related consequence for this plan: `G22`/`G23` are still **open** here,
and the drafted fix text quoted above is recorded in 补记一百四十八 as an approvable proposal, not as an applied
change (those files are user-ratified text; see decision items 7 and 9).


### Which gate cases survive an isolated clone (measured 2026-09-28)

`scripts/native/checks.json` registers **72 cases**, of which **25 have `argv[0] === "cargo"`** — 21
through `--manifest-path core/Cargo.toml`, 4 through `src-tauri/Cargo.toml`. A naive substring count
returns 26 because `skeleton::inv02-core-source-scan` only says "cargo" in prose: count the argv, not
the JSON.

All 25 need the sibling `../dbx` checkout, because both workspaces path-depend on it
(`core/Cargo.toml:40-41` → `../../dbx/vendor/{tiberius,pageant}`; `src-tauri/Cargo.toml:116` →
`dbx-core`, `:152-153` → the same two vendor crates). Cargo resolves that
`<manifestDir>/../../dbx`, i.e. **`<repoParent>/dbx`**, and the path dependency is not constrained by
`Cargo.lock`. Measured in a `git archive HEAD` copy at `/tmp/nodbx` (so `repoParent` was `/private/tmp`):

| command in the isolated copy | exit | result |
|---|---|---|
| `cargo metadata --manifest-path core/Cargo.toml --locked` | 101 | `Unable to update /private/tmp/dbx/vendor/pageant/Cargo.toml … No such file (os error 2)` |
| `cargo metadata --manifest-path src-tauri/Cargo.toml --locked` | 101 | same pageant error |
| `node scripts/native/contracts.mjs --check` | 1 | `Command failed: cargo run --quiet --manifest-path core/Cargo.toml --locked -p aeroric-protocol --bin export-schema` |
| `node scripts/native/inventory.mjs --check` | 0 | consistent at HEAD (`gaps=17 registered=660 mapped=660`) |
| `node scripts/native/digest.mjs --check` | 0 | verified (HEAD shard set) |
| `extraction.mjs --check` (database-notebook, sessions) | 0 | both portable, no cargo |
| `node --test scripts/native/digest.test.mjs` | 0 | `# pass 10` — HEAD's count, which is how a replay tree is recognisable |

This is **not a CI gap**: `.github/workflows/checks.yml:35-53` and `release-desktop.yml` both run a
"Checkout dbx dependency" step (`git init ../dbx` → `fetch --depth 1 https://github.com/t8y2/dbx.git
$DBX_REF` → `checkout --detach`, ref from `scripts/dbx-ref.txt`) followed by `./scripts/prepare-dbx.sh`.
What the measurement does constrain is everything *outside* those workflows:

- A **sibling** copy/worktree (`…/LYX/Aeroric-x`) still resolves the real `…/LYX/dbx` and runs the full
  matrix; a **nested** one (`.worktrees/<project>/<name>`, `../.zcf/<proj>/…`) resolves a non-existent
  path and loses all 25 cargo cases plus `contracts`. Layout choice, not tooling choice. This is not
  hypothetical here: `git worktree list` shows `Aeroric/.worktrees/arch-debt` and
  `Aeroric/.worktrees/review-fixes` registered today, i.e. two nested trees where the matrix cannot run.
- `scripts/prepare-dbx.sh` **does not clone** — read from source, it exits 1 with "Clone the pinned DBX
  dependency first" when `$dbx_dir/.git` is missing, and its `DBX_DIR` override only relocates the
  *patch target*, not what cargo resolves. So `DBX_DIR=/somewhere ./scripts/prepare-dbx.sh` can succeed
  while the build still looks at `<repoParent>/dbx`. (Not executed: this checkout's `../dbx` carries the
  user's ~38 dirty files and is pinned as untouchable.)
- The replay recipe used for commit-readiness checks therefore has to reproduce those four CI lines, or
  run only the JS set — which is exactly the split the table above measures.

### Every unregistered module name refuses loudly (probed 2026-09-28)

`checks.json`'s `$comment` asserts "check.mjs must fail on any of these until it exists". That claim was
previously an inference; it is now measured per name. `plannedModules` holds **43 keys, one per Task** from
P2-04 to P10-05 (so "38/43 planned modules are not gate-ready" is the correct denominator: 43 minus the 5
already registered — `tasks`, `tauri-adapter`, `runtime`, `agents`, `terminal-core`, which stay listed
deliberately as the audit trail).

| probe | count | result |
|---|---|---|
| planned-but-unregistered name | 37 | exit **2** + `Refusing to treat an unmapped module as success` every time |
| `all` (also a planned key, owner P10-05) | 1 | exit **3** + the NOT READY list; a different refusal by design, not a pass |
| typo of a registered name (`skelton`) | 1 | exit **2** — an unmapped string can never be mistaken for a green module |

`check.mjs:83-97` also closes the collision that this layout invites: `module === "all"` is intercepted
*before* the registry lookup, and when the aggregate does run, the set is derived from
`Object.keys(config.modules)` **excluding** a module literally named `all` — so P10-05 registering a module
called `all` cannot shadow the aggregate mode. Verified by reading, plus the fact that the pre-fix shape
(`config.modules["all"]`, which does not exist) is documented in those lines as the bug that was removed.
No hole found; recorded because "the gate refuses" is the kind of claim that must be probed per name.

### §5.1 inventory vs Task `Input` coverage (audited 2026-09-28)

All 9 `docs/native/extraction/*.md` tables were parsed and compared against the `- **Input**：` line of all
51 `### Task` sections, after expanding brace lists (`session/{parse,approval,export}.rs` → 3 paths). Two
table shapes exist and must not be conflated: **file-per-row migration inventories** — `sessions.md` 11,
`dsh-webui.md` 9, `remote.md` 22, `database-notebook.md` 75 = **117 rows** whose second column is a source
path — and **behavior-citation tables** — `task-lifecycle-react.md` 65 rows whose second column is a
behaviour and third column is `path:line` (21 distinct paths), plus `agents-launch.md` in that latter style.
Two-sided controls were required: the first matcher was too loose (an empty path prefix matched everything →
0 uncovered, vacuous) and the second was too strict (directory-level Inputs such as P8-03's
`src-tauri/src/remote/`, P7-04's `database/` and P7-05/06's `notebook/` did not register as covering their
children → 69 phantom "defects"). With both fixed:

| inventory | rows | uncovered | verdict |
|---|---|---|---|
| `sessions.md` | 11 | 1 | `agent_usage.rs` is named by **P8-04's** Input, not P3-04's, while this row list carries it — third carrier of the open Q1 disagreement |
| `dsh-webui.md` | 9 | 1 | **`dsh_webui/build_readiness.rs` (335 lines) appears in 0 places in the plan** — 7 of the 8 files in that directory are in P3-05's brace list, this one is not |
| `remote.md` | 22 | 0 | P8-03 claims the whole `src-tauri/src/remote/` directory; P3-03/P3-06 name 3 more children |
| `database-notebook.md` | 75 | 0 | covered at directory level by P7-04 (`database/`) and P7-05/P7-06 (`notebook/`) |
| `task-lifecycle-react.md` | 65 (21 paths) | 5 | **not a defect**: it cites where a behaviour lives today, and the 5 unmatched paths are host-shell files (`src/App.tsx`, `src/AppOpsProvider.tsx`, `src/appProjectState.ts`) plus `src/taskId.ts` / `src/sessionHandoffPrompt.ts` — a citation table is allowed to point at files no Task moves |

The `build_readiness.rs` case is the actionable one and it is the F-family shape exactly: a **gated** artifact
(`extraction-docs-current-dsh-webui` byte-compares that table against the source tree) commits to migrating a
file, while the plan text never names it, so an implementer obeying §3.1 #1 ("only touch the files this Task
lists") would skip it and the gate would stay green. Fixing it either way is a plan-scope edit, so it is
raised as decision item 13 (add `build_readiness` to P3-05's `Input` list, or de-scope the inventory row).

### The §5.1 `destination` column is not a usable path (audited 2026-09-28)

Following the same thread one step further: all **117** rows in the four file-per-row inventories put their
destination at `core/crates/aeroric-core/<module>/<file>.rs` — **none under `src/`** — while every module the
migration has already landed lives at `core/crates/aeroric-core/src/{agents,projects,runtime,storage,tasks,terminal}`.
`scripts/native/extraction.mjs:116` is the cause: it templates the column as
`core/crates/aeroric-core/${moduleName}/${underModule}`, dropping `src/` and reusing the host directory name.

The vocabulary disagrees with the Tasks too, which matters more than the prefix:

| inventory | rows | destination says | the Task's own Action says |
|---|---|---|---|
| `sessions.md` | 11 | `session/session-root.rs`, `session_dsh/session_dsh-root.rs`, … (one file per source file) | P3-04: `src/sessions/{mod,discovery,registry,parser,claude,codex,dsh,omp,history,resume,export}.rs` |
| `dsh-webui.md` | 9 | `dsh_webui/*.rs` | P3-05: `src/agents/dsh_host/{mod,startup,client,dispatcher,state,approvals,questions,attachments,projection}.rs` |

`find core/crates/aeroric-core/src -name '*root*.rs'` returns nothing — real names are `mod.rs` plus
responsibility files (`service.rs`, `lease.rs`, `replay.rs`). Registered as **G25** (`P3-04, P10-05`); adding
that row pushed the ledger's largest digest shard to 23 342 B = 71% of the 32 768 B cap.
The gate cannot see any of this: `extraction.mjs:200` compares destinations against the same template, so it
**freezes** the convention instead of validating it. Changing the column means regenerating four
gate-compared documents, which is why it is decision item 14 rather than a drive-by edit. **P3-04 is next, so
this must be settled before its first slice**: creating directories from the inventory would contradict the
Task's own Action list while every gate stays green.

### Five of nine §5.1 artifacts have no freshness guard (audited 2026-09-28)

Pulling on the same thread a third time asked who, if anyone, watches the line-number citations inside these
documents. Measured from `checks.json`: only **4** of the 9 files under `docs/native/extraction/` are compared
by any case (`database-notebook.md`, `dsh-webui.md`, `remote.md`, `sessions.md`). The five unguarded ones are
`task-lifecycle-react.md` (255 path citations — the largest set), `event-topics-pending.md` (55),
`terminal-stream.md` (36), `runtime-ports.md` (25) and `agents-launch.md` (16).

A full sweep of all 678 `path[:line[-line]]` citations in the directory — resolving each against four roots
(repo root, `src-tauri/`, `src-tauri/src/`, `src/`) — classified them as: **291 resolvable and in range**,
186 file-only, **9 out of range**, and 192 that are *not* defects once categorised (bare filenames whose row
already carries the full path, `destination` paths that don't exist yet — see G25 — and prose fragments like
`.js/.ts`). Registered as **G26** (`P3-04, P10-05`).

The 9 out-of-range hits are 4 distinct citations, all inside the unguarded `task-lifecycle-react.md`:

| citation | file length today | symbol's real line today |
|---|---|---|
| `src/appProjectState.ts:212-214` (×5) | 189 lines | `shouldIgnoreTaskStatusTransition` at **:183** |
| `src/appProjectState.ts:216-218` (×2) | 189 lines | `isLiveTerminalTaskStatus` at **:187** |
| `src/state/app/taskStatus.ts:56-68` | 54 lines | past EOF |
| `src/state/app/taskStatus.ts:65-68` | 54 lines | past EOF |

So the React-side drift is roughly **−29 lines** and two citations point past end-of-file. Method warning,
learned the hard way in this same pass: a first version of the "is the anchor near that line" check harvested
prose words (`cancelled`, `frontend`, `covered`, `statistics_updated`) as identifiers and reported 164 of 233
citations as wrong — almost none of them defects. **Out-of-range is a hard predicate; anchor proximity is not,
and must be positive-controlled and classified before any number is published.** Also: `remote.md` is
attributed to **P3-06** in the plan's §5.1 record, while its Input hit comes from P8-03 claiming the whole
`src-tauri/src/remote/` directory — do not read those two as the same statement.

### Q1's cost is now measurable: the command map already splits sessions 7 / usage 6 (measured 2026-09-28)

`docs/native/command-map.json` assigns `check_module` and `core_service` per command. Grouping by the module
prefix of `rust_path` gives the exact boundary the plan's Q1 (does the usage family move with P3-04?) has to
cross:

| commands | rust_path prefix | `check_module` | `core_service` |
|---|---|---|---|
| 7 — `read_session_messages`, `read_session_message_page`, `read_session_id`, `session_supports_native_resume`, `recover_task_session`, `adopt_session_for_agent`, `export_session_markdown` | `session::` | `sessions` | `sessions` |
| 6 — `read_usage_snapshot`, `record_agent_config_usage`, `load_agent_usage_snapshot`, `refresh_usage_statistics_index`, `read_session_metrics`, `read_usage_statistics` | `usage::`/`agent_usage::`/`usage_index::`/`analytics::` | `maintenance` | `metrics::usage` / `metrics::agent_usage` / `metrics::index` / `metrics::analytics` |

So the map puts the usage wrappers in a **third** bucket — `check_module: maintenance`, `core_service: metrics::*` — and
`checks.json::plannedModules` says that module's owner is **P8-04** (it is not yet registered; 35 commands already carry
`check_module: "maintenance"`). In other words **the machine-readable ledger already sides with "the usage family
belongs to P8-04"**, which is also where `agent_usage.rs` is claimed (P8-04's `Input`, see the coverage audit above) —
and it disagrees with `sessions.md` carrying rows 8–11. Each answer to Q1 then has a concrete cost:

- **usage moves with P3-04** ⇒ 6 commands must be re-tagged `check_module: maintenance → sessions`, i.e. moved out of
  the module P8-04 is slated to register, and the 35-command `maintenance` grouping loses rows it currently holds.
- **usage stays with P8-04** ⇒ the tagging is already right, but `sessions.md` must lose rows 8–11 (four of its eleven
  rows), which the gated `extraction-docs-current-sessions` case will only accept once P8-04 has its own §5.1
  expansion — a document that does not exist yet.

Method note: the first pass looked for a camelCase `coreService` key and found **0** commands, which would have
been reported as "the map has no session tagging at all" — a fabricated defect. The real keys are
`core_service` / `check_module`, and `commands[]` has no `module` field. Verify the schema before trusting a
zero.

### Accepted-baseline re-verification, 8 of 9 modules (measured 2026-09-28, 14:35–14:42 local)

Re-ran every registered gate module against today's dirty tree, one module per process, sequentially, using
`node scripts/native/check.mjs <module>` (confirmed equivalent to the `native:check` alias in `package.json`):

| module | cases | test cases | result |
|---|---|---|---|
| `skeleton` | 22 | 160 | ok |
| `protocol` | 6 | 98 | ok |
| `storage` | 5 | 361 | ok (see the transient below) |
| `projects` | 5 | 21 | ok |
| `tasks` | 7 | 151 | ok |
| `runtime` | 1 | 5 | ok |
| `agents` | 1 | 2 | ok |
| `terminal-core` | 13 | 107 | ok |

That is 60 cases / 905 test cases green. **`tauri-adapter` was deliberately not run**: its
`src-tauri-lib-tests` case writes the user's real `~/.aeroric` (G19/G20/G21 — it re-clobbers the credential
sidecars and appends fake audit rows), so re-verifying it needs the user's go-ahead rather than being done as
a routine check.

**Transient worth recording**: `storage-core-rust` first reported `FAIL (exit 101)` with *0 test cases
executed*, while `storage-core-clippy` passed in the same run. Cause was mine, not the tree's: I had
`pkill`ed an earlier runner while its cargo was mid-build and started the replacement ~40 s later, and the
aborted build poisoned the next invocation. Re-running the single module returned `5 cases / 361` green,
exactly at the floor. Rule: **a red that appears immediately after you killed a gate is about your process
management, not about the code — re-run before writing it down.** "0 test cases executed" is the tell: a real
regression reports its counts.

### Credential-directory hygiene: the census's sha256 view missed 24 leftover temp files (measured 2026-09-28)

Static count of the user's real `~/.aeroric/agent-credentials/`: **101 files** against **78 profile ids** in
`settings.json`. Of the 36 unmatched entries, **24 are hidden atomic-write temporaries** following
`storage.rs:606/723/729`'s template `.{file_name}.{pid}-{ns}.tmp`, clustered into three process groups
(pids 18598 / 22895 / 73551). One of them is `.empty-target-<uuid>…tmp`, and `empty-target-{uuid}` is a
**test fixture** (`app_settings/agent_scripts.rs:3389`) which isolates its agent dir into `temp_dir()` but not
its credential dir. Separately, a 7-byte sidecar named `custom` holds content whose sha256 equals `sk-test`,
mtime 2026-08-05; `id: "custom"` exists only in test code (`agent_scripts.rs:2972/3014/3195/3317`), and there
is no `custom` profile in `settings.json` — so unlike `liwan`/`muyuan`/`agentrouter` it cannot be restored
from settings, only deleted (which needs the user's word). Consequence for G20's exit criteria: comparing
sha256 of known profiles is not enough — the assertion must also pin **file count and hidden-`.tmp` count** in
that directory, because "new garbage" is invisible to a hash-only check. (The nine `claude-refresh-*`
sidecars were already recorded in the plan as tests panicking before their own cleanup; not re-registered
here.)

### Accepted-baseline re-verification, part 2: contracts, typecheck, and 10 of 12 host cases (measured 2026-09-28)

Continuing the round that stopped at 8 of 9 modules, the remaining surface was verified without touching the
real `~/.aeroric`:

| check | how | result |
|---|---|---|
| `contracts.mjs --check` | direct (cargo builds `export-schema`, writes to a temp dir) | **rc 0 — "12 document(s) match the Rust DTOs byte for byte"**; matters because this batch edits `contracts/native/v1/event-topics.schema.json` |
| `pnpm typecheck` | direct | rc 0. Scope fact: the script is **two** commands — `tsc --noEmit` for the web app **and** `pnpm --dir mobile typecheck`; "typecheck 绿" in prose means both |
| `adapter-invariants.test.mjs` / `adapter-invariants.mjs` | direct | rc 0 (gate variant reports "235 rust + 991 web files, ADR-020/021/023/031/032") |
| `rg -n resolve_data_dir\|DataDir:: src-tauri/src/core_adapter.rs` | direct | **no match** = the pass condition for an `absent` case (rg exits 1 — do not read that as a failure) |
| `cargo fmt --check` (both workspaces) | direct | rc 0 ×2 |
| `cargo clippy --all-targets -D warnings` (both workspaces) | direct | rc 0 ×2, but 0.28 s / 2.15 s = warm cache, so **not** self-evidently a real re-lint |
| 2 vitest cases (`native-core-adapter`, `task-intent-queue`) | direct | rc 0 ×2 |

So **10 of `tauri-adapter`'s 12 cases are verified**; the two still excluded are exactly the polluting ones
(`cargo test --lib` at 2465 and `--lib core_adapter_tests::` at 19), because they write the user's real
credential/audit/storage files (G19/G20/G21).

**The clippy case was mutation-tested rather than trusted**: after touching `aeroric-core/src/lib.rs` forced a
re-lint in 3.28 s (`[profile.dev] debug = 0` makes core compile fast, so the speed was not itself evidence),
an injected `if s.len() == 0` made it exit **101** with `error: length comparison to zero` under `-D
warnings`; restoring the file returned rc 0 and `git status` shows the file byte-identical to HEAD. Fast ≠
vacuous here, and the way to know is to break it on purpose.

### Full TS regression, run by hand (measured 2026-09-28, 169 s)

`pnpm test` (= `vitest run`) is executed by **no** registered gate case — that is the F2 family — yet §3.2
defines `native:check all` as including "旧 Tauri/TS 回归". Run manually against today's tree: **376 test files
passed (376), 5483 tests passed (5483), 0 failed, exit 0**, wall 169.05 s against 760 s of parallel test time
and 465 s of environment setup. The pair matches the figures recorded for the previous full run, so the
frontend net has not lost strands. Grep warning: vitest writes ANSI escapes, so `^Tests` and `✓ src/…`
patterns match **nothing** until they are stripped — two of my own "0" readings came from that, not from
absent output.

### What remains unverified in the accepted baseline (stated as a set)

1. `tauri-adapter`'s two `cargo test --lib` cases (2465, plus `core_adapter_tests::` 19) — excluded because they
   write the user's real `~/.aeroric` (G19/G20/G21); needs an explicit go-ahead.
2. The whole Swift/Xcode side: `native:generate`／`native:build`／`native:dev`／`native:ui-test` are compliant
   exit-78 NOT-READY stubs naming P4-01, and `macos/` still holds only the `AeroricKit` SwiftPM package — no
   `project.yml`, no `.xcodeproj`. Scheduled absence, not a hole.
3. `native:check all` — refuses by design until P10-05 (38/43 planned entries unregistered).

### How much of the surface is actually migrated (census 2026-09-28, 补记一百六十八)

The plan counts progress in Tasks (12/51 accepted). That answers "how many gates closed", not "how much of the product has moved". Two complementary measures, both re-runnable:

**Code volume (proxy, with a stated weakness).**

| tree | files | lines |
|---|---|---|
| `core/crates/aeroric-core/src` | 51 | 17 592 |
| other core crates' `src` (protocol/host/cli) | 27 | 12 398 |
| core crates `tests/` | 15 | 9 319 |
| `src-tauri/src` (host Rust) | 234 | 176 549 |
| `src` (TS/TSX frontend) | 991 | 288 820 |
| `macos` (Swift) | 13 | 3 330 |

So the migrated core is **9.9% of host Rust by line count** (17 592 / 176 549; 17% counting all four core crates). Read it as a floor, not a score: Phase 1–3 deliberately moved *leaves* (pure modules, tables, reducers) rather than whole vertical slices, and the host's 660 registered command handlers are precisely what Phase 4+ still has to absorb. LOC also silently rewards verbatim copies and punishes consolidation — which is exactly what the migration is trying to do.

**Gate coverage by command surface (the categorical measure).** Of the **660** registered commands, only **27 (4.1%)** carry a `check_module` that names a **registered** gate module; **633 (95.9%)** still point at `plannedModules` entries that have no cases yet. "8 of 9 registered modules green today" therefore secures **4.1% of the command surface**, and the remaining 38/43 planned modules are the real distance to P10-05's first full matrix. Reproduce: read `scripts/native/checks.json`, filter `plannedModules` keys starting with `$` (there is one — `$comment`), then bucket `command-map.json` `commands[].check_module` by whether the name is in `modules`.

**Ledger completeness (all 660 rows).** `core_service`, `check_module`, `test_id`, `feature_ids` are set on **100%** of commands. `rpc_method` is set on **641/660**, and the 19 exceptions are **not drift** — every one is `category=host_local_os_ui`, spread over shell 4, maintenance 6, settings 6 (incl. the 5 `theme_custom_*`), files 1, extensions 1, dsh 1 = 19, That set is independently corroborated inside the repo: `docs/native/adr/001-native-boundary.md:75` tabulates `host_local_os_ui（19）` as the client-side category that is deliberately *not* in the protocol (ADR-001, not ADR-027 — the earlier draft of this section cited the wrong ADR and was fixed the same round). `reachable`: 567 true / 93 false.

**Injection hygiene (same round).** `.trellis/tasks/09-21-native-phase-3/{implement,check}.jsonl` use `{file, reason}`: **15 artifact paths, 0 missing, 0 over the 32 768 B single-file cap** (largest injected shard 21 199 B = 65%). The two oversized files (`command-map.summary.json` 33 296 B, `feature-matrix.json` 100 513 B) appear only inside `reason` prose — so a naive "referenced file exceeds cap" scan reports a defect that does not exist. Path-in-prose must never be counted as an injection reference.

### All nine gate modules re-measured on one day, and the exact exclusion list (measured 2026-09-29, 02:13–02:15 本地)

此前"门禁全绿"是由多天、逐模块的碎片数字拼出来的（`baseline.md` 上方 2026-09-28 那几节）。本轮把它们换成**同一天的一次实测**，并把"没跑什么"写成集合而不是散文。

| 模块 | 命令 | 今天 | 用例数 |
|---|---|---|---|
| skeleton | `node scripts/native/check.mjs skeleton` | rc 0 | 22 case／**160** |
| protocol | 同上 `protocol` | rc 0 | 6 case／**98** |
| storage | 同上 `storage` | rc 0 | 5 case／**361** |
| projects | 同上 `projects` | rc 0 | 5 case／**21** |
| tasks | 同上 `tasks` | rc 0 | 7 case／**151** |
| runtime | 同上 `runtime` | rc 0 | 1 case／**5** |
| agents | 同上 `agents` | rc 0 | 1 case／**2** |
| terminal-core | 同上 `terminal-core` | rc 0 | 13 case／**107** |
| tauri-adapter | **逐条 argv**（`check.mjs` 不支持按 case 过滤，模块级会撞下面两条） | **10 PASS／0 FAIL／2 SKIP** | 见下 |

**`tauri-adapter` 的 12 条按原 argv 逐条跑的清单**（用 `execFileSync`，与 runner 同一判据：`absent` 型 rc=1 为通过，其余 rc=0 为通过）：`adapter-invariants-unit` ✓、`adapter-invariants-src` ✓、`src-tauri-cargo-fmt` ✓、`core-cargo-fmt` ✓、`core-clippy` ✓、`src-tauri-clippy` ✓、`ts-typecheck` ✓、`adapter-event-payload-contract` ✓、`adapter-intent-queue` ✓、`core-adapter-never-relocates-data-dir` ✓（rc=1＝无禁匹配＝通过）；**跳过的两条正是会写真实 `~/.aeroric` 的那两条**：`src-tauri-lib-tests`（`cargo test --lib`，地板 2465）与 `adapter-acceptance-real-kernel`（`--lib core_adapter_tests::`，19 条）——它们仍等用户答 B 项（`G19`／`G20`／`G21`）。

**两条如实的局限**：① `cargo fmt`／`clippy` 四条只记到 rc，**本轮没测它们是否命中 warm cache**——2026-09-28 那节已量过"0.28 s／2.15 s＝热缓存，不证自明地不是重 lint"，并按需用改坏 `aeroric-core/src/lib.rs` 的变异证过一次；今天的 rc=0 是"再次一致"，不是"重跑一遍完整 lint"。② 本轮第一次想用 shell 计时这几条时，`rc=$?` 实际取到的是管道里 `bc` 的返回码而非命令本身，得到的四个数字（含 `.004 s`）**全部弃用**——只有上面那次按 `checks.json` argv 的执行是可信计数。这条也再次说明：**改完台账必须跑 `native:check skeleton`**（`digest-unit` 的 80% headroom 规则只活在那条用例里，`digest --check` 看不见；见 2026-09-29 `补记一百八十九`，本轮就是这么撞出来的）。

**同一天的其它不变量**：`inventory --check` consistent（registered=660 mapped=660 **gaps=31** definitions=660 taskStates=51/12accepted）、`--freeze-check` 字节 noop、`digest --check` verified 11 片／最大 22 579 B（69% 上限）、`extraction --check` 四份被门禁比对的工件全 rc=0（sessions 11/13、dsh-webui 9/66、remote 22/9、database-notebook 75/168）、`contracts.mjs --check` → 「current — 12 document(s) match the Rust DTOs byte for byte」、`git diff --check` 干净。

## 接线普查：core 的公开面有多少真的被宿主调用（实测 2026-09-29，MIGRATION_PLAN 补记一百九十五）

前面各节量的是「门禁有多绿」，这一节量的是**另一件完全不同的事**：已迁进 core 的权威面，今天有多少被 shipping 宿主真正引用。方法与判据：
1. 从 `core/crates/aeroric-core/src/` 每个模块抽出全部 `pub fn/struct/enum/trait/const/type/use` 名字（模块目录读 `mod.rs` ＋ 兄弟文件）；
2. 扫 `src-tauri/src/` 全部 234 个 `.rs`，把「宿主提到过的 core 名字」集合取出来——三种来源都算：`use aeroric_core::{…}` 的多行大括号、`use aeroric_core::x::Y;`、以及任意 `aeroric_core::…::…` 全路径的**每一段**标识符（上一轮只用「按模块路径 grep」，会把 `storage::projects::load_projects` 这类**再导出路径**漏掉，本轮改成按符号计数就是为了避开这个坑）；
3. 对照表：

| core 模块 | 公开项 | 宿主提到 | 宿主从未提到 |
| --- | --- | --- | --- |
| tasks | 109 | 17 | **92** |
| projects | 44 | 0 | **44（整面）** |
| runtime | 32 | 8 | **24** |
| storage | 34 | 6 | **28** |
| terminal | 41 | 20 | **21** |
| agents | 20 | 13 | 7 |
| context | 9 | 2 | 7 |
| location | 8 | 6 | 2 |
| storage_lock | 7 | 2 | 5 |
| clock | 4 | 3 | 1 |
| paths | 2 | 1 | 1 |

合计 114 个 core 名字被宿主提到。三条最有分量的具体结论（都带控制实验）：

- **`src/projects/` 整面零引用**（44/44）：`group_projects_for_rail`、`apply_order`、`normalize_order`、`assign_group`、`rename_group`、`set_avatar`、`resolve_location` 等今天在宿主里一次都没出现（直接 grep 复核：`group_projects_for_rail` → **0 命中**）。**正控**是读路径确实接上了——`load_projects` 在宿主 8 个文件出现，`src-tauri/src/core_adapter.rs:1113` 走的是 `aeroric_core::storage::projects::load_projects`（core 里另一个同名区域）。⇒ P2-03 已验收的**分组/排序/改名/头像**这套权威面还在宿主／前端那一侧。
- **`terminal` 缺的正是上一轮点名的两面**：21 个未被提到的名字里包含 `plan_replay`、`ReplayPlan`、`ReplayFrame`、`ResumePoint`、`ResetReason`、`projected_utf16_units`（读面）与 `take_control`、`check_write`、`ControlLease`、`LeaseHandle`、`InputRejection`（租约面），另有三个容量常量；而 `settle_exit`／`ExitFacts`／`activate_pty_session`／`TerminalService`／`publish` 一侧是接上的（20 个被提到）。
- **协议 v1 的服务词汇表是旧宿主的，不是 core 的**：`contracts/native/v1/methods.schema.json` 的 `coreService` 取值共 **11** 种＝database 95／notebook 73／settings 42／git 36／files 12／remote 9／sessions 7／local_router 6／storage_backends 5／operations 3／platform 2 —— **没有 terminal、tasks、projects、agents、runtime**。`sessions` 的 7 与 补记一百二十七 独立量的会话方法数吻合（正控）。⇒ 「有没有协议入口」与「core 有没有实现」是两套账，P5-01／P3-06／P8-05 要接的是前者。

**三条局限（必须一起读，否则这张表会被当成定罪证据）**：① 「没提到」不等于「绝对没人用」——经由 trait 方法调用、闭包内的推断、或再导出别名都可能绕过符号名匹配；因此这张表**只能证伪「已迁移=已接线」这种说法，不能证明未使用**。② 「提到」里含宿主自己的 `#[cfg(test)]` 引用，所以 wired 列是**上界**（`projects` 的 0 不受此影响，0 就是 0）。③ `macos/` 与 `mobile/` 尚未消费 core（Phase 4＋），本表只算 Tauri 宿主。`src-tauri/src/core_adapter.rs:29-31` 的注释本来就承认这一点：「Items still unused by the product path carry a targeted `dead_code` allowance at the item」——所以「零引用」在 core 里是被**逐项标注**过的，不是意外。

**这张表给谁用**：P3-06（切换读路径）与 P3-07、P5-01、P8-05 的**待接清单规模**第一次有了数字（tasks 92／terminal 21／runtime 24／storage 28／projects 44），P10-05 的全矩阵出口如果要声明「权威已切换」，判据应当是本表而不是门禁 case 数。复现命令：见 `MIGRATION_PLAN.md` 补记一百九十五 记录的脚本要点（按符号取并集，勿按模块路径 grep）。**补记一百九十六 追加一条仪器纪律**：符号并集的收集侧必须锚定 `aeroric_core::` 路径或 `use aeroric_core::{…}` 导入——裸把符号名拿去宿主全文计数会因同名自有函数（`remove`／`rename`／`text`／`is_hidden` 等）虚高，实测同一模块在两种口径下给出「宿主在用 11」与「宿主提到 0」两个相反答案；只有「零引用」一侧在这种错法下仍然可信（`G34` 的四项就是这么找出来的）。

**候选门禁① 的可复现仪器（2026-09-29 补记一百九十七）**：判据是「前端所有 invoke 命令表 ⊆ 已注册 660 条命令」。今天照此复现＝**表 30 张／成员 71 个／例外 0 条**，与 补记一百七十八 记的数字逐字吻合。**作用域是这条判据的一部分，必须一起抄走**：只取 `src/lib/api/` 里 `const *COMMANDS = { key: "command_name" }` 形态的**值**（`agentSettings`／`appCommands`／`runtimeCommands`／`sftpCommands`／`worktree` 五个文件）。把同一个正则放到整个 `src/` 会得到**表 36 张／字符串 209 个／"例外" 123 条**——那些"例外"全是同名不同物：`src/lib/redisCommandSafety.ts` 的 `BLOCKED_COMMANDS`／`CONFIRM_COMMANDS`（Redis 命令安全集，21＋81 项）、`src/dshSlashCommands.ts` 的斜杠命令与 i18n 键（18 项）、`src/components/file-viewer/editorCommandEvents.ts` 的 Monaco 命令 id（`findReferences`／`renameSymbol`／`quickFix`）、以及两份 `src/test/` 里的测试局部 Set。**结论：这条候选门禁的判据只有在锚定「目录＋形态（对象的值）＋与 660 名单比对」三件事时才成立**，缺任一件就会把 0 条例外算成 123 条，而两边都是"绿色的确定性"。

## 第 14 项（`G25`）的可复现推导：154 行清单里有 117 条落点，且 117 条全部缺一层 `src/`（实测 2026-09-29，MIGRATION_PLAN 补记一百九十八）

答题纸第 14 项长期写作「§5.1 清单的 destination 列 117 行全在 `src/` 之外，且与 P3-04 Action 1 逐行冲突（11/11）」，但**从没写过 117 是"哪 117"**。今天给出可复现口径：

- **集合**：被 `extraction-docs-current*` 四条 case 比对的**四份**工件 —— `sessions.md` 11 行、`dsh-webui.md` 9 行、`remote.md` 22 行、`database-notebook.md` 75 行，表格行合计 **154**。
- **判据**：行内任一单元格（去反引号后）以 `core/crates/` 开头 ⇒ 该行携带迁移落点。命中 **117 行**（＝历史上那个 117，逐字复现），其余 37 行是 retained wrapper／测试目标／无落点的条目。
- **新量到的一条形状**：这 117 个落点路径**全部缺 `src/` 那一层** —— 写作 `core/crates/aeroric-core/session/session-root.rs`，而真实 crate 的源码根是 `core/crates/aeroric-core/src/…`。⇒ 答题纸里选项 ③ 的前提「生成器补 `src/`（规则性改动）」**不是猜测，是实测**；而照清单字面建目录会在 crate 根旁边开一堆与 `src/` 平级的目录。
- **落点形状本身也说明这列不是迁移落点**：机械镜像规则把源文件 `session.rs` 变成目录 `session/` 里的 `session-root.rs`，于是 11 条落点里有 6 条是 `…-root.rs`（`session-root`／`session_dsh-root`／`session_omp-root`／`event_watcher-root`／`usage-root`／`agent_usage-root`／`usage_index-root`／`analytics-root`），与任何按职责命名的 Action 都不可能一一对应。
- **P3-04 的 11/11 冲突复现法**：取 `sessions.md` 的 11 个 destination 的 **basename**，逐一检查是否出现在本文 `### Task P3-04` 的 Action 段原文里 ⇒ **11／11 未出现**（Action 用的是 `sessions/{mod,parser,claude,codex,…}.rs` 与 `metrics/{usage,index,analytics,…}.rs` 这套词汇，且**带 `src/`**）。门禁对此**全盲**：`extraction-docs-current-sessions` 只比对表的机械字段与代码符号，不比对 Action 的文字。

**复现命令口径**：脚本要点＝「读四份工件 → 按 `^\|` 取表格行并去掉表头分隔行 → 单元格去反引号 → 以 `core/crates/` 判定落点 → basename 与 P3-04 的 Action 段做包含测试」。注意**别把整张表所有路径单元格都当落点**：同一行里源文件（`src-tauri/src/…`）也匹配路径形态，早期一次粗扫把 236 行全counting成"落点行"、得出「src/ 之外仅 1.7%」的错误结论，正是列位没锚定的典型。**「117」只有在『四份被门禁比对的工件』这个集合上才成立**，换成整个 `extraction/` 目录（9 份、236 行）就是另一个数。

## `G26` 引用体检（实测 2026-09-29，MIGRATION_PLAN 补记一百九十九）：9 条越界引用集中在一份工件，真值行号已查到

扫描对象＝**没有新鲜度门禁的那 5 份** §5.1 工件（`task-lifecycle-react`／`event-topics-pending`／`terminal-stream`／`runtime-ports`／`agents-launch`）。判据：把文内每条 `path:line(-line)` 引用的目标文件按 `wc -l` 真值比行数，引用超过末行即"越界"。

- **结果：越界 9 条（＝4 个不同的 `path:line` 写法），全部在 `task-lifecycle-react.md`**（其 17、18、44、45、92、96、124、160 行）；另有 **274 条引用行号落在范围内**。
- 越界的两组与它们的真值：`src/appProjectState.ts:212-214` 与 `:216-218` —— 该文件 `wc -l`＝**189 行**，而这两个符号现在实在 **183**（`shouldIgnoreTaskStatusTransition`）与 **187**（`isLiveTerminalTaskStatus`）⇒ 漂移 **−29／−29 行、且引用整体落在 EOF 之后**；`src/state/app/taskStatus.ts:56-68` 与 `:65-68` —— 该文件 **54 行**，唯一的公开入口是 `5: export type UpdateTaskStatusDeps`／`11: export function applyTaskStatusTransition` ⇒ 这两处引用不可能成立。
- **一条方法上的更正（写在这里免得下一轮再犯）**：扫描器另报了 4 条「文件找不到」，分别是 `AppOpsProvider.tsx`（真在 `src/state/app/`，被引用两处）、`agentRuntimeRecovery.ts`（真在 `src/components/`）、`registry.rs`（真在 `core/crates/aeroric-core/src/runtime/`）。**它们不是断链，是引用只写了 basename 没写目录**，而我的候选前缀表不够长 ⇒ 判废这 4 条。反过来这恰好是 `G26` requirement 里那句话的实测样本：**引用不带路径时，"行号存在性"判据根本无法定义**（补记一百三十五 也是这个结论）。
- **`wc -l` 与 `split('\n').length` 差 1**（末行换行符），本节的行数一律按 `wc -l`。

**待你授权的那一句（＝答题纸第 15 项的 ②）**：若答 `15=②`，我就把这 **9 处**引用改成上面查到的真值（4 个不同写法：`appProjectState.ts:183`／`:187` 一带与 `taskStatus.ts:5`／`:11`），**只改数字、不改任何编排或表结构**，并按本节目的留一份改前／改后对照。

## 宿主 `--lib` 套件的可隔离跑法（实测 2026-09-29，MIGRATION_PLAN 补记二百一十一）：串行 2465／2465 且真实 `~/.aeroric` 一字节未动

`tauri-adapter` 的 `src-tauri-lib-tests`（2465 条）与 `adapter-acceptance-real-kernel`（19 条）此前一直是 SKIP，理由记在 `G19`／`G20`／`G21`：这两条 case 会把用户的真实 `~/.aeroric` 写脏（key sidecar 被 `sk-test` 覆盖、`remote-audit.log`、`settings.json`）。本轮实测出**一条今天就能用、不改任何产品代码与门禁 argv 的隔离办法**，代价与边界都写在这里。

**机制（读源码定死的，不是试出来的）**：`src-tauri/src/platform/mod.rs:40` 的 `home_dir()` 在 `#[cfg(test)]` 下先取进程全局的 scratch SLOT（`Mutex<Option<PathBuf>>`，由 `install_scratch_home` 装入，全仓 **17 处**调用点），**取不到才回落到 `imp::home_dir()`＝`std::env::var_os("HOME")`**。SLOT 只覆盖"自己装了根"的那些测试；`platform/mod.rs` 的文档注释明写了这一点，并给出把 `HOME` 指到别处正好接住剩下的那批 ⇒ 隔离手段是 `HOME=<临时根>`，与 SLOT 是互补的两层。

**可复现命令（两条，顺序不能换）**：

1. `cargo test --lib --manifest-path src-tauri/Cargo.toml --locked --no-run` —— **必须用正常 `HOME`**：`HOME=/tmp/… cargo test` 会 rc 1 报 `rustup could not choose a version of cargo…no default is configured`（rustup 读 `$HOME/.rustup`）。产物为 `src-tauri/target/debug/deps/aeroric_lib-2dc29bcd8586f84d`（119 417 488 B）。
2. `cd src-tauri && HOME=<新建的空临时目录> ./target/debug/deps/aeroric_lib-<hash> --test-threads=1` —— **cwd 必须是包根**：`pty::tests::agent_launch_parity_fixture_harvest` 的 `FIXTURE` 是相对路径 `../core/crates/aeroric-core/tests/fixtures/agent-launch-parity.json`（`pty.rs:3902`），在仓库根跑时 `../core` 不存在，rc 101 且 panic 在 `pty.rs:4114` 的 `expect("parity fixture must exist")`——看着像 fixture 漂移，其实是我命令写错了。

**当天读数**：串行 **rc 0／2465 passed／0 failed／54.35 s**，与门禁 case 的 `minCases: 2465` 和 argv（`-- --test-threads=1`）同配置同结果。默认并行两轮各 **rc 101／2464 passed／1 failed**（14.90 s、15.14 s），失败的都是 `app_settings::agent_scripts::tests::resets_codex_chat_bridge_wrappers_when_setting_is_off`；该测试单跑 rc 0（0.11 s），整个 `app_settings::agent_scripts::tests::` 模块并行跑 **47／47 rc 0**（0.35 s）⇒ 干扰来自**跨模块**。

**这不是本轮新发现的缺陷**：`scripts/native/checks.json` 里 `src-tauri-lib-tests` 的 `$note` 早已把它列为三个受害者之一（原话记着「15 次并行跑红 4 次」），`platform/mod.rs` 的注释同样点名这条测试，并把门禁改串行称作 **「Containment, not cure」**——真正的修法是 Action 3 的可注入数据根。本轮的 2/2 并行红是这段已记录行为的独立复现，不是回归，也不是新问题。

**隔离效果的实证（前后对照，不是事后一句"应该没写"**）：4 次跑之前与之后各取一次真实家的 sha256 —— `~/.aeroric/remote-audit.log`（132 194 B）／`agent-credentials/liwan`（51 B）／`agent-credentials/custom`（7 B）三份**逐字节相同**；而每个临时根各收到 **6 个文件／24 KiB**：`.aeroric/settings.json`、`.aeroric/remote-audit.log`、`agent-credentials/{liwan,muyuan,custom,agentrouter}` —— 正好是 `G19`／`G20`／`G21` 点名的载体集合（只跑那个模块时是 4 个文件，缺 `settings.json` 与 `remote-audit.log`，与范围一致）。

**边界（别把它读成"G19–G21 已闭"）**：这只是**跑测试时**的外层包装。门禁里那条 case 的 argv 并没有设 `HOME`，所以 `pnpm native:check tauri-adapter` 照原样跑仍会写真实家；三条 gap 的 owner 与状态一律未动，本轮也**没有**擅自给门禁加 `env`。它改变的是行政 **B** 的可行性：那两条 SKIP 现在可以按上面的两步跑完而不用拿用户的真实数据冒险。

**两条 SKIP 的风险画像其实不同（同日实测，值得单独记）**：把范围收到 `core_adapter_tests::` 那 **19 条**（就是 `adapter-acceptance-real-kernel` 的过滤集）在同一隔离下跑 ⇒ **rc 0／19 passed／0 failed／9.75 s**，而临时根里**一个文件都没有**（`find … -type f` 空、`du` 0 B），真实 `~/.aeroric` 三份哈希照旧逐字节相同。原因是这 10 个场景各自 `install_scratch_home` 装自己的根（正是 SLOT 的设计用途），所以**根本不依赖 `$HOME` 回落**；上面那 6 个文件是同一套件里"没装根的那些测试"写出来的。⇒ 结论要分开说：`src-tauri-lib-tests`（2465）需要 `HOME=<临时根>` 才隔离得住，`adapter-acceptance-real-kernel`（19）今天**没有任何证据**表明它会写 `~/.aeroric`——它被归进"会写真实家"那一组，本轮实测不支持这个归类。**但这条更正不自动改门禁**：先查清机制再说改法——`scripts/native/checks.json` 与 `check.mjs` 里**根本没有任何 skip／opt-in 字段或跳过逻辑**（今天 grep 实测：`checks.json` 里含 "skip" 的只有两处**无关字符串**——第 9 行的判据描述「skipped required case … all fail」与第 1419 行的一个 Rust 测试名 `skip_leading_utf8_continuations`，没有任何 skip／opt-in **字段**；门禁 runner 侧 `check.mjs`／`gate-config.mjs` **0 命中**，`skipped` 一词只出现在 `inventory.mjs`／`task-status.mjs` 的无关统计里），所以那 **2 SKIP** 从来不是配置项，而是**操作者逐条执行时的人工决定**（`reviews/tauri-adapter.md:409` 记的就是这个口径：`check.mjs` 不支持按 case 过滤，遂按原 argv 单条执行、这两条**没有执行**）。⇒ 行政 **B** 要做的不是"翻一个开关"，而是"由你点头把这两条按上面的两步跑一遍"；而想让门禁本身变得非破坏性，则要给 argv 注入 `HOME`＝**门禁面改动**，本轮没做，仍等你表态。

## 8 个门禁模块的当日串行读数，以及两次"假红"的成因（实测 2026-09-29，MIGRATION_PLAN 补记二百三十一，台账 `G38`）

本会话把文档／台账／`digest` 改了一大批之后，把 9 个模块里**不碰真实 `~/.aeroric` 的 8 个**串行跑了一遍。读数与
2026-09-28 那份全矩阵逐字相同，说明这批改动没有触碰任何产品代码行为：

| 模块 | 结果 | 用例数 |
|---|---|---|
| `skeleton` | rc 0 | 22 case／**160** |
| `protocol` | rc 0 | 6 case／**98** |
| `projects` | rc 0 | 5 case／**21** |
| `tasks` | rc 0 | 7 case／**151** |
| `runtime` | rc 0 | 1 case／**5** |
| `agents` | rc 0 | 1 case／**2** |
| `storage` | rc 0 | 5 case／**361** |
| `terminal-core` | rc 0 | 13 case／**107**（串行连跑 **4 次**，四次读数完全相同） |
| `tauri-adapter` | **未跑** | 其 2 条 `cargo test --lib`（`src-tauri-lib-tests` 2465／`adapter-acceptance-real-kernel` 19）会写真实 `~/.aeroric`（`G19`／`G20`），仍等行政项 B |

同批还有两项非模块级检查：`pnpm native:doctor` ⇒ **all 18 checks satisfied**；`node scripts/native/digest.mjs --check` ⇒ **verified 13 shards, largest 22 824 B**（`feature-matrix-gaps-5.json`，69.7% of the 32 768 B cap）。

**跑的过程中得到两次红，都是假的**，而且成因值得留在案面上，因为 P10-05 的第一次真·全矩阵运行最容易撞上：

1. `▶ storage-core-rust … FAIL (exit 101)`，汇总行是 **`native:check storage: 1 case(s) failed, 0 test cases executed`**。
   `exit 101` 是 `cargo test` 的失败码，但**"0 test cases executed" 说明该 case 根本没产出可解析的用例行**——整段输出里没有一行 `test result: FAILED`。
2. `▶ terminal-pty-bytes-rust … FAIL (exit timeout)`（该 case `timeoutMs=600 000`、`minCases=6`），同一时刻另一侧
   `cargo test --manifest-path core/Cargo.toml --locked -p aeroric-core` 正打印
   `a_hundred_mebibytes_through_a_real_pty_keep_their_hash_intact_and_bounded` 与
   `replay_memory_does_not_grow_with_total_output` 两条 `has been running for over 60 seconds`。

**根因**：我并发开了第二个 `pnpm native:check`。`scripts/native/check.mjs` 在模块内是**串行**的（`:158` 遍历 case、`:161` `await run(testCase.argv, timeoutMs)`），
所以不是 runner 自己赛跑；但两条 cargo 套件共享同一个 `target/`（cargo 的 build 锁会把第二个等待者排到后面）并抢 CPU，于是**慢用例被拖过 per-case timeout**。
把两个模块各自**单独**重跑：`terminal-core` 连跑 4 次都是 13／107，`storage` 5／361 —— 全部转绿。

**为什么这仍然是一条缺口而不是一句提醒**：`minCases` 与 `runner.floors` 只看"解析到几条用例"，所以**"没跑完"会被报成"红了"**，
而日志不区分抬 timeout／串行化／修测试这三种完全不同的动作。已登记为台账 `G38`（`status: open`、owner `P10-05`、`decision_item: 27`），
处置方案（分报 `PASS`／`FAILED`／`ABORTED`，且 `ABORTED` 不抬 `minCases` 红，配 ≥2 条负例）**属门禁面，等用户点头**；
本轮**没有改 `check.mjs`、没有改 `checks.json`、没有抬任何 timeout**。今天即刻生效的只有运行纪律：**跑门禁不要并行开第二个 `native:check`。**

## IME 与「独立 CLI」两项 §6.1 主张的当日复测（实测 2026-09-29，MIGRATION_PLAN 补记二百三十三）

**判据口径**：词边界 `rg -i '\bIME\b' --hidden --no-ignore`，排除 `.worktrees/`、`target/`、`node_modules/`、`dist/`。上一轮用的是宽松 `grep -i ime`，命中里混着 `runtime`／`timeline`，我当时把 4 个文件**全部判废**并写下"IME 没有证据"——那是把一个假阴性当成裁决。本轮更正，同时纠正反方向的假阳性：先前记的 `src-tauri` 9 行命中全部来自 `src-tauri/target/x86_64-pc-windows-msvc/**/*.d`，是构建产物不是源码。

| 区域 | 命中文件 | 命中行 | 说明 |
| --- | --- | --- | --- |
| `src/` | 19 | 80 | 6 个测试文件＋实现（`terminalInputFix.ts` 13、`terminalShared.ts` 6、`App.css` 6、`shortcuts.ts` 4） |
| `mobile/` | 7 | 32 | 主体是 `scripts/build-terminal-html.mjs`（26 行）与其生成的 `terminal-html.generated.ts`；另有 `glyph-fallback.test.ts` |
| `docs/` | 5 | 6 | 台账 F04 的 capability 字符串（两份副本）＋ 三份历史文档 |
| `src-tauri/` | **0** | **0** | 排除 `target/` 之后源码零命中 |
| `core/`、`macos/`、`scripts/`、`contracts/` | 0 | 0 | native 侧确实一条 IME 断言都没有 |

**具名证据**：`src/test/terminal-input-fix.test.ts` — 声明点 79（71 条字面 `it(` ＋ 8 张 `it.each(` 表），`pnpm exec vitest run` 单文件实跑 **109 条用例全绿（3 456 ms）**；109 与 79 的差是 each 表展开，属"executed vs declared"老口径。71 条字面标题里 64 条命中 IME／composition／pinyin／WeChat／candidate 词表。与另 5 个提到 IME 的测试文件（`prompt-editor.test.tsx`、四个 notebook-*）合并跑＝**6 files／365 tests 全绿（41.63 s）**。**位置很重要**：这些用例跑在 `pnpm test` 的 5 483 条里，`native:check` 的 72 条 case 一条都不碰 IME，所以"门禁没管 IME"依然成立——成立的方式从"没有测试"变成"测试在门禁之外"。

**独立 CLI**：`core/crates/aeroric-cli/src/main.rs` `wc -l` ＝ **6**，函数体打印 `aeroric-cli: not yet implemented (see MIGRATION_PLAN.md Task P3-07)` 后 `exit(2)`；注释明写桩必须非零退出，免得脚本把桩当可用的无头入口。整个 crate `#[test]`／`#[tokio::test]` 属性 **0** 个。`scripts/native/checks.json` 全文只有 **1** 处 `aeroric-cli`，位置是 `modules.skeleton.cases[4]`（`name: core-no-tauri-deps`、`kind: forbid`、`argv: cargo tree --manifest-path core/Cargo.toml --locked`）的 `mustContain[3]`＝只断言 crate 出现在依赖树里。**结论**：§6.1 的「独立 CLI 不打开窗口跑完整任务生命周期」应判 **未开始（排期如此，P3-07）**，不是"未验"——后者会被读成"存在但我没查到"。附带一条：`checks.json` 里没有任何 `--workspace` 用例，因此 **CLI crate 将来写了测试也不会被门禁跑到**；补法（加 `-p aeroric-cli` 的 case 或 `--workspace` 腿）属门禁面，等用户点头，本轮未改 `checks.json`。

**同一轮复算、无变化的两项**：phase-3 注入总数仍是 `implement.jsonl` 79 830 B（8 条）＋ `check.jsonl` 49 722 B（4 条）＝ **129 552 B＝98.8%**、headroom **1 520 B**（本轮没碰台账 ⇒ 不重切分片，注入名单里也没有 `baseline.md` 与 `MIGRATION_PLAN.md`）；`native:check skeleton` **22／160 rc 0**、`inventory --check` consistent（gaps=37、open=28）、`--freeze-check` 字节 noop。

## 全 9 模块当日串行复跑，以及一次「sanctioned 模块级命令」把真实 `~/.aeroric` 写进去的记录（实测 2026-09-29，MIGRATION_PLAN 补记二百三十五）

**运行方式**：`node scripts/native/check.mjs <module>` **逐条串行**（遵 `G38` 那条当日纪律：不开第二个 `native:check`）。九模块退出码全 0。

| 模块 | case／用例（今日） | 与 2026-09-29 早前读数的关系 |
| --- | --- | --- |
| `skeleton` | 22／160 | 逐字相同（补记二百三十一） |
| `protocol` | 6／98 | 逐字相同 |
| `storage` | 5／361 | 逐字相同 |
| `projects` | 5／21 | 逐字相同 |
| `tasks` | 7／151 | 逐字相同 |
| `runtime` | 1／5 | 逐字相同 |
| `agents` | 1／2 | 逐字相同 |
| `terminal-core` | 13／107 | 逐字相同 |
| `tauri-adapter` | **12／2 526** | 同数，但**这一轮两条污染源用例真的执行了**（见下） |

前两行合计口径可复算：除 `tauri-adapter` 外的 8 个模块＝**60 case／905 用例**，与 补记二百三十一／补记一百六十 两笔独立仪器同一棵树同一答案（逐字相同）。`tauri-adapter` 的 12 条全部执行，包含 `src-tauri-lib-tests` **2 465／2 465** 与 `adapter-acceptance-real-kernel` **19／19**；墙钟 235 s。

**一条必须如实写下的自报**：补记一百二十七 把这两条记成 `2 SKIP`，那是**那一轮手工逐条 `execFileSync` 的选择**，不是 `check.mjs` 的行为——`scripts/native/check.mjs` 全文 `grep skip` **0 命中**，`checks.json` 对这两条也没有任何隔离标记。⇒ 任何人执行计划里那句标准命令 `pnpm native:check tauri-adapter`，就会跑宿主 `--lib` 全套并触碰真实家目录。本轮后果（事后取证，**跑前未取快照**，因此 1963→1982 的增量无法逐条归因）：

- `~/.aeroric/agent-credentials/{liwan,muyuan,agentrouter}` 三个 mtime 全部变成 **13:42:36**（同一次秒级批量写，落在本轮 `tauri-adapter` 窗口内），形状与 `G20` 点名的三条肇事测试一一对应。当前内容**不是** 7 B 的 `sk-test`：三个文件都是 **51 B、`sk-` 前缀**，与同目录其它真实边车同形 ⇒ 常驻 `Aeroric.app`（pid 95896）把真值写了回来，**未观测到密钥丢失**；但"被覆盖成假值的那几十秒"无法从磁盘排除，`Aeroric.app` 若在窗口内没写回，就是真丢。
- `~/.aeroric/remote-audit.log`：132 371 B、mtime 13:43:52、`invite-created` **1 982** 条（台账 `G19` 记的 09-28 值是 1 963）；文件**末尾 200 行 100% 是 `invite-created`**（本轮实测，脚本口径：`json.loads` 后按 `event` 计最近 200 行）。
- 同窗口内 mtime 变动的 depth-1 文件另有 `agent-usage.json`（13:40:29）、`notifications.json`（13:36:13）、`marketplace_cache.json`（13:36:05）、`projects.json`（13:43:44）——App 常驻会自己写这些，**不归因给测试**；`ssh-passwords.json`（09:49）与 `settings.json`（09-28 14:50）本轮未动。
- 本轮**没有**再跑任何会写真实 `~/.aeroric` 的套件去"验证"上述机制（`G20` 已于 09-28 单条隔离当场证实），也**没有**删改审计日志或边车。

**今日基线快照（只读，供下一轮取差）**：`~/.aeroric` depth-1＝22 文件＋15 目录；关键载体 sha256 前 16 位 `remote-audit.log=4ebe58f175584b98`、`settings.json=778e6d13283dfb3b`、`projects.json=fbc3f169c4566982`、`ssh-passwords.json=ca3d163bab055381`、`remote-keypair.json=f3ca12f0fda84d7d`、`agent-usage.json=a9b33898bdcc53e4`、`notifications.json=72fbb32c1a326cd4`。凭据类目录只记 mtime/字节数，不记内容。

**其余当日复算（全部 rc 逐条单读）**：`inventory --check` consistent（`gaps=37`／trace 现读 open 30／`taskStates=51/12accepted`／`definitions=660`）；`contracts --check` 12 份逐字节；`digest --check` 13 片／最大 22 824 B；`native:doctor` 18/18；`git diff --check` rc 0；`git diff --no-index --check /dev/null` 对 `feature-matrix-gaps-1.json`／`-7.json`／`.zcodeignore`／`MIGRATION_PLAN.md` 四个未跟踪或已改文件均 **rc 1 且输出 0 字节**（双控口径再确认）。`extraction --check` 四份被门禁比对的工件由 `skeleton` 的 `extraction-docs-current*` 四条覆盖，本轮随 `skeleton` 全绿。

**答题纸第 24 项（`G35`）的前提今日逐字复现**：`pnpm lint` rc 1／**1 条 warning**（`src/test/native-host-protocol.test.ts:36:27` `no-explicit-any`）；`pnpm format:check` rc 1／**17 个文件**（逐条清单：`src/App.tsx`、`src/components/ReadOnlyStorageBadge.tsx`、`src/i18n/en.ts`、`src/lib/nativeHostProtocol.ts`、`src/state/app/taskIntentAdoption.ts`、`src/state/app/taskLifecycle.ts`、`src/state/app/useAppMaintenance.ts`、`src/state/startupDiagnostics.ts`、`src/taskIntentQueue.ts`、`src/test/{app-remote-task-request,contracts-parity,native-core-adapter,native-host-protocol,project-core-parity,task-core-parity,task-intent-queue}.test.ts(x)`、`src/test/taskIntentFakeCore.ts`）。本轮**未跑 `pnpm format`、未改任何文件**——第 24 项 ①②③ 是用户表态，未点头即不动。  **【其后被推翻】**用户答第 24 项＝②，同一天的 补记二百三十七 跑了那 17 个路径的 prettier 并消除了那条 `any`，两条腿当日 rc 0——见本节末尾「污染源改为默认 opt-in、`--write` 覆盖缺陷、第 24 项落地」。

**注入预算未被本轮影响（实测而非推断）**：`phase-3` 两份名单展开合计 **129 552 B＝98.8%**（`implement.jsonl` 8 条／79 830 B ＋ `check.jsonl` 4 条／49 722 B），与补记二百三十三 一字不差；两份名单里都**没有** `baseline.md` 与 `MIGRATION_PLAN.md`（`grep -rl` 逐条展开核对）⇒ 本轮的文档改动不吃 headroom。

## 污染源改为默认 opt-in、`--write` 覆盖缺陷、第 24 项落地（实测 2026-09-29，MIGRATION_PLAN 补记二百三十六／二百三十七）

**这一节记的是"门禁机器自己"的三条当日变化，全部有 rc 与前后对照。**

**① 两条污染源用例改为默认 opt-in。** `scripts/native/gate-config.mjs::caseSkipReason()` 只认字面量 `1`；`check.mjs` 逐条打 `SKIP (opt-in): …`、汇总单列 skipped、并把 "passed" 从 `entry.cases.length` 改成 `entry.cases.length - skipped.length`；`checks.json` 给 `src-tauri-lib-tests` 与 `adapter-acceptance-real-kernel` 打上 `optInEnv: AERORIC_ALLOW_REAL_HOME_TESTS` ＋ 具名 `optInReason`，`runner.rules` 增第 5 条。**读数**：同一条 `native:check tauri-adapter` 从 **12 case／2 526 用例／235 s** 变成 **10 case／42 用例＋2 SKIP／17 s**，rc 仍 0。**真实家目录对照**：跑前后对 `remote-audit.log`／`settings.json`／`projects.json`／`agent-usage.json`／`notifications.json` ＋ `agent-credentials/{liwan,muyuan,agentrouter}` 取 (size, mtime_ns, sha256) 差 ⇒ **`CHANGED KEYS: NONE`**，`invite-created` **1 982→1 982**。`gate-runner-unit` 29→36（7 条新用例：跳过不计数、env=1 真跑、`0`／`true`／空／`yes` 仍跳过、缺 reason 拒注册、变量名非法拒注册、红用例与跳过并存仍红、committed 清单里"恰好这两条"被标记）。**代价登记 `G39`（owner P3-06）**：那 2 484 条用例今天不在任何默认绿里。其余 8 个模块同日复跑，逐字不变：`skeleton` 22／173、`protocol` 6／98、`storage` 5／361、`projects` 5／21、`tasks` 7／151、`runtime` 1／5、`agents` 1／2、`terminal-core` 13／107。

**② `extraction.mjs --write` 原先会吃掉生成区之外的散文。** 本轮一次 `--write` 把 `docs/native/extraction/sessions.md` 从 147 行／25 883 B 打成 30 行，`手写约束` 八条＋三节 2026-09-29 普查全部消失；`--check` 只比对表格行，所以**门禁全绿与散文被删同时成立**。修法：生成区由 `<!-- generated:start/end -->` 圈定，`spliceGenerated()` 只换区内内容；**目标文件存在却没有两个标记 ⇒ exit 1 拒绝覆盖**，负例断言"文件逐字节不变"。丢失内容按 补记一百八十六／一百八十七／一百八十八 重铺到 `docs/native/extraction/sessions-corpus.md`（11 572 B，刻意不进 Trellis 注入清单：phase-3 注入 129 552 B＝131 072 的 98.8%，余量 1 520 B）。`extraction-unit` 9→15。

**③ 第 24 项（`G35`）按 ② 落地：两条 CI 前端腿转绿。** `pnpm exec prettier --write` 只跑那 17 个路径；`src/test/native-host-protocol.test.ts` 的 `type Row = Record<string, any>` 整体删除，改为按 `contracts/native/v1/*.json` 实际字段声明的 interface（`EnvelopeContract`／`MethodSurface`／`EventSurface`／`TopicSurface`／各 fixture 行形状），`parseFrame(...)` 用 `src/lib/nativeHostProtocol.ts` 导出的 `Frame` 联合窄化。**读数的不变量**：`it(` 声明 **21→21**、`expect(` **145→145**、`pnpm exec vitest run src/test/native-host-protocol.test.ts` **21 passed**、`native:check protocol` **6／98**（`protocol-ts-golden` 地板 21 未动）、`pnpm exec tsc --noEmit` rc **0**、`pnpm typecheck`（web＋mobile）rc **0**、`pnpm lint`（`--max-warnings 0`）rc **0**、`pnpm test` 全量当日两遍 **376 files／5 483 tests 全绿**（改动之后，与 09-28 基线同数）、`pnpm exec prettier --check "{src,packages,mobile/app,mobile/src}/**/*.{ts,tsx,css,json}"` rc **0**。唯一需要说明的写法变化：legacy-task 用例里 `sessionOwner` 的 `family` 字段被共享 `TaskRecord` 故意不建模（fixture 的 `unknownFieldSurvives: true` 就是这件事的证据），因此用 `LegacyTaskRecord = TaskRecord & { sessionOwner?: { family?: string; … } }` 在测试侧加宽，**没有**改断言值、**没有**用 `any`/`as any`/`eslint-disable`。第 24 项的后半（这两条要不要进门禁面）**仍未表态**——今天没加任何 case，CI 腿靠的是人工执行。

**当日一致性复算（改动之后）**：`inventory --check` consistent（gaps **38**／open 31／`taskStates=51/12accepted`）、`contracts --check` 12 份逐字节、`digest --check` **14 片／最大 20 827 B**（`chunks.gaps` 7→8，`digest-unit` 第 7 次 80% 否决）、`git diff --check` rc 0、四份 `extraction --check` 各 rc 0（两次变异各有命中：改 Action 引文 ⇒ rc 1 `no longer quotes Action line`；手改表内落点 ⇒ rc 1 `Action-target cell differs`；还原后 `cmp -s` 逐字节相同）。批次文件数 **38 → 48**。


## 聚合风险登记纳入比对，当场抓出两处已烂数字（实测 2026-09-29，MIGRATION_PLAN 补记二百三十九）

`tableDrift` 过去只比表格行与逐字 Action 引文，**从不看每张清单下面那节「Aggregate risk register」**——而那节每个数字都是生成数据。第一次去比就不一致：

| 清单 | 登记里写的 `lines` | 今日源码树 | 结论 |
|---|---|---|---|
| `dsh-webui.md` | **9 323** | **9 349** | 已漂，门禁仍绿 |
| `remote.md` | **9 977** | **9 962** | 已漂，门禁仍绿 |
| `sessions.md` | 11 226 | 11 226 | 未漂 |
| `database-notebook.md` | JSON 登记四个标量 | 全部一致 | 未漂；三个数组字段是 09-21 快照（`withSpawn` 3 vs 7、`withSecrets` 4 vs 21、`zeroPublic` 10 vs 5），不进断言、改由该文件手写节声明 |

修法与验证：`refreshRegister()` 让 `--write` 原地刷新七条 bullet（非该形状一律不动）；`registerDrift()` 让 `--check` 逐字比 bullet、JSON 形比四个标量；`extraction-unit` 15→**19**（新增 4 条含 3 类负例）。变异实测：bullet 改一个数字 ⇒ rc **1** `risk register line stale or missing`；JSON 标量改一个 ⇒ rc **1** `register pubSymbolsTotal differs (committed 651, generated 650)`；`cp`＋`cmp -s` 还原后 rc 0。修后数字：dsh-webui **9 349**、remote **9 962**。

两条自纠（都记进 补记二百三十九）：① 一次 edit 的边界自动修复吞掉了 `tableDrift` 的 `return problems`，CLI 直接 `TypeError` 四条门禁同时红——补 return 并加一条"永远返回数组"的用例；② 为还原 JSON 变异我误用 `git checkout -- docs/native/extraction/database-notebook.md`，而 HEAD 停在当日早上的 `797e06c`，把本轮刚写进该文件的标记／Action 列／caveat 一起回滚，已重做。**这批的正确还原手段是 `cp` 备份 ＋ `cmp -s` 断言。**

当日读数更新：`native:check skeleton` **22／177 rc 0**、四份 `extraction --check` 各 rc 0、`inventory` consistent、`digest` 14 片／最大 20 827 B、`contracts` 12 份逐字节、`git diff --check` rc 0、批次 65 个文件。
