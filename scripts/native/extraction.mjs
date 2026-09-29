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
/** Expand one `{a,b}` brace group: `src/x/{a,b}.rs` -> two paths. Nested braces recurse. */
export function expandBraced(candidate) {
  const open = candidate.indexOf("{");
  if (open === -1) return [candidate];
  const close = candidate.indexOf("}", open);
  if (close === -1) return [candidate];
  const before = candidate.slice(0, open);
  const after = candidate.slice(close + 1);
  return candidate.slice(open + 1, close).split(",").flatMap((part) => expandBraced(before + part.trim() + after));
}

/**
 * Pull the `**Action**` block of the named Tasks out of MIGRATION_PLAN.md: the verbatim
 * numbered lines (so a human can re-read the decision) and every `.rs` path they name, brace
 * expanded. The destination column is a mechanical mirror, so this is what makes the two
 * carriers of "where does this file go" read the same source (表态项 14，2026-09-29 定为 ②):
 * the plan stays the authority, and the table says so in its own header.
 */
export function parseTaskActions(planText, tasks) {
  const lines = String(planText).split("\n");
  const wanted = new Set(tasks);
  const found = tasks.map((task) => ({ task, verbatim: [], targets: new Set() }));
  const byTask = new Map(found.map((f) => [f.task, f]));
  let active = null;
  let seen = null;
  for (const line of lines) {
    const heading = /^### Task (P\d+-\d+) /.exec(line);
    if (heading) { seen = heading[1]; active = null; continue; }
    // Activation happens only on the `**Action**` line and stops at the next top-level field:
    // `**Input**` precedes it and names *host* files (`session.rs`, `session/export.rs`), and
    // the 2026-09-29 first version treated those as migration targets — row 1 of sessions.md
    // then "matched" its own source path, which is the one thing this column must never do.
    // Acceptance lines leaked too, quoting `tests/*.rs` as a landing point.
    if (/^- \*\*Action\*\*/.test(line)) { active = wanted.has(seen ?? "") ? seen : null; continue; }
    if (!active) continue;
    if (/^- \*\*(Input|Acceptance Criteria|Non-Goals|Output)\*\*/.test(line) || /^#{1,6} /.test(line)) { active = null; continue; }
    if (!/^\s*\d+\.\s/.test(line) && !/^\s*[-*]\s/.test(line)) continue;
    const entry = byTask.get(active);
    entry.verbatim.push(line.trim());
    for (const token of line.matchAll(/`([^`\n]+\.rs)`/g)) {
      for (const expanded of expandBraced(token[1])) {
        // Only landing points. An Action also names test suites (`tests/dsh_host.rs`) and, in
        // other fields, host files (`src-tauri/src/x.rs`); a basename match against either would
        // put a source or a test into the destination column.
        if (expanded.startsWith("tests/") || expanded.includes("src-tauri/")) continue;
        entry.targets.add(expanded);
      }
    }
  }
  return found.map((f) => ({ task: f.task, verbatim: f.verbatim, targets: [...f.targets].sort() }));
}

/**
 * A row is only "named" by an Action when the basename matches. Anything else is reported as
 * unmapped instead of guessed: 8 of the 11 sessions rows have no 1:1 name because one host file
 * is split into several responsibility files, and inventing that correspondence is precisely the
 * option the user did not take.
 */
export function actionTargetsFor(source, targets) {
  const stem = path.basename(source);
  const hits = targets.filter((candidate) => path.basename(candidate) === stem);
  return hits.length ? { kind: "named", paths: hits } : { kind: "unmapped", paths: [] };
}

/** Keep the cell machine-comparable: no prose inside a `named` cell. */
export function renderActionCell(match) {
  if (match.kind === "named") return match.paths.map((p) => `\`${p}\``).join("<br>");
  return "unmapped（Action 未给同名文件，见上方 Action 原文）";
}

export function expandModules({ root = repoRoot, modules, readFile = (p) => readFileSync(p, "utf8"), actions = [] }) {
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
        action: actionTargetsFor(rel, actions),
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
  return `| ${index} | \`${row.source}\` | \`${row.destination}\` | ${row.commands} | ${symbols} | ${renderActionCell(row.action)} | ${row.hazards.length ? row.hazards.join("; ") : "—"} |`;
}

/** Full markdown body for a module set, including the aggregate risk register. */
export const GENERATED_START = "<!-- generated:start — `scripts/native/extraction.mjs` owns everything between the two markers; everything outside them is hand-written and survives `--write` -->";
export const GENERATED_END = "<!-- generated:end -->";

/**
 * The marked block: the ② banner, the verbatim Task Actions and the table. The "Cross-checked"
 * line and the risk register deliberately stay OUTSIDE the markers — the four committed documents
 * keep their own register shape (database-notebook's is a JSON block listing per-file arrays that
 * the generator cannot re-derive), and pulling them inside would either duplicate that heading or
 * delete information. Wrapper totals are enforced by the CLI abort, not by prose.
 */
export function renderGeneratedBlock({ rows, actionDocs = [] }) {
  return [
    `> **落点以这一段为准（2026-09-29 表态项 14 定为 ②）**：下表 \`destination\` 列是生成器按 \`core/crates/aeroric-core/<模块>/<文件>\` 机械推导的**镜像**——它只保证"每个源文件都有一条"，不承诺落点（全列还缺 \`src/\` 那一层）。一个宿主文件被拆成多个职责文件时"同名 1:1"本来就不存在，所以 \`Action 给定的落点\` 列只在 basename 相同才算命名，否则写 \`unmapped\`，**不做猜测**。真正授权搬迁的是所属 Task 的 Action 原文，逐字抄在下面（\`--check\` 连这几行一起比对）。`,
    ">",
    ...actionDocs.flatMap((doc) => [
      `> **Task ${doc.task} — Action 原文（机械抽取）**`,
      ...(doc.verbatim.length ? doc.verbatim.map((line) => `> ${line}`) : ["> （该 Task 的 Action 里没有 \`.rs\` 路径——本清单的落点全部 \`unmapped\`）"]),
      ">",
    ]),
    "",
    "| # | source | destination（机械镜像，非落点） | commands | moved symbols (top-level public) | Action 给定的落点 | per-file hazards to verify |",
    "|---|---|---|---|---|---|---|",
    ...rows.map((row, i) => renderRow(i + 1, row)),
  ].join("\n");
}

/** Replace only the marked block; a document without both markers is refused (null). */
export function spliceGenerated(existingText, block) {
  const start = existingText.indexOf(GENERATED_START);
  const end = existingText.indexOf(GENERATED_END);
  if (start === -1 || end === -1 || end < start) return null;
  return `${existingText.slice(0, start)}${GENERATED_START}\n${block}\n${existingText.slice(end)}`;
}

/** Register bullets — emitted only when a document is created from scratch. */
export function renderRiskRegister(rows, totalCommands) {
  const count = (predicate) => rows.filter((r) => r.hazards.some(predicate)).length;
  const total = (field) => rows.reduce((sum, r) => sum + r[field], 0);
  return [
    "### Aggregate risk register for this expansion",
    "",
    `- files: **${rows.length}**, lines: **${total("lines")}**, command wrappers: **${totalCommands}**, test items excluded from the symbol lists: **${total("testItems")}**`,
    `- files carrying \`AppHandle\`/\`State\`: **${count((h) => h.startsWith("AppHandle"))}**`,
    `- files emitting events: **${count((h) => h === "emits events")}**`,
    `- files with \`include_str!\`/\`include_bytes!\`: **${count((h) => h.startsWith("include_str"))}**`,
    `- files with \`cfg(windows)\`: **${count((h) => h === "cfg(windows) branch")}**; with \`cfg(unix)\`: **${count((h) => h === "cfg(unix) branch")}**`,
    `- files that spawn (need an explicit cancellation/reap owner before extraction): **${count((h) => h.startsWith("spawns"))}**`,
    `- files touching credentials: **${count((h) => h === "credential owner")}**`,
  ];
}
/**
 * The aggregate risk register is generated data too. Leaving it as prose is what let two of the
 * four documents keep stale counts (measured 2026-09-29: dsh-webui said `lines: **9323**` and
 * remote said `**9977**` while the source tree reads 9349 and 9962) with `--check` green — the
 * row comparison never looked at the register. `--write` now refreshes the seven-bullet form in
 * place, and leaves any other shape (the hand-written JSON register in `database-notebook.md`)
 * untouched; `registerDrift` verifies whichever form the document carries.
 */
export function refreshRegister(existingText, rows, totalCommands) {
  const head = existingText.indexOf("### Aggregate risk register for this expansion");
  if (head === -1) return existingText; // no bullet register at all
  const lines = existingText.slice(head).split("\n");
  let i = 1;
  while (i < lines.length && lines[i].trim() === "") i += 1;
  const generated = renderRiskRegister(rows, totalCommands).filter((l) => l.startsWith("- "));
  const committed = [];
  for (let k = i; k < lines.length && lines[k].startsWith("- "); k += 1) committed.push(lines[k]);
  if (committed.length !== generated.length) return existingText; // not our seven-line shape: prose wins
  const tail = lines.slice(i + committed.length).join("\n");
  return `${existingText.slice(0, head)}### Aggregate risk register for this expansion\n\n${generated.join("\n")}\n${tail}`;
}

/** Verify the register: the seven bullets verbatim, or the scalars of a hand-written JSON one. */
export function registerDrift(committedText, rows, totalCommands) {
  if (typeof totalCommands !== "number") return []; // caller passes no totals ⇒ not checking the register
  const problems = [];
  const text = String(committedText);
  const jsonStart = text.indexOf("```json");
  const jsonEnd = jsonStart === -1 ? -1 : text.indexOf("\n```", jsonStart + 7);
  if (jsonStart !== -1 && jsonEnd > jsonStart) {
    let parsed = null;
    try { parsed = JSON.parse(text.slice(jsonStart + 7, jsonEnd)); } catch { return []; }
    if (parsed && typeof parsed === "object" && "files" in parsed) {
      const total = (field) => rows.reduce((sum, r) => sum + r[field], 0);
      const expect = {
        files: rows.length,
        commands: totalCommands,
        testOnlyItemsExcluded: total("testItems"),
        // `symbols` is a list of rendered names, not a count; summing it directly stringifies the
        // whole table — which is exactly what the first run of this check caught.
        pubSymbolsTotal: rows.reduce((sum, r) => sum + r.symbols.length, 0),
      };
      for (const [key, want] of Object.entries(expect)) {
        if (parsed[key] !== want) problems.push(`register ${key} differs (committed ${parsed[key]}, generated ${want})`);
      }
      return problems;
    }
  }
  for (const line of renderRiskRegister(rows, totalCommands).filter((l) => l.startsWith("- "))) {
    if (!text.includes(line)) problems.push(`risk register line stale or missing: "${line.slice(0, 64)}…"`);
  }
  return problems;
}

/** Body for a brand-new expansion document (nothing to preserve). */
export function renderDocument({ modules, rows, totalCommands, mapCounts, actionDocs = [] }) {
  return [
    `# §5.1 extraction expansion — \`${modules.join("` / `")}\``,
    "",
    "> Generated by `scripts/native/extraction.mjs`; the table below is machine-reproducible and",
    "> `--check` fails if it drifts from the source or from `docs/native/command-map.json`. Planning",
    "> data only — it authorises no code movement.",
    "",
    `Cross-checked: ${totalCommands} command wrappers counted here against ${mapCounts} matching entries in \`command-map.json\`.`,
    "",
    GENERATED_START,
    renderGeneratedBlock({ rows, actionDocs }),
    GENERATED_END,
    "",
    ...renderRiskRegister(rows, totalCommands),
    "",
  ].join("\n");
}

/** Compare a committed expansion document's table and register against a fresh expansion. */
export function tableDrift({ committedText, rows, actionDocs = [], totalCommands }) {
  const problems = [];
  const existing = [...String(committedText).matchAll(/^\| (\d+) \| `([^`]+)` \| `([^`]+)` \| (\d+) \|([^|]*)\((\d+) test items excluded\) \| ([^|]*) \|/gm)];
  if (existing.length === 0) return ["committed document contains no §5.1 table rows to compare"];
  if (existing.length !== rows.length) problems.push(`row count differs: committed ${existing.length} vs generated ${rows.length}`);
  const bySource = new Map(rows.map((r) => [r.source, r]));
  for (const [, , source, destination, commands, symbolsCell, testItems, actionCell] of existing) {
    const row = bySource.get(source);
    if (!row) { problems.push(`${source}: present in the committed document but not in the current source tree`); continue; }
    if (row.destination !== destination) problems.push(`${source}: destination differs (${destination} vs ${row.destination})`);
    if (renderActionCell(row.action) !== actionCell.trim()) problems.push(`${source}: Action-target cell differs (committed ${actionCell.trim()}, derived ${renderActionCell(row.action)})`);
    if (String(row.commands) !== commands) problems.push(`${source}: command wrappers differ (committed ${commands}, generated ${row.commands})`);
    if (String(row.testItems) !== testItems) problems.push(`${source}: test items differ (committed ${testItems}, generated ${row.testItems})`);
    const committedSymbols = symbolsCell.split(/,(?![^()]*\))/).length && symbolsCell.trim() !== "—" ? countRenderedSymbols(symbolsCell) : 0;
    if (committedSymbols !== row.symbols.length) problems.push(`${source}: moved-symbol count differs (committed ${committedSymbols}, generated ${row.symbols.length})`);
  }
  for (const row of rows) if (!existing.some(([, , source]) => source === row.source)) problems.push(`${row.source}: new file, missing from the committed document`);
  // The header block is the only place the plan's own words enter the document, so it is
  // compared too — otherwise the "plan is the authority" banner could rot silently while
  // every table cell still matched (the exact shape G26 says the 4 compared docs can have).
  for (const doc of actionDocs) {
    for (const line of doc.verbatim) {
      if (!String(committedText).includes(line)) problems.push(`${doc.task}: committed document no longer quotes Action line "${line.slice(0, 70)}…"`);
    }
  }
  problems.push(...registerDrift(committedText, rows, totalCommands));
  return problems;
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const argv = process.argv.slice(2);
  const flag = (name) => { const i = argv.indexOf(name); return i === -1 ? null : argv[i + 1]; };
  const modules = argv.flatMap((a) => (a.startsWith("--module=") ? [a.slice("--module=".length)] : []));
  const writePath = flag("--write");
  const checkPath = flag("--check");
  const tasks = argv.flatMap((a) => (a.startsWith("--task=") ? [a.slice("--task=".length)] : []));
  const mapPath = path.join(repoRoot, "docs/native/command-map.json");
  if (!modules.length || (!writePath && !checkPath)) {
    console.error("usage: extraction.mjs --module=<dir>[ --module=<dir>…][ --task=P3-04] <--write docs/native/extraction/x.md|--check <existing.md>>");
    process.exit(2);
  }
  const map = JSON.parse(readFileSync(mapPath, "utf8"));
  // `rust_path` is a module path (`database::query::dbx_query_table`), not a file path.
  const prefixes = modules.map((m) => `${m}::`);
  const mapCount = map.commands.filter((c) => prefixes.some((p) => c.rust_path.startsWith(p))).length;
  const actionDocs = tasks.length
    ? parseTaskActions(readFileSync(path.join(repoRoot, "MIGRATION_PLAN.md"), "utf8"), tasks)
    : [];
  for (const doc of actionDocs) {
    // A Task that resolves to nothing is almost always a renamed heading, not an empty plan:
    // quoting zero Action lines would quietly make every row `unmapped` and still pass.
    if (!doc.verbatim.length) {
      console.error(`extraction: --task=${doc.task} matched no Action block in MIGRATION_PLAN.md — refuse to publish a target-less expansion`);
      process.exit(1);
    }
  }
  const actions = actionDocs.flatMap((doc) => doc.targets);
  const { rows, totalCommands } = expandModules({ modules, actions });
  if (totalCommands !== mapCount) {
    console.error(`extraction: refusing to write — counted ${totalCommands} command wrappers in ${modules.join(", ")} but command-map.json records ${mapCount}. A disagreement here means either the inventory or this expansion is wrong; fix the counting, not the assertion.`);
    process.exit(1);
  }
  if (checkPath) {
    const problems = tableDrift({ committedText: readFileSync(path.resolve(checkPath), "utf8"), rows, actionDocs, totalCommands });
    if (problems.length) { console.error(`extraction --check ${checkPath}: DRIFT\n` + problems.map((p) => `  - ${p}`).join("\n")); process.exit(1); }
    console.log(`extraction --check ${checkPath}: ${rows.length} rows, ${totalCommands} wrappers, matches the source tree and command-map.json`);
    process.exit(0);
  }
const block = renderGeneratedBlock({ rows, actionDocs });
const target = path.resolve(writePath);
const existing = existsSync(target) ? readFileSync(target, "utf8") : "";
let body;
if (existing) {
  const merged = spliceGenerated(existing, block);
  if (!merged) {
    console.error(`extraction: refusing to overwrite ${writePath} — it carries no \`${GENERATED_START.slice(0, 24)}…\` markers, and a whole-file rewrite deletes every hand-written section below the table. Measured 2026-09-29: an unmarked --write destroyed four hand sections of docs/native/extraction/sessions.md (169 → 30 lines). Wrap the generated region in the markers, then re-run.`);
    process.exit(1);
  }
  body = refreshRegister(merged, rows, totalCommands);
} else {
  body = renderDocument({ modules, rows, totalCommands, mapCounts: mapCount, actionDocs });
}
mkdirSync(path.dirname(target), { recursive: true });
writeFileSync(target, body);
console.log(`extraction: wrote ${rows.length} rows (${totalCommands} wrappers) to ${writePath}${existing ? " (hand-written prose preserved)" : " (new document)"}`);
}
