#!/usr/bin/env node
// Slice 25: session id and native resume capability reading invariants.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");
const read = (file) => readFileSync(path.join(root, file), "utf8");

const tauriSession = read("src-tauri/src/session.rs");
assert.match(tauriSession, /host_dispatch::dispatch\(\s*&app,\s*"session\.session_supports_native_resume"/);
assert.match(tauriSession, /host_dispatch::dispatch\(\s*&app,\s*"session\.read_session_id"/);
assert.match(tauriSession, /local_session_supports_native_resume\(/);
assert.match(tauriSession, /local_read_session_id\(/);

const hostServices = read("core/crates/aeroric-host/src/services.rs");
assert.match(hostServices, /set\.register\(\s*"session\.session_supports_native_resume"/);
assert.match(hostServices, /set\.register\(\s*"session\.read_session_id"/);
assert.match(hostServices, /session_supports_native_resume\(request, context\)/);
assert.match(hostServices, /session_read_session_id\(request, context\)/);

console.log("session-query-invariants: session query host registration, tauri host_dispatch wiring verified");
