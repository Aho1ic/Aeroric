#!/usr/bin/env node
// Slice 26: one project-config reader/model, real frozen IPC, no local fallback.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");
const read = (file) => readFileSync(path.join(root, file), "utf8");
const core = read("core/crates/aeroric-core/src/settings/project_config.rs");
assert.equal(core.match(/pub struct ProjectConfig\b/g)?.length, 1);
assert.equal(core.match(/pub fn read_project_config\(/g)?.length, 1);
assert.match(core, /Path::new\(project_path\)/);
assert.match(core, /fs::read_to_string\(&config_path\)/);
assert.match(core, /toml::from_str\(&raw\)\.unwrap_or_default\(\)/);
assert.doesNotMatch(core, /home_dir|load_settings|configure_data_dir|\.trim\(/);
// Slice27 adds explicit mutations beside this reader; do not weaken the read
// invariant or accidentally prohibit the separately checked write entrypoints.
const reader = core.slice(core.indexOf("pub fn read_project_config("));
assert.doesNotMatch(reader, /set_permissions|create_dir|fs::write|write_legacy_atomic|require_write/);
assert.match(read("core/crates/aeroric-core/src/settings/mod.rs"), /pub mod project_config;/);

const local = read("src-tauri/src/config.rs");
assert.doesNotMatch(local, /pub struct (ProjectConfig|AgentConfig|GitConfig|EditorConfig|SortPreferenceConfig)\b/);
assert.match(local, /pub use aeroric_core::settings::project_config::/);
assert.match(local,
  /pub fn read_project_config\(project_path: String\) -> Result<ProjectConfig, String> \{\s*aeroric_core::settings::project_config::read_project_config\(&project_path\)\s*\}/);
assert.doesNotMatch(local, /#\[tauri::command\]\s*pub fn read_project_config\(/);
assert.match(local, /pub mod commands;/);

// Scope the read contract, not its neighbouring mutation/agent-config IPCs.
// This is a ratchet for rustfmt's top-level functions, not a Rust parser.
const commands = read("src-tauri/src/config/commands.rs");
const definitions = [...commands.matchAll(
  /^(?:pub(?:\([^)\n]*\))?\s+)?(?:async\s+)?fn\s+read_project_config\b/gm,
)];
assert.equal(definitions.length, 1, "one real read_project_config command");
const tail = commands.slice(definitions[0].index);
const end = tail.search(/^}/m);
assert(end >= 0, "read_project_config must have a closed top-level body");
const adapter = tail.slice(0, end + 1);
assert.match(adapter, /read_project_config<R:\s*Runtime>\(\s*app:\s*AppHandle<R>,\s*project_path:\s*String,/);
assert.match(adapter, /Result<ProjectConfig, String>/);
assert.match(adapter, /host_dispatch::dispatch\(\s*&app,\s*"settings\.read_project_config"/);
assert.match(adapter, /"projectPath":\s*project_path/);
const deferred = adapter.indexOf("async {");
assert(deferred >= 0, "the synchronous reader must stay inside a deferred future");
assert.doesNotMatch(adapter.slice(0, deferred), /super::read_project_config\(/);
assert.match(adapter.slice(deferred), /serde_json::to_value\(super::read_project_config\(project_path\)\?\)/);
assert.match(adapter, /serde_json::from_value\(value\)/);
assert.doesNotMatch(adapter, /unwrap_or|or_else\s*\(|\.trim\(|load_settings|fs::/);
const registration = read("src-tauri/src/lib.rs");
assert.match(registration, /^\s*config::commands::read_project_config,/m);
assert.doesNotMatch(registration, /^\s*config::read_project_config,/m);
assert.match(read("src-tauri/src/pty.rs"), /crate::config::read_project_config\(config_project_path\)/);
assert.match(read("src-tauri/src/git.rs"), /crate::config::read_project_config\(project_path\.clone\(\)\)/);

const host = read("core/crates/aeroric-host/src/settings_services.rs");
assert.match(host, /set\.register\(\s*"settings\.read_project_config",\s*settings_handler\(&context, settings_read_project_config\)/);
const handler = host.slice(
  host.indexOf("fn settings_read_project_config("),
  host.indexOf("fn settings_init_project_config("),
);
assert(handler.length > 0);
assert.match(handler, /preference_param\(&request, "projectPath"\)/);
assert.match(handler, /aeroric_core::settings::project_config::read_project_config\(&project_path\)/);
assert.match(handler, /ErrorCode::InvalidArgument/);
assert.match(handler, /ErrorCode::IoError/);
assert.doesNotMatch(handler, /fs::|load_settings|require_write|\.trim\(/);

const row = JSON.parse(read("docs/native/command-map.json")).commands
  .find((entry) => entry.command === "read_project_config");
assert(row);
assert.equal(row.rust_path, "config::commands::read_project_config");
assert.equal(row.rpc_method, "settings.read_project_config");
assert.match(read("scripts/native/smoke-headless.mjs"), /searchRpc\("settings\.read_project_config"/);
console.log("project-config-invariants: shared read/model, frozen real command/host, deferred local IO and unchanged internal callers");
