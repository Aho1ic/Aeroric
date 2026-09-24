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
//                      core's sink. `task-status` entered this rule when its 22 legacy sites
//                      were deleted; `task-session` joins it after step F5.
//
// Each rule carries a positive control: if the scan cannot show it read the text it claims to
// have read, the check fails rather than passing on an empty corpus.
import { readFileSync, readdirSync } from "node:fs";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..");
const TAURI_SRC = "src-tauri/src";

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

/** @returns {string[]} problems; empty means the three ADR rules all hold. */
export function adapterInvariantProblems(files) {
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
  // "core is the sole emitter" is only a claim if it is countable. A family enters this list
  // when its legacy sites are gone; `task-session` is not here yet because its six sites move
  // through apply_resolved_session rather than the observation channel, and listing a family
  // whose count is still 7 would just be a permanently red case.
  for (const family of ["task-status"]) {
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

  return problems;
}

function main() {
  const files = rustFiles(TAURI_SRC).map((name) => ({ name, text: readFileSync(path.join(repoRoot, name), "utf8") }));
  if (files.length < 20) {
    console.error(`adapter-invariants: only ${files.length} rust files found under ${TAURI_SRC} — the scan target is wrong, not clean`);
    process.exit(1);
  }
  const problems = adapterInvariantProblems(files);
  for (const problem of problems) console.error(`  - ${problem}`);
  if (problems.length) {
    console.error(`adapter-invariants: ${problems.length} problem(s) — ADR-020/021/031/032 are load-bearing, see docs/native/adr/`);
    process.exit(1);
  }
  console.log(`adapter-invariants: ${files.length} files, ADR-020/021/031/032 hold`);
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main();
