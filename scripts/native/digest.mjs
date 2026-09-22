#!/usr/bin/env node
// The Trellis context loader silently truncates any injected file at
// context_injection.max_file_bytes = 32768 (deviation D8). Both frozen digests
// sat within ~1.8 KB of that cap, so each is split into per-topic shards that
// are individually far below it.
//
// `--check` is the point of the whole file: sharding must never become a way to
// lose a row. It fails if a canonical key belongs to no shard, if a shard is
// stale with respect to the canonical digest, or if a shard exceeds the cap.
//
// Deterministic by construction — no clock reads, sorted keys — otherwise a
// freshly regenerated shard could never compare equal to the one on disk.
import { createHash } from "node:crypto";
import { mkdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

const CAP_BYTES = 32768;
const defaultRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..");
const OUT_DIR = path.join("docs", "native", "digest");

// Every top-level key of a canonical digest must land in exactly one shard.
const SPECS = [
  {
    source: "docs/native/command-map.summary.json",
    prefix: "command-map",
    parts: {
      // "*" = everything no other shard claims, so adding a top-level key can
      // never mean "quietly left out of the injection" — the coverage assertion
      // stays, the manual key list goes away.
      head: ["*"],
      modules: ["modules"],
      surfaces: ["event_dispositions", "panel_targets", "dynamic_sites"],
      unreachable: ["unreachable_commands"],
    },
  },
  {
    source: "docs/native/feature-matrix.json",
    prefix: "feature-matrix",
    parts: {
      head: ["*"],
      features: ["features"],
      gaps: ["gaps"],
    },
  },
];

const sha256 = (bytes) => createHash("sha256").update(bytes).digest("hex");
const serialize = (value) => `${JSON.stringify(value, null, 2)}\n`;

function canonicalize(value) {
  if (Array.isArray(value)) return value.map(canonicalize);
  if (value && typeof value === "object") {
    return Object.fromEntries(Object.keys(value).sort().map((k) => [k, canonicalize(value[k])]));
  }
  return value;
}

// Compared by value: two parses of identical JSON are distinct objects, so a
// by-reference `!==` would report every shard as stale.
const sameValue = (a, b) => JSON.stringify(canonicalize(a)) === JSON.stringify(canonicalize(b));

/**
 * Split one canonical object into shard payloads, reporting any key that would be
 * dropped or duplicated by the split. A part declared as `["*"]` receives every key
 * no other part claims: adding a top-level key to the canonical digest then can never
 * mean "quietly missing from the injected shards", and the key list stops needing
 * manual upkeep.
 */
function claimedElsewhere(key, parts, exceptPart) {
  return Object.entries(parts).some(
    ([part, listed]) => part !== exceptPart && !listed.includes("*") && listed.includes(key),
  );
}

function partKeys(part, listed, canonicalKeys, parts) {
  return listed.includes("*")
    ? canonicalKeys.filter((key) => !claimedElsewhere(key, parts, part))
    : listed;
}

export function shardPlanFor(spec, canonical) {
  const problems = [];
  const canonicalKeys = Object.keys(canonical);
  const assigned = new Map();
  for (const [part, listed] of Object.entries(spec.parts)) {
    for (const key of partKeys(part, listed, canonicalKeys, spec.parts)) {
      if (assigned.has(key)) {
        problems.push(`${spec.source}: key "${key}" is assigned to both shards "${assigned.get(key)}" and "${part}"`);
      }
      assigned.set(key, part);
      if (!canonicalKeys.includes(key)) {
        problems.push(`${spec.source}: shard "${part}" names key "${key}" which the canonical file does not contain`);
      }
    }
  }
  for (const key of canonicalKeys) {
    if (!assigned.has(key)) {
      problems.push(`${spec.source}: key "${key}" is in no shard — sharding would silently drop it`);
    }
  }
  const shards = Object.entries(spec.parts).map(([part, listed]) => ({
    name: `${spec.prefix}-${part}.json`,
    payload: Object.fromEntries(
      partKeys(part, listed, canonicalKeys, spec.parts)
        .filter((key) => canonicalKeys.includes(key))
        .map((key) => [key, canonical[key]]),
    ),
  }));
  return { problems, shards };
}

/**
 * Verify the shards on disk against the canonical digests, or write them.
 * Returns `{ problems, rows }`; `problems` empty means the shards are current.
 */
export function runShards({
  repoRoot = defaultRoot,
  write = false,
  capBytes = CAP_BYTES,
  readFileSync: read = readFileSync,
  writeFileSync: put = writeFileSync,
  mkdirSync: mk = mkdirSync,
  statSync: stat = statSync,
} = {}) {
  const problems = [];
  const rows = [];
  const index = { generatedBy: "scripts/native/digest.mjs", capBytes, sources: [] };
  if (write) mk(path.join(repoRoot, OUT_DIR), { recursive: true });

  for (const spec of SPECS) {
    const sourcePath = path.join(repoRoot, spec.source);
    let raw;
    try {
      raw = read(sourcePath, "utf8");
    } catch (error) {
      problems.push(`${spec.source}: unreadable (${error.message})`);
      continue;
    }
    const sourceDigest = sha256(Buffer.from(raw));
    const canonical = JSON.parse(raw);
    const { problems: specProblems, shards } = shardPlanFor(spec, canonical);
    problems.push(...specProblems);

    const merged = {};
    const entry = { path: spec.source, bytes: Buffer.byteLength(raw), sha256: sourceDigest, shards: [] };
    for (const shard of shards) {
      Object.assign(merged, shard.payload);
      const doc = {
        shard: shard.name,
        from: spec.source,
        sourceSha256: sourceDigest,
        capBytes,
        payload: shard.payload,
      };
      const bytes = serialize(doc);
      const size = Buffer.byteLength(bytes);
      const file = path.join(OUT_DIR, shard.name);
      if (size > capBytes) {
        problems.push(`${file}: ${size} B exceeds the ${capBytes} B injection cap — split it further`);
      }
      if (write) {
        put(path.join(repoRoot, file), bytes);
      } else {
        const target = path.join(repoRoot, file);
        let onDisk = null;
        try {
          onDisk = stat(target) ? read(target, "utf8") : null;
        } catch {
          onDisk = null;
        }
        if (onDisk === null) {
          problems.push(`${file}: missing — run \`node scripts/native/digest.mjs --write\``);
        } else {
          const parsed = JSON.parse(onDisk);
          if (parsed.sourceSha256 !== sourceDigest) {
            problems.push(`${file}: stale — it was cut from a different ${spec.source} (recorded ${parsed.sourceSha256?.slice(0, 12)}…, now ${sourceDigest.slice(0, 12)}…)`);
          }
          if (!sameValue(parsed.payload, shard.payload)) {
            problems.push(`${file}: payload does not match the canonical digest`);
          }
        }
      }
      entry.shards.push({ file, bytes: size });
      rows.push({ file, bytes: size, headroom: CAP_BYTES - size });
    }

    // The load-bearing assertion: shards are a cut, not a summary.
    if (!sameValue(merged, canonical)) {
      problems.push(`${spec.source}: the shards of ${spec.prefix}-* do not reconstruct the canonical digest`);
    }
    index.sources.push(entry);
  }

  const indexBytes = serialize(index);
  const indexFile = path.join(OUT_DIR, "index.json");
  if (Buffer.byteLength(indexBytes) > capBytes) problems.push(`${indexFile}: exceeds the injection cap`);
  if (write) put(path.join(repoRoot, indexFile), indexBytes);
  else {
    const target = path.join(repoRoot, indexFile);
    try {
      if (read(target, "utf8") !== indexBytes) problems.push(`${indexFile}: does not match the regenerated index`);
    } catch {
      problems.push(`${indexFile}: missing — run \`node scripts/native/digest.mjs --write\``);
    }
  }
  return { problems, rows, index };
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const mode = process.argv[2];
  if (mode !== "--write" && mode !== "--check") {
    console.error("usage: digest.mjs <--write|--check>");
    process.exit(2);
  }
  if (mode === "--write") mkdirSync(path.join(defaultRoot, OUT_DIR), { recursive: true });
  const { problems, rows } = runShards({ repoRoot: defaultRoot, write: mode === "--write" });
  for (const row of rows) {
    console.log(`${(row.bytes / 1024).toFixed(2).padStart(6)} KB  headroom ${(row.headroom / 1024).toFixed(1).padStart(5)} KB  ${row.file}`);
  }
  if (problems.length) {
    for (const problem of problems) console.error(`PROBLEM ${problem}`);
    console.error(`digest: ${problems.length} problem(s)`);
    process.exit(1);
  }
  console.log(`${mode === "--write" ? "wrote" : "verified"} ${rows.length} shard(s), largest ${Math.max(...rows.map((r) => r.bytes))} B against a ${CAP_BYTES} B cap`);
}
