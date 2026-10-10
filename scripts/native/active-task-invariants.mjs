#!/usr/bin/env node
// P8-05 slice 19: supplement real command/UDS tests with a placement wiring ratchet.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");
const read = (file) => readFileSync(path.join(root, file), "utf8");
const source = read("src-tauri/src/pty.rs");
const declarations = [...source.matchAll(/^pub async fn get_active_task_ids(?:<[^>]+>)?\(/gm)];
assert.equal(declarations.length, 1, "there must be one actual Tauri active-task command");
const start = declarations[0].index;
const end = source.indexOf("\nfn preflight_agent_launch_spec(", start);
assert(end > start, "active-task command boundary was not found");
const body = source.slice(start, end);
assert.match(body, /app:\s*AppHandle<R>/, "the command must receive the managed runtime handle");
assert.match(body, /host_dispatch::dispatch\s*\(/);
assert(body.includes('"terminal.get_active_task_ids"'), "forward the real registered method");
assert.match(body, /serde_json::json!\(\{\}\)/, "preserve the no-argument wire");
const localStart = body.indexOf("async move {");
assert(localStart >= 0, "the legacy lookup must stay deferred");
assert.doesNotMatch(body.slice(0, localStart), /child_handles/,
  "do not read local liveness before placement chooses the owner");
assert.match(body.slice(localStart), /\.child_handles/);
assert.match(body.slice(localStart), /validate_task_id\(run\.id\(\)\)\.is_ok\(\)/);
assert.doesNotMatch(body, /\.live_task_ids\(|\.runs\b|\.sort\b|\.dedup\b/,
  "Embedded must retain its original child-handle semantics");
assert.match(body, /serde_json::from_value\(value\)/, "decode only the chosen owner's reply");
assert.doesNotMatch(body, /unwrap_or_default|unwrap_or_else|or_else\s*\(/,
  "an External error or invalid payload cannot become a local or empty result");
assert.match(read("core/crates/aeroric-host/src/services.rs"),
  /set\.register\(\s*"terminal\.get_active_task_ids"/);
assert.match(read("src/state/app/useAppStartup.ts"),
  /new Set\(await invoke<string\[\]>\("get_active_task_ids"\)\)/);
console.log("active-task-invariants: actual command dispatch, deferred legacy lookup, strict reply and unchanged startup wire");
