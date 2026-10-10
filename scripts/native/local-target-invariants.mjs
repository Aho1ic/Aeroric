#!/usr/bin/env node
// Slices30/31: one pure target decoder and all seven real definition call sites.
import assert from "node:assert/strict";
import { readFileSync, readdirSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");
const hostRoot = "core/crates/aeroric-host/src/";
const read = (file) => readFileSync(path.join(root, file), "utf8");
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

const owner = read(`${hostRoot}local_targets.rs`);
const definitions = readdirSync(path.join(root, hostRoot))
  .filter((name) => name.endsWith(".rs"))
  .flatMap((name) => [...read(`${hostRoot}${name}`).matchAll(/\bfn local_target_error\b/g)].map(() => name));
assert.deepEqual(definitions, ["local_targets.rs"]);
assert.match(read(`${hostRoot}lib.rs`), /pub\(crate\) mod local_targets;/);
assert.match(owner, /use aeroric_protocol::project::ProjectTarget/);
const guard = body(owner, "local_target_error");
assert.match(guard, /request\.params\.get\("target"\)\?/);
assert.match(guard, /serde_json::from_value::<ProjectTarget>\(value\.clone\(\)\)/);
assert.match(guard, /Ok\(ProjectTarget::Local \{ path \}\) if path == project_path => return None/);
assert.match(guard, /ErrorCode::InvalidArgument/);
assert.match(guard, /ErrorCode::UnsupportedCapability,\s*unsupported_message\.to_string\(\)/);
assert.match(guard, /invalid target: \{error\}/);
assert.doesNotMatch(owner, /std::fs|fs::|home_dir\(|std::env::|canonicalize\(|\.trim\(|Command::|require_write|CoreContext/);

const settings = read(`${hostRoot}settings_services.rs`);
const ide = read(`${hostRoot}ide_services.rs`);
for (const source of [settings, ide]) {
  assert.match(source, /use crate::local_targets::local_target_error;/);
  assert.doesNotMatch(source, /fn local_target_error\b/);
}
const schema = JSON.parse(read("contracts/native/v1/methods.schema.json"));
const methods = [
  [settings, "settings_read_project_config", "settings.read_project_config", "project_config::read_project_config(", "preference_param"],
  [settings, "settings_init_project_config", "settings.init_project_config", "project_config::init_project_config(", "preference_param"],
  [settings, "settings_write_project_config", "settings.write_project_config", "project_config::write_project_config(", "preference_param"],
  [ide, "read_run_configs", "ide.read_run_configs", "run_config::read_run_configs(", "param"],
  [ide, "write_run_configs", "ide.write_run_configs", "run_config::write_run_configs(", "param"],
  [ide, "read_debug_configs", "ide.read_debug_configs", "dap::read_debug_configs(", "param"],
  [ide, "write_debug_configs", "ide.write_debug_configs", "dap::write_debug_configs(", "param"],
];
for (const [source, name, method, coreCall, decoder] of methods) {
  const handler = body(source, name);
  const check = handler.indexOf("local_target_error(");
  assert(check >= 0, `${method} must check target`);
  assert(handler.indexOf(`${decoder}(&request, "projectPath")`) >= 0);
  assert(handler.indexOf(`${decoder}(&request, "projectPath")`) < check);
  assert(handler.indexOf(coreCall) > check, `${method} must reject before core IO`);
  assert.match(handler, /if let Some\(error\)\s*=[\s\S]*?local_target_error\([\s\S]*?\)\s*\{\s*return error;\s*\}/);
  const typedPayload = name.includes("write_project_config")
    ? 'preference_param(&request, "config")'
    : ["write_run_configs", "write_debug_configs"].includes(name) ? 'param(&request, "document")' : null;
  if (typedPayload) {
    const decoded = handler.indexOf(typedPayload);
    assert(decoded >= 0 && decoded < check, `${method} must decode its payload before target checking`);
  }
  assert.doesNotMatch(handler, /fs::|canonicalize\(|\.trim\(|_legacy|Command::/);
  assert(source.includes(`"${method}"`), `${method} registration missing`);
  assert.equal(schema.methods.find((entry) => entry.name === method)?.takesTarget, true);
}
assert.match(ide, /"Run-config target is not hosted: SSH\/WSL definition IO is unavailable"/);
console.log("local-target-invariants: single pure protocol decoder; all seven typed handlers refuse unsupported/conflicting targets before core IO");
