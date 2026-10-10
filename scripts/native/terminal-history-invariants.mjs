#!/usr/bin/env node
// Slices 24/28: shared history naming/read/delete and actual placement boundaries.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");
const read = (file) => readFileSync(path.join(root, file), "utf8");

// A focused source ratchet, not a Rust parser or replacement for behavior tests.
function body(source, name) {
  const start = source.search(new RegExp(`^\\s*(?:pub(?:\\([^)]*\\))?\\s+)?(?:async\\s+)?fn\\s+${name}\\b`, "m"));
  assert(start >= 0, `missing function ${name}`);
  const open = source.indexOf("{", start);
  let depth = 0;
  for (let i = open; i < source.length; i += 1) {
    if (source[i] === "{") depth += 1;
    if (source[i] === "}" && --depth === 0) return source.slice(open + 1, i);
  }
  assert.fail(`unclosed body ${name}`);
}

const tauriCommands = read("src-tauri/src/storage/commands.rs");
assert.match(tauriCommands, /pub async fn read_task_terminal_history<R: Runtime>/);
assert.match(tauriCommands, /host_dispatch::dispatch\(\s*&app,\s*"project\.read_task_terminal_history"/);
assert.match(tauriCommands, /"taskId":\s*task_id/);
assert.match(tauriCommands, /super::read_task_terminal_history\(task_id\)\.await/);

const tauriLib = read("src-tauri/src/lib.rs");
assert.match(tauriLib, /^\s*storage::commands::read_task_terminal_history,/m);
assert.doesNotMatch(tauriLib, /^\s*storage::read_task_terminal_history,/m);

const hostServices = read("core/crates/aeroric-host/src/services.rs");
assert.match(hostServices, /set\.register\(\s*"project\.read_task_terminal_history"/);
assert.match(hostServices, /project_read_task_terminal_history\(request, context\)/);

const coreHistory = read("core/crates/aeroric-core/src/terminal/history.rs");
assert.match(coreHistory, /pub fn read_terminal_history_tail_from_path\(/);

const paths = read("core/crates/aeroric-core/src/paths.rs");
assert.match(body(paths, "terminal_history_name"), /Self::validate_id\(task_id, "task"\)\?/);
assert.match(body(paths, "terminal_history_name"), /task_id\.trim\(\)/);
assert.match(body(paths, "terminal_history_file"), /Self::terminal_history_name\(task_id\)\?/);
const storage = read("src-tauri/src/storage.rs");
assert.match(body(storage, "safe_task_history_name"), /AppPaths::terminal_history_name\(task_id\)/);
const compatDelete = body(storage, "delete_task_terminal_histories");
assert.match(compatDelete, /delete_task_terminal_histories_legacy\(\s*&task_ids,\s*terminal_history_dir/);
assert.doesNotMatch(compatDelete, /for task_id|remove_file|create_dir/);

const checked = body(coreHistory, "delete_task_terminal_histories");
assert.match(checked, /context\.require_write\(\)\?/);
assert(checked.indexOf("require_write()") < checked.indexOf("delete_histories_with_directory("));
assert.match(body(coreHistory, "delete_task_terminal_histories_legacy"), /delete_histories_with_directory/);
assert.doesNotMatch(body(coreHistory, "delete_task_terminal_histories_legacy"), /require_write|fs::|acquire/);
const sequential = body(coreHistory, "delete_histories_with_directory");
assert.match(sequential, /for task_id in task_ids/);
assert(sequential.indexOf("for task_id") < sequential.indexOf("history_dir()"));
assert(sequential.indexOf("history_dir()") < sequential.indexOf("AppPaths::terminal_history_name"));
assert.match(sequential, /if path\.exists\(\)\s*\{\s*std::fs::remove_file\(path\)\?/);
assert.doesNotMatch(sequential, /create_dir|set_permissions|remove_dir|\.collect\(/);
assert.doesNotMatch(coreHistory, /home_dir\(|std::env::|tauri::/);

const command = body(tauriCommands, "delete_task_terminal_histories");
assert.match(command, /host_dispatch::dispatch\(\s*&app,\s*"project\.delete_task_terminal_histories"/);
assert.match(command, /"taskIds": task_ids/);
assert(command.indexOf("async {") >= 0);
assert(command.indexOf("super::delete_task_terminal_histories(") > command.indexOf("async {"));
assert.match(command, /Ok\(serde_json::Value::Null\)/);
assert.match(command, /serde_json::from_value\(value\)/);
assert.doesNotMatch(command, /unwrap_or|or_else|\.trim\(|fs::/);
assert.match(tauriLib, /^\s*storage::commands::delete_task_terminal_histories,/m);
assert.doesNotMatch(tauriLib, /^\s*storage::delete_task_terminal_histories,/m);

assert.match(hostServices, /set\.register\(\s*"project\.delete_task_terminal_histories"/);
const handler = body(hostServices, "project_delete_task_terminal_histories");
assert.match(handler, /request\.params\.is_object\(\)/);
assert.match(handler, /serde_json::from_value\(request\.params\.clone\(\)\)/);
assert.match(handler, /tokio::task::spawn_blocking/);
assert(handler.indexOf("serde_json::from_value") < handler.indexOf("spawn_blocking"));
assert.match(handler, /history::delete_task_terminal_histories\(&context, &params\.task_ids\)/);
assert.match(handler, /ErrorCode::InvalidArgument/);
assert.match(handler, /ErrorCode::IoError/);
assert.match(handler, /ok\(&request, Value::Null\)/);
assert.doesNotMatch(handler, /_legacy|fs::|require_write|\.trim\(/);
assert.match(body(hostServices, "project_read_task_terminal_history"), /context\.paths\.terminal_history_file\(task_id\)/);

const map = JSON.parse(read("docs/native/command-map.json")).commands;
for (const name of ["read_task_terminal_history", "delete_task_terminal_histories"]) {
  const row = map.find((entry) => entry.command === name);
  assert.equal(row?.rpc_method, `project.${name}`);
  assert.equal(row?.rust_path, `storage::commands::${name}`);
  assert(read("scripts/native/smoke-headless.mjs").includes(`searchRpc("project.${name}"`));
}
console.log("terminal-history-invariants: shared filename/read/sequential-delete, checked host authority and deferred actual commands verified");
