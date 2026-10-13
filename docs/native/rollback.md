# Rollback boundary and rehearsal plan

> `MIGRATION_PLAN.md` Task P1-05 Action 3. This file records the baseline commit,
> the backup/restore rehearsal path and the per-phase switch. **No copy or restore
> of a real user directory was performed here**, and none is authorised by it.
> Companion: `docs/native/environment.md`, `.trellis/tasks/09-21-native-macos/implement.md`.

## Current Apple-only boundary (approved planning, 2026-10-12)

The current authority is the Apple-only baseline at the top of `MIGRATION_PLAN.md`
and ADR-039 §7. This is a plan, not evidence of a sandbox/data/signing cutover.
The original P1-era baseline, switch rows and observations below remain historical;
their words “currently”, “inert” or “no runtime code” are not today's facts and
never authorise deleting the current core, macOS tree or user work.

| Stage | Safe boundary | Required proof |
|---|---|---|
| AP0–AP7, before user-data cutover | Use isolated data; retain frozen old clients and source data. Do not stop existing production tasks or rewrite credentials. | Real sandbox/tool/permission evidence before relying on a new execution path; a failed probe does not authorise weaker security or a different product. |
| AP8 migration/rehearsal | Stop writers; make a consistent backup including DB/WAL/attachments; explicitly authorise import into the approved managed root; compare records and files. Re-login/re-pair is permitted where credentials cannot lawfully transfer. | Rehearse on an isolated copy first. Real paths, Keychain and device changes each need contemporaneous approval; source data is not deleted. |
| AP9 review, before old-client deletion | Keep the verified compatible fallback/recovery path while both App Store reviews are pending; do not equate a runnable window or signed archive with review acceptance. | Complete product journeys, migration/recovery results and both real review decisions. Approval to submit or release is separate. |
| AP10 retirement and later | Migrate shared Rust/vendor/remote/data dependencies first, then remove legacy UI/platform code under the approved gate. Recovery must use a tested native-version/data recovery path, not a deleted Tauri client. | Both store reviews and full engineering/migration gates passed; current clean builds and real changed paths verified after removal. |

Rollback never means downgrading a data format, restoring stale memory over new
writes, adopting a lost PTY master, deleting a lock, auto-killing an unverified PID,
or `git reset --hard`. Code may be recoverable from version control; that does not
reverse already-migrated user data, permission identities or distributed binaries.
The old-source backup and post-import changes must remain distinguishable so a
failed migration cannot silently discard work performed after the backup.

## Historical baseline (not the current recovery procedure)

| Item | Value |
|---|---|
| Branch | `main` (INV-01: no feature/fix/worktree branches for this project) |
| Baseline commit before any migration code | `ca69a4b` — `fix(sftp): drop unused Windows merge-check helper that failed clippy -D warnings` |
| Phase 1 committed (Q5 answered 2026-09-22) | `3091c8f` on `main` — 64 files / 32 748 insertions, local only: **not pushed, no tag, no release**. Reverting the migration now means `git revert 3091c8f` (or `git reset --soft ca69a4b` locally), which is recoverable in a way the previous single untracked copy was not. |
| Migration commits so far | none. Working tree carries P1-01…P1-05 output as **uncommitted** changes; nothing has been pushed, tagged or released |

> ### ⚠ Self-inflicted loss on 2026-09-21, and the rule it produced
>
> While demonstrating the `minCases` floor guard, `scripts/native/checks.json` was truncated
> to **0 bytes**. Cause: the cleanup step used `git checkout -- <file>`, which does nothing for
> a **still-untracked** file, and the `||` fallback then wrote `/dev/null` into it. It was
> recovered by reconstructing the file and re-running all 10 gate cases plus 18 tests to confirm
> identical behaviour — but recovery depended on the content still being held in the agent's
> context. It is not a durable mechanism.
>
> Standing rules adopted from this:
>
> 1. **Untracked files have no git safety net.** Never "restore" one with `git checkout`, a
>    redirect, `/dev/null`, or a pipe. Before any destructive verification, `cp <file> <file>.bak`
>    explicitly, restore by `cp` back, and delete the backup only after re-running the checks.
> 2. **Destructive gate experiments run against a temporary copy**, the way
>    `inventory.test.mjs` already does via `tempTree()` — never against the frozen artifact in
>    place. The floor demonstration should have followed that pattern.
> 3. Detection must not rely on the gate staying red: at the moment of loss the gate still passed,
>    because the config had already been loaded into the process. Size/parse assertions
>    (`wc -c`, JSON parse) are part of "did I just break something".
> 4. This is concrete evidence for **Q5**: everything in `docs/native/`, `scripts/native/`,
>    `core/` and `macos/` currently exists in exactly one copy on one machine, outside version
>    control. Committing to `main` (without pushing) is the only real mitigation.

