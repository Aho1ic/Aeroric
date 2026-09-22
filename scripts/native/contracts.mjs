#!/usr/bin/env node
// `pnpm native:contracts` — generate and verify `contracts/native/v1/` (P2-01).
//
// Rust is the source of truth for the wire contract: `bin/export-schema.rs` prints
// the envelope/method/event schemas and the six fixtures, and Swift + TypeScript
// decode *those* files. This script exists so the committed artifacts can never be
// hand-edited into disagreement with the DTOs:
//
//   --check (default)  generate into a temp dir, compare byte-for-byte, exit 1 on
//                      drift. Never writes into the repository.
//   --write            copy the generated tree over `contracts/native/v1/`.
//
// An always-exit-0 placeholder is forbidden by plan §3.2, so a failure here is loud.
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..");
// `--out` is a repository root, so both sides of the comparison are the same
// relative tree: <root>/contracts/native/v1/**.
const committed = path.join(repoRoot, "contracts", "native");
const mode = process.argv.includes("--write") ? "write" : "check";

function run(argv, args = []) {
  return execFileSync(argv, args, { cwd: repoRoot, encoding: "utf8" });
}

/** Every file under `dir`, as `relative path -> Buffer`. */
function snapshot(dir) {
  const out = new Map();
  const walk = (current) => {
    for (const entry of fs.readdirSync(current, { withFileTypes: true })) {
      const full = path.join(current, entry.name);
      if (entry.isDirectory()) walk(full);
      else out.set(path.relative(dir, full).split(path.sep).join("/"), fs.readFileSync(full));
    }
  };
  if (fs.existsSync(dir)) walk(dir);
  return out;
}

function generate(into) {
  run("cargo", [
    "run",
    "--quiet",
    "--manifest-path",
    "core/Cargo.toml",
    "--locked",
    "-p",
    "aeroric-protocol",
    "--bin",
    "export-schema",
    "--",
    "--out",
    into,
  ]);
  return path.join(into, "contracts/native");
}

/**
 * A generated contract is shared by three languages and committed to git, so it may
 * not carry anything that is true only on the machine that produced it.
 */
function assertNoLocalState(files) {
  const never = [repoRoot, os.homedir(), process.env.TMPDIR ?? "/var/folders"];
  for (const [name, bytes] of files) {
    const text = bytes.toString("utf8");
    for (const needle of never) {
      if (needle && text.includes(needle)) {
        throw new Error(`${name} embeds a machine-local path (${needle}) — refusing to publish it`);
      }
    }
  }
}

function describeDrift(before, after) {
  const a = before.toString("utf8").split("\n");
  const b = after.toString("utf8").split("\n");
  const index = a.findIndex((line, i) => line !== b[i]);
  if (index === -1) return `length differs (${a.length} vs ${b.length} lines)`;
  return `line ${index + 1}: ${JSON.stringify(a[index])} -> ${JSON.stringify(b[index])}`;
}

const temp = fs.mkdtempSync(path.join(os.tmpdir(), "aeroric-contracts-"));
let drift = 0;
try {
  const fresh = snapshot(generate(temp));
  assertNoLocalState(fresh);
  if (fresh.size === 0) throw new Error("export-schema produced no documents");
  const existing = snapshot(committed);

  const missing = [...fresh.keys()].filter((k) => !existing.has(k));
  const extra = [...existing.keys()].filter((k) => !fresh.has(k));
  const changed = [...fresh.keys()].filter((k) => existing.has(k) && !existing.get(k).equals(fresh.get(k)));
  drift = missing.length + extra.length + changed.length;

  if (mode === "write") {
    fs.mkdirSync(committed, { recursive: true });
    for (const name of extra) fs.rmSync(path.join(committed, name));
    for (const [name, bytes] of fresh) {
      const target = path.join(committed, name);
      fs.mkdirSync(path.dirname(target), { recursive: true });
      fs.writeFileSync(target, bytes);
    }
    console.log(
      `contracts: wrote ${fresh.size} document(s) into contracts/native/v1 — ` +
        `${missing.length} added, ${extra.length} removed, ${changed.length} changed`,
    );
  } else if (drift) {
    console.error(`contracts: NOT CURRENT — ${drift} of ${fresh.size} committed file(s) disagree with the DTOs.`);
    for (const name of missing) console.error(`  missing   ${name}`);
    for (const name of extra) console.error(`  stale     ${name} (nothing generates it; hand-authored?)`);
    for (const name of changed) console.error(`  changed   ${name}: ${describeDrift(existing.get(name), fresh.get(name))}`);
    console.error("  Fix with `pnpm native:contracts --write`, then re-run the decoders.");
  } else {
    console.log(`contracts: current — ${fresh.size} document(s) match the Rust DTOs byte for byte.`);
  }
} catch (error) {
  console.error(`contracts: ${error.message}`);
  process.exitCode = 1;
} finally {
  fs.rmSync(temp, { recursive: true, force: true });
}
if (mode === "check" && drift) process.exitCode = 1;
