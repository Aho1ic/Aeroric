# Module review — skeleton (Phase 1, Task P1-04/P1-05)

> **数字口径（2026-09-29 加，审计出处 `MIGRATION_PLAN.md` 补记一百七十九）**：本文件里的 case 数／用例数／`minCases` 一律是**验收当天的快照**，只作证据留档，不是今天的地板值——模块在此之后被别的 Task 扩过。要看今天的数字：现场跑 `node scripts/native/check.mjs <module>`，或读 `scripts/native/checks.json` 各 case 的 `minCases`（`inventory --check` 会核对地板与冻结清单一致）。**既不要把这里的旧数字当成回归证据，也不要为了两边对齐而去改任何一份带日期的记录。**

**Gate status: `PASS` — accepted 2026-09-22.** 原头部长期写着 `BLOCKED: grill-me entrypoint not loaded in this session`，那是**技能装好之前的旧状态**：`grill-me` 链路先经 `Skill(grilling)`、后经用户安装的 `grillme-matt` 插件各真实跑过一轮（见下文 Actual invocation 与第十四次补记），Q1／Q5／Q6／Q7 也已逐一拍板并落成 ADR-002～005 + 门禁断言。这条过期声明本身就说明「阻塞字样没人回写」是什么现象，故原文保留在此句内而不删除。

Per `MIGRATION_PLAN.md` §4.1 and §5.4, this file records the real state instead of
substituting an agent self-review for the tool call. The skeleton may not be
declared accepted, and the Phase 1 gate stays open, until an actual `grill-me`
invocation is recorded in §"Actual invocation" below.

## 审查 commit 锚点（2026-09-29 反查补齐；§4.2 要求的这个字段当时没有记）

`MIGRATION_PLAN.md` §4.2 明文要求每份评审保存七项——**tool 来源/revision、实际调用入口、审查 commit/diff hash、问答、证据、问题级别、修复后复验结果**。本轮插入本节之前全量普查 `docs/native/reviews/`：**40 位十六进制串只有 2 处（插入之后本节自己会再引入锚点，那是预期结果），且两处都是外部技能 pin**（`c55ee46073ed923f86ce59a5eb3b6d895095d1b7`＝`mattpocock/skills` 的 grill-me，见 `docs/native/reference-provenance.md`）⇒ **九份载体没有一份记过被审查仓库的 commit**。这一节用 `git log -1 --until=<日期>T23:59:59` 反查补齐，**是事后重建，不是当时的记录**。

- Task **P1-04** accepted_on＝2026-09-21 ⇒ 当天最后一个提交＝`ca69a4bc278f5d230744464aff475c0feaf052e0`（ca69a4b，2026-09-21，「fix(sftp): drop unused Windows merge-check helper that f」）
- Task **P1-05** accepted_on＝2026-09-21 ⇒ 当天最后一个提交＝`ca69a4bc278f5d230744464aff475c0feaf052e0`（ca69a4b，2026-09-21，「fix(sftp): drop unused Windows merge-check helper that f」）

**局限（必须一起读）**：反查锚点只覆盖**当时已提交**的树；acceptance 常在脏树上完成，未提交部分**无法事后重建**，所以 §4.2 要的「diff hash」这一格今天仍然是空的。缺口已登记为 `G31`：今后的每一次 `grill-me` 必须**当场**写 `git rev-parse HEAD` ＋ `git status --porcelain` 的文件数与路径摘要（`docs/native/reviews/*` 的内容不被任何 case 解析，机器无法事后追补）。本文撰写时 HEAD＝`797e06c3c35ce9b28b53be3f0149894e6e038a8a`（797e06c 2026-09-28 11:04:54 +0800）。

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