| Pre-existing tracked-file edits | `.gitignore`, `package.json`. `package.json` is additive (8 `native:*` scripts). **`.gitignore` is not additive**: the `docs/native/` block was first extended, then its per-directory whitelist was *replaced* by extension-based rules (`!docs/native/**/` + `!**/*.md` + `!**/*.json`) after enumeration silently swallowed `docs/native/extraction/`. Reverting it to `ca69a4b` re-ignores every planning artifact — verify with `git diff .gitignore`, not `git diff --stat` |
| Untracked additions | `MIGRATION_PLAN.md`, `core/`, `macos/`, `scripts/native/` (incl. `digest.mjs`, `digest.test.mjs`), `docs/native/` (incl. `digest/` shards, `extraction/`) |
| Repointed, outside any diff | 21 `.trellis/tasks/**/{implement,check}.jsonl` now inject `docs/native/digest/*` instead of the two canonical digests. Deleting the shards without repointing them returns injection to a silently-truncating path; the gitignored Trellis tree is exactly where that break would hide |
| Deliberately not in a diff | `AGENTS.md`, `.trellis/`, `.claude/` — all gitignored in this repo (see D4). `AGENTS.md` **was** edited under the P1-02 approval; that edit is local to this machine until the ignore policy is decided |
| Reference repos | `../ZCode`, `../opencode`, `../MiMo-Code`, `../minimax-code`, `../primuse`, `../dbx` — never written to. `../dbx` has 38 dirty files belonging to the user; `checkout`/`reset`/`stash` there is prohibited |

## Switch inventory

A rollback is only meaningful if you know which knob turns what off.

| Phase | Knob that turns the new path off | Reverts to | Data written by the new path |
|---|---|---|---|
| P1–P2 | nothing to turn off; no production entry point exists | original Tauri client | none — read-only inventory plus fixtures copied into temp dirs |
| P3–P7 | do not launch `aeroric-cli serve` / Native Preview against `~/.aeroric`; they only accept a test `--data-dir` | Tauri client with stepwise embedded core | isolated temp dirs only; user data never touched |
| P8-05 onwards | set the embedded/external choice back to **embedded** in `src-tauri/src/core_adapter.rs`; the Tauri external adapter is the first rollback target | Tauri embedded core | the **same** host data format — rolling back the client must not roll back formats |
| P10 | keep Native Preview as a non-default entry point | Tauri as macOS default | as above |
| **ADR-039 / AP0–AP9** | Cancel an unexecuted switch, or use only a rehearsed compatible client/data recovery path; the old UI remains until the complete Q12 gate | Frozen legacy clients before retirement; no silent embedded fallback or signature downgrade | Source data remains intact; imported/post-import data follows the current schema, lease and explicit recovery protocol |
| **ADR-039 / AP10 and later** | Use the verified native-version recovery procedure; do not assume removed Tauri/Expo clients are available | A known-good compatible native version or an explicitly rehearsed restore | Never overwrite newer data or restore a stale full-memory snapshot; retained source/backup is not an automatic rollback destination |

Rules that survive every rollback:

1. **Roll back the client, never the data format** (§6.2). An old install that does not understand the new runtime metadata must never be pointed straight at a migrated directory.
2. Before any host upgrade or data migration: stop writers, back up, verify consistency with the DB backup API or a stopped-write window — never `cp` a live SQLite database with WAL in flight.
3. A host that really crashed cannot have its orphaned PTY master adopted by another binary. Tasks that lose their process are marked `interrupted` and resumed only through the family's supported session-resume path, on an explicit user action.
4. Never restore a stale full in-memory snapshot over newer writes.
5. `git reset --hard`, deleting a lock file to force progress, and auto-killing an unconfirmed PID are prohibited as rollback mechanisms (§5.4).

## Rehearsal path to execute later (not executed now)

`scripts/native/migration-rehearsal.mjs` and `docs/native/migration-rehearsal.md` are created in **P8-05**; the rehearsal it must perform:

1. Build a synthetic data dir (or a user-approved sanitised copy) containing projects, tasks, sessions, notebooks DB with WAL, MCP/hook config and attachments.
2. Old reader → new host → old UI/client compatibility: read-only pass, then a mutation pass, then re-read with the old reader and diff.
3. Fault injection: kill during a write, kill during spawn, corrupt the runtime metadata, drop the lock, disk-full on rename. Assert no truncated JSON and no second writer.
4. Lock contention: three clients cold-start at once; assert exactly one epoch, one lock holder, one watcher set.
5. Restore drill: back up, write new-format data, restore the backup, and confirm the old client reopens it and no cancelled task resurrects.
6. Only after the rehearsal passes **and** the user consents: stop old writers, back up the real `~/.aeroric`, verify the backup, start the host.

