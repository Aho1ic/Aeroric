import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";

/**
 * ADR-013 Q29. `handleOpen` and `handleOpenSshProject` used to call their upsert twice —
 * once to get the project to activate and once inside the `setProjects` updater — and each
 * call read its own `Date.now()`. When the two reads straddled a millisecond, the id that got
 * persisted was not the id the UI activated: `mountProject(project.id)` then pointed at a
 * record that does not exist in `projects.json`.
 *
 * A source scan is a blunt guard, and it is honest about being blunt: it proves the single
 * `Date.now()` per open handler and that both upsert calls receive it, which is exactly the
 * shape of the bug. It does not prove anything about the rest of the dialog flow.
 */
const source = readFileSync(resolve(process.cwd(), "src/App.tsx"), "utf8");

function handlerBody(name: string): string {
  const start = source.indexOf(`function ${name}(`);
  expect(start, `${name} is gone from App.tsx`).toBeGreaterThan(-1);
  const next = source.indexOf("\n  function ", start + 1);
  const body = source.slice(start, next === -1 ? source.length : next);
  // Comments are the reason a naive count lied: the explanation of this very bug contains
  // the string `Date.now()`, and a guard that counts prose proves nothing.
  return body
    .split("\n")
    .filter((line) => !line.trimStart().startsWith("//"))
    .join("\n");
}

describe("opening a project computes one identity", () => {
  for (const [handler, upsert] of [
    ["handleOpen", "upsertLocalProject"],
    ["handleOpenSshProject", "upsertSshProject"],
    ["handleOpenWslProject", "upsertWslProject"],
  ] as const) {
    it(`${handler} reads the clock once and passes it to both upsert calls`, () => {
      const body = handlerBody(handler);
      const nowReads = body.match(/Date\.now\(\)/g) ?? [];
      expect(nowReads.length, "exactly one timestamp per open").toBe(1);
      const calls = body.match(new RegExp(`${upsert}\\(`, "g")) ?? [];
      expect(calls.length, "computed for the activation and for the updater").toBe(2);
      const withNow = body.match(new RegExp(`${upsert}\\([^)]*\\bnow\\b`, "g")) ?? [];
      expect(withNow.length, "both calls must receive the same now").toBe(2);
    });
  }
});
