#!/usr/bin/env node
// Slice31: definition ownership/placement, not a claim of hosted DAP execution.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");
const read = (file) => readFileSync(path.join(root, file), "utf8");
// Focused source ratchet, not a Rust parser or substitute for real IO tests.
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

const core = read("core/crates/aeroric-core/src/ide/dap.rs");
const legacy = read("src-tauri/src/dap.rs");
const paths = read("src-tauri/src/dap/paths.rs");
for (const type of ["DebugConfigType", "DebugRequestType", "DebugBreakpoint", "DebugConfig", "DebugConfigDocument"]) {
  assert.equal(core.match(new RegExp(`pub (?:struct|enum) ${type}\\b`, "g"))?.length, 1);
  assert.doesNotMatch(legacy, new RegExp(`(?:struct|enum) ${type}\\b`));
}
assert.match(core, /#\[serde\(rename_all = "lowercase"\)\]\s*pub enum DebugConfigType/);
assert.match(legacy, /pub use aeroric_core::ide::dap::/);
for (const [shared, old] of [
  ["ensure_legacy_path_inside_root", "ensure_path_inside_root"],
  ["legacy_path_candidate", "candidate_path"],
  ["validate_legacy_attach_endpoint", "validate_attach_endpoint"],
  ["validate_legacy_debug_config", "validate_debug_config"],
]) {
  assert(paths.includes(`${shared} as ${old}`));
  assert.doesNotMatch(paths, new RegExp(`fn ${old}\\b`));
  assert(body(core, shared).length > 0);
}
assert.doesNotMatch(legacy + paths, /fn (?:read_debug_configs|write_debug_configs|read_debug_configs_from_root|write_debug_configs_from_root|normalize_path_lexically|validate_breakpoint)\b/);
assert.doesNotMatch(core, /tauri::|std::env::|home_dir\(|load_settings|Command::|TcpStream::/);
const nativePolicy = body(core, "validate_document");
assert.match(nativePolicy, /document\.version != DEBUG_CONFIG_VERSION/);
assert.match(nativePolicy, /duplicate config id/);
const checked = body(core, "write_debug_configs");
const compatibility = body(core, "write_debug_configs_legacy");
for (const entry of [checked, compatibility]) {
  assert.match(entry, /DEBUG_CONFIG_WRITES\.lock\(\)/);
  assert.match(entry, /write_debug_configs_inner\(project_path, document\)/);
}
assert(checked.indexOf(".lock()") < checked.indexOf("context.require_write()"));
assert(checked.indexOf("context.require_write()") < checked.indexOf("write_debug_configs_inner("));
assert.doesNotMatch(compatibility, /require_write|acquire|fs::/);
const write = body(core, "write_debug_configs_inner");
assert.match(write, /validate_legacy_project_root\(project_path\)/);
assert.match(write, /document\.version = DEBUG_CONFIG_VERSION/);
assert(write.indexOf("validate_legacy_debug_config(") < write.indexOf("fs::create_dir_all("));
assert(write.indexOf("fs::create_dir_all(") < write.indexOf("serde_json::to_string_pretty("));
assert.match(write, /write_legacy_atomic\(&path, &raw\)/);
assert.doesNotMatch(write, /validate_document\(|fs::write|set_permissions|attachments/);
const reader = body(core, "read_debug_configs_from_root");
assert.match(reader, /if !path\.exists\(\)/);
assert.match(reader, /if document\.version == 0/);
assert.match(reader, /validate_legacy_debug_config\(root, config\)/);
assert.doesNotMatch(reader, /validate_document\(|write|create_dir|set_permissions|unwrap_or/);
assert.doesNotMatch(body(core, "validate_legacy_debug_config"), /canonicalize|fs::|resolve_debug/);
// The retained execution boundary must still canonicalize candidates separately.
for (const name of ["resolve_debug_cwd", "resolve_debug_program"]) {
  assert.match(body(paths, name), /\.canonicalize\(\)/);
  assert.match(body(paths, name), /starts_with\(root\)/);
}

const adapter = read("src-tauri/src/dap/commands.rs");
const registration = read("src-tauri/src/lib.rs");
const host = read("core/crates/aeroric-host/src/ide_services.rs");
const map = JSON.parse(read("docs/native/command-map.json")).commands;
const schema = JSON.parse(read("contracts/native/v1/methods.schema.json"));
assert.match(host, /tokio::task::spawn_blocking/);
assert.match(body(host, "param"), /request\s*\.params\s*\.as_object\(\)/);
assert.match(body(host, "param"), /serde_json::from_value/);
assert.match(body(host, "debug_document_reply"), /ErrorCode::InvalidArgument/);
assert.match(body(host, "debug_document_reply"), /ErrorCode::IoError/);
assert.match(host, /use crate::local_targets::local_target_error/);
assert.match(read("core/crates/aeroric-host/src/dispatcher.rs"), /ide_services::register_ide_services\(&mut set, Arc::clone\(&context\)\)/);
for (const name of ["read_debug_configs", "write_debug_configs"]) {
  const command = body(adapter, name);
  assert(command.includes(`"ide.${name}"`));
  assert.match(command, /"projectPath": project_path/);
  assert.match(command, /host_dispatch::dispatch/);
  const deferred = command.indexOf("async {");
  assert(deferred >= 0 && command.indexOf(`dap::${name}`) > deferred);
  assert.match(command, /serde_json::from_value\(value\)/);
  assert.doesNotMatch(command, /unwrap_or|or_else|\.trim\(|validate_.*root|fs::/);
  assert.match(registration, new RegExp(`^\\s*dap::commands::${name},`, "m"));
  assert.doesNotMatch(registration, new RegExp(`^\\s*dap::${name},`, "m"));
  const handler = body(host, name);
  assert(handler.includes('param(&request, "projectPath")'));
  assert(handler.indexOf("local_target_error(") < handler.indexOf(`dap::${name}(`));
  assert.doesNotMatch(handler, /_legacy|fs::|Command::|validate_document/);
  assert.match(host, new RegExp(`set\\.register\\(\\s*"ide\\.${name}",\\s*definition_handler\\(&context, ${name}\\)`));
  const row = map.find((entry) => entry.command === name);
  assert.equal(row?.rpc_method, `ide.${name}`);
  assert.equal(row?.rust_path, `dap::commands::${name}`);
  assert.equal(schema.methods.find((entry) => entry.name === `ide.${name}`)?.takesTarget, true);
  assert(read("scripts/native/smoke-headless.mjs").includes(`searchRpc("ide.${name}"`));
}
assert.match(body(adapter, "write_debug_configs"), /"document": document/);
assert.match(body(adapter, "write_debug_configs"), /write_debug_configs_legacy/);
assert.doesNotMatch(body(adapter, "write_debug_configs"), /Value::Null/);
assert.match(body(host, "write_debug_configs"), /param\(&request, "document"\)/);
console.log("debug-config-invariants: shared definition wire/lexical IO, separate native/live policies, checked target-aware host and deferred actual IPC");
