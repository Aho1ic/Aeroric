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
import { mkdirSync, readdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
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
    // One gap record is a paragraph, so the whole array crossed 96% of the cap on
    // 2026-09-28. `chunks` splits a *single-key* part's array into ordered shards
    // (gaps-1 … gaps-N) instead of trimming prose — trimming the ledger to fit an
    // injection limit is losing rows.
    // 2026-09-28 第二次加片：`G23` 把 gaps-1 顶到 28 617 B（87% 上限），被 `digest-unit` 的 80% headroom 用例当场判红。
    // 判据不是「能不能塞进 32 768」，而是「注入清单里每片都得留着改字的地方」，所以宁可多一片。
    // 2026-09-28 第三次：`G27` 把 gaps-2 顶到 26 959 B（82%），同一条用例再次判红 ⇒ 3 片→4 片。
    // 2026-09-29 第四次：登记 `G32` 之后 gaps-2 又回到 26 959 B（82%），`digest-unit` 的 80% headroom 用例判红。
    // 归因用工具自己跑的两组数（不是手算）：30 条⇒最大片 23 486 B（72%，绿）；31 条⇒26 959 B（82%，红）。
    // 同一次还暴露一件事：`digest --check` 只校验 32 768 硬上限，80% 规则只活在 `digest-unit` 里，
    // 所以改完台账必须把 `native:check skeleton` 一起跑，不能只跑三个 --check。⇒ 4 片→5 片。
    // 2026-09-29 第五次：登记 `G37` 之后 gaps-2 到 26 987 B（82%），同一条 80% headroom 用例再次判红。
    // 这次没有新机制可查——照第 52～56 行记的既有口径加一片即可；教训是**这条用例本来就是设计来在每次登记时判红的**，
    // 红不代表登记做错了，代表片数该进一格。⇒ 5 片→6 片。
    // 2026-09-29 第六次：给**全部 29 条 open gap** 逐条静态复跑并盖 `verified_on=2026-09-29` 之后，
    // gaps-2 到 26 393 B＝80.5%（只超线 **179 B**），`digest-unit` 第 6 次行使否决 ⇒ 6 片→7 片。
    // 这一次的形状和前五次不同：**没有新增、也没有闭合任何 gap**，一次纯复核就能吃掉一片的余量
    // ⇒ 下次整本复核之前，先把片数抬一格比事后被动补救便宜。
    // 2026-09-29 第七次：第 14 项定 ② 之后改 G25（status＋一段 retriage_note）并登记 `G39`，gaps-2 到 26 436 B＝80.7%，
    // 同一条 80% headroom 用例第 7 次行使否决 ⇒ 7 片→8 片。这次的形状介于第 5 次与第 6 次之间：
    // 一条 gap 关闭、一条新登记，净增 1 条（37→38）就吃掉一格——**台账的"进出相抵"不等于体积相抵**。
    // 2026-10-02 第八次：P4-04 收口登记 `G40`/`G41`（后者三段长文），gaps-6 到 27 397 B＝83.6%，
    // 同一条 80% headroom 用例第 8 次行使否决 ⇒ 8 片→9 片。
    // 2026-10-08 第九次：P8-05 切片十六补记 G43 的真实方法名/图片边界后，
    // gaps-3 到 26 404 B（80.6%）；保留同一 80% 余量判据，9 片→10 片。
    chunks: { gaps: 10 },
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

/**
 * Cut one array into `want` contiguous pieces whose serialized sizes are as close
 * as possible, never emptying a piece. Contiguous keeps the canonical order for
 * free, which a by-weight distribution would not.
 */
function splitRows(rows, want) {
  const sizes = rows.map((row) => Buffer.byteLength(serialize(row)));
  const total = sizes.reduce((a, b) => a + b, 0);
  const cuts = [];
  let acc = 0;
  for (let i = 0; i < rows.length - 1 && cuts.length < want - 1; i++) {
    acc += sizes[i];
    const done = cuts.length + 1;
    // Only cut where every remaining piece still gets at least one row.
    const forced = rows.length - (i + 1) === want - done;
    if (forced || acc >= (total * done) / want) cuts.push(i + 1);
  }
  const bounds = [0, ...cuts, rows.length];
  return bounds.slice(0, -1).map((from, idx) => rows.slice(from, bounds[idx + 1]));
}

/**
 * Expand the parts a spec declared `chunks` for into numbered shards, refusing any
 * chunk request that could not be honoured — a silently unchunked shard is the
 * failure mode here, because it reads as "the split happened".
 */
export function expandChunks(spec, canonical, shards) {
  const problems = [];
  const wanted = Object.entries(spec.chunks ?? {});
  if (!wanted.length) return { problems, shards };
  const out = [];
  for (const shard of shards) {
    const part = shard.name.slice(spec.prefix.length + 1).replace(/\.json$/, "");
    const want = Number(Object.fromEntries(wanted)[part] ?? 0);
    if (!want) {
      out.push(shard);
      continue;
    }
    const keys = Object.keys(shard.payload);
    if (keys.length !== 1) {
      problems.push(`${spec.source}: shard "${part}" is declared chunked but claims ${keys.length} keys — chunking takes exactly one array key`);
      out.push(shard);
      continue;
    }
    const key = keys[0];
    const rows = canonical[key];
    if (!Array.isArray(rows)) {
      problems.push(`${spec.source}: shard "${part}" is declared chunked but canonical key "${key}" is not an array`);
      out.push(shard);
      continue;
    }
    if (want > rows.length) {
      problems.push(`${spec.source}: cannot split ${rows.length} row(s) of "${key}" into ${want} shards — every shard needs a row`);
      out.push(shard);
      continue;
    }
    splitRows(rows, want).forEach((piece, idx) => {
      out.push({ name: `${spec.prefix}-${part}-${idx + 1}.json`, payload: { [key]: piece } });
    });
  }
  return { problems, shards: out };
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
  const planned = Object.entries(spec.parts).map(([part, listed]) => ({
    name: `${spec.prefix}-${part}.json`,
    payload: Object.fromEntries(
      partKeys(part, listed, canonicalKeys, spec.parts)
        .filter((key) => canonicalKeys.includes(key))
        .map((key) => [key, canonical[key]]),
    ),
  }));
  const expanded = expandChunks(spec, canonical, planned);
  problems.push(...expanded.problems);
  return { problems, shards: expanded.shards };
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
    // Planned payloads only prove the SPECS are a full cut. What a session injects is
    // the bytes on disk, so reconstruct from those as well — otherwise a hand-edited
    // or truncated shard passes every per-file compare while carrying a wrong row.
    const fromDisk = {};
    let diskReadable = true;
    const fold = (into, payload, where) => {
      for (const [key, value] of Object.entries(payload)) {
        if (!(key in into)) {
          into[key] = value;
        } else if (Array.isArray(into[key]) && Array.isArray(value)) {
          into[key] = into[key].concat(value);
        } else {
          problems.push(`${spec.source}: key "${key}" appears in more than one shard and is not an array — sharding would lose a value${where}`);
        }
      }
    };
    const entry = { path: spec.source, bytes: Buffer.byteLength(raw), sha256: sourceDigest, shards: [] };
    for (const shard of shards) {
      fold(merged, shard.payload, "");
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
          diskReadable = false;
        } else {
          const parsed = JSON.parse(onDisk);
          if (parsed.sourceSha256 !== sourceDigest) {
            problems.push(`${file}: stale — it was cut from a different ${spec.source} (recorded ${parsed.sourceSha256?.slice(0, 12)}…, now ${sourceDigest.slice(0, 12)}…)`);
          }
          if (!sameValue(parsed.payload, shard.payload)) {
            problems.push(`${file}: payload does not match the canonical digest`);
          }
          fold(fromDisk, parsed.payload ?? {}, ` (from ${file})`);
        }
      }
      entry.shards.push({ file, bytes: size });
      rows.push({ file, bytes: size, headroom: CAP_BYTES - size });
    }

    // The load-bearing assertion: shards are a cut, not a summary.
    if (!sameValue(merged, canonical)) {
      problems.push(`${spec.source}: the shards of ${spec.prefix}-* do not reconstruct the canonical digest`);
    }
    if (!write && diskReadable && !sameValue(fromDisk, canonical)) {
      problems.push(`${spec.source}: the shard files on disk do not reconstruct the canonical digest — a shard was edited, truncated or left behind`);
    }
    index.sources.push(entry);
  }

  // A shard the plan stopped emitting must not survive: injection references files
  // by path, so an orphaned shard keeps feeding its outdated content into context.
  const emitted = new Set(rows.map((row) => row.file));
  let present = [];
  try {
    present = readdirSync(path.join(repoRoot, OUT_DIR));
  } catch {
    present = [];
  }
  for (const entry of present.sort()) {
    if (!entry.endsWith(".json") || entry === "index.json") continue;
    if (!SPECS.some((spec) => entry.startsWith(`${spec.prefix}-`))) continue;
    const rel = `${OUT_DIR}/${entry}`.split(path.sep).join("/");
    if (!emitted.has(rel)) {
      problems.push(`${rel}: stale shard — the plan no longer emits this file; delete it, an injection entry pointing here would keep loading old content`);
    }
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
  // `pnpm native:digest` already appends `--write`, so `pnpm native:digest --check`
  // arrives as two modes and used to silently take the writing one — every such call
  // reported success while repairing the shards it was supposed to verify.
  const requested = process.argv.slice(2);
  if (requested.length !== 1 || (requested[0] !== "--write" && requested[0] !== "--check")) {
    console.error(`usage: digest.mjs <--write|--check> — got: ${requested.join(" ") || "(nothing)"}`);
    if (requested.includes("--write") && requested.includes("--check")) {
      console.error("  both modes given: `pnpm native:digest --check` expands to `--write --check` and would WRITE.");
      console.error("  to verify without editing, run: node scripts/native/digest.mjs --check  (that is what the gate case does)");
    }
    process.exit(2);
  }
  const mode = requested[0];
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
