import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";
import { MAX_LINES, verifyDocs } from "./verify-docs.mjs";

function fixture(files) {
  const root = mkdtempSync(path.join(tmpdir(), "verify-docs-"));
  for (const [rel, text] of Object.entries(files)) {
    mkdirSync(path.dirname(path.join(root, rel)), { recursive: true });
    writeFileSync(path.join(root, rel), text);
  }
  return root;
}

test("a clean tree passes, and the scan really reads the files (positive control)", () => {
  const root = fixture({ "docs/a.md": "# A\n\n见 [b](b.md)。\n", "docs/b.md": "# B\n", "core/AGENTS.md": "[a](../docs/a.md)\n" });
  try {
    assert.deepEqual(verifyDocs(root), []);
    writeFileSync(path.join(root, "docs/b.md"), "宿主不再读取它。\n");
    assert.equal(verifyDocs(root).length, 1, "the narrative word in b.md must be seen");
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("over-long pages fail except the exempt backlog and docs/native artefacts", () => {
  const long = "x\n".repeat(MAX_LINES + 1);
  const root = fixture({ "docs/long.md": long, "docs/backlog.md": long, "docs/native/m.md": long });
  try {
    const errors = verifyDocs(root);
    assert.equal(errors.length, 1);
    assert.match(errors[0], /^docs\/long\.md: /);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("broken relative links fail; URLs and code spans are not links", () => {
  const root = fixture({ "docs/a.md": "[x](missing.md) [y](https://example.com) `[z](nope.md)`\n" });
  try {
    assert.deepEqual(verifyDocs(root), ["docs/a.md: broken link missing.md"]);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("AGENTS.md may quote the banned words; docs pages may not", () => {
  const root = fixture({ "docs/AGENTS.md": "不写「以前/不再」。\n", "docs/p.md": "以前是这样。\n" });
  try {
    assert.deepEqual(verifyDocs(root).map((e) => e.split(":")[0]), ["docs/p.md"]);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
