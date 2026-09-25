#!/usr/bin/env node
// tauri-adapter invariants (Task P2-05): the host-side ADRs that must not regress silently.
//
// Three rules are mechanically checkable today. Everything else ADR-020…032 decided is either
// not yet implemented (and therefore *not* registered as a passing case — see the module note in
// checks.json) or is covered by Rust tests through the case-count floor.
//
//   ADR-020 + ADR-032  run identity comes from core's per-task generation only. The startup-signal
//                      counter survives solely as an entry version stamp, read inside two
//                      functions of pty.rs, and the old process-global identity name is gone.
//   ADR-031            a session path may be un-claimed in exactly one place, the reverse-lookup
//                      helper, because releasing it while another entry still reads it lets two
//                      agents write one transcript.
//   ADR-021            the writer role is handed to core on macOS and nowhere else; that decision
//                      lives in the host, never in core.
//   ADR-023            a handed-over event family is emitted by exactly one line of the host —
//                      core's sink. Both `task-status` (22 legacy sites) and `task-session`
//                      (6) are now under the rule; this is the check that makes "only core
//                      emits" countable instead of reviewed.
//   Action 4 (P2-05)   the call direction is `legacy command -> TaskService -> executor -> *_core`
//                      and never back up. Two things the acceptance criteria name as forbidden —
//                      a recursive call from the executor into a command wrapper, and a port that
//                      quietly succeeds without launching — are both decidable from source, so
//                      they are checked rather than reviewed. The command names come from the
//                      frozen inventory, so a newly added command is covered without editing this
//                      file.
//
// Each rule carries a positive control: if the scan cannot show it read the text it claims to
// have read, the check fails rather than passing on an empty corpus.
import { readFileSync, readdirSync } from "node:fs";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..");
const TAURI_SRC = "src-tauri/src";
const EXECUTOR = `${TAURI_SRC}/legacy_task_executor.rs`;
const COMMAND_MAP = "docs/native/command-map.json";

function escapeRegExp(text) {
  return text.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

/** Every `.rs` file under `dir`, recursively, as repo-relative paths. */
export function rustFiles(dir, root = repoRoot) {
  const out = [];
  for (const entry of readdirSync(path.join(root, dir), { withFileTypes: true })) {
    const relative = path.posix.join(dir, entry.name);
    if (entry.isDirectory()) out.push(...rustFiles(relative, root));
    else if (entry.name.endsWith(".rs")) out.push(relative);
  }
  return out.sort();
}

/** The name of the `fn` enclosing `lineIndex`, or null when there is none (top-level/impl edge). */
export function enclosingFunction(lines, lineIndex) {
  for (let i = lineIndex; i >= 0; i -= 1) {
    // Methods are indented and carry `pub(crate)`/`&self`; a top-level `fn` line alone would
    // attribute every method call to the last free function above it.
    const match = /^\s*(?:pub(?:\([^)]*\))?\s+)?(?:default\s+)?(?:const\s+)?(?:async\s+)?fn\s+([A-Za-z0-9_]+)/.exec(lines[i]);
    if (match) return match[1];
  }
  return null;
}

function lineOf(text, index) {
  return text.slice(0, index).split("\n").length - 1;
}

/**
 * Comments blanked out, byte length and line numbers preserved.
 *
 * Action 4's rule is a *reference* check, and this repo has already been fooled twice in opposite
 * directions: a doc comment naming a forbidden identifier made an absence guard report a violation
 * that did not exist, while an ignored file made it report cleanliness that did not. Blanking
 * comments keeps the module doc that explains the direction readable to a human while making it
 * invisible to the rule. `//` inside a string literal is not a comment, so string state is tracked;
 * raw strings are not modelled — the file this runs on has none.
 */
