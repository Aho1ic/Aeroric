#!/usr/bin/env node
// Slice 22: exact task-file query; task.list is not its semantic equivalent.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");
const read = (file) => readFileSync(path.join(root, file), "utf8");
const adapter = read("src-tauri/src/storage/commands.rs");
const commandStart = adapter.indexOf("pub async fn load_project_tasks<");
assert(commandStart >= 0, "the real IPC command must be separate from the synchronous helper");
const commandEnd = adapter.indexOf("\n#[tauri::command]", commandStart);
const command = adapter.slice(commandStart, commandEnd < 0 ? undefined : commandEnd);
assert.match(command,
  /load_project_tasks<R:\s*Runtime>\(\s*app:\s*AppHandle<R>,\s*project_id:\s*String,\s*\)/);
assert.match(command, /Result<Vec<Task>, String>/);
assert.match(command, /host_dispatch::dispatch\s*\(/);
assert.match(command, /"project\.load_project_tasks"/);
assert.match(command, /"projectId":\s*project_id/);
const deferred = command.indexOf("async {");
assert(deferred >= 0, "retain local directory/permission effects only in a deferred future");
assert.doesNotMatch(command.slice(0, deferred), /super::load_project_tasks\(/);
assert.match(command.slice(deferred), /serde_json::to_value\(super::load_project_tasks\(project_id\)\?\)/);
assert.match(command, /serde_json::from_value\(value\)/);
assert.doesNotMatch(command, /"task\.list"|\.trim\(|unwrap_or|or_else\s*\(|filter_map|\.sort|\.dedup/,
  "no aggregate alias, path rewrite, partial data, empty default or local fallback");

const storage = read("src-tauri/src/storage.rs");
assert.doesNotMatch(storage, /#\[tauri::command\]\s*pub fn load_project_tasks\(/);
assert.match(storage,
  /pub fn load_project_tasks\(project_id: String\) -> Result<Vec<Task>, String> \{\s*ensure_project_dir\(&project_id\)\?;\s*let path = tasks_path\(&project_id\)\?;\s*if !path\.exists\(\) \{\s*return Ok\(vec!\[\]\);\s*\}\s*ensure_private_file_permissions\(&path\)\?;\s*let raw = fs::read_to_string\(&path\)\.map_err\(\|e\| e\.to_string\(\)\)\?;\s*serde_json::from_str\(&raw\)\.map_err\(\|e\| e\.to_string\(\)\)\s*\}/,
  "preserve the synchronous helper and its exact existing behavior");
const registration = read("src-tauri/src/lib.rs");
assert.match(registration, /^\s*storage::commands::load_project_tasks,/m);
assert.doesNotMatch(registration, /^\s*storage::load_project_tasks,/m);

const host = read("core/crates/aeroric-host/src/services.rs");
assert.match(host, /set\.register\(\s*"project\.load_project_tasks"/);
assert.match(host, /project_load_tasks\(request, context\)/);
const handlerStart = host.indexOf("fn project_load_tasks(");
const handlerEnd = host.indexOf("\nfn task_list(", handlerStart);
assert(handlerStart >= 0 && handlerEnd > handlerStart);
const handler = host.slice(handlerStart, handlerEnd);
assert.match(handler, /\.get\("projectId"\)\s*\.and_then\(Value::as_str\)/);
assert.equal(handler.match(/load_tasks\(&context, project_id\)/g)?.length, 1,
  "one call to the existing core raw reader");
assert.match(handler, /ok\(&request, json!\(tasks\)\)/);
assert.match(handler, /StorageError::InvalidId\(_\)/);
assert.match(handler, /ErrorCode::InvalidArgument/);
assert.match(handler, /ErrorCode::IoError/);
assert.doesNotMatch(handler,
  /ProjectService|load_stamped|require_writer|ensure_private|fs::|\.trim\(|"tasks":|\.insert\(/,
  "no registry, fingerprint, writer requirement, second IO algorithm or record rewrite");

const core = read("core/crates/aeroric-core/src/storage/tasks.rs");
const readStart = core.indexOf("pub fn load_tasks(");
const readEnd = core.indexOf("\npub fn save_tasks(", readStart);
assert(readStart >= 0 && readEnd > readStart);
assert.match(core.slice(readStart, readEnd),
  /compat::read_array\(&context\.paths\.tasks_file\(project_id\)\?\)\?/);

const row = JSON.parse(read("docs/native/command-map.json"))
  .commands.find((entry) => entry.command === "load_project_tasks");
assert(row);
assert.equal(row.rust_path, "storage::commands::load_project_tasks");
assert.equal(row.rpc_method, "project.load_project_tasks");
assert.match(read("src/state/app/useAppStartup.ts"),
  /invoke<Task\[\]>\("load_project_tasks",\s*\{\s*projectId:/);
console.log("project-tasks-invariants: frozen exact-ID IPC, existing raw core read, no aggregation and deferred unchanged local helper");
