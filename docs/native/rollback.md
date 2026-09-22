# Rollback boundary and rehearsal plan

> `MIGRATION_PLAN.md` Task P1-05 Action 3. This file records the baseline commit,
> the backup/restore rehearsal path and the per-phase switch. **No copy or restore
> of a real user directory was performed here**, and none is authorised by it.
> Companion: `docs/native/environment.md`, `.trellis/tasks/09-21-native-macos/implement.md`.

## Baseline

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

## What is currently safe to abandon

Everything produced by P1–P4 of this migration is inert: deleting `core/`, `macos/`,
`scripts/native/`, `docs/native/` (including its `digest/` shards), reverting the two
tracked-file edits and restoring the original manifest pointers returns the repository
to `ca69a4b` behaviour with zero user-data impact, because no runtime code path has been
changed yet and no user directory has been written. "Inert" is not the same as
"additive" — the `.gitignore` block was rewritten, not only extended, and it is the one
tracked edit whose removal has a consequence (the planning artifacts stop being
committable). That property stops holding at P2-05, where the Tauri client starts
routing through the embedded core.
