/**
 * P2-05 Action 5: the client half of the adapter's event contract (ADR-023).
 *
 * ADR-023 hands `task-status` / `task-session` to core as their only emitter, and the decision
 * it records is that this happens **without changing the event names or the payloads**. That is
 * a claim about two files that do not import each other: `useAppTauriEvents.ts` on one side and
 * a `json!` literal in `core_adapter.rs` on the other. Nothing in the build ties them together,
 * and `src/tauriEvents.ts` says so in as many words — "Rust 的 emit 侧没有任何静态检查兜底,
 * 只能靠运行期失配暴露". This file is that missing check.
 *
 * Nothing here is transcribed. Every key set is read out of the source that is supposed to agree
 * with it, because a hand-written fixture of "what the client expects" is exactly how a false
 * fact survives a parity test: the two sides match each other and both disagree with the product.
 *
 * The load-bearing direction is `legacy ⊆ core`. The eight legacy sites are being deleted; any
 * key one of them sends and core's builder cannot send would vanish from the wire mid-deletion,
 * and the UI would simply stop showing the information with nothing failing anywhere.
 */
import { readdirSync, readFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { describe, expect, it } from "vitest";

// `process.cwd()` rather than `import.meta.url`: same convention as
// `native-host-protocol.test.ts`, and this repo's path is non-ASCII so a URL pathname
// would come back percent-encoded and point nowhere.
const source = (relative: string) => readFileSync(resolve(process.cwd(), relative), "utf8");

const TASK_STATUS = "task-status";
const TASK_SESSION = "task-session";

function rustFiles(dir: string): string[] {
  const found: string[] = [];
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) {
      found.push(...rustFiles(path));
    } else if (entry.name.endsWith(".rs")) {
      found.push(path);
    }
  }
  return found;
}

const HOST_SRC = resolve(process.cwd(), "src-tauri/src");

/** Index just past the bracket matching the one at `open`, skipping string contents. */
function matchBrackets(text: string, open: number): number {
  const pairs: Record<string, string> = { "(": ")", "[": "]", "{": "}" };
  const closes = new Set(Object.values(pairs));
  let depth = 0;
  let inString = false;
  let escaped = false;
  for (let i = open; i < text.length; i += 1) {
    const ch = text[i];
    if (inString) {
      if (escaped) escaped = false;
      else if (ch === "\\") escaped = true;
      else if (ch === '"') inString = false;
      continue;
    }
    if (ch === '"') inString = true;
    else if (pairs[ch]) depth += 1;
    else if (closes.has(ch)) {
      depth -= 1;
      if (depth === 0) return i + 1;
    }
  }
  return -1;
}

