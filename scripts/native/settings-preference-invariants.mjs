#!/usr/bin/env node
// Slices 15–18 ownership/wiring ratchet; behavioral evidence is in core/UDS tests.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");
const read = (file) => readFileSync(path.join(root, file), "utf8");
const adapter = read("src-tauri/src/app_settings/commands.rs");
for (const [command, ...params] of [
  ["save_send_shortcut", "sendShortcut"],
  ["save_shift_enter_newline", "enabled"],
  ["update_auto_cleanup_settings", "autoCleanupSettings"],
  ["update_weekly_report_settings", "weeklyReportSettings"],
  ["update_notebook_embedding_settings", "notebookEmbeddingSettings"],
  ["update_dsh_reasoning_effort", "agent", "effort"],
  ["update_proxy_settings", "proxySettings"],
]) {
  const start = adapter.indexOf(`pub async fn ${command}(`);
  assert(start >= 0, `missing command ${command}`);
  const next = adapter.indexOf("\n#[tauri::command]", start);
  const body = adapter.slice(start, next < 0 ? adapter.length : next);
  assert.match(body, /host_dispatch::dispatch\s*\(/, `${command} bypasses placement`);
  assert(body.includes(`"settings.${command}"`), `${command} forwards the wrong method`);
  for (const param of params) {
    assert(body.includes(`"${param}"`), `${command} changed its canonical wire parameter ${param}`);
  }
  assert.match(body, /async move\s*\{/, `${command} must defer local work`);
}

const coreSchema = read("core/crates/aeroric-core/src/settings/schema.rs");
const desktopSchema = read("src-tauri/src/app_settings/schema.rs");
for (const name of [
  "normalize_send_shortcut", "normalize_auto_cleanup_settings", "normalize_weekly_report_settings",
]) {
  const declaration = new RegExp(`\\bfn\\s+${name}\\s*\\(`, "g");
  assert.equal([...coreSchema.matchAll(declaration)].length, 1, `${name} must be owned by core`);
  assert.equal([...desktopSchema.matchAll(declaration)].length, 0, `${name} was copied back to Tauri`);
}
for (const name of [
  "AutoCleanupSettings", "WeeklyReportSettings", "NotebookEmbeddingSettings",
  "ProxySettings", "LegacyAgentProxyConfig",
]) {
  assert.match(coreSchema, new RegExp(`\\bstruct\\s+${name}\\b`));
  assert.doesNotMatch(desktopSchema, new RegExp(`\\bstruct\\s+${name}\\b`));
}
const coreNormalize = read("core/crates/aeroric-core/src/settings/normalize.rs");
const desktopNormalize = read("src-tauri/src/app_settings/normalize.rs");
for (const name of [
  "normalize_notebook_embedding_settings", "normalize_proxy_url", "normalize_no_proxy",
  "normalize_proxy_settings", "normalize_agent_proxy_enabled",
]) {
  const declaration = new RegExp(`\\bfn\\s+${name}\\s*\\(`, "g");
  assert.equal([...coreNormalize.matchAll(declaration)].length, 1, `${name} must be owned by core`);
  assert.equal([...desktopNormalize.matchAll(declaration)].length, 0, `${name} was copied back to Tauri`);
}
const coreAgentEnv = read("core/crates/aeroric-core/src/settings/agent_env.rs");
const desktopAgentEnv = read("src-tauri/src/app_settings/agent_env.rs");
for (const name of ["migrate_legacy_proxy_fields", "migrate_agent_proxy_enabled_fields"]) {
  assert.match(coreAgentEnv, new RegExp(`\\bpub\\s+fn\\s+${name}\\b`));
  assert.doesNotMatch(desktopAgentEnv, new RegExp(`\\bfn\\s+${name}\\b`));
  assert(desktopAgentEnv.includes(`aeroric_core::settings::agent_env::${name}(`),
    `legacy Tauri proxy migration must use ${name}`);
}
assert.doesNotMatch(read("core/crates/aeroric-host/src/settings_services.rs"),
  /\bfn\s+settings_update\s*\(/, "proxy must not keep a parallel generic host field writer");
for (const [file, shared] of [
  ["core/crates/aeroric-core/src/settings/load_save.rs", "crate::storage::settings::settings_lock()"],
  ["src-tauri/src/app_settings/load_save.rs", "aeroric_core::storage::settings::settings_lock()"],
]) {
  const source = read(file);
  assert(source.includes(shared), `${file} must share the map-writer mutex`);
  assert.doesNotMatch(source, /\bstatic\s+SETTINGS_LOCK\b/, `${file} reinstated a separate lock`);
}
function literalStringArray(source, name) {
  const matches = [...source.matchAll(new RegExp(`\\b${name}\\b[^=\\n]*=\\s*\\[([^\\]]*)\\]`, "g"))];
  assert.equal(matches.length, 1, `${name} must have one literal declaration`);
  return [...matches[0][1].matchAll(/"([^"]+)"/g)].map((match) => match[1]);
}
const corePreferences = read("core/crates/aeroric-core/src/settings/preferences.rs");
assert.match(corePreferences, /migrate_agent_proxy_enabled_fields\s*\(/,
  "proxy preference must share the legacy enabled-flag migration");
assert.doesNotMatch(corePreferences.split("#[cfg(test)]")[0],
  /\b(?:load_settings_internal|update_settings_locked|migrate_legacy_proxy_fields)\s*\(/,
  "preference replacement must not run a loader or revive old proxy credentials");
const dshEfforts = literalStringArray(corePreferences, "DSH_REASONING_EFFORTS");
assert.deepEqual(dshEfforts, ["off", "high", "max"]);
assert.deepEqual(literalStringArray(read("src/modelOptions.ts"), "DSH_REASONING_EFFORTS"), dshEfforts);
assert.deepEqual(literalStringArray(
  read("macos/AeroricKit/Sources/AeroricUI/Settings/SettingsCapability.swift"), "dshEffortLevels",
), dshEfforts);
for (const file of [
  "core/crates/aeroric-core/src/settings/builtin.rs",
  "src-tauri/src/app_settings/builtin.rs",
]) {
  assert.match(read(file), /DshReasoningEffort::for_builtin_agent\(agent, effort\)/,
    `${file} must share the preference value validator`);
}
const settingsViews = read("macos/AeroricKit/Sources/AeroricUI/Settings/SettingsViews.swift");
const modelViewStart = settingsViews.indexOf("public struct ModelSettingsView:");
assert(modelViewStart >= 0);
const modelViewEnd = settingsViews.indexOf("\npublic struct ", modelViewStart + 1);
const modelView = settingsViews.slice(modelViewStart, modelViewEnd < 0 ? settingsViews.length : modelViewEnd);
assert.match(modelView, /SettingsCapability\.effortLevels\(for:\s*family\)/);
assert.match(modelView, /guard\s+SettingsCapability\.supportsEffort\(/,
  "a stale selection must be refused before calling the native callback");
console.log("settings-preference-invariants: seven deferred dispatches; shared schema/mutex/proxy helpers/DSH validator and three-language effort vocabulary");
