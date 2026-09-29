# Reference provenance and licensing gate

> Required by `MIGRATION_PLAN.md` §7 final paragraph: before any reference code is copied,
> its `LICENSE` / `NOTICE` / usage limits and contribution origin must be registered here.
> This is a **provenance governance gate, not a legal opinion**. If a case is not clearly
> covered below, the answer is "implement independently", not "copy and hope".
> Written 2026-09-21. Every license statement below was read from the file named in the
> "verified from" column at the recorded HEAD — none was inferred from the repository's name
> or from the web.

## Default posture

Learn the architecture; re-implement. Never copy trademarks, music or media assets,
commercial account configuration, signing teams, private credentials, or bundled
third-party components whose own licence differs from the repository root licence.

Copying a *snippet* is allowed only when all four hold: (1) the source licence permits it,
(2) the origin and commit are recorded in §3, (3) any required notice is retained in the
destination file, and (4) the copied content is not one of the exceptions listed in §2.

## 1. Reference repositories

| Repo | Local HEAD | Root licence | Verified from | Notes that change what may be copied |
|---|---|---|---|---|
| `../ZCode` | `872ad96` | **Apache-2.0** | `ZCode/LICENSE` (11 344 B) | Apache-2.0 §4 redistribution requires **retaining the NOTICE**. `ZCode/NOTICE.md` exists and is a Chinese-language third-party component statement covering runtime forms, permissions, storage locations and network behaviour — it must travel with anything taken. Also grants a patent licence, which MIT does not. |
| `../opencode` | `0e3dfd1769` | **MIT** | `opencode/LICENSE` (1 065 B), `Copyright (c) 2025 opencode` | Plain MIT. Note aeroric's own remote/E2EE layer predates and is independent; do not re-license Aeroric code by importing MIT text headers wholesale. |
| `../MiMo-Code` | `1592084b` | **MIT** (dual copyright) | `MiMo-Code/LICENSE` (1 114 B): `Copyright (c) 2026 MiMo Code, Xiaomi Corporation` **and** `Copyright (c) 2025 opencode` | It is a **derivative of opencode**. Two copyright holders must be preserved if anything is taken. §1.1 already records that shared origin is counted once for adoption purposes; for licensing the two notices are still two notices. |
| `../minimax-code` | `73a2581` | **MIT**, explicitly *not* all files | `minimax-code/LICENSE` (1 069 B) + `minimax-code/LICENSE-STATUS.md` | `LICENSE-STATUS.md` states in terms: *"The root MIT license is not a statement that every file or bundled component is MIT licensed."* Named exceptions: `third_party/sandbox-runtime` stays **Apache-2.0** with retained upstream copyright and modification notices; Pi, derived terminal code, the model catalog and bundled assets **retain their original declarations**; `release/dependency-licenses.json` records declared licences without altering terms. Treat any file under those paths as **not copyable**. Public source also does not contain its desktop client (§1.1), so no GUI claim may be made about it. |
| `../primuse` | `4d07ac42` | **MIT** | `primuse/LICENSE` (1 063 B), `Copyright (c) 2026 Welape` | MIT covers code, not trade dress. §1.1 already excludes its music business content, App Groups, store signing and private view-hierarchy workarounds; additionally its name/branding/icons are not reusable. |
| `../dbx` | `79042315661307745edb0c20763ea4390e97e3d6` | **Apache-2.0** | `dbx/LICENSE` (11 357 B) | **Not a reference repo — a build dependency**, pinned by `scripts/dbx-ref.txt` and patched by `patches/dbx-security.patch`. Its `vendor/tiberius` and `vendor/pageant` are consumed through `[patch.crates-io]` in both `src-tauri/Cargo.toml` and `core/Cargo.toml` (kept identical by `scripts/native/check-patches.mjs`). Apache-2.0 NOTICE obligations apply to shipped binaries. `../dbx` has 38 dirty files belonging to the user; the migration must never `checkout`/`reset` them. |

## 2. Copy-prohibited categories (regardless of the root licence)

- Trademarks, logos, product names, app icons and marketing copy of any of the above.
- Music/media/bundled asset files (primuse), model catalogs and bundled assets (minimax-code),
  and anything under a `third_party/` path carrying its own declaration.
- Commercial account configuration, signing identities / teams, provisioning profiles,
  App Group identifiers, private keys, tokens, credentials of any kind.
- minimax-code `third_party/sandbox-runtime`, Pi, and derived terminal code (see §1).
- Any file inside the five reference repos: they are read-only research material. No writes,
  and none of their install scripts are executed.

## 3. Copy log

| # | Date | From (repo @ commit, path) | To (path) | Licence | Notice retained | Reviewer |
|---|---|---|---|---|---|---|
| — | — | **Empty. No reference code has been copied as of 2026-09-21.** | — | — | — | — |

