#!/usr/bin/env node
// P8-05 slice 20: supplement actual DTO/UDS tests with a single-owner ratchet.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");
const read = (file) => readFileSync(path.join(root, file), "utf8");
function section(source, start, end) {
  const from = source.indexOf(start);
  assert(from >= 0, `missing section start: ${start}`);
  assert.equal(source.indexOf(start, from + start.length), -1, `duplicate section: ${start}`);
  const to = source.indexOf(end, from + start.length);
  assert(to > from, `missing section end: ${end}`);
  return source.slice(from, to);
}

const core = read("core/crates/aeroric-core/src/maintenance/updates.rs");
const helper = section(core, "pub fn pending_update_result(", "\n/// `v1.2.3`");
assert.match(helper, /pending:\s*Option<PendingReleaseUpdate>/);
assert.match(helper, /tag_name:\s*Option<&str>/);
assert.match(helper, /->\s*Option<ReleaseUpdatePrepareResult>/);
assert.match(helper, /pending\.tag_name != tag/);
assert.match(helper, /ready_to_restart:\s*true/);
assert.doesNotMatch(helper, /\.trim|\.to_lowercase|\.to_uppercase|std::fs|\.await|\.save\(|\.load\(/,
  "query mapping must remain pure and use exact tags");
const persisted = section(core, "pub struct PendingReleaseUpdate {", "\n/// 旧盘上");
assert.match(persisted, /pub checksum_path:\s*String/);
assert.doesNotMatch(persisted, /ready_to_restart|readyToRestart/,
  "do not repair the public wire by changing the private persisted DTO");

const adapter = read("src-tauri/src/notification.rs");
const command = section(adapter, "pub async fn get_pending_release_update(", "\n#[tauri::command]");
assert.match(command, /host_dispatch::dispatch\s*\(/);
assert(command.includes('"maintenance.get_pending_release_update"'));
assert.match(command, /"tagName":\s*tag_name/);
const deferred = command.indexOf("async {");
assert(deferred >= 0, "local IO must remain deferred until placement chooses it");
assert.doesNotMatch(command.slice(0, deferred), /service\(\)|pending_update\(/);
assert.match(command.slice(deferred), /pending_update_result\(service\(\)\.pending_update\(\), tag_name\.as_deref\(\)\)/);
assert.match(command, /serde_json::from_value\(value\)/);
assert.doesNotMatch(command, /unwrap_or_default|unwrap_or_else|or_else\s*\(/,
  "External failures must not become a local or absent reply");

const host = read("core/crates/aeroric-host/src/maintenance_services.rs");
const handler = section(host, '"maintenance.get_pending_release_update",',
  '"maintenance.prepare_release_update",');
assert.match(handler, /pending_update_result\(service\.pending_update\(\), tag\)/);
for (const source of [command, handler]) {
  assert.doesNotMatch(source, /ReleaseUpdatePrepareResult\s*\{|pending\.tag_name\s*!=/,
    "adapters must not own a second mapping/filter");
}
for (const source of [adapter, host]) {
  assert.match(source, /use aeroric_core::maintenance::updates::(?:pending_update_result|{pending_update_result,)/);
  assert.doesNotMatch(source, /\bfn\s+pending_update_result\s*\(/);
}
assert.match(read("core/crates/aeroric-core/src/maintenance/service.rs"),
  /pub fn pending_update\(&self\) -> Option<PendingReleaseUpdate> \{\s*PendingReleaseUpdate::load\(&self\.data_dir\)\.ok\(\)\.flatten\(\)\s*\}/,
  "preserve the old read-error-to-None IO contract");
assert.match(read("core/crates/aeroric-host/src/dispatcher.rs"),
  /maintenance_services::register_if_hosted\(\s*&mut set,\s*wiring\.maintenance\.take\(\)/,
  "assembly must pass its optional maintenance port to the conditional registrar");
assert.match(host,
  /pub fn register_if_hosted\([\s\S]*?match host \{\s*Some\(host\) => \{[\s\S]*?register_maintenance_services\(set, service\);[\s\S]*?None => false,/,
  "maintenance registration must remain conditional on an injected host port");
const cli = read("core/crates/aeroric-cli/src/main.rs");
assert.match(cli, /HostWiring::default\(\)/);
assert.doesNotMatch(cli, /maintenance:\s*Some\(/,
  "do not broaden default CLI maintenance capability as a query DTO side effect");
console.log("pending-update-invariants: core-only public mapping, unchanged persisted DTO, deferred IO and conditional capability");
