#!/usr/bin/env node
// Slice 23: platform runtime info and runnable-file commands invariants.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");
const read = (file) => readFileSync(path.join(root, file), "utf8");

const tauriPlatform = read("src-tauri/src/platform/mod.rs");
assert.match(tauriPlatform, /host_dispatch::dispatch\(\s*&app,\s*"platform\.get_platform_runtime_info"/);
assert.match(tauriPlatform, /host_dispatch::dispatch\(\s*&app,\s*"platform\.build_runnable_file_command"/);
assert.match(tauriPlatform, /local_platform_runtime_info\(\)/);
assert.match(tauriPlatform, /local_runnable_file_command\(/);
assert.match(tauriPlatform, /decode_runnable_file_command\(/);

const hostServices = read("core/crates/aeroric-host/src/services.rs");
assert.match(hostServices, /set\.register\(\s*"platform\.get_platform_runtime_info"/);
assert.match(hostServices, /set\.register\(\s*"platform\.build_runnable_file_command"/);
assert.match(hostServices, /platform_get_platform_runtime_info\(request\)/);
assert.match(hostServices, /platform_build_runnable_file_command\(request\)/);

const corePlatform = read("core/crates/aeroric-core/src/platform/mod.rs");
assert.match(corePlatform, /pub fn current_platform_runtime_info\(\) -> PlatformRuntimeInfo/);
assert.match(corePlatform, /pub fn runnable_file_command\(/);
assert.match(corePlatform, /pub fn default_shell_command\(\) -> ShellCommand/);
assert.match(corePlatform, /pub fn shell_kind_and_label\(program: &str\) -> \(String, String\)/);

console.log("platform-invariants: host platform registration, core pure builders, tauri host_dispatch wiring verified");