Everything produced so far in `core/`, `macos/` and `scripts/native/` is independently
written for this repository. Append a row **before** the copy lands, not after.

## 4. Third-party artefacts already introduced (non-code)

| Artefact | Source | Licence | Where it lives | Provenance evidence |
|---|---|---|---|---|
| `grill-me` skill text | `mattpocock/skills` @ `c55ee46073ed923f86ce59a5eb3b6d895095d1b7`, `skills/productivity/grill-me/` | **MIT** | `<project>/.claude/skills/grill-me/` and `~/.mirasim/skills/grill-me/` (symlinked via `~/.claude/skills`) | sha256 + byte sizes in `docs/native/tooling.md` §2; licence confirmed via GitHub API |
| `grilling` skill text | same repo/revision, `skills/productivity/grilling/` | **MIT** | as above | as above; delegation chain recorded in `tooling.md` §3 |

Only the two skill texts were taken, at a pinned revision, fetched with `curl -fsS` rather than
transcribed. No code was copied from that repository.

**Pin re-verified byte-for-byte 2026-09-29 (补记二百二十四).** The nine `docs/native/reviews/*.md` carriers all
answer §4.2's "tool 来源/revision" field with *the same* handle `c55ee46073ed923f86ce59a5eb3b6d895095d1b7` pointing
back here; **7 of 9** then add "当时未记" (all but `skeleton.md` and `terminal-core.md` — `grep -c "当时未记"` per
carrier gives 1/1/1/1/0/1/1/1/0), so this row is the only place that claim can be checked. Re-runnable
recipe (four GETs, no credentials, public raw endpoints):

```bash
B=https://raw.githubusercontent.com/mattpocock/skills/c55ee46073ed923f86ce59a5eb3b6d895095d1b7/skills/productivity
for p in grill-me/SKILL.md grilling/SKILL.md grill-me/agents/openai.yaml grilling/agents/openai.yaml; do
  curl -fsS -o /tmp/up "$B/$p" && shasum -a 256 /tmp/up | cut -c1-12
  shasum -a 256 ".claude/skills/$p" | cut -c1-12
done
```

Today every pair is identical: `caaf8b8de168` 157 B (`grill-me/SKILL.md`), `10ff989e7498` 1 987 B
(`grilling/SKILL.md`), `c061e39c3e0f` 137 B and `1411d7df7d99` 113 B (the two `agents/openai.yaml`). All four
returned HTTP 200, so **the pinned revision is still fetchable and the installed copy has not drifted**.
The same four hashes were also taken from the *global* skill root `~/.mirasim/skills/…` (the path that
`~/.claude/skills` symlinks to — `lrwxr-xr-x … /Users/macbook/.claude/skills -> ../.mirasim/skills`; the project's
own `.claude/skills/` is a real directory, not that symlink) and match the upstream files exactly —
**three copies (upstream / project / global) agree byte-for-byte**.
Note the installed set is **four files**, while §4 above says "only the two skill texts" — the two
`agents/openai.yaml` (137 B / 113 B) are byte-identical to files that exist at those same pinned paths upstream,
so they came along with the skill directories (this row proves *content identity*, not the install command).
The wording above is not wrong about *code* (none was copied from that repository), but "two texts" understates
the file count, so a future reader should take this row as the authoritative inventory:
**4 files, all from `skills/productivity/{grill-me,grilling}/`, same revision.**

## 5. Planned dependencies — must be re-verified at introduction time

Not yet added; each needs its licence, version and provenance recorded here **before** it
enters a manifest.

| Candidate | Purpose | Verification owed |
|---|---|---|
| **SwiftTerm** (W4) | native AppKit `TerminalView` renderer for P5-01 | licence and version at the pinned revision; its own bundled `Terminal.swift` tables; confirm it does not force a spawn-based usage (`LocalProcessTerminalView`) that would violate the "one writer, host owns the PTY" rule |
| **XcodeGen** (W5) | generate `AeroricNative.xcodeproj` from `macos/project.yml` | already installed at **2.46.0** as a *build-time* tool, not a linked dependency; record its licence only if it becomes vendored |
| SQLite vector extension dylibs | notebook RAG (P7-06) | these are loaded at runtime by existing code; licences and redistribution terms must be registered before any change to bundling or signing |

## 6. Rules for this file

1. A new row in §1 or §3 is written **before** the first copy, not retroactively.
2. "I read the architecture" is not a licence claim; §1 must cite the file and HEAD that prove it.
3. Where a repository's root licence and a sub-path's licence disagree, the **sub-path governs** and that path is not copyable (§2).
4. This file records provenance. It does not assert that any given use is lawful; that judgement belongs to the user and, where material, to counsel.

## 7. Verification pass on §9.1 boundary claims (2026-09-21)

