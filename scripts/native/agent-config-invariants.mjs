#!/usr/bin/env node
// Slices39–43 admission, selection, IO, roots and actual deferred config IPC.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");
const source = readFileSync(path.join(root, "core/crates/aeroric-host/src/settings_services.rs"), "utf8");
function functionBody(text, name) {
  const matches = [...text.matchAll(new RegExp(`\\bfn ${name}(?:<[^>\\n]+>)?\\(`, "g"))];
  assert.equal(matches.length, 1, `one real ${name} definition`);
  const start = matches[0].index;
  const next = text.slice(start + 1).search(/\n(?:pub(?:\([^)\n]*\))? )?(?:async )?fn /);
  return text.slice(start, next < 0 ? undefined : start + 1 + next);
}
const body = (name) => functionBody(source, name);

const loader = body("load_typed_settings");
assert.match(loader, /storage::settings::load_settings\(context\)/);
assert.match(loader, /Err\(error\) => return Err\(error\.to_string\(\)\)/);
assert.match(loader, /serde_json::from_value\(Value::Object\(fields\)\)\s*\.map_err\(\|error\| format!\("invalid stored app settings: \{error\}"\)\)/,
  "typed corruption must stay an error, not defaults or a partial projection");
const decode = loader.slice(loader.indexOf("serde_json::from_value"));
assert.doesNotMatch(decode, /\b(?:unwrap|unwrap_or|unwrap_or_default|unwrap_or_else|ok)\s*\(/,
  "do not swallow typed-settings failure");

for (const [method, handler] of [
  ["get_agent_config_file_path", "settings_agent_config_file_path"],
  ["read_agent_config_file", "settings_read_agent_config_file"],
  ["write_agent_config_file", "settings_write_agent_config_file"],
]) {
  assert.match(source, new RegExp(`set\\.register\\(\\s*"settings\\.${method}",\\s*agent_config_handler\\(&context, &config_paths, ${handler}\\)`),
    `the actual ${method} registration must use the checked handler`);
  const code = body(handler);
  assert.match(code, /let settings = match load_typed_settings\(&context\)/);
  assert.match(code, /Err\(error\) => return failure\(request\.id\.clone\(\), ErrorCode::IoError, &error\)/,
    `${method} must propagate stored-settings failures`);
  assert.doesNotMatch(code, /\b(?:load_settings_internal|save_settings|save_app_settings|update_settings)\s*\(/,
    `${method} must not repair or rewrite stored settings`);
  if (method !== "write_agent_config_file") {
    assert.doesNotMatch(code, /require_write|write_refusal/, `${method} remains read-only-capable`);
  }
}
const write = body("settings_write_agent_config_file");
assert.match(write, /if let Err\(error\) = context\.require_write\(\) \{\s*return failure\(request\.id\.clone\(\), ErrorCode::IoError, &error\.to_string\(\)\);\s*\}/,
  "every write needs a live context lease with a propagated refusal");
const points = [
  write.indexOf('get("agent")'),
  write.indexOf('get("content")'),
  write.indexOf("context.require_write()"),
  write.indexOf("load_typed_settings(&context)"),
  write.indexOf("agent_config_path_from_settings(&agent, &settings, paths)"),
  write.indexOf("aeroric_core::settings::agent_config_files::write(&context, &path, &content)"),
];
assert(points.every((value, index) => value >= 0 && (index === 0 || value > points[index - 1])),
  "typed parameters -> current writer -> settings -> path -> existing IO");
const shared = readFileSync(path.join(root, "core/crates/aeroric-core/src/settings/agent_config.rs"), "utf8");
const desktop = readFileSync(path.join(root, "src-tauri/src/config.rs"), "utf8");
const ipc = readFileSync(path.join(root, "src-tauri/src/config/commands.rs"), "utf8");
assert.equal([...shared.matchAll(/\bpub fn resolve_path\(/g)].length, 1);
assert.doesNotMatch(shared, /\b(?:std::fs|std::env|std::process|load_settings_internal|home_dir|aeroric_dir)\b/,
  "the shared selector must not acquire environment or IO ownership");
assert.match(shared, /AgentFamily::parse\(profile\.family\.trim\(\)\)/);
assert.match(shared, /AgentFamily::from_codex_like\(profile\.codex_like\)/);
assert.match(shared, /\.find\(\|profile\| profile\.id == agent\)/, "keep exact first-match lookup");
assert.match(functionBody(shared, "dsh_config_path"), /home\.join\("settings.yaml"\)/);
assert.match(functionBody(shared, "resolve_path"), /\.map\(\|home\| dsh_config_path\(&home\)\)/);
const hostedSelector = functionBody(source, "agent_config_path_from_settings");
assert.match(hostedSelector, /agent_config_roots::resolve_path\(/);
assert.match(hostedSelector, /AgentConfigPaths::from\(settings\),\s*paths,/);
assert.doesNotMatch(hostedSelector, /default_builtin_agent_config_path|custom_agent_home|std::env/);
const legacySelector = functionBody(desktop, "agent_config_path_from_settings");
assert.match(legacySelector, /aeroric_core::settings::agent_config::resolve_path\(/);
assert(legacySelector.includes("app_settings::default_builtin_agent_config_path,") &&
  legacySelector.includes("app_settings::custom_agent_home,"),
"Embedded and reasoning selection must retain their lazy legacy callbacks");
for (const text of [source, desktop]) {
  const adapter = functionBody(text, "agent_config_path_from_settings");
  assert.doesNotMatch(adapter, /match agent|\.find\(|\.trim\(|\.agent_family\(/,
    "adapters must not retain a second selector");
  assert.doesNotMatch(text, /\bfn configured_path\(/, "configured path policy lives only in core");
}
const projection = functionBody(desktop, "agent_config_paths");
for (const field of ["claude_config_path", "claude_gpt55_config_path", "codex_config_path", "dsh_config_path"]) {
  assert(projection.includes(`${field}: &settings.${field}`), `${field} projection is exact and borrowed`);
}
for (const field of ["id", "path", "family"]) {
  assert(projection.includes(`${field}: &profile.${field}`), `${field} must not be normalized by the adapter`);
}
assert(projection.includes("codex_like: profile.codex_like"));
assert.doesNotMatch(projection, /serde_json|api_key|password|username|base_url|\.agent_family\(/,
  "project only path-selection fields, not a full credential-bearing settings DTO");
const io = readFileSync(path.join(root, "core/crates/aeroric-core/src/settings/agent_config_files.rs"), "utf8");
const checked = functionBody(io, "write");
assert.match(checked, /context\s*\.require_write\(\)\s*\.map_err\(\|error\| error\.to_string\(\)\)\?/);
assert(checked.indexOf("require_write()") < checked.indexOf("write_legacy(path, content)"),
  "core must recheck the writer before any IO");
const legacy = functionBody(io, "write_legacy");
assert.match(io, /use crate::storage::atomic::write_legacy_atomic;/);
assert.match(legacy, /fs::create_dir_all\(parent\)/);
assert.match(legacy, /write_legacy_atomic\(path, content\)/);
assert.doesNotMatch(io, /\bfs::write\b|\bfs::rename\b|File::create|\.sync_all\(/,
  "reuse the existing legacy atomic owner, not a new direct/atomic implementation");
assert.doesNotMatch(io, /\b(?:home_dir|aeroric_dir|load_settings_internal)\b|std::env/,
  "explicit-path IO must not acquire global settings or home ownership");
assert.match(functionBody(io, "read"), /if !path\.exists\(\) \{\s*return Ok\(None\);/);
assert.match(functionBody(io, "read"), /fs::read_to_string\(path\)\s*\.map\(Some\)/);
for (const [text, readName, writeName, writeCall] of [
  [source, "settings_read_agent_config_file", "settings_write_agent_config_file",
    "write(&context, &path, &content)"],
  [ipc, "read_agent_config_file", "write_agent_config_file", "write_legacy(&path, &content)"],
]) {
  const reader = functionBody(text, readName);
  const writer = functionBody(text, writeName);
  assert(reader.includes("aeroric_core::settings::agent_config_files::read(&path)"));
  assert(writer.includes(`aeroric_core::settings::agent_config_files::${writeCall}`));
  assert.doesNotMatch(reader + writer, /\bfs::|atomic_write\(/,
    "these commands must delegate IO, not retain a second implementation");
}
const roots = readFileSync(path.join(root, "core/crates/aeroric-core/src/settings/agent_config_roots.rs"), "utf8");
assert.doesNotMatch(roots, /std::(?:env|fs|process)|(?:load_settings_internal|aeroric_dir|home_dir)\(/,
  "shared root shaping must not acquire environment or IO");
assert.match(functionBody(roots, "resolve_path"), /super::agent_config::resolve_path\(/);
assert.match(roots, /paths\.user_home\(\)/);
assert.match(roots, /custom_home\("dsh", \|\| Ok\(paths\.agent_homes_dir\(\)\)\)/,
  "preserve actual local_dsh normalization rather than renaming the directory");
assert.match(roots, /paths\.agent_homes_dir\(\)\.join\("omp"\)/);
assert.match(roots, /\|id\| custom_home\(id, \|\| Ok\(paths\.agent_homes_dir\(\)\)\)/);
assert.doesNotMatch(roots, /user_home\(\)\.join\(/, "managed data must not be derived from the external home");
const customHome = functionBody(roots, "custom_home");
assert.match(customHome, /let normalized = sanitize_custom_agent_id\(id\);/);
assert(customHome.indexOf("if normalized.is_empty()") < customHome.indexOf("agent_homes_root()?"));
assert.match(customHome, /home\.parent\(\) != Some\(root\.as_path\(\)\)/);
for (const base of ["core/crates/aeroric-core/src/settings", "src-tauri/src/app_settings"]) {
  const builtin = functionBody(readFileSync(path.join(root, base, "builtin.rs"), "utf8"), "default_builtin_agent_config_path");
  assert.match(builtin, /agent_config_roots::builtin_path\(/);
  assert(builtin.indexOf("home_dir()") < builtin.indexOf("agent_config_roots::builtin_path("),
    "legacy eager home error precedes shared filename selection");
  assert.doesNotMatch(builtin, /match agent|\.join\(/, "builtin filename policy has one owner");
  const custom = functionBody(readFileSync(path.join(root, base, "normalize.rs"), "utf8"), "custom_agent_home");
  assert.match(custom, /agent_config_roots::custom_home\(id, \|\|\s*(?:\{\s*)?Ok\(aeroric_dir\(\)\?\.join\("agent-homes"\)\)/,
    "legacy root acquisition stays lazy, after shared ID validation");
  assert.doesNotMatch(custom, /sanitize_custom_agent_id|home\.parent\(\)/);
}
for (const file of ["core/crates/aeroric-core/src/settings/dsh_home.rs", "src-tauri/src/dsh_home.rs"]) {
  const dsh = functionBody(readFileSync(path.join(root, file), "utf8"), "dsh_settings_path");
  assert.match(dsh, /agent_config::dsh_config_path\(\s*&dsh_home\(\)\?\s*,?\s*\)/);
  assert.doesNotMatch(dsh, /\.join\(/, "retain the old helper through shared DSH filename policy");
}
const registration = functionBody(source, "register_settings_services");
assert.match(registration, /AppPaths::with_data_dir\(\s*agent_config_external_home\s*\.as_deref\(\)\s*\.unwrap_or\(context\.paths\.user_home\(\)\),\s*Some\(context\.paths\.data_dir\(\)\),/s,
  "only the trusted external home may vary; data root always belongs to context");
const capture = functionBody(source, "agent_config_handler");
assert.match(capture, /let paths = Arc::clone\(paths\);/);
assert.match(capture, /settings_handler\(context, move \|request, context\| \{\s*run\(request, context, &paths\)\s*\}\)/,
  "capture immutable paths while retaining the original checked context and worker");
assert.doesNotMatch(capture, /CoreContext::|std::env|load_settings/);
const dispatcher = readFileSync(path.join(root, "core/crates/aeroric-host/src/dispatcher.rs"), "utf8");
assert.match(dispatcher, /register_settings_services\(\s*&mut set,\s*Arc::clone\(&context\),\s*wiring\.agent_config_external_home\.take\(\),/);
const cli = readFileSync(path.join(root, "core/crates/aeroric-cli/src/main.rs"), "utf8");
const home = functionBody(cli, "agent_config_external_home");
assert.match(home, /std::env::var_os\("HOME"\)/);
assert.match(home, /std::env::var_os\("USERPROFILE"\)/);
assert.match(home, /\.filter\(\|path\| path\.is_absolute\(\)\)/);
assert.doesNotMatch(home, /std::fs|std::process|unwrap_or|canonicalize|trim\(/);
const serve = functionBody(cli, "serve");
// 切片95：run_startup 改名 run_startup_with_secrets（OS 密钥适配仅由生产 CLI 显式注入），
// 顺序语义不变——可信 HOME 捕获仍必须先于 startup。
assert(serve.indexOf("match agent_config_external_home()") < serve.indexOf("match run_startup_with_secrets("));
assert.match(serve, /Err\(error\) => \{\s*eprintln!\("aeroric-cli serve: \{error\}"\);\s*return exit::CONNECT;/);
assert.match(serve, /agent_config_external_home: Some\(agent_config_external_home\)/);
assert.match(serve, /AppPaths::with_data_dir\(Path::new\("\/"\), Some\(data_dir\)\)/,
  "do not widen unrelated metrics/session roots as a config-only side effect");
const lib = readFileSync(path.join(root, "src-tauri/src/lib.rs"), "utf8");
const commandTests = readFileSync(path.join(root, "src-tauri/src/config/agent_config_command_tests.rs"), "utf8");
for (const [name, result] of [
  ["get_agent_config_file_path", "String"],
  ["read_agent_config_file", "Option<String>"],
  ["write_agent_config_file", "()"],
]) {
  assert(lib.includes(`config::commands::${name},`), `${name} actual async IPC must be registered`);
  assert(!lib.includes(`config::${name},`), "no duplicate legacy command registration");
  assert(!new RegExp(`\\bfn ${name}(?:<|\\()`).test(desktop), "parent must not retain another command body");
  const code = functionBody(ipc, name);
  assert(code.includes(`-> Result<${result}, String>`), `${name} keeps its typed reply`);
  assert(code.includes(`"settings.${name}"`), `${name} keeps its frozen method`);
  const stages = [
    "crate::host_dispatch::dispatch(",
    `serde_json::json!({"agent": &agent${name === "write_agent_config_file" ? ', "content": &content' : ""}})`,
    "async move {", "tokio::task::spawn_blocking(move ||",
    "crate::app_settings::load_settings_internal()",
    "super::agent_config_path_from_settings(&agent, &settings)",
  ].map((stage) => code.indexOf(stage));
  assert(stages.every((value, i) => value >= 0 && (i === 0 || value > stages[i - 1])),
    `${name}: exact params -> deferred future -> worker -> unchanged loader/selector`);
  assert.match(code, /serde_json::from_value\(value\)\.map_err\(\|error\| error\.to_string\(\)\)/,
    "invalid host types must be errors, not local/default successes");
  assert.doesNotMatch(code.slice(0, stages[2]), /load_settings|agent_config_path_from_settings|spawn_blocking|std::fs|std::env/);
  assert.equal([...code.matchAll(/load_settings_internal\(\)/g)].length, 1, "one deferred loader");
  assert(commandTests.includes(`super::commands::${name}(`), "test must call the actual registered command");
}
assert(commandTests.includes(".env_clear()") && commandTests.includes('contains("1 passed; 0 failed; 0 ignored")'),
  "real legacy-loader tests must isolate environment and reject zero-case children");
console.log("agent-config-invariants: checked admission; shared selection/IO/roots; scoped CLI home and three strictly deferred typed IPCs");