1. **Q1** RPC naming — **ANSWERED 2026-09-22: A1′ (service-area prefix + recorded derivation)**,
   now `docs/native/adr/002-rpc-method-naming.md`. The question went through **two** corrections
   before the user decided: it was first framed as "strip prefixes and risk collisions" (false:
   all three options are collision-free), then I justified "keep `domain.name`" by claiming the
   name is derivable from `rust_path` by rule — also false, only **229/579** prefixes match the
   Rust module, and 11 rows use a `task/terminal.` style. The decided shape keeps the 22 authored
   service prefixes but *requires* `meta.rpc_resolution` (56 module→service rows + named,
   justified overrides) with `inventory --check` deriving every value, so the field stops being
   an unaccountable hand-written table. A transient "30 collisions" measurement was my own
   script bug and is withdrawn; re-derived: 0. Also corrected: "20% redundant prefixes" is
   184/579 ≈ 31% by the module reading.
   Original framing, kept for the record: it was
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
5. **Q5** Commit to `main` — **ANSWERED and executed 2026-09-22**: `3091c8f` on `main`, 64 files /
   32 748 insertions, **not pushed, no tag, no release**. The incident that justified asking — an
   untracked `checks.json` truncated to 0 bytes and recovered only by reconstruction
   (`docs/native/rollback.md` › Self-inflicted loss) — is what this removes: an accidental write now
   has a second copy again. Gate was re-run immediately before the commit (20 cases / 92 tests,
   exit 0) and again after it, so nothing depended on the files being untracked.
