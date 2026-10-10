#!/usr/bin/env node
// Slice 21: actual IPC placement, not a claim that every internal reader migrated.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");
const read = (file) => readFileSync(path.join(root, file), "utf8");
const adapter = read("src-tauri/src/storage/commands.rs");
assert.match(adapter, /pub async fn load_projects<R:\s*Runtime>\(app:\s*AppHandle<R>\)/);
assert.match(adapter, /host_dispatch::dispatch\s*\(/);
assert(adapter.includes('"project.load_projects"'), "forward the frozen legacy method (ADR-002)");
assert.doesNotMatch(adapter, /"project\.list"/, "do not rename the frozen wire to an intrinsic endpoint");
assert.match(adapter, /serde_json::json!\(\{\}\)/);
const deferred = adapter.indexOf("async {");
assert(deferred >= 0, "the old reader must stay in a deferred future");
assert.doesNotMatch(adapter.slice(0, deferred), /super::load_projects\(/);
assert.match(adapter.slice(deferred), /super::load_projects\(\)/);
assert.match(adapter, /Result<Vec<Project>, String>/);
assert.match(adapter, /serde_json::to_value\(super::load_projects\(\)\?\)/);
assert.match(adapter, /serde_json::from_value\(value\)/);
assert.doesNotMatch(adapter, /ProjectListReply|"projects":|reply\.projects/,
  "the frozen wire is a bare array, not the intrinsic method's object wrapper");
assert.doesNotMatch(adapter, /serde\(default|unwrap_or|or_else\s*\(|filter_map|\.sort|\.dedup/,
  "missing/wrong host data must not become an empty, partial, sorted or local list");
assert.doesNotMatch(adapter, /\bstruct Project\s*\{/,
  "the adapter must use the existing desktop Project, not invent another DTO");

const storage = read("src-tauri/src/storage.rs");
assert.match(storage, /^pub mod commands;/m);
assert.doesNotMatch(storage, /#\[tauri::command\]\s*pub fn load_projects\(/,
  "the synchronous helper must no longer be the IPC command");
assert.match(storage,
  /pub fn load_projects\(\) -> Result<Vec<Project>, String> \{\s*let _guard = projects_lock\(\)\.lock\(\);\s*load_projects_unlocked\(\)\s*\}/,
  "keep the existing synchronous helper for internal Rust callers");
const helperStart = storage.indexOf("fn load_projects_unlocked()");
const helperEnd = storage.indexOf("\n#[tauri::command]", helperStart);
assert(helperStart >= 0 && helperEnd > helperStart);
const helper = storage.slice(helperStart, helperEnd);
assert.match(helper, /ensure_aeroric_dirs\(\)\?/);
assert.match(helper, /ensure_private_file_permissions\(&path\)\?/);
assert.match(helper, /serde_json::from_str\(&raw\)/);
const registration = read("src-tauri/src/lib.rs");
assert.match(registration, /^\s*storage::commands::load_projects,/m);
assert.doesNotMatch(registration, /^\s*storage::load_projects,/m);

const host = read("core/crates/aeroric-host/src/services.rs");
assert.match(host, /set\.register\(\s*"project\.list"/);
assert.match(host, /set\.register\(\s*"project\.load_projects"/,
  "the frozen legacy method needs a real registered handler, not a map rename");
assert.match(host, /project_list\(request, context, ProjectListReplyShape::Object\)/);
assert.match(host, /project_list\(request, context, ProjectListReplyShape::Array\)/);
const readStart = host.indexOf("fn project_list(");
const readEnd = host.indexOf("\nfn project_open(", readStart);
assert(readStart >= 0 && readEnd > readStart);
const reader = host.slice(readStart, readEnd);
assert.equal(reader.match(/ProjectService::new\(context\)\.list\(\)/g)?.length, 1,
  "both wire shapes must use one core read/error path");
assert.match(reader, /ProjectListReplyShape::Object => json!\(\{ "projects": projects \}\)/);
assert.match(reader, /ProjectListReplyShape::Array => json!\(projects\)/);
assert.match(reader, /ErrorCode::IoError/);
const command = JSON.parse(read("docs/native/command-map.json"))
  .commands.find((entry) => entry.command === "load_projects");
assert(command, "load_projects must remain in the command inventory");
assert.equal(command.rust_path, "storage::commands::load_projects");
assert.equal(command.rpc_method, "project.load_projects");
assert.match(read("src/state/app/useAppStartup.ts"),
  /invoke<Project\[\]>\("load_projects"\)/);
console.log("project-list-invariants: frozen bare-array IPC, unchanged intrinsic object wire, shared core read and deferred synchronous helper");
