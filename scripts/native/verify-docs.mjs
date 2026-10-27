#!/usr/bin/env node
// Docs gate: docs/ states current facts only. Checks, for every Markdown file
// under docs/ plus every AGENTS.md / CLAUDE.md in the tree:
//   - length: at most MAX_LINES lines (docs/native/ artefacts and docs/backlog.md exempt);
//   - change narrative: none of NARRATIVE_WORDS (docs/ only; AGENTS files may cite dates);
//   - relative Markdown links resolve to an existing file.
// Rationale: .agents/notes/implemented/process/2026-10-27-docs-notes-split.md
import { existsSync, readFileSync, readdirSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

export const MAX_LINES = 300;
export const NARRATIVE_WORDS = ["以前", "不再", "曾经", "原先", "如今", "no longer", "used to", "previously"];
const LENGTH_EXEMPT = [/^docs\/native\//, /^docs\/backlog\.md$/];
const NARRATIVE_EXEMPT = [/^docs\/native\//, /^docs\/backlog\.md$/, /^docs\/cookbook\/reference-provenance\.md$/];
const SKIP_DIRS = new Set([".git", "node_modules", "target", ".build", "DerivedData", ".artifacts", ".claude", ".trellis", ".agents", "vendor"]);

function walk(root, rel, out) {
  for (const entry of readdirSync(path.join(root, rel), { withFileTypes: true })) {
    const child = rel ? `${rel}/${entry.name}` : entry.name;
    if (entry.isDirectory()) {
      if (!SKIP_DIRS.has(entry.name)) walk(root, child, out);
    } else if (entry.isFile() && entry.name.endsWith(".md")) {
      if (child.startsWith("docs/") || entry.name === "AGENTS.md" || entry.name === "CLAUDE.md") out.push(child);
    }
  }
  return out;
}

// Drops fenced code blocks and inline code so examples are not scanned.
function prose(text) {
  return text.replace(/```[\s\S]*?```/g, "").replace(/`[^`\n]*`/g, "");
}

export function checkFile(root, rel) {
  const errors = [];
  const text = readFileSync(path.join(root, rel), "utf8");
  const lines = text.split("\n").length;
  if (!LENGTH_EXEMPT.some((re) => re.test(rel)) && lines > MAX_LINES) {
    errors.push(`${rel}: ${lines} lines (max ${MAX_LINES}); split it or move detail into a subsystem page`);
  }
  const body = prose(text);
  if (rel.startsWith("docs/") && !rel.endsWith("AGENTS.md") && !NARRATIVE_EXEMPT.some((re) => re.test(rel))) {
    for (const word of NARRATIVE_WORDS) {
      if (body.toLowerCase().includes(word)) errors.push(`${rel}: change narrative "${word}"; describe the current state, put history in .agents/notes/`);
    }
  }
  for (const match of body.matchAll(/\]\(([^)\s]+)\)/g)) {
    const target = match[1].split("#")[0];
    if (!target || /^[a-z]+:/i.test(target)) continue;
    if (!existsSync(path.resolve(root, path.dirname(rel), target))) errors.push(`${rel}: broken link ${match[1]}`);
  }
  return errors;
}

export function verifyDocs(root) {
  return walk(root, "", []).sort().flatMap((rel) => checkFile(root, rel));
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");
  const errors = verifyDocs(root);
  for (const e of errors) console.error(e);
  if (errors.length) process.exit(1);
  console.log(`ok: docs verified (${walk(root, "", []).length} files)`);
}