export function codeOnly(text) {
  const out = text.split("");
  let inString = false;
  let inBlock = 0;
  for (let i = 0; i < text.length; i += 1) {
    const c = text[i];
    const next = text[i + 1];
    if (inBlock > 0) {
      if (c === "/" && next === "*") inBlock += 1;
      else if (c === "*" && next === "/") inBlock -= 1;
      if (c !== "\n") out[i] = " ";
      continue;
    }
    if (inString) {
      if (c === "\\") {
        if (next && next !== "\n") out[i + 1] = " ";
        i += 1;
        continue;
      }
      if (c === '"') inString = false;
      if (c !== "\n") out[i] = " ";
      continue;
    }
    if (c === '"') {
      inString = true;
      out[i] = " ";
      continue;
    }
    if (c === "/" && next === "/") {
      let j = i;
      while (j < text.length && text[j] !== "\n") {
        out[j] = " ";
        j += 1;
      }
      i = j - 1;
      continue;
    }
    if (c === "/" && next === "*") {
      inBlock = 1;
      out[i] = " ";
      out[i + 1] = " ";
      i += 1;
      continue;
    }
  }
  return out.join("");
}

/** @returns {string[]} problems; empty means every rule above holds. */
export function adapterInvariantProblems(files, commandNames = []) {
  const problems = [];
  const named = new Map(files.map((file) => [file.name, file.text]));
  const pty = named.get(`${TAURI_SRC}/pty.rs`);
  const lib = named.get(`${TAURI_SRC}/lib.rs`);
  const adapter = named.get(`${TAURI_SRC}/core_adapter.rs`);

  // ---- positive controls: prove the scan actually read the three files it judges
  if (!pty?.includes("StartupSignalRegistration")) {
    return [`${TAURI_SRC}/pty.rs: no StartupSignalRegistration — the scan read nothing, so absence proves nothing`];
  }
  if (!lib?.includes("fn release_session_path_if_unreferenced")) {
    return [`${TAURI_SRC}/lib.rs: the claim-release helper is missing — the scan read nothing, so absence proves nothing`];
  }
  if (!adapter?.includes("fn writer_handover_allowed")) {
    return [`${TAURI_SRC}/core_adapter.rs: writer_handover_allowed is missing — the scan read nothing, so absence proves nothing`];
  }

  // ---- ADR-020 + ADR-032: one run-identity source; the counter is a version stamp only
  const staleIdentity = files.filter((file) => /STARTUP_SIGNAL_GENERATION/.test(file.text));
  if (staleIdentity.length) {
    problems.push(
      `ADR-020/032: STARTUP_SIGNAL_GENERATION is back in ${staleIdentity.map((f) => f.name).join(", ")} — a second run-identity source must not return`,
    );
  }
  const stampReaders = files.filter((file) => /STARTUP_SIGNAL_VERSION/.test(file.text));
  if (stampReaders.length !== 1 || stampReaders[0].name !== `${TAURI_SRC}/pty.rs`) {
    problems.push(
      `ADR-032: STARTUP_SIGNAL_VERSION is read outside pty.rs (${stampReaders.map((f) => f.name).join(", ") || "nowhere"}) — it is an entry version stamp, not an identity other code may compare`,
    );
  } else {
    const lines = stampReaders[0].text.split("\n");
    const users = [...stampReaders[0].text.matchAll(/STARTUP_SIGNAL_VERSION/g)]
      .map((match) => enclosingFunction(lines, lineOf(stampReaders[0].text, match.index)))
      .filter((name) => name !== null);
    const allowed = new Set(["register_initial_input_signal", "clear_initial_input_signal_if_current"]);
    const outside = [...new Set(users.filter((name) => !allowed.has(name)))];
    if (outside.length) {
      problems.push(`ADR-032: the version stamp is also read by ${outside.join(", ")} — only the register/clear pair may see it`);
    }
    if (!users.length) {
      problems.push("ADR-032: no function reads STARTUP_SIGNAL_VERSION, so the stamp is dead and the rule unchecked");
    }
  }

  // ---- ADR-031: one place may take a session path off the claim set
  const releases = [];
  for (const file of files) {
    const lines = file.text.split("\n");
    for (const match of file.text.matchAll(/([A-Za-z_][A-Za-z0-9_]*)\s*\.\s*remove\s*\(\s*(&?path|&?path_string|&?late_info\.session_path)/g)) {
      const varName = match[1];
      if (!/claimed/.test(varName)) continue;
      releases.push({ file: file.name, fn: enclosingFunction(lines, lineOf(file.text, match.index)) });
    }
    for (const match of file.text.matchAll(/claimed_session_paths[\s\S]{0,80}?\.\s*remove\s*\(/g)) {
      releases.push({ file: file.name, fn: enclosingFunction(lines, lineOf(file.text, match.index)) });
    }
  }
  const legal = new Set(["release_session_path_if_unreferenced"]);
  const offenders = releases.filter((entry) => !legal.has(entry.fn));
  if (!releases.some((entry) => legal.has(entry.fn))) {
    problems.push("ADR-031: the reverse-lookup release helper contains no claim removal — the rule has no implementation to guard");
  }
  if (offenders.length) {
    problems.push(
      `ADR-031: claim removal outside the reverse-lookup helper: ${offenders.map((o) => `${o.file}:${o.fn ?? "top-level"}`).join(", ")} — a path another run still reads would go back on the market`,
    );
  }

  // ---- ADR-021: handover is macOS-only and lives in the host
  const handover = /fn writer_handover_allowed\(\)\s*->\s*bool\s*\{([\s\S]*?)\n\}/.exec(adapter);
  if (!handover) {
    problems.push("ADR-021: writer_handover_allowed's body could not be read — refusing to pass an unreadable guard");
  } else if (!/cfg!\s*\(\s*target_os\s*=\s*"macos"\s*\)/.test(handover[1])) {
    problems.push(`ADR-021: writer_handover_allowed no longer returns cfg!(target_os = "macos"); non-macOS builds could hand the writer role over: ${handover[1].trim().slice(0, 120)}`);
  } else if (/target_os\s*=\s*"(windows|linux)"/.test(handover[1])) {
    problems.push("ADR-021: writer_handover_allowed names windows/linux — the handover is macOS-only until P7");
  }

  // ---- ADR-023: exactly one line in the host emits each handed-over family
  //
  // "core is the sole emitter" is only a claim if it is countable. Both families are listed now
  // that their 28 legacy sites are gone: `task-status` was 23 emitters before step F, and
  // `task-session` was 7. A family belongs here only while the criterion actually holds —
  // registering one at 7 would have been a permanently red case, which is the failure mode this
  // repo has kept stepping on.
  for (const family of ["task-status", "task-session"]) {
    const constant = family.toUpperCase().replace(/-/g, "_");
    // Over the whole file, not line by line: the sink writes `emit(` and the event name on
    // separate lines, and a line-scoped scan would report "no sink in view" for real code.
    const pattern = new RegExp(
      `\\.emit\\(\\s*(?:"${family}"|crate::event_names::${constant})`,
      "g",
    );
    const sites = [];
    for (const file of files) {
      // Test-only modules emit whatever they need to exercise; the rule is about who the
      // product emits through, and `*_tests.rs` files are not in that path.
      if (file.name.endsWith("_tests.rs")) continue;
      for (const match of file.text.matchAll(pattern)) {
        sites.push(`${file.name}:${1 + (file.text.slice(0, match.index).match(/\n/g) || []).length}`);
      }
    }
    const isSink = (site) => site.split(":")[0].endsWith("/core_adapter.rs");
    const inSink = sites.filter(isSink);
    const legacy = sites.filter((site) => !isSink(site));
    if (inSink.length !== 1) {
      problems.push(
        `ADR-023: ${family} must be emitted by exactly one line of core_adapter.rs, found ${inSink.length} — with no sink in view, "only core emits" would be passing on an empty corpus`,
      );
    }
    if (legacy.length) {
      problems.push(
        `ADR-023: ${family} still has legacy emitters outside the sink: ${legacy.join(", ")} — two emitters is the double-event loop P2-05's acceptance forbids`,
      );
    }
  }

  // ---- Action 4: the executor delegates downward and never calls back up
  //
  // `legacy command -> TaskService -> LegacyTaskExecutor -> *_core launch body` is the whole
  // reason each launch body was split in two. The two ways it rots are both visible from source:
  // a call from the executor into a command wrapper (the recursion), and a port that reports
  // success without reaching a launch body (the silent no-op the plan forbids by name).
  const executorRaw = named.get(EXECUTOR);
  // Offsets are preserved, so every slice below lands where the raw file does — the line numbers
  // this rule reports still point at real code, it just cannot be satisfied or fooled by prose.
  const executor = executorRaw ? codeOnly(executorRaw) : executorRaw;
  if (!executor) {
    problems.push(`Action 4: ${EXECUTOR} is not in the scan — the call-direction rule cannot judge an absent file`);
  } else {
    const implAt = executor.search(/\nimpl[^\n]*\bTaskExecutor\s+for\b/);
    const implStart = implAt < 0 ? -1 : executor.indexOf("{", implAt);
    // A trait impl's methods close at 4+ spaces, so the first column-0 `}` is the impl's own end.
    const implEnd = implStart < 0 ? -1 : executor.indexOf("\n}\n", implStart);
    if (implEnd < 0) {
      problems.push(`Action 4: the \`impl … TaskExecutor for …\` block in ${EXECUTOR} could not be read — refusing to pass an unreadable direction rule`);
    } else {
      const implText = executor.slice(implStart, implEnd);
      const marks = [...implText.matchAll(/\n    (?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?fn\s+([A-Za-z0-9_]+)/g)]
        .map((match) => ({ name: match[1], at: match.index }));
      if (marks.length === 0) {
        problems.push(`Action 4: the TaskExecutor impl in ${EXECUTOR} exposes no method — the scan read an empty block, so absence proves nothing`);
      }
      for (const [index, mark] of marks.entries()) {
        const body = implText.slice(mark.at, marks[index + 1] ? marks[index + 1].at : implText.length);
        const delegated = /\b[A-Za-z0-9_]*_core\s*\(/.exec(body);
        if (!delegated) {
          problems.push(
            `Action 4: LegacyTaskExecutor::${mark.name} reaches no *_core launch body — a port that launches nothing must refuse out loud (ExecutorFailure), not return quietly`,
          );
          continue;
        }
        const earlySuccess = body.indexOf("return Ok(");
        if (earlySuccess >= 0 && earlySuccess < delegated.index) {
          problems.push(
            `Action 4: LegacyTaskExecutor::${mark.name} returns Ok before delegating to a *_core body — that is the silent no-op P2-05's acceptance names`,
          );
        }
      }
      for (const match of implText.matchAll(/ExecutorFailure::Unsupported/g)) {
        const before = implText.slice(Math.max(0, match.index - 40), match.index);
        if (!/Err\(\s*$/.test(before)) {
          problems.push("Action 4: an ExecutorFailure::Unsupported is built outside an Err(...) — a refusal has to travel back to core as an error to be recorded");
        }
      }
      if (/\bTaskService\b/.test(executor)) {
        problems.push(`Action 4: ${EXECUTOR} names TaskService — the executor may not call back into the service that owns it`);
      }
      const lines = executor.split("\n");
      for (const name of commandNames) {
        const upward = new RegExp(`(?<![A-Za-z0-9_])${escapeRegExp(name)}\\s*\\(`, "g");
        for (const match of executor.matchAll(upward)) {
          const fn = enclosingFunction(lines, lineOf(executor, match.index));
          problems.push(
            `Action 4: ${EXECUTOR}:${fn ?? "top-level"} calls the command wrapper ${name}() — the direction is command -> service -> executor -> *_core, and a call back up re-enters the ledger`,
          );
        }
      }
    }
  }

  return problems;
}

/** The command names from the frozen inventory — the set the executor must never call. */
export function inventoryCommandNames(root = repoRoot) {
  const map = JSON.parse(readFileSync(path.join(root, COMMAND_MAP), "utf8"));
  const names = (map.commands ?? []).map((entry) => entry.command).filter((name) => typeof name === "string");
  if (names.length < 100) {
    throw new Error(`${COMMAND_MAP}: only ${names.length} command names — a thin inventory would make the no-recursion rule pass on nothing`);
  }
  return [...new Set(names)].sort();
}

function main() {
  const files = rustFiles(TAURI_SRC).map((name) => ({ name, text: readFileSync(path.join(repoRoot, name), "utf8") }));
  if (files.length < 20) {
    console.error(`adapter-invariants: only ${files.length} rust files found under ${TAURI_SRC} — the scan target is wrong, not clean`);
    process.exit(1);
  }
  const problems = adapterInvariantProblems(files, inventoryCommandNames());
  for (const problem of problems) console.error(`  - ${problem}`);
  if (problems.length) {
    console.error(`adapter-invariants: ${problems.length} problem(s) — ADR-020/021/023/031/032 and Action 4 are load-bearing, see docs/native/adr/`);
    process.exit(1);
  }
  console.log(`adapter-invariants: ${files.length} files, ADR-020/021/023/031/032 and Action 4 hold`);
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main();
