#!/usr/bin/env node
// G43's source-ownership ratchet, not a substitute for behavioral/UDS tests.
import assert from "node:assert/strict";
import { readFileSync, readdirSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");
function rustFiles(relative) {
  const directory = path.join(root, relative);
  return readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    const file = path.join(relative, entry.name);
    return entry.isDirectory() ? rustFiles(file) : entry.name.endsWith(".rs") ? [file] : [];
  });
}
const parsers = [...rustFiles("src-tauri/src"), ...rustFiles("core/crates")]
  .flatMap((file) => [...readFileSync(path.join(root, file), "utf8")
    .matchAll(/\bfn\s+parse_rg_json_lines\s*\(/g)].map(() => file));
assert.deepEqual(parsers, ["core/crates/aeroric-core/src/files/search.rs"],
  "exactly one root-aware search parser must be owned by core");
// Slice33: target rejection remains pure and first; only the two apply
// operations require the current writer before any registered-root IO.
const hostedSearch = readFileSync(path.join(root, "core/crates/aeroric-host/src/search_services.rs"), "utf8");
const searchRoot = hostedSearch.slice(hostedSearch.indexOf("fn root_for("), hostedSearch.indexOf("fn within_record_root("));
assert.match(hostedSearch, /use crate::file_access::\{require_local_record, write_refusal\};/);
assert.match(searchRoot, /operation: Operation/);
assert.match(searchRoot, /if matches!\(operation, Operation::Apply \| Operation::NativeApply\) \{\s*if let Some\(frame\) = write_refusal\(request, context\) \{\s*return Err\(Box::new\(frame\)\);\s*\}\s*\}/);
const authority = searchRoot.indexOf("write_refusal(");
assert(searchRoot.indexOf("ErrorCode::UnsupportedCapability") >= 0);
assert(searchRoot.indexOf("ErrorCode::UnsupportedCapability") < authority);
assert(authority < searchRoot.indexOf("load_projects(context)"));
assert.match(hostedSearch, /root_for\(&request, context, notebook, operation\)/);
assert.doesNotMatch(searchRoot, /\.lease\(\)|acquire\(/);
// Slice38: id-selected errors retain typed location detail; path-selected
// roots skip ineligible records before both raw-path and task-file access.
const byIdGuard = searchRoot.indexOf("require_local_record(record)");
const byIdPath = searchRoot.indexOf('let root = validate(record.get("path")');
assert(byIdGuard >= 0 && byIdPath > byIdGuard);
assert.match(searchRoot, /require_local_record\(record\)\.map_err\(\|error\| Box::new\(error\.into_frame\(request\)\)\)\?/);
assert.match(searchRoot, /for project in projects \{\s*(?:\/\/[^\n]*\n\s*)*if require_local_record\(&project\)\.is_err\(\) \{\s*continue;\s*\}/);
const candidates = searchRoot.slice(searchRoot.indexOf("for project in projects"));
const candidateGuard = candidates.indexOf("require_local_record(&project)");
const projectPath = candidates.indexOf("within_record_root(");
const tasks = candidates.indexOf("load_stamped(");
const vaults = candidates.indexOf(".registered_vaults()");
assert(candidateGuard >= 0 && projectPath > candidateGuard && tasks > candidateGuard && vaults > tasks);
assert.match(candidates, /vaults\.iter\(\)\.any\(\|vault\| selected\.starts_with\(vault\)\)/);
const adapter = readFileSync(path.join(root, "src-tauri/src/search.rs"), "utf8");
assert.equal([...adapter.matchAll(/#\[tauri::command\]/g)].length, 8);
assert.equal([...adapter.matchAll(/host_dispatch::dispatch\s*\(/g)].length, 8);
assert.doesNotMatch(adapter, /\bfn\s+(fallback_search|collect_replace_preview|compile_structured)/);
const aliases = JSON.parse(readFileSync(path.join(root, "docs/native/host-methods.json"), "utf8")).aliases;
assert(!aliases.some(({ derived }) =>
  ["files.replace_text_preview", "files.apply_text_replacements"].includes(derived)),
"legacy project-wide replacement is not a name-only alias of native single-file replacement");
console.log("search-invariants: one core parser; eight dispatched adapters; target/writer/stored-record admission; eligible project/worktree and independent vault roots");
