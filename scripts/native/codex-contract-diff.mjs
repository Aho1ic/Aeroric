#!/usr/bin/env node
// E9: compare two `codex app-server` schema snapshots in contracts/codex/
// before raising the Codex the native driver is checked against.
//
//   pnpm native:codex-diff                   # the two newest snapshots
//   pnpm native:codex-diff 0.162.1 0.167.0   # any two, oldest first
//
// Prints, per message file, the methods added and removed, and for every
// method the driver uses, the parameter fields added, removed, newly required
// or changed. Exits 1 when a method the driver uses is gone from the newer
// snapshot. The Rust gate (`sessions::live::codex::contract`, run by every
// driver test) checks what the driver actually sends; this report is for the
// person doing the upgrade.

import { readdirSync, readFileSync } from "node:fs";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..");
const CONTRACTS = path.join(repoRoot, "contracts", "codex");
const DRIVER = path.join(repoRoot, "core", "crates", "aeroric-core", "src", "sessions", "live", "codex");
export const FILES = ["ClientRequest.json", "ServerRequest.json", "ServerNotification.json"];
const MAX_DEPTH = 6;

const versionOf = (name) => name.replace(/^app-server-/, "").split(".").map(Number);
const compareVersions = (a, b) => {
  for (let i = 0; i < Math.max(a.length, b.length); i++) {
    const d = (a[i] ?? 0) - (b[i] ?? 0);
    if (d !== 0) return d;
  }
  return 0;
};

/** Snapshot directory names, oldest first. */
export function snapshotNames(dir = CONTRACTS) {
  return readdirSync(dir)
    .filter((name) => /^app-server-\d+(\.\d+)*$/.test(name))
    .sort((a, b) => compareVersions(versionOf(a), versionOf(b)));
}

export function loadSnapshot(name, dir = CONTRACTS) {
  const files = {};
  for (const file of FILES) files[file] = JSON.parse(readFileSync(path.join(dir, name, file), "utf8"));
  return files;
}

/** method → its `oneOf` entry. */
export function methodTable(file) {
  const table = new Map();
  for (const entry of file.oneOf ?? []) {
    const method = entry?.properties?.method?.enum?.[0];
    if (typeof method === "string") table.set(method, entry);
  }
  return table;
}

/**
 * The methods the driver names: every string literal in the driver's
 * non-test sources that is a method of either snapshot.
 */
export function driverMethods(known, dir = DRIVER) {
  const used = new Set();
  for (const file of readdirSync(dir)) {
    if (!file.endsWith(".rs") || file.endsWith("_tests.rs") || file === "fake.rs" || file === "contract.rs") continue;
    for (const [, literal] of readFileSync(path.join(dir, file), "utf8").matchAll(/"([A-Za-z0-9_/]+)"/g)) {
      if (known.has(literal)) used.add(literal);
    }
  }
  return used;
}

const resolve = (root, schema) => {
  let seen = 0;
  while (schema && typeof schema.$ref === "string" && seen++ < 20) {
    schema = root.definitions?.[schema.$ref.replace("#/definitions/", "")];
  }
  return schema ?? {};
};

/**
 * Flattens a schema into `path → signature` (type and enum), following
 * `$ref`, `anyOf`/`oneOf` branches and object properties, `required` marked.
 */
