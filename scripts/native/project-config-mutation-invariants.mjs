#!/usr/bin/env node
// Slice27: literal real handlers, checked vs legacy authority, one atomic writer.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");
const read = (file) => readFileSync(path.join(root, file), "utf8");

// A source ratchet, not a Rust parser or substitute for the runtime regressions.
function body(source, name) {
  const start = source.search(new RegExp(`^(?:pub(?:\\([^)]*\\))?\\s+)?(?:async\\s+)?fn\\s+${name}\\b`, "m"));
  assert(start >= 0, `missing function ${name}`);
  const open = source.indexOf("{", start);
  let depth = 0;
  for (let i = open; i < source.length; i += 1) {
    if (source[i] === "{") depth += 1;
    if (source[i] === "}" && --depth === 0) return source.slice(open + 1, i);
  }
  assert.fail(`unclosed body ${name}`);
}

const core = read("core/crates/aeroric-core/src/settings/project_config.rs");
assert.match(core, /static PROJECT_CONFIG_WRITES:\s*parking_lot::Mutex<\(\)>/);
assert.doesNotMatch(core, /home_dir|load_settings|configure_data_dir|\.trim\(/);
for (const name of ["init_project_config", "write_project_config"]) {
  const checked = body(core, name);
  assert.match(checked, /PROJECT_CONFIG_WRITES\.lock\(\)/);
  assert.match(checked, /context\.require_write\(\)\?/);
  assert(checked.indexOf(".lock()") < checked.indexOf("require_write()"));
  assert(checked.indexOf("require_write()") < checked.indexOf(`${name}_inner(`));
  const legacy = body(core, `${name}_legacy`);
  assert.match(legacy, /PROJECT_CONFIG_WRITES\.lock\(\)/);
  assert(legacy.includes(`${name}_inner(`));
  assert.doesNotMatch(legacy, /require_write|acquire|fs::/);
  assert.equal(core.match(new RegExp(`fn ${name}_inner\\(`, "g"))?.length, 1);
}
const initialize = body(core, "init_project_config_inner");
assert(initialize.indexOf("create_dir_all") < initialize.indexOf("config_path.exists()"));
assert.match(initialize, /fs::write\(&config_path, DEFAULT_CONFIG\)/);
assert.match(initialize, /toml::from_str\(&raw\)\.unwrap_or_default\(\)/);
const save = body(core, "write_project_config_inner");
assert(save.indexOf("create_dir_all") < save.indexOf("toml::to_string_pretty"));
assert.match(save, /write_legacy_atomic\(&config_path, &raw\)/);
assert.doesNotMatch(save, /attachments|fs::write|backup|set_permissions/);

const atomic = read("core/crates/aeroric-core/src/storage/atomic.rs");
const mechanism = body(atomic, "write_legacy_atomic");
assert.match(mechanism, /fs::File::create\(&tmp\)/);
assert.match(mechanism, /file\.write_all\(content\.as_bytes\(\)\)/);
assert.match(mechanism, /file\.sync_all\(\)/);
assert.match(mechanism, /finish_legacy_atomic_rename\(&tmp, path\)/);
assert.doesNotMatch(mechanism, /create_dir|set_permissions|write_atomic|FileMode/);
assert.match(body(atomic, "finish_legacy_atomic_rename"), /fs::remove_file\(tmp\)/);
const storage = read("src-tauri/src/storage.rs");
assert.match(body(storage, "atomic_write").trim(),
  /^aeroric_core::storage::atomic::write_legacy_atomic\(path, content\)$/);
assert.match(body(storage, "finish_atomic_rename").trim(),
  /^aeroric_core::storage::atomic::finish_legacy_atomic_rename\(tmp, path\)$/);

const local = read("src-tauri/src/config.rs");
assert.doesNotMatch(local, /pub fn (init_project_config|write_project_config)\(/);
const adapter = read("src-tauri/src/config/commands.rs");
const registration = read("src-tauri/src/lib.rs");
const host = read("core/crates/aeroric-host/src/settings_services.rs");
const map = JSON.parse(read("docs/native/command-map.json")).commands;
for (const name of ["init_project_config", "write_project_config"]) {
  const command = body(adapter, name);
  assert(command.includes(`"settings.${name}"`));
  assert(command.includes('"projectPath": project_path'));
  assert.match(command, /host_dispatch::dispatch/);
  const deferred = command.indexOf("async {");
  assert(deferred >= 0 && command.indexOf(`${name}_legacy(`) > deferred);
  assert.match(command, /serde_json::from_value\(value\)/);
  assert.doesNotMatch(command, /unwrap_or|or_else|fs::|\.trim\(/);
  assert.match(registration, new RegExp(`^\\s*config::commands::${name},`, "m"));
  const handler = body(host, `settings_${name}`);
  assert(handler.includes('preference_param(&request, "projectPath")'));
  assert.match(handler, /ErrorCode::InvalidArgument/);
  assert.match(handler, /ErrorCode::IoError/);
  assert(handler.includes(`project_config::${name}(`));
  assert.doesNotMatch(handler, /_legacy|fs::|load_settings|\.trim\(/);
  assert.match(host, new RegExp(`set\\.register\\(\\s*"settings\\.${name}",\\s*settings_handler\\(&context, settings_${name}\\)`));
  const row = map.find((entry) => entry.command === name);
  assert.equal(row?.rust_path, `config::commands::${name}`);
  assert.equal(row?.rpc_method, `settings.${name}`);
  assert(read("scripts/native/smoke-headless.mjs").includes(`searchRpc("settings.${name}"`));
}
assert.match(body(adapter, "write_project_config"), /"config": config/);
assert.match(body(adapter, "write_project_config"), /Ok\(serde_json::Value::Null\)/);
assert.match(body(host, "settings_write_project_config"), /preference_param\(&request, "config"\)/);
assert.match(body(host, "settings_write_project_config"), /ok\(&request, Value::Null\)/);
console.log("project-config-mutation-invariants: shared init/write, checked host authority, deferred compatibility IPC and legacy atomic mechanism");
