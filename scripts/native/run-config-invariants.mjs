#!/usr/bin/env node
// Slice29: one local definition owner, distinct policies and genuine placement.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");
const read = (file) => readFileSync(path.join(root, file), "utf8");

// Focused source ratchet, not a Rust parser or replacement for real IO tests.
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

const core = read("core/crates/aeroric-core/src/ide/run_config.rs");
const legacy = read("src-tauri/src/run_config.rs");
for (const type of ["RunConfig", "RunConfigDocument", "RunDebugConfigType", "RunDebugBreakpoint", "RunProcessStatus", "RunProcessSnapshot"]) {
  assert.equal(core.match(new RegExp(`pub (?:struct|enum) ${type}\\b`, "g"))?.length, 1);
  assert.doesNotMatch(legacy, new RegExp(`(?:struct|enum) ${type}\\b`));
}
assert.match(core, /#\[serde\(rename_all = "lowercase"\)\]\s*pub enum RunDebugConfigType/);
assert.match(legacy, /pub use aeroric_core::ide::run_config::/);
assert.match(legacy, /validate_legacy_run_config as validate_run_config/);
assert.match(legacy, /legacy_cwd_candidate as cwd_candidate/);
assert.doesNotMatch(legacy, /fn (?:read_run_configs|write_run_configs|validate_run_config|normalize_path_lexically)\b/);
assert.doesNotMatch(core, /tauri::|std::env::|home_dir\(|load_settings|Command::/);

const nativePolicy = body(core, "validate_document");
assert.match(nativePolicy, /document\.version != RUN_CONFIG_VERSION/);
assert.match(nativePolicy, /duplicate config id/);
const checked = body(core, "write_run_configs");
const compatibility = body(core, "write_run_configs_legacy");
for (const entry of [checked, compatibility]) {
  assert.match(entry, /RUN_CONFIG_WRITES\.lock\(\)/);
  assert.match(entry, /write_run_configs_inner\(project_path, document\)/);
}
assert(checked.indexOf(".lock()") < checked.indexOf("context.require_write()"));
assert(checked.indexOf("context.require_write()") < checked.indexOf("write_run_configs_inner("));
assert.doesNotMatch(compatibility, /require_write|acquire|fs::/);
const write = body(core, "write_run_configs_inner");
assert.match(write, /validate_legacy_project_root\(project_path\)/);
assert.match(write, /document\.version = RUN_CONFIG_VERSION/);
assert(write.indexOf("validate_legacy_run_config(") < write.indexOf("fs::create_dir_all("));
assert(write.indexOf("fs::create_dir_all(") < write.indexOf("serde_json::to_string_pretty("));
assert.match(write, /write_legacy_atomic\(&path, &raw\)/);
assert.doesNotMatch(write, /validate_document\(|fs::write|set_permissions|attachments/);
const reader = body(core, "read_run_configs_from_root");
assert.match(reader, /if !path\.exists\(\)/);
assert.match(reader, /if document\.version == 0/);
assert.match(reader, /validate_legacy_run_config\(root, config\)/);
assert.doesNotMatch(reader, /validate_document\(|write|create_dir|set_permissions|unwrap_or/);

const policy = read("core/crates/aeroric-core/src/files/path_policy.rs");
const guard = body(policy, "validate_legacy_project_root");
assert(guard.indexOf(".canonicalize()") < guard.indexOf(".is_dir()"));
assert.match(guard, /Cannot resolve project path:/);
assert.doesNotMatch(guard, /\.trim\(|\.exists\(\)/);
assert.match(body(policy, "validate_project_root"), /Project path does not exist/);
const tauriGuard = read("src-tauri/src/path_guard.rs");
assert.match(tauriGuard, /validate_legacy_project_root as validate_project_root/);
assert.doesNotMatch(tauriGuard, /fn validate_project_root\(/);

const adapter = read("src-tauri/src/run_config/commands.rs");
const registration = read("src-tauri/src/lib.rs");
const host = read("core/crates/aeroric-host/src/ide_services.rs");
const map = JSON.parse(read("docs/native/command-map.json")).commands;
const schema = JSON.parse(read("contracts/native/v1/methods.schema.json"));
assert.match(host, /tokio::task::spawn_blocking/);
assert.match(body(host, "param"), /request\s*\.params\s*\.as_object\(\)/);
assert.match(body(host, "param"), /serde_json::from_value/);
assert.match(body(host, "document_reply"), /ErrorCode::InvalidArgument/);
assert.match(body(host, "document_reply"), /ErrorCode::IoError/);
assert.match(host, /use crate::local_targets::local_target_error/);
assert.doesNotMatch(host, /fn local_target_error\b/);
const targetOwner = read("core/crates/aeroric-host/src/local_targets.rs");
assert.match(targetOwner, /use aeroric_protocol::project::ProjectTarget/);
const target = body(targetOwner, "local_target_error");
assert.match(target, /serde_json::from_value::<ProjectTarget>/);
assert.match(target, /path == project_path/);
assert.match(target, /ErrorCode::UnsupportedCapability/);
assert.match(target, /ErrorCode::InvalidArgument/);
assert.doesNotMatch(target, /fs::|canonicalize|\.trim\(/);
assert.match(read("core/crates/aeroric-host/src/dispatcher.rs"), /ide_services::register_ide_services\(&mut set, Arc::clone\(&context\)\)/);
assert.match(read("core/crates/aeroric-host/src/lib.rs"), /pub mod ide_services;/);
for (const name of ["read_run_configs", "write_run_configs"]) {
  const command = body(adapter, name);
  assert(command.includes(`"ide.${name}"`));
  assert.match(command, /"projectPath": project_path/);
  assert.match(command, /host_dispatch::dispatch/);
  const deferred = command.indexOf("async {");
  assert(deferred >= 0 && command.indexOf(`run_config::${name}`) > deferred);
  assert.match(command, /serde_json::from_value\(value\)/);
  assert.doesNotMatch(command, /unwrap_or|or_else|\.trim\(|validate_.*root|fs::/);
  assert.match(registration, new RegExp(`^\\s*run_config::commands::${name},`, "m"));
  assert.doesNotMatch(registration, new RegExp(`^\\s*run_config::${name},`, "m"));
  const handler = body(host, name);
  assert(handler.includes('param(&request, "projectPath")'));
  assert(handler.indexOf("local_target_error(") < handler.indexOf(`run_config::${name}(`));
  assert.doesNotMatch(handler, /_legacy|fs::|Command::|validate_document/);
  assert.match(host, new RegExp(`set\\.register\\(\\s*"ide\\.${name}",\\s*definition_handler\\(&context, ${name}\\)`));
  const row = map.find((entry) => entry.command === name);
  assert.equal(row?.rpc_method, `ide.${name}`);
  assert.equal(row?.rust_path, `run_config::commands::${name}`);
  assert.equal(schema.methods.find((entry) => entry.name === `ide.${name}`)?.takesTarget, true);
  assert(read("scripts/native/smoke-headless.mjs").includes(`searchRpc("ide.${name}"`));
}
assert.match(body(adapter, "write_run_configs"), /"document": document/);
assert.match(body(adapter, "write_run_configs"), /write_run_configs_legacy/);
assert.doesNotMatch(body(adapter, "write_run_configs"), /Value::Null/);
assert.match(body(host, "write_run_configs"), /param\(&request, "document"\)/);
console.log("run-config-invariants: shared models/legacy IO, separate native policy, checked authority, target refusal and deferred real commands");