Any real-directory step above needs explicit user approval at the time; this document does not grant it.

## Historical P1–P4 abandonment assessment — obsolete, do not execute

The following was an early-stage observation, not a current deletion recipe.
Use the approved Apple-only boundary above; current runtime and user work must
not be inferred to be inert from this preserved paragraph.

Everything produced by P1–P4 of this migration is inert: deleting `core/`, `macos/`,
`scripts/native/`, `docs/native/` (including its `digest/` shards), reverting the two
tracked-file edits and restoring the original manifest pointers returns the repository
to `ca69a4b` behaviour with zero user-data impact, because no runtime code path has been
changed yet and no user directory has been written. "Inert" is not the same as
"additive" — the `.gitignore` block was rewritten, not only extended, and it is the one
tracked edit whose removal has a consequence (the planning artifacts stop being
committable). That property stops holding at P2-05, where the Tauri client starts
routing through the embedded core.

## 8. 2026-09-29 回源更正（MIGRATION_PLAN 补记二百三十四）

两条行话指向今天还不存在的东西，都写在本文档自己的表里。它们不是错误——是把未来写成了现在时，下一个读者会照着去找。

1. **Switch inventory 的 P3–P7 行写着「they only accept a test `--data-dir`」**。今天**没有任何二进制接受 `--data-dir`**：`core/crates/aeroric-cli/src/main.rs` 是 6 行（`wc -l`；`split('\n')`＝7；271 B）的 `exit(2)` 桩；`core/crates/aeroric-host/src` 只有 `lib.rs`，`Cargo.toml` 没有 `[[bin]]`；两棵树里 `clap`／`env::var`／`args()` 命中 **0**。`AppPaths::with_data_dir`（`core/crates/aeroric-core/src/paths.rs:47`）是留给 P3-07 的 API 而不是入口，**生产调用者 0**（`aeroric-core/src` 内每一处调用都在 `#[cfg(test)]` 之下，逐文件核过）。所以这一行的保护**今天成立的原因是"没有可启动的新入口"**，不是旋钮本身；断点在 P3-07，已作为具名前置写进 MIGRATION_PLAN §进度总表 的 P3-07 行：`--data-dir` 落地时必须同时规定缺省语义（不给就 `exit 2`，或显式确认后才准指向真实 `~/.aeroric`），否则 `AppPaths::from_user_home` 的兼容缺省（`paths.rs:38`，P2-02 已接受）会让这条边界在实现那天自动失效。
2. **P8-05 onwards 行写着「set the embedded/external choice back to embedded in `src-tauri/src/core_adapter.rs`」**。该文件今天 `embedded`／`external`（不分大小写）命中 **0**。它有的枚举是 `CoreMode{Writer, HandoverDeferred, ReadOnlyBusy, ReadOnlyLocation}`——ADR-021／027／010 的**写入者角色**，不是运行时选择。那个互斥旋钮由 **MIGRATION_PLAN Task P8-05 Action 2** 创建。在那之前，"把选择调回 embedded"是指向未来的指令，不是可以现在就拧的开关。

同时钉住三条**有实物**的，不需要更正：规则 3 的 `interrupted` 由 core `tasks/operations.rs:399/407` 与 `tasks/reducer.rs:550/555`（`interrupted_with_child`）产出，协议层 `task.rs:243/302` 定词并断言其属终态，`knowledge/task-parity-cases.json` 有 **68** 处、含 `transition:*->interrupted` 三条命名用例，Swift 金标 `ProtocolGoldenTests.swift:309`，React `appProjectState.ts:171/175` 与 mobile `SessionPane.tsx:183` 消费同一个词。规则 2 的备份语义在 `storage/tasks.rs:178`（`*.json.bak`）、`storage/atomic.rs:139`、`storage/projects.rs:51`、`storage/runtime.rs:172`，真实 DB 备份 API 的先例是 `src-tauri/src/database/connections.rs:756` 的 `.backup(DatabaseName::Main, …)`；停写由 `storage_lock.rs` 的租约与 `context.rs:255/278` 强制。规则 1 与本文 §6.2 第二条一字对应。规则 2 提到的演练脚本 `scripts/native/migration-rehearsal.mjs` 今天不存在（P8-05 Action 4 创建）——本文「Rehearsal path to execute later (not executed now)」那节本来就这么写的，同批尚不存在的还有 `scripts/native/prepare-tauri-host.mjs` 与 `src-tauri/tauri.macos.conf.json`，`checks.json` 也还没有 `cli`／`cutover` 两个模块（今天是 9 个模块）。
