#!/usr/bin/env node
// §5.1 extraction expansion: one row per source file, generated from the files
// themselves and cross-checked against docs/native/command-map.json.
//
// The plan requires this expansion before a whole-directory module may be
// extracted, and says the generator must abort rather than write when the counts
// disagree. Before this file existed that claim was unverifiable: the expansion
// documents were produced by throwaway code, so nothing could re-derive them.
//
// Command counting reuses inventory.parseCommandDefinitions — the same parser
// that produced the frozen 660-command inventory — so "168 wrappers" here and
// "168 rows in command-map.json" cannot drift into two different definitions.
import { existsSync, mkdirSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";
import { parseCommandDefinitions } from "./inventory.mjs";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..");

const PUB_ITEM = /^\s*pub(?:\([^)]*\))?\s+(?:async\s+|unsafe\s+|extern\s+)*(?:(?:struct|enum|trait|type|union|const|static|mod|fn)\s+)([A-Za-z_]\w*)/;
// Column 0 only: `pub fn` inside an `impl` block is a method, not a top-level
// item this file exports, and counting methods inflated every row by ~50 %.
const KIND_OF = /^pub(?:\([^)]*\))?\s+(?:async\s+|unsafe\s+|extern\s+)*(struct|enum|trait|type|union|const|static|mod|fn)\s+([A-Za-z_]\w*)/;
const TEST_ATTR = /^\s*#\[(?:tokio::|actix::)?test(?:\(|\])/;

/**
 * Symbols outside any `#[cfg(test)] mod …` block, tracked by brace depth, plus
 * the count of test items that were skipped. Test modules are not always at the
 * end of a file, so "cut at the first #[cfg(test)]" would misclassify.
 */
