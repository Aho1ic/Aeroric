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

/**
 * Keys core's own payload builder puts on the wire for one notification variant, including
 * subscript additions.
 *
 * 切片一百零五 moved the payload construction from two `core_adapter.rs` functions into one
 * `TaskNotification::window_payload` in core (`core/crates/aeroric-core/src/tasks/events.rs`) so
 * the host EventLog and the Embedded window share a single implementation. The builder is one
 * match over both notification variants, so the scanner slices the requested arm (Status or
 * Session) and extracts the `json!({...})` object keys from that arm only — the naive whole-body
 * scan mispairs the `json!(reason)` subscript with the next arm's brace and loses the session
 * keys (observed, not hypothetical).
 */
function corePayloadKeys(functionName: string, variant: "Status" | "Session"): string[] {
  const text = source("core/crates/aeroric-core/src/tasks/events.rs");
  const at = text.indexOf("fn window_payload");
  expect(at, "window_payload must exist in core tasks/events.rs").toBeGreaterThan(-1);
  const brace = text.indexOf("{", text.indexOf(")", at));
  const end = matchBrackets(text, brace);
  const body = text.slice(brace, end);
  const armStart = body.indexOf(`TaskNotification::${variant}`);
  const armEnd =
    variant === "Status"
      ? body.indexOf("TaskNotification::Session(facts)")
      : body.length;
  expect(armStart, `${variant} arm must exist in window_payload`).toBeGreaterThan(-1);
  const arm = body.slice(armStart, armEnd);
  const keys = new Set<string>();
  for (const m of arm.matchAll(/json!\s*\(\s*\{/g)) {
    const brace = m.index! + m[0].length - 1;
    const end = matchBrackets(arm, brace);
    if (end < 0) continue;
    for (const key of topLevelKeys(arm.slice(brace, end))) keys.add(key);
  }
  for (const m of arm.matchAll(/payload\s*\[\s*"([^"]+)"\s*\]/g)) keys.add(m[1]);
  void functionName;
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

/**
 * Keys the deleted legacy sites put on the wire that no client listener destructures. They are
 * here because ADR-023 promises payloads do not change during the handover: core must keep
 * sending them, and no *new* key may join them. If the client starts reading one, delete it from
 * this list — `core invents no key...` fails while a listed key is also declared, so the list
 * cannot quietly grow stale.
 *
 * The mirror case is asserted too: `approval` used to ride on `task-status` from the hook
 * watcher and was deliberately dropped (no reader anywhere, and the approval dialogs use dsh's
 * own events). Recording it as absent is what keeps a decision that was written down in an ADR
 * from being quietly re-added, or re-removed from a family that still ships it.
 */
const carriedOver: Record<string, string[]> = {
  // `session_dsh.rs` announced dsh transcripts with a preset; no client listener
  // destructures it (`useAppTauriEvents.ts` takes task_id/session_id/session_path/
  // codex_like/family), but it has always been on the wire, so it stays.
  [TASK_SESSION]: ["agent_preset"],
  [TASK_STATUS]: [],
};

const contractRows = JSON.parse(source("contracts/native/v1/events.schema.json")).events as {
  name: string;
  disposition: string;
}[];

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

  it("the scan reads a surface with exactly one emitter per family, and can see both sides", () => {
    // This is the same positive control the first version had, re-pointed at what is true now.
    // Before step F it asserted "more than one legacy site"; after the handover that assertion
    // would be satisfied by nothing, so the control now has two halves that must hold together:
    // the legacy side is empty (the criterion), and the scan still sees the sink plus the key
    // sets on both other sides (so emptiness is a fact, not a broken scanner). A brace-stripping
    // bug in the first version made every key set empty while the file stayed green, which is
    // the exact failure this guards.
    for (const event of [TASK_STATUS, TASK_SESSION]) {
      // `*_tests.rs` files emit whatever they need to exercise — here the remote bridge's
      // forwarding of these very names. The rule is about who the product emits through, and
      // this exclusion mirrors the one in scripts/native/adapter-invariants.mjs rather than
      // inventing a second policy.
      const sites = literalEmitSites(event).filter((site) => !site.file.endsWith("_tests.rs"));
      const legacy = sites.filter((site) => !site.file.endsWith("core_adapter.rs"));
      const sink = sites.filter((site) => site.file.endsWith("core_adapter.rs"));
      expect(
        legacy.map((site) => `${site.file}:${site.keys.join("/")}`),
        `${event}: ADR-023 wants one emitter, so any legacy site is now a regression`,
      ).toEqual([]);
      expect(sink.length, `${event} must still be emitted from core_adapter.rs`).toBe(1);
    }
    expect(corePayloadKeys("task_status_payload", "Status")).toContain("task_id");
    expect(corePayloadKeys("task_session_payload", "Session")).toContain("session_path");
    expect(clientKeys("TASK_STATUS_EVENT").required).toContain("status");
    expect(clientKeys("TASK_SESSION_EVENT").required).toContain("session_id");
    expect(corePayloadKeys("task_status_payload", "Status")).not.toContain("approval");
  });

  it("core sends every key the client requires, so no listener reads undefined", () => {
    const cases: [string, string, "Status" | "Session"][] = [
      ["TASK_STATUS_EVENT", "task_status_payload", "Status"],
      ["TASK_SESSION_EVENT", "task_session_payload", "Session"],
    ];
    for (const [constant, builder, variant] of cases) {
      const { required } = clientKeys(constant);
      const emitted = corePayloadKeys(builder, variant);
      const missing = required.filter((key) => !emitted.includes(key));
      expect(missing, `${builder} must carry the required keys ${required.join(", ")}`).toEqual([]);
    }
  });

  it("core can carry everything the deleted sites used to send, because that is the promise", () => {
    // The legacy literal sites are gone, so the set this used to compare against is empty and
    // the assertion would pass on nothing. The keys those sites put on the wire are recorded
    // here instead, one line each, with where they came from: ADR-023 promises payloads do not
    // change during the handover, so a key core emits without a shipping source is a new wire
    // field, and a key core drops is a silent regression.
    const cases: [string, string, "Status" | "Session"][] = [
      [TASK_STATUS, "task_status_payload", "Status"],
      [TASK_SESSION, "task_session_payload", "Session"],
    ];
    for (const [event, builder, variant] of cases) {
      const legacy = new Set<string>([
        ...literalEmitSites(event)
          .filter((site) => !site.file.endsWith("_tests.rs"))
          .flatMap((site) => site.keys),
        ...(carriedOver[event] ?? []),
      ]);
      const emitted = new Set(corePayloadKeys(builder, variant));
      const dropped = [...legacy].filter((key) => !emitted.has(key));
      expect(
        dropped,
        `${event}: core no longer carries ${dropped.join(", ")} that the shipping wire has`,
      ).toEqual([]);
    }
  });

  it("core invents no key that neither the client nor today's wire has", () => {
    const cases: [string, string, "Status" | "Session"][] = [
      [TASK_STATUS, "task_status_payload", "Status"],
      [TASK_SESSION, "task_session_payload", "Session"],
    ];
    for (const [event, builder, variant] of cases) {
      const constant = event === TASK_STATUS ? "TASK_STATUS_EVENT" : "TASK_SESSION_EVENT";
      const declared = new Set([
        ...clientKeys(constant).required,
        ...clientKeys(constant).optional,
      ]);
      const carried = carriedOver[event] ?? [];
      for (const key of carried) {
        expect(
          declared.has(key),
          `${event}: ${key} is listed as shipped-but-unread yet the client now reads it — drop it from carriedOver`,
        ).toBe(false);
      }
      const known = new Set([
        ...literalEmitSites(event)
          .filter((site) => !site.file.endsWith("_tests.rs"))
          .flatMap((site) => site.keys),
        ...declared,
        ...carried,
      ]);
      const invented = corePayloadKeys(builder, variant).filter((key) => !known.has(key));
      expect(
        invented,
        `${builder} adds ${invented.join(", ")} that no shipping event carries`,
      ).toEqual([]);
    }
  });
});