/** The keys declared at the top level of one `json!({ ... })` / `json!{ ... }` literal. */
function keysOfJsonMacros(text: string): string[] {
  const keys = new Set<string>();
  const pattern = /\bjson!\s*(\(|\{)/g;
  for (let m = pattern.exec(text); m; m = pattern.exec(text)) {
    const brace = m[1] === "(" ? text.indexOf("{", m.index) : m.index + m[0].length - 1;
    if (brace < 0) continue;
    const end = matchBrackets(text, brace);
    if (end < 0) continue;
    for (const key of topLevelKeys(text.slice(brace, end))) keys.add(key);
    pattern.lastIndex = end;
  }
  return [...keys];
}

/** `"a": …` at depth 1 only, so a nested object cannot donate its keys to its parent. */
function topLevelKeys(objectText: string): string[] {
  const keys: string[] = [];
  // `objectText` keeps its surrounding braces, which is what makes "one level down" depth 1.
  let depth = 0;
  let i = 0;
  while (i < objectText.length) {
    const ch = objectText[i];
    if (ch === '"' && depth === 1) {
      const close = objectText.indexOf('"', i + 1);
      if (close < 0) break;
      const literal = objectText.slice(i + 1, close);
      if (/^\s*:/.test(objectText.slice(close + 1))) keys.push(literal);
      i = close + 1;
      continue;
    }
    if (ch === "{" || ch === "[" || ch === "(") depth += 1;
    else if (ch === "}" || ch === "]" || ch === ")") depth -= 1;
    i += 1;
  }
  return keys;
}

type EmitSite = { file: string; keys: string[] };

/**
 * Every `.emit("<event>", <literal payload>)` call in the host crate.
 *
 * A payload built by a function (`task_status_payload(…)`) contributes no keys on purpose: those
 * are collected separately from the builder itself, and counting the literal sites is what keeps
 * "found nothing" distinguishable from "nothing matches" (repo discipline: a scan needs a
 * positive control).
 */
function literalEmitSites(event: string): EmitSite[] {
  const sites: EmitSite[] = [];
  for (const file of rustFiles(HOST_SRC)) {
    const text = readFileSync(file, "utf8");
    const pattern = /\.emit\s*(?:::<[^>]*>)?\s*\(/g;
    for (let m = pattern.exec(text); m; m = pattern.exec(text)) {
      const end = matchBrackets(text, m.index + m[0].length - 1);
      if (end < 0) break;
      const args = text.slice(m.index + m[0].length, end - 1);
      // `.emit("<event>", …)` — the app handle may come first on some call shapes, so the
      // event literal only has to appear among the arguments.
      if (!args.includes(`"${event}"`)) {
        pattern.lastIndex = end;
        continue;
      }
      sites.push({ file: file.replace(process.cwd() + "/", ""), keys: keysOfJsonMacros(args) });
      pattern.lastIndex = end;
    }
  }
  return sites;
}

/** Keys core's own payload builders put on the wire, including subscript additions. */
function corePayloadKeys(functionName: string): string[] {
  const text = source("src-tauri/src/core_adapter.rs");
  const at = text.indexOf(`fn ${functionName}`);
  expect(at, `${functionName} must exist in core_adapter.rs`).toBeGreaterThan(-1);
  const brace = text.indexOf("{", text.indexOf(")", at));
  const end = matchBrackets(text, brace);
  const body = text.slice(brace, end);
  const keys = new Set(keysOfJsonMacros(body));
  for (const m of body.matchAll(/payload\s*\[\s*"([^"]+)"\s*\]/g)) keys.add(m[1]);
  return [...keys];
}

/** What the client destructures out of each event, with required/optional kept apart. */
function clientKeys(eventConstant: string): { required: string[]; optional: string[] } {
  const text = source("src/state/app/useAppTauriEvents.ts");
  const at = text.indexOf(`listen<`);
  expect(at, "the listener block must be findable").toBeGreaterThan(-1);
  const sites: { typeArg: string; event: string }[] = [];
  const pattern = /listen<\s*\{/g;
  for (let m = pattern.exec(text); m; m = pattern.exec(text)) {
    const brace = text.indexOf("{", m.index);
    const end = matchBrackets(text, brace);
    const after = text.slice(end, end + 200);
    const eventMatch = after.match(/\(\s*([A-Z_]+)\s*,/);
    if (eventMatch) sites.push({ typeArg: text.slice(brace, end), event: eventMatch[1] });
    pattern.lastIndex = end;
  }
  const wanted = sites.find((site) => site.event === eventConstant);
  expect(wanted, `a listen<> typed for ${eventConstant}`).toBeTruthy();
  const required: string[] = [];
  const optional: string[] = [];
  for (const m of wanted!.typeArg.matchAll(/(\w+)\??\s*:/g)) {
    (m[0].includes("?") ? optional : required).push(m[1]);
  }
  return { required, optional };
}

/** `tauriEvents.ts` is the single source for the names; the contract must agree with it. */
function eventName(constant: string): string {
  const value = new RegExp(`export const ${constant} = "([^"]+)"`).exec(
    source("src/tauriEvents.ts"),
  );
  expect(value, `${constant} must be a plain string literal`).toBeTruthy();
  return value![1];
}

const contractRows = JSON.parse(
  source("contracts/native/v1/events.schema.json"),
).events as { name: string; disposition: string }[];

describe("P2-05 adapter contract: task-status / task-session survive the handover unchanged", () => {
  it("the event names are the ones the client listens for and the contract records", () => {
    expect(eventName("TASK_STATUS_EVENT")).toBe(TASK_STATUS);
    expect(eventName("TASK_SESSION_EVENT")).toBe(TASK_SESSION);
    for (const name of [TASK_STATUS, TASK_SESSION]) {
      const row = contractRows.find((entry) => entry.name === name);
      expect(row, `${name} must be in the contract`).toBeTruthy();
      expect(row!.disposition, `${name} is still in use`).toBe("in-use");
    }
  });

  it("the scan really reads both sides (a guard that matches nothing is worse than none)", () => {
    // Positive control, and it earns its keep: the first version of this file counted *sites*
    // only, and a brace-stripping bug that made every key set empty still reported green.
    // What must be non-empty is the key sets.
    for (const event of [TASK_STATUS, TASK_SESSION]) {
      const sites = literalEmitSites(event);
      expect(sites.length, `legacy ${event} literal emit sites`).toBeGreaterThan(1);
      const keys = new Set(sites.flatMap((site) => site.keys));
      expect(keys.has("task_id"), `${event} sites must be read down to their keys`).toBe(true);
    }
    expect(corePayloadKeys("task_status_payload")).toContain("task_id");
    expect(corePayloadKeys("task_session_payload")).toContain("session_path");
    expect(clientKeys("TASK_STATUS_EVENT").required).toContain("status");
    expect(clientKeys("TASK_SESSION_EVENT").required).toContain("session_id");
  });

  it("core sends every key the client requires, so no listener reads undefined", () => {
    const cases: [string, string][] = [
      ["TASK_STATUS_EVENT", "task_status_payload"],
      ["TASK_SESSION_EVENT", "task_session_payload"],
    ];
    for (const [constant, builder] of cases) {
      const { required } = clientKeys(constant);
      const emitted = corePayloadKeys(builder);
      const missing = required.filter((key) => !emitted.includes(key));
      expect(missing, `${builder} must carry the required keys ${required.join(", ")}`).toEqual([]);
    }
  });

  it("core can carry everything the legacy sites send, because those are being deleted", () => {
    const cases: [string, string][] = [
      [TASK_STATUS, "task_status_payload"],
      [TASK_SESSION, "task_session_payload"],
    ];
    for (const [event, builder] of cases) {
      const legacy = new Set(literalEmitSites(event).flatMap((site) => site.keys));
      const emitted = new Set(corePayloadKeys(builder));
      const dropped = [...legacy].filter((key) => !emitted.has(key));
      expect(
        dropped,
        `${event}: deleting the legacy sites would drop ${dropped.join(", ")} from the wire`,
      ).toEqual([]);
    }
  });

  it("core invents no key that neither the client nor today's wire has", () => {
    const cases: [string, string][] = [
      [TASK_STATUS, "task_status_payload"],
      [TASK_SESSION, "task_session_payload"],
    ];
    for (const [event, builder] of cases) {
      const constant = event === TASK_STATUS ? "TASK_STATUS_EVENT" : "TASK_SESSION_EVENT";
      const known = new Set([
        ...literalEmitSites(event).flatMap((site) => site.keys),
        ...clientKeys(constant).required,
        ...clientKeys(constant).optional,
      ]);
      const invented = corePayloadKeys(builder).filter((key) => !known.has(key));
      expect(invented, `${builder} adds ${invented.join(", ")} that no shipping event carries`).toEqual([]);
    }
  });
});