export function splitPublicSymbols(text) {
  const lines = text.split("\n");
  const symbols = [];
  let testItems = 0;
  let depth = 0;
  let pendingCfgTest = false;
  let testDepth = null;
  for (const line of lines) {
    const trimmed = line.trim();
    const before = depth;

    if (testDepth === null) {
      if (/^#\[cfg\(test\)\]\s*$/.test(trimmed)) pendingCfgTest = true;
      else if (pendingCfgTest && trimmed) {
        if (/^(?:pub\s+)?mod\s+[A-Za-z_]\w*\s*\{/.test(trimmed)) testDepth = before;
        pendingCfgTest = false;
      }
    }
    for (const ch of line) {
      if (ch === "{") depth += 1;
      else if (ch === "}") depth -= 1;
    }
    const inTests = testDepth !== null;
    if (inTests && depth <= testDepth && trimmed.includes("}")) testDepth = null;

    if (TEST_ATTR.test(trimmed)) {
      if (inTests) testItems += 1;
      continue;
    }
    if (inTests) continue;
    const kind = line.match(KIND_OF);
    if (!kind) continue;
    const [, keyword, name] = kind;
    symbols.push(keyword === "fn" ? `${name}()` : `${keyword[0].toUpperCase()}${keyword.slice(1)} ${name}`);
  }
  const unique = [];
  const seen = new Set();
  for (const s of symbols) if (!seen.has(s)) { seen.add(s); unique.push(s); }
  return { symbols: unique.sort(), testItems };
}

/** Hazards §5.1 step 5 makes every row verify before the file moves. */
export function hazardsFor(text, commandCount) {
  const found = [];
  if (commandCount > 0) found.push("command-layer");
  if (/AppHandle|tauri::State|State</.test(text)) found.push("AppHandle/State");
  if (/\.emit\(|emit_all|emit_to\(/.test(text)) found.push("emits events");
  if (/include_str!|include_bytes!/.test(text)) found.push("include_str! path rewrite");
  if (/#\[cfg\(windows\)\]/.test(text)) found.push("cfg(windows) branch");
  if (/#\[cfg\(unix\)\]/.test(text)) found.push("cfg(unix) branch");
  if (/tokio::spawn|thread::spawn|spawn_blocking|\bspawn\(/.test(text)) found.push("spawns task/thread (needs a cancellation token)");
  if (/keychain|secret|password|passphrase|token/i.test(text)) found.push("credential owner");
  return found;
}

/** Build the §5.1 rows for the given top-level modules under src-tauri/src. */
export function expandModules({ root = repoRoot, modules, readFile = (p) => readFileSync(p, "utf8") }) {
  const definitions = parseCommandDefinitions(path.join(root, "src-tauri", "src"), readFile);
  const perFile = new Map();
  for (const [, at] of definitions) {
    const file = at.slice(0, at.lastIndexOf(":"));
    perFile.set(file, (perFile.get(file) ?? 0) + 1);
  }
  const rows = [];
  for (const moduleName of modules) {
    const dir = path.join(root, "src-tauri", "src", moduleName);
    // A module is often both `foo.rs` and `foo/`: dsh_webui keeps 14 of its 66
    // command wrappers in the sibling file, so walking only the directory would
    // silently under-count the module (the cross-check below would then abort).
    const files = walkRust(dir);
    const sibling = path.join(root, "src-tauri", "src", `${moduleName}.rs`);
    if (existsSync(sibling)) files.unshift(sibling);
    files.sort();
    if (!files.length) throw new Error(`no .rs files found under ${dir} — check the module name before trusting an empty expansion`);
    for (const file of files) {
      const text = readFile(file);
      const rel = path.relative(root, file).split(path.sep).join("/");
      const underModule = rel.startsWith(`src-tauri/src/${moduleName}/`)
        ? rel.slice(`src-tauri/src/${moduleName}/`.length)
        : `${moduleName}-root.rs`;
      const commands = perFile.get(file) ?? 0;
      const { symbols, testItems } = splitPublicSymbols(text);
      rows.push({
        source: rel,
        destination: `core/crates/aeroric-core/${moduleName}/${underModule}`,
        commands,
        symbols,
        testItems,
        hazards: hazardsFor(text, commands),
        lines: text.split("\n").length,
      });
    }
  }
  return { rows, totalCommands: rows.reduce((sum, r) => sum + r.commands, 0) };
}

/** Every `.rs` file under `dir`, sorted by repository-relative path. */
/** The rendered cell truncates at 15 names with "+N more"; undo that to compare. */
function countRenderedSymbols(cell) {
  const more = cell.match(/\+(\d+) more/);
  if (more) return 15 + Number(more[1]);
  return cell.split(/,(?![^(]*\))/).filter((part) => part.trim()).length;
}

function walkRust(dir) {
  const out = [];
  if (!existsSync(dir)) return out;
  const stack = [dir];
  while (stack.length) {
    const current = stack.pop();
    for (const dirent of readdirSync(current, { withFileTypes: true })) {
      const full = path.join(current, dirent.name);
      if (dirent.isDirectory()) stack.push(full);
      else if (dirent.isFile() && full.endsWith(".rs")) out.push(full);
    }
  }
  return out.sort();
}

export function renderRow(index, row) {
  const shown = row.symbols.length > 15
    ? `${row.symbols.slice(0, 15).join(", ")} +${row.symbols.length - 15} more`
    : row.symbols.join(", ");
  const symbols = `${shown || "—"} (${row.testItems} test items excluded)`;
  return `| ${index} | \`${row.source}\` | \`${row.destination}\` | ${row.commands} | ${symbols} | ${row.hazards.length ? row.hazards.join("; ") : "—"} |`;
}

/** Full markdown body for a module set, including the aggregate risk register. */
export function renderDocument({ modules, rows, totalCommands, mapCounts }) {
  const count = (predicate) => rows.filter((r) => r.hazards.some(predicate)).length;
  const total = (field) => rows.reduce((sum, r) => sum + r[field], 0);
  const lines = [
    `# §5.1 extraction expansion — \`${modules.join("` / `")}\``,
    "",
    "> Generated by `scripts/native/extraction.mjs`; the table below is machine-reproducible and",
    "> `--check` fails if it drifts from the source or from `docs/native/command-map.json`. Planning",
    "> data only — it authorises no code movement.",
    "",
    `Cross-checked: ${totalCommands} command wrappers counted here against ${mapCounts} matching entries in \`command-map.json\`.`,
    "",
    "| # | source | destination | commands | moved symbols (top-level public) | per-file hazards to verify |",
    "|---|---|---|---|---|---|",
    ...rows.map((row, i) => renderRow(i + 1, row)),
    "",
    "### Aggregate risk register for this expansion",
    "",
    `- files: **${rows.length}**, lines: **${total("lines")}**, command wrappers: **${totalCommands}**, test items excluded from the symbol lists: **${total("testItems")}**`,
    `- files carrying \`AppHandle\`/\`State\`: **${count((h) => h.startsWith("AppHandle"))}**`,
    `- files emitting events: **${count((h) => h === "emits events")}**`,
    `- files with \`include_str!\`/\`include_bytes!\`: **${count((h) => h.startsWith("include_str"))}**`,
    `- files with \`cfg(windows)\`: **${count((h) => h === "cfg(windows) branch")}**; with \`cfg(unix)\`: **${count((h) => h === "cfg(unix) branch")}**`,
    `- files that spawn (need an explicit cancellation/reap owner before extraction): **${count((h) => h.startsWith("spawns"))}**`,
    `- files touching credentials: **${count((h) => h === "credential owner")}**`,
    "",
  ];
  return lines.join("\n");
}

/** Compare a committed expansion document's table against a fresh expansion. */
export function tableDrift({ committedText, rows }) {
  const problems = [];
  const existing = [...String(committedText).matchAll(/^\| (\d+) \| `([^`]+)` \| `([^`]+)` \| (\d+) \|([^|]*)\((\d+) test items excluded\)/gm)];
  if (existing.length === 0) return ["committed document contains no §5.1 table rows to compare"];
  if (existing.length !== rows.length) problems.push(`row count differs: committed ${existing.length} vs generated ${rows.length}`);
  const bySource = new Map(rows.map((r) => [r.source, r]));
  for (const [, , source, destination, commands, symbolsCell, testItems] of existing) {
    const row = bySource.get(source);
    if (!row) { problems.push(`${source}: present in the committed document but not in the current source tree`); continue; }
    if (row.destination !== destination) problems.push(`${source}: destination differs (${destination} vs ${row.destination})`);
    if (String(row.commands) !== commands) problems.push(`${source}: command wrappers differ (committed ${commands}, generated ${row.commands})`);
    if (String(row.testItems) !== testItems) problems.push(`${source}: test items differ (committed ${testItems}, generated ${row.testItems})`);
    const committedSymbols = symbolsCell.split(/,(?![^()]*\))/).length && symbolsCell.trim() !== "—" ? countRenderedSymbols(symbolsCell) : 0;
    if (committedSymbols !== row.symbols.length) problems.push(`${source}: moved-symbol count differs (committed ${committedSymbols}, generated ${row.symbols.length})`);
  }
  for (const row of rows) if (!existing.some(([, , source]) => source === row.source)) problems.push(`${row.source}: new file, missing from the committed document`);
  return problems;
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const argv = process.argv.slice(2);
  const flag = (name) => { const i = argv.indexOf(name); return i === -1 ? null : argv[i + 1]; };
  const modules = argv.flatMap((a) => (a.startsWith("--module=") ? [a.slice("--module=".length)] : []));
  const writePath = flag("--write");
  const checkPath = flag("--check");
  const mapPath = path.join(repoRoot, "docs/native/command-map.json");
  if (!modules.length || (!writePath && !checkPath)) {
    console.error("usage: extraction.mjs --module=<dir>[ --module=<dir>…] <--write docs/native/extraction/x.md|--check <existing.md>>");
    process.exit(2);
  }
  const map = JSON.parse(readFileSync(mapPath, "utf8"));
  // `rust_path` is a module path (`database::query::dbx_query_table`), not a file path.
  const prefixes = modules.map((m) => `${m}::`);
  const mapCount = map.commands.filter((c) => prefixes.some((p) => c.rust_path.startsWith(p))).length;
  const { rows, totalCommands } = expandModules({ modules });
  if (totalCommands !== mapCount) {
    console.error(`extraction: refusing to write — counted ${totalCommands} command wrappers in ${modules.join(", ")} but command-map.json records ${mapCount}. A disagreement here means either the inventory or this expansion is wrong; fix the counting, not the assertion.`);
    process.exit(1);
  }
  if (checkPath) {
    const problems = tableDrift({ committedText: readFileSync(path.resolve(checkPath), "utf8"), rows });
    if (problems.length) { console.error(`extraction --check ${checkPath}: DRIFT\n` + problems.map((p) => `  - ${p}`).join("\n")); process.exit(1); }
    console.log(`extraction --check ${checkPath}: ${rows.length} rows, ${totalCommands} wrappers, matches the source tree and command-map.json`);
    process.exit(0);
  }
  const body = renderDocument({ modules, rows, totalCommands, mapCounts: mapCount });
  mkdirSync(path.dirname(path.resolve(writePath)), { recursive: true });
  writeFileSync(path.resolve(writePath), body);
  console.log(`extraction: wrote ${rows.length} rows (${totalCommands} wrappers) to ${writePath}`);
}