export function fields(root, schema, at = "params", depth = 0, out = new Map()) {
  schema = resolve(root, schema);
  if (depth > MAX_DEPTH) return out;
  const branches = schema.anyOf ?? schema.oneOf;
  if (Array.isArray(branches)) {
    const own = branches.map((b) => resolve(root, b)).filter((b) => b.type !== "null");
    if (own.length === 1) return fields(root, own[0], at, depth, out);
    const shapes = own.map((b) => (b.enum ? JSON.stringify(b.enum) : b.properties?.type?.enum?.[0] ?? b.type ?? "?"));
    out.set(at, `one of ${shapes.join(" | ")}`);
    // Fields named beside the branches belong to every branch.
    const common = () => {
      const { anyOf, oneOf, ...rest } = schema;
      if (rest.properties) fields(root, { ...rest, type: undefined }, at, depth, new Map()).forEach((sig, key) => key !== at && out.set(key, sig));
      return out;
    };
    const tags = own.map((b) => b.properties?.type?.enum?.[0]);
    if (tags.every((t) => typeof t === "string") && new Set(tags).size === tags.length) {
      own.forEach((branch, index) => fields(root, branch, `${at}<${tags[index]}>`, depth + 1, out));
      return common();
    }
    // Untagged branches (one shape or another) merge at the same path: a
    // field is listed with every signature it has, required only when every
    // branch requires it.
    const maps = own.filter((b) => b.properties).map((b) => fields(root, b, at, depth + 1, new Map()));
    const keys = new Set(maps.flatMap((m) => [...m.keys()]).filter((k) => k !== at));
    for (const key of keys) {
      const sigs = [...new Set(maps.map((m) => m.get(key)).filter(Boolean))];
      if (key.endsWith(" (required)") && maps.some((m) => !m.has(key))) continue;
      out.set(key, sigs.join(" | "));
    }
    return common();
  }
  const type = Array.isArray(schema.type) ? schema.type.join("|") : schema.type ?? "any";
  out.set(at, schema.enum ? `${type} ${JSON.stringify(schema.enum)}` : type);
  const required = new Set(schema.required ?? []);
  for (const [key, property] of Object.entries(schema.properties ?? {})) {
    const child = `${at}.${key}`;
    fields(root, property, child, depth + 1, out);
    if (required.has(key)) out.set(`${child} (required)`, "required");
  }
  if (schema.items) fields(root, schema.items, `${at}[]`, depth + 1, out);
  return out;
}

const paramsOf = (file, entry) => (entry?.properties?.params ? fields(file, entry.properties.params) : new Map());

/** The report lines and whether a used method disappeared. */
export function diffSnapshots(older, newer, used) {
  const lines = [];
  let broken = false;
  for (const file of FILES) {
    const before = methodTable(older[file]);
    const after = methodTable(newer[file]);
    const added = [...after.keys()].filter((m) => !before.has(m)).sort();
    const removed = [...before.keys()].filter((m) => !after.has(m)).sort();
    lines.push(`## ${file}: ${before.size} → ${after.size}`);
    for (const m of added) lines.push(`  + ${m}`);
    for (const m of removed) {
      const inUse = used.has(m);
      broken ||= inUse;
      lines.push(`  - ${m}${inUse ? "   ← 驱动在用" : ""}`);
    }
    for (const method of [...before.keys()].filter((m) => after.has(m) && used.has(m)).sort()) {
      const a = paramsOf(older[file], before.get(method));
      const b = paramsOf(newer[file], after.get(method));
      const changes = [];
      for (const [key, sig] of b) {
        if (!a.has(key)) changes.push(`    + ${key}${sig === "required" ? "" : `: ${sig}`}`);
        else if (a.get(key) !== sig) changes.push(`    ~ ${key}: ${a.get(key)} → ${sig}`);
      }
      for (const [key, sig] of a) if (!b.has(key)) changes.push(`    - ${key}${sig === "required" ? "" : `: ${sig}`}`);
      if (changes.length) lines.push(`  ${method}（驱动在用）`, ...changes);
    }
  }
  return { lines, broken };
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const names = snapshotNames();
  const asked = process.argv.slice(2).map((v) => (v.startsWith("app-server-") ? v : `app-server-${v}`));
  const [oldName, newName] = asked.length === 2 ? asked : names.slice(-2);
  if (!oldName || !newName) {
    console.error(`需要两份快照，contracts/codex/ 里有：${names.join(", ") || "（没有）"}`);
    process.exit(2);
  }
  for (const name of [oldName, newName]) {
    if (!names.includes(name)) {
      console.error(`没有 contracts/codex/${name}，现有：${names.join(", ")}`);
      process.exit(2);
    }
  }
  const older = loadSnapshot(oldName);
  const newer = loadSnapshot(newName);
  const known = new Set(FILES.flatMap((f) => [...methodTable(older[f]).keys(), ...methodTable(newer[f]).keys()]));
  const used = driverMethods(known);
  const { lines, broken } = diffSnapshots(older, newer, used);
  console.log(`# ${oldName} → ${newName}（驱动用到 ${used.size} 个方法）`);
  console.log(lines.join("\n"));
  if (broken) {
    console.error("\n驱动在用的方法在新版里没有了，先改驱动再升门槛。");
    process.exit(1);
  }
}