6. **Q6** Remote/WSL git check-module ownership — **ANSWERED 2026-09-22: ADR-004 (hybrid)**. `git` proves target-agnostic semantics from one shared fixture set (local/ssh/wsl as parameters); `connections` owns SSH adapter counterexamples, `platforms` owns WSL adapter + ADR-003 capability-gate negatives, and neither may re-implement git assertions.
7. **Q7 / G17** — **ANSWERED 2026-09-22: ADR-005 (typed full surface + per-row caller evidence)**. The question as first posed was invalid: `remote_mux.rs` holds no `#[tauri::command]` and dispatches no method table, and the 41 zero-evidence commands match 0 of the 117 distinct upstream names, so "typed vs one mux" was a false choice. `inventory --check` now requires every `dsh_webui::*` method to carry `proven:` or `pending: <existing Task>`.
   (Original open wording, kept for the record: the triage revision moved 42 →
   **41** genuinely unevidenced `dsh_webui::*` commands, since `list_dsh_agent_presets` turned
   out to have test evidence.

**Gate closed 2026-09-22**: 用户逐一回答后，决定已实现并复验（`pnpm native:check skeleton` = 20 case／100 用例 exit 0；12 条新负例覆盖命名推导、能力门控与 caller 证据）。原句为： Recording the questions is not the same as passing the review; §4.2 forbids writing unanswered questions as passes.

Rounds 11–13 added, beyond the cases listed above: `extraction-docs-current` ×3
(database+notebook / remote / dsh_webui — §5.1 row lists must be reproducible from source and agree with
`command-map.json`), `extraction-unit` (9), `invariants-unit` (13, covering INV-09 and patch parity),
and 5 category-derivation negatives inside `inventory-unit`. Round 13 also made this review's own
tooling claims honest in `adr/001-native-boundary.md`: its "`grill-me` unavailable → blocks Phase 1"
and "AGENTS.md native rules deferred" rows were stale and are corrected there.

## Late-phase audit of the gate runner itself (2026-09-27, MIGRATION_PLAN 补记一百零四)

Two defects in the tooling this module owns (`scripts/native/check.mjs`, §3.2's unified script
interface). Neither affects today's per-module verdicts; both are on the path to the final gate,
which is why they are recorded here instead of being fixed by a drive-by edit (§3.1 #1: a task
touches only its listed files; new dependencies go to the owning task).

**F1 — `native:check all` cannot run the matrix, ever.** Proved, not inferred:

- Today: `node scripts/native/check.mjs all` → `NOT READY: 38/43 planned modules are not
  gate-ready (…)`, **exit 3**. That branch is correct and intended (P10-05 owns the first full gate).
- The readiness check, though, is a *pre-flight only*: control then falls through to
  `config.modules[module]` with `module === "all"`, and there is no `all` key in `modules`
  (registered set is 9 modules). Temp-copy probe (`/tmp/allprobe`, a copy of `scripts/native/`
  with `plannedModules` emptied so the pre-flight passes, `modules` untouched):
  `native:check all — unknown module. Registered: agents, projects, protocol, runtime, skeleton,
  storage, tasks, tauri-adapter, terminal-core` / **EXIT=2**. The repo's own rule
  (`Refusing to treat an unmapped module as success.`) is what fires.
- Consequence: when the 43 planned modules are finally all gate-ready, the command §3.2 defines
  as "矩阵中全部必选模块、旧 Tauri/TS 回归、契约、平台兼容、签名检查" still does not execute
  anything. And the tempting "fix" — registering a module literally named `all` in `checks.json`
  with a hand-copied case list — is exactly the two things §3.2 and P10-05 forbid ("不能把 `all`
  偷改为'当前碰巧存在的测试'" / "不能靠删除矩阵行使总数达标").
- Required shape of the fix (owner: P10-05, before its acceptance can be attempted): when
  `module === "all"`, derive the run set from `Object.keys(config.modules)` after the readiness
  pre-flight, execute every registered module, aggregate failures, and fail if the derived set is
  empty or drops a registered/planned module. Negative tests belong in `gate-runner-unit`
  (floor 9) using the existing `tempTree()` discipline: all-ready config runs N>0 modules and
  exits 0; a config where the derived set is empty must fail; a red child module must fail `all`.

**F2 — two rows of §3.2's `all` have no registered case at all.** Enumerated the command surface
of all 68 registered cases (`node -e` over `checks.json`): `pnpm vitest run <named file>` appears
under protocol/projects/tasks/tauri-adapter, and `pnpm typecheck` under tauri-adapter, so
per-feature TS coverage exists — but **no case runs the full `pnpm test`**, and **no case runs
`cargo check --target …`**. So:

- The "旧 Tauri/TS 回归" half of `all` is only covered feature-by-feature: a TS test file that no
  module names is invisible to every gate. The 376 files / 5483 tests number quoted in most
  补记 entries is a *manual habit*, not a machine gate — honest status, but it must not be read as
  coverage.
- The "平台兼容" half has zero gate coverage. P2-05 recorded that the Linux/Windows `cfg` branches
  were only ever verified by hand (`cargo check --target`), and nothing kept that from rotting
  since. This belongs to P7-02 (WSL 与跨平台运行时保留) as a registered case, not to memory.

Both rows are cross-checked against the plan: see `MIGRATION_PLAN.md` P10-05 移交项 and P7-02.

**F3 — §1.2 / §6.1 要求的"最终状态"字段结构性未满足，且正在静默腐烂（2026-09-27）。** 全部实测（`node -e` 直接读 `docs/native/feature-matrix.json`）：

- `status` 分布：`planned` ×22 + `planned-new-capability-no-existing-command` ×4；**`final_state` = `unstarted` ×26，一条未动过**。
- 这与已发生的事实矛盾：F01（P2-03 **accepted**）、F02（P2-04 **accepted**）、F03（P3-02 **accepted**）、F04（P3-03 实现面收口、门禁 11/92）——四行的核心半边都已交付，矩阵仍说 `unstarted`。
- **`review` 根本没有这个字段**：26 行的键集是 `id, capability, existing_or_reference_evidence, target, task_ids, owner, commands_mapped, positive_test, negative_test, status, final_state`，而 §6.1 那条勾选要求"F01–F26 的所有子项有 owner、文件、测试、**review**、最终状态"。`docs/native/reviews/` 下已有 **8** 份模块评审（skeleton / protocol / storage / projects / tasks / tauri-adapter / runtime / agents），数据在，**矩阵与它零关联**。
- **门禁看不见它**：`inventory.mjs` 对 features 只做两件事——`:111` 校验命令引用的 `F` 号存在于矩阵、`:402` 数一行 `features.length` 打进日志（`features=26`）。**没有任何一条检查读过 `status` / `final_state`**，所以它腐烂至今、`inventory --check` 一路 consistent。
- **根因是结构性的，不是"忘了填"**：逐 Task 的权威状态（`accepted` / `in_progress`）**没有任何机器可读载体**——它只活在本文 §进度总表 的 prose 表格里；`checks.json` 只有 `registeredBy: P2-03`（模块→任务，不含状态），`.trellis` 只到 Phase 粒度（`phase-1/2` 的 status 今天才由补记一百零三补上，见父任务 D17）。**没人能从现有工件里派生出 `final_state`**，所以手填的列一次性冻结了。

**要求的修法（照本仓 D14/D16 的先例：让生成器取代手维护列，而不是再手改一遍）**：

1. 建一个逐 Task 状态的**规范源**（最小形状：`docs/native/task-status.json`，每行 `{task, status, accepted_on, review}`），由 `inventory.mjs` 与本文 §进度总表 **交叉校验**——表格改了 JSON 没改就 FAIL（照 `extraction.mjs` 的 `--check` 双向回读口径，并带字节级 noop 断言）。
2. `feature-matrix.json` 的行改为**派生字段**：`final_state` 拆成 `core_state` / `native_state`（按 `target` 的 C/N 两半边各由对应 Task 状态推出），新增 `reviews: [...]` 关联实际存在的评审文件；`inventory --check` 拒绝"行里写了未存在的 review 路径"与"Task accepted 而 core_state 仍 unstarted"两类漂移。
3. 门禁：`skeleton` 模块新增一条 case（或扩 `inventory-unit`，现地板 40）含 ≥4 条负例——漂移必红、未存在路径必红、空派生集必红、"读不到规范源就报错"不许退化成"当作没变化"（D12 的同一条纪律）。

**处置**：**不顺手改**（§3.1 #1；它需要新工件 + schema 变更 + 门禁 case，超出 P3-03 的文件面）。已作为 **Q7** 提交用户排期，并已在本文 §6.1 那条勾选旁写明"今天不可当进度读"。在修法落地前，任何"矩阵显示 unstarted"或"矩阵显示 X 条 planned"都**不得**被引用成迁移进度证据——进度证据只有 §进度总表 与各模块 review。

**F3 已闭合（2026-09-28，补记一百零九，用户 Q7 答复 A）。** 上面三条修法逐条落地：① 规范源 `docs/native/task-status.json`（51 条 Task）与 `inventory.mjs` 双向交叉校验，`--check` 与 `--freeze` 都读它，`--freeze` 在镜像输入互相矛盾时**拒绝写出**；② `final_state` 从 26 行删除，代之以 `core_state`/`native_state`/`side_states`/`reviews` 四列（`--freeze` 派生、字节级 noop 断言覆盖），哨兵词表分 `not-declared`（该行 target 不含这一半）与 `no-task-declared`（含而无 Task，判为输入不一致）；③ `skeleton` 新增 case `task-status-unit`（`minCases`=`runner.floors`=18，12 条反例，首条即 D12"读不到规范源必须报错"）。当天实测：`skeleton` **21 case／136** exit 0、`--check` consistent（`taskStates=51/12accepted`）、`--freeze-check` exit 0、`digest --check` 重切后 exit 0、`terminal-core` 13/107 不动。

两条留给下游的事实：**(a) 派生表本身会错**——上一会话写的 `meta.side_phases` 不含 Phase 6/9 与 K/A 的第二 Phase，`--check` 当场报 10 条 `side 'C' has no Task among`；现按 26 行全枚举改死并把判据写进 `meta.side_phases_evidence`，findings 非空即拒绝冻结，这类错从此是红的。**(b) 我写的第一版负例是空洞的**——三条反例只直接调 `carrierCrossCheck`，没走 `--check`，于是"摘掉 problems.push" 的变异体首轮 18 条全绿；改成经 `check()` 断言各自分支原文后拿到 **3 红**。同批查明：临时树里 `result.ok===false` 单独当断言不成立（缺 `src-tauri` 源文件本就会因 `stale source path` 变红），一律改断具体消息文本。证据与逐条对照在 `docs/native/reviews/terminal-core.md` §4.2。

**F6 — 「被 --freeze 照带的字段」是第二个盲区家族：9 条注入抓到 3 条，已当场闭合（2026-09-28，补记一百二十五）。** 注入矩阵第二轮专打这一族：`--freeze` 设计上要把人写字段原样带过去，代价是这些字段从此再没人核对。实测红：panel `target_scene` 清空、删一条动态站点、`events[].gap_id=G99`、`commands[].feature_ids=F99`、`task-status.json` review 指向不存在文件、topic `legacyChannels` 未知（`contracts --check`）。实测盲：`panels[].at` 行号（exit 0）、`panels[].task_ids=P9-99`（exit 0）、**`event_dynamic_emit_sites` 的路径被改写但条数不变（exit 0，旧判据只比 `.length`）**。闭合方式：`inventory.mjs` 加三个纯函数（`dynamicSiteProblems` 集合双向比对／`panelDriftProblems` 派生列 `key,via,at` 与 `extractPanels` 对账／`panelTaskIdProblems` 复用 gap 的 Task universe），负例 +6，旧用例中那条**断言旧形状（只比两个计数）**的拆成「空清单不得空转」＋「改写路径但条数不变必须红」；`inventory-unit` `minCases` 与 `runner.floors` 同批 46→53（实测 `ok (53 cases, min 53)`）。复注入三条全部 exit 1 且给出精确证据，`command-map.json` `cmp` 还原一致。**先证数据没错再补守卫**：`git show HEAD:` 副本逐条回源 ⇒ panel `at` 19/19、动态站点 29/29 命中（第一版脚本把常量派生的 `ALL_AGENT_CONFIGS_NAV_KEY` 误判 stale，读 `extractPanels` 后确认是判据粗糙）。所以这轮修的是「守不住未来」。

**F5 — 门禁有效性矩阵：12 条注入变异，10 红 2 盲（2026-09-28，补记一百二十四）。** 不再用「读代码判断它会红」，而是对每个派生工件注入错值、跑它自己的 checker、取退出码、`cmp` 还原。覆盖：契约产物、`commands[]` 的 category／definition_resolved／check_module／rpc_method、`task_ids` 区间、`task-status.json` 状态翻转、派生列 core_state／side_states／reviews（这些消息还直接给出 `--freeze` 补救）——**10 条全部当场红**。两条盲的：**`command-map.json` 顶层 `modules` 表的 `category` 与 `core_service` 改成错值，`inventory --check` 仍 exit 0**。这张表 53 条、被切成 `digest/command-map-modules.json`（15 405 B）注入 phase-3 会话，是 §5.1 展开与下游路由的读取面，却是同文件里唯一没有派生断言的表；今天它恰好自洽（52 模块 × 4 列全命中其命令行值／`module_default`）。更深一层：`modules` 用 `(crate_root)` 收纳根级命令，而 `category_resolution.module_default` 把这 4 条根级命令逐条当成模块（`module_default["(crate_root)"]` 不存在）——**两种「模块身份」粒度不一致**，所以任何朴素断言要么因这 4 条永久红，要么被人放松成「匹配任意一个」而失去意义。处置：不代定口径，已写进 §进度总表 P3-05 行与 Task P4-02 移交项；判据本身等口径确定后一并实现（四列：`category` 必须等于 `module_default`，其余三列必须命中其命令行的值集合）。刻意不新建 gap 编号（`G18` 在散文里已被家族 pin 占用）。

**F4 — 台账里"owner 已关门"这一整类失效（2026-09-28，T1 轮）。** 同一个失效模式的第四个实例：`feature-matrix.json` 的 17 条 gap 有 `task_ids`（人写）、Task 有状态（`docs/native/task-status.json` 才有），但**没有任何一条检查把两者对上**，所以 `G03` 挂着已 accepted 的 P3-01、`G05` 干脆写成一条区间 `P2-02~P2-05`（四个 Task 现在全部 accepted）而两条 gap 都还是 `open`——`inventory --check` 一路 consistent。修法照 F3 的先例（生成/校验取代人记）：`check()` 里加四条判据，`open` 而 owner 全 accepted 且无 `accepted_by` 即红、`~` 区间即红、点到不存在的 `### Task` 即红、空 owner 即红；`accepted_by` 是**具名逃生门**，谁接受风险必须写进工件。负例 6 条进 `inventory-unit`（地板同批 40→**46**），变异 `if (false && …)` 恰好红 1 条守它的用例。**当场被规则逼出两笔真实改判**：G03 按当天核对重派 P3-04+P7-06（三处 `thread::spawn(run_loop)` 仍在 `event_watcher.rs:66`/`usage_index.rs:1048`/`notebook/sync/daemon.rs:404`，P3-01 交的是机制不是这三站），G05 按当天 grep 把五项核成"三处仍在 + 两处不是缺陷（`taskId.ts` 铸 uuidv7＝ADR-013、`orderIndex` 权威在 P2-03）"并点名 P3-07/P3-05/P8-03。**一条自曝**：新负例首跑 45/46，红的是我自己的正则 `/P2-01\(.*\) is accepted/`（括号实际包的是整份 id 列表），改正则而非改规则。另记一处**同类但更尖锐**的矛盾：`G01.task_ids` 写 `P2-01, P3-05`，而 `agent-tool-install-progress` 的事件级 note 点名 P3-02 处置，两份工件各指一个 Task——已按新规则派给 P8-02 并写进 ADR-007 修正案。

## absent 门禁「牙齿」电池（2026-09-29 实测，登记 `G30`）

**为什么做**：`gate-config.mjs:56-60` 强制每条 `kind:"absent"` case 都带 `controls`，但那只证明「被扫的语料读得到」；**没有任何机器检查证明那条「禁止正则」还会开火**。正则改错一个转义、删掉一个 alternation 分支，这条 case 就变成永不 FAIL 的空门禁，`pnpm native:check` 整模块照样全绿——这正是「门禁绿 ≠ 约束还在」的形状。

**怎么复跑（规则可机械重建，不依赖任何临时脚本）**：① 从 `scripts/native/checks.json` 取每条 absent case 的 `pattern` 与 targets；② 按**顶层 `|`**（括号深度为 0 处）切分 alternation 分支；③ 每个分支渲染成一条「真实违规写法」——`\.` `\(` `\)` `\[` `\]` `\|` `\s` `\"` 还原为字面量、`[a-z-]+`→`abc`、`(_os)?`→`_os`、`\b` 删（零宽）、开头的 `^` 删（样本写第 0 列）；渲染后仍含 `( ) [ ] ? * + { } |` 的分支改用手写样本；④ 对样本文件跑 `rg -n --no-ignore <原 pattern> <样本>`，**期望 rc=0**（不命中＝这条分支没牙齿）；⑤ 再跑该 case 自己声明的 `controls`，期望 rc=0。

**今天的实测（2026-09-29）**：14 条 case／**58 个顶层分支**——41 个由上面那条规则自动渲染，17 个走手写样本（14 条不同的手写样本共展开 20 条，其中 `\.emit\(`／`\bAppHandle\b`／`\bWebviewWindow\b` 在 `projects` 与 `tasks` 两条 case 各跑一次）⇒ **64 条 canary，64 条全部命中，0 条分支无法渲染**；顺带把这 14 条 case 声明的 **44 条 `controls` 全部重跑，44/44 仍命中**。所以**今天不存在空门禁**，`host-exit-settlement-goes-through-core` 那类「宿主不得自己判状态」的禁正则也都还在射程内。手写样本依次是：`tauri = "2"`、`wry = { version = "0.45" }`、`webkit2gtk = "2.40"`、`cocoa = "0.26"`、`objc2 = "0.5"`、`tauri-plugin-shell = "2"`、`env::var("HOME")`、`env::var_os("HOME")`、`child.kill();`、`Command::new("kill").arg("-9").spawn()`、`app.emit("evt", payload)`、`invoke("some_command")`、`String::from_utf8(&combined[..]).unwrap()`、`if exit_ok || timed_out { settle() }`、`if cols < 2 || rows < 2 { return Err(Frozen) }`、`let p = pty.spawn_command(cfg)?;`、`let r = stream.try_clone_reader();`、`let w = stream.take_writer();`、`let w: WebviewWindow = 1;`、`fn x(handle: AppHandle) {}`。

**顺带量到的一处结构性弱点（今天不是活漏洞）**：`skeleton/inv02-core-manifest-denylist` 的 pattern 以 `^` 锚定，依赖键必须写在第 0 列。仓库当前风格确实写在 0 列——`core/crates/aeroric-core/Cargo.toml:25-26` 是 `[target.'cfg(unix)'.dependencies]` 紧接未缩进的 `libc = "0.2"`，而 `rg -n --no-ignore '^[[:space:]]+(tauri|wry|webkit2gtk|cocoa|objc2|tauri-plugin-[a-z-]+)[[:space:]]*=' core/` → 0 命中。但对照实验证明缩进写法会静默逃逸：同一行加四个空格缩进 → `rg` rc=1（扫不到），去掉缩进 → rc=0（命中）。

**修法已登记为 `G30`**（owner：`P3-04` 落第一刀、`P10-05` 收口）：给每条 absent case 增加 `canaries` 字段并由 `check.mjs` 在真实扫描之前逐条跑——**canary 不命中即该 case FAIL**（禁止把「扫不到」当成「干净」），`gate-config.mjs` 的校验器像今天强制 `controls` 一样强制 `canaries` 非空；同批把 denylist 锚定改成 `^\s*(tauri|wry|webkit2gtk|cocoa|objc2|tauri-plugin-[a-z-]+)\s*=`。负例 ≥4 条：删掉 `canaries` 报错、canary 永不命中 ⇒ case 变红、缩进依赖键能被抓到、把禁正则改错 ⇒ 对应变红而不是变绿。

**本轮门禁复跑**：`node scripts/native/check.mjs skeleton` → **22 case／160 用例 exit 0**（台账从 28 扩到 29 条 gap 之后，`digest-shards-current` 与 headroom 两条仍在射程内；最大片 24 033 B／32 768 B 上限＝73%）。

## §4.2 七字段落点（2026-09-29 反查补齐，出处 `MIGRATION_PLAN.md` 补记一百九十二；本节只回答「每个字段落在本文哪一处、哪一格当时是空的」，不重述内容）

- **tool 来源/revision**：「Tool provenance」表（来源仓库、MIT、pin `c55ee46073ed923f86ce59a5eb3b6d895095d1b7`）——与 `terminal-core` 并列的两个自带者。
- **实际调用入口**：「Actual invocation」一节，且「Why the invocation failed」把**第一次调用失败**原样留着（不是只留成功的那次）。
- **审查 commit/diff hash**：见上面「审查 commit 锚点」一节——commit 由 `git log -1 --until` **事后反查**，diff 句柄**当时没记、今天补不回来**（缺口＝`G31`）。
- **问答**：各节的 Q1～Q7 与 F1／F2 处置；门禁面问题的问答都在这里。
- **证据**：各节命令与 exit／case 数；「absent 门禁『牙齿』电池」给了可重跑的规则与 20 个手样。
- **问题级别**：**九份里唯一写了级别的一份**：Late-phase audit 的「Finding raised by the review itself (severity: medium, pre-existing, now gated)」(第 127 行)。**接受人**：F1 已修并由用户追认；F2 明确移交 `P10-05`／`P7-02`，未代接受。
- **修复后复验**：2026-09-29 `native:check skeleton` **22／160 exit 0**（含 `digest-unit` 17 条——上一轮台账改动正是被它拦下的一次红，补记一百八十九）。
