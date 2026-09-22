#!/usr/bin/env node
// Compares [patch.*] tables in core/Cargo.toml (the patch owner) against
// src-tauri/Cargo.toml. A patch that silently stops matching means the core
// build is resolving different source than the shipped client — cargo will not
// tell you, so this does.
//
// Both sides must be *non-empty*: "0 entries identical" is otherwise a pass that
// a deleted or renamed patch table would produce, which is the same vacuity that
// defeated the INV-02 source scan.
import { readFileSync } from "node:fs";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..");

/**
 * Parse every `[patch.<registry>]` table. Quoted registries
 * (`[patch."https://example.com/x"]`) are counted too — skipping them silently
 * would turn a rename into an empty comparison.
 */
export function parsePatchTables(text, ownerDir, root = repoRoot) {
  const out = new Map();
  let registry = null;
  for (const rawLine of text.split("\n")) {
    const line = rawLine.trim();
    const header = line.match(/^\[patch\.(crates-io|"[^"]+")\]\s*$/);
    if (header) { registry = header[1]; continue; }
    if (/^\[/.test(line)) { registry = null; continue; }
    if (!registry || !line || line.startsWith("#")) continue;
    const m = line.match(/^([A-Za-z0-9_.-]+)\s*=\s*\{(.*)\}\s*$/);
    if (!m) { out.set(`${registry}|${line}`, { unparsed: true, name: line, registry }); continue; }
    const [, name, body] = m;
    const pathMatch = body.match(/path\s*=\s*"([^"]+)"/);
    const gitMatch = body.match(/git\s*=\s*"([^"]+)"/);
    const revMatch = body.match(/rev\s*=\s*"([^"]+)"/);
    out.set(`${registry}|${name}`, {
      name,
      registry,
      source: pathMatch ? "path" : gitMatch ? "git" : "other",
      spec: pathMatch ? path.resolve(root, ownerDir, pathMatch[1]) : (gitMatch ?? ""),
      rev: revMatch ? revMatch[1] : "",
      raw: line,
    });
  }
  return out;
}

/** @returns {string[]} problems; empty means the two patch owners agree. */
export function patchProblems(desktop, core, minEntries = 1) {
  const problems = [];
  for (const [label, table] of [["src-tauri", desktop], ["core", core]]) {
    if (table.size === 0) problems.push(`${label}: no [patch.*] entries parsed — an empty comparison is not an equal one`);
    else if (table.size < minEntries) problems.push(`${label}: only ${table.size} patch entries, expected at least ${minEntries} — a dropped or renamed table must fail`);
    for (const entry of table.values()) {
      if (entry.unparsed) problems.push(`${label}: could not parse "${entry.name}" — fix the parser, do not guess`);
    }
  }
  if (desktop.size !== core.size) problems.push(`patch count differs: src-tauri ${desktop.size} vs core ${core.size}`);
  for (const key of new Set([...desktop.keys(), ...core.keys()])) {
    const a = desktop.get(key);
    const b = core.get(key);
    if (!a) { problems.push(`${key}: patched in core but not in src-tauri`); continue; }
    if (!b) { problems.push(`${key}: patched in src-tauri but missing from core (its fork would be ignored)`); continue; }
    if (a.unparsed || b.unparsed) continue;
    if (a.source !== b.source || a.rev !== b.rev) {
      problems.push(`${key}: source kind or rev differs (src-tauri ${a.source}/${a.rev} vs core ${b.source}/${b.rev})`);
    }
    if (a.source === "path") {
      if (path.resolve(a.spec) !== path.resolve(b.spec)) {
        problems.push(`${key}: path patches resolve differently\n    src-tauri → ${a.spec}\n    core      → ${b.spec}`);
      }
      let readable = false;
      try { readFileSync(path.join(path.resolve(a.spec), "Cargo.toml")); readable = true; } catch { readable = false; }
      if (!readable) problems.push(`${key}: path ${a.spec} has no readable Cargo.toml`);
    }
  }
  return problems;
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const argIndex = process.argv.indexOf("--min-entries");
  const minEntries = argIndex === -1 ? 1 : Number(process.argv[argIndex + 1]);
  if (!Number.isInteger(minEntries) || minEntries < 1) {
    console.error(`check-patches: --min-entries must be a positive integer, got ${process.argv[argIndex + 1]}`);
    process.exit(2);
  }
  const read = (rel) => readFileSync(path.join(repoRoot, rel), "utf8");
  const desktop = parsePatchTables(read("src-tauri/Cargo.toml"), "src-tauri");
  const core = parsePatchTables(read("core/Cargo.toml"), "core");
  const problems = patchProblems(desktop, core, minEntries);
  if (problems.length) {
    console.error("check-patches: MISMATCH\n" + problems.map((p) => `  - ${p}`).join("\n"));
    console.error("\nA drifted patch means core and the shipped client build against different source.");
    process.exit(1);
  }
  console.log(`check-patches: ${desktop.size} patch entries identical between src-tauri and core (registry, path spec and git rev), at least ${minEntries} required`);
}