The `.trellis/.../research/reference-map.md` table lists upstream behaviours that constrain
Phase 9 (`G20`–`G23`). Those lines were inherited from the plan's earlier review, not measured
by this migration. `MIGRATION_PLAN.md` §1.1 requires each research pass to record repository
HEAD, file, symbol, behaviour, adoption point **and unverified items**, and to note whether the
reference worktree is dirty — a HEAD alone does not prove the working content matches it. Both
gaps are closed here.

**Worktree state at measurement time — all five clean, so the citations below describe committed code:**

| Repo | HEAD | Branch | Dirty | Untracked | Commit date |
|---|---|---|---|---|---|
| ZCode | `872ad960de7e…` | `main` | 0 | 0 | 2026-09-21 05:14 +08:00 |
| opencode | `0e3dfd176944…` | **`dev`** | 0 | 0 | 2026-09-21 02:15 Z |
| MiMo-Code | `1592084b2e1f…` | `main` | 0 | 0 | 2026-09-20 23:52 +08:00 |
| minimax-code | `73a2581c6c75…` | `main` | 0 | 0 | 2026-09-20 22:31 +08:00 |
| primuse | `4d07ac42c4b1…` | `main` | 0 | 0 | 2026-09-20 19:23 −07:00 |

opencode sits on `dev`, not `main`; that is recorded rather than silently assumed. All five HEADs
match the values cited in §1 and in the plan's §1.1 table.

**All 16 file paths cited by the §9.1 table exist.** Per-claim result:

| Claim that constrains Phase 9 | Status | Evidence |
|---|---|---|
| minimax ACP adapter does **not** advertise image / audio / embeddedContext | **verified exactly** | `packages/tui/src/acp/agent.ts:447-450` → `promptCapabilities: { image: false, audio: false, embeddedContext: false }`; factory at `:76` |
| minimax sandbox **fails closed**, never silently degrades | **verified** | `local-sandbox-service.ts:339, 381, 385, 393` throw `SandboxError('SANDBOX_UNAVAILABLE', …)`; `backend/selector.ts:29, 37, 52` throw on no candidate / unsupported field |
| minimax tools lease carries **audience + scopes + generation** | **verified** | `lease-broker.ts:136-137` (`audience: AUTH_LEASE_AUDIENCE`, `scopes: AUTH_LEASE_SCOPES`), `:75, 88, 99, 115` generation/epoch/admission gating, `:239-240` scope identity check |
| minimax OAuth refresh has **cross-process ownership and generation** | **verified** | `oauth-core/src/auth-core.ts:11` imports `CrossProcessAuthLock`, `:154, :166` hold it, `:130, :135, :145` `generation: number`, `:178-179` states |
| minimax plugin enable/import is **prepare / commit / rollback** | **verified** | `plugin-system.ts:88, 91, 94, 96` (`preparePluginPublication`, `commitPreparedPluginPublication`, `preparedPluginPublicationFinalizer`, `rollbackPreparedPluginPublication`), `:200` wiring |
| MiMo goal state has an in-memory owner | **partially verified** | `session/goal.ts:113, 119` build a `Map<string, Goal>` store keyed by session; `:93, 144` `evaluate`. **Not established:** whether anything is reloaded on restart (`:163` references a "persisted-parts" path). The plan's phrasing "cannot promise cross-restart recovery" stays correct; the stronger claim "memory only" is **not** established |
| MiMo checkpoint has its own writer and advances a watermark | **partially verified** | `session/checkpoint.ts:468` `tryStartCheckpointWriter`, `:453` outcome type, `:204, :546` watermark. **Not established:** "advances the watermark only on success" — the success-gating line was not located, so P9-02 must verify it directly rather than rely on this row |
| MiMo task tree / actor spawn / workflow / auto-dream / voice rows | **file exists; behaviour not re-derived** | paths confirmed; the constraints they justify (`external node keys`, `shared cwd needs a conflict policy`, `side effects not exactly-once`, `explicit enablement`, `provider-specific ASR`) remain **inherited claims** |

### Consequence for the migration

1. The four **verified** rows may be used as design constraints now — in particular ACP must
   **reject** image/audio rather than drop attachments, and a sandbox that is enabled but has no
   working backend must fail closed in `core` (`tests/sandbox_fail_closed.rs`, P9-04).
2. The two **partially verified** rows must be re-checked by the P9-02 implementing agent before
   anything is built on them; the specific sentences to confirm are named above.
3. The remaining §9.1 rows are **not verified by this migration** and must not be cited as
   settled constraints. Re-deriving them is Phase 9 work, one file at a time, per §1.1 — not a
   claim of having read every line of five repositories.
4. Reference repositories remain read-only: nothing was written, no install script was run, and
   the clean worktrees must stay clean.
