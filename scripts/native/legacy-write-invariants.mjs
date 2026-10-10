#!/usr/bin/env node
// Slice54: one legacy write composition owner; no native/mobile writer alias.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");
const read = (file) => readFileSync(path.join(root, file), "utf8");
function fn(source, name) {
  const matches = [...source.matchAll(new RegExp(
    `^(?:pub(?:\\([^\\n)]*\\))?\\s+)?(?:async\\s+)?fn\\s+${name}\\b`, "gm",
  ))];
  assert.equal(matches.length, 1, `one real ${name}`);
  const tail = source.slice(matches[0].index);
  const end = tail.search(/^}/m);
  assert(end >= 0, `closed ${name}`);
  return tail.slice(0, end + 1).replace(/^\s*\/\/[^\n]*\n/gm, "");
}

const expectedCore = `pub fn write_file_content(
    project_path: &str,
    file_path: &str,
    content: &str,
) -> Result<(), String> {
    let validated_path = validate_path_within(file_path, project_path, false)?;
    let validated_path_text = validated_path.to_string_lossy();
    record_snapshot_before_write(project_path, &validated_path_text, content)?;
    std::fs::write(validated_path, content).map_err(|e| e.to_string())
}`;
const expectedCommand = `pub async fn write_file_content(
    path: String,
    content: String,
    project_path: String,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        aeroric_core::files::legacy_write::write_file_content(&project_path, &path, &content)
    })
    .await
    .map_err(|e| e.to_string())?
}`;

// ---- core owner ----
const core = read("core/crates/aeroric-core/src/files/legacy_write.rs");
assert.equal(fn(core, "write_file_content"), expectedCore);
assert.match(core, /^use super::legacy_history::record_snapshot_before_write;$/m);
assert.match(core, /^use super::legacy_path::validate_path_within;$/m);
assert.doesNotMatch(
  core,
  /tauri::|AppHandle|host_dispatch|load_settings|Command::|std::env|home_dir|fs::rename|create_new\(|MAX_FILE_BYTES|resolve_file_write_destination|metadata\(/,
);
assert.match(read("core/crates/aeroric-core/src/files/mod.rs"), /^pub mod legacy_write;$/m);

// ---- desktop command is a thin delegate; the body moved out ----
const desktop = read("src-tauri/src/fs.rs");
const command = fn(desktop, "write_file_content");
assert.equal(command, expectedCommand);
assert.doesNotMatch(command, /validate_path_within|record_snapshot_before_write|std::fs::write/);
assert.doesNotMatch(desktop, /record_snapshot_before_write\(/);
assert.match(
  read("src-tauri/src/local_history.rs"),
  /^pub\(crate\) use aeroric_core::files::legacy_history::record_snapshot_before_write;$/m,
);
assert.equal(read("src-tauri/src/lib.rs").match(/\bfs::write_file_content,/g)?.length, 1);

// ---- mobile RPC keeps its outer checks and still calls the same command ----
const rpc = read("src-tauri/src/remote/files_rpc.rs");
assert.match(rpc, /crate::fs::write_file_content\(path, content, project\.path\)\.await\?;/);
assert.match(rpc, /Only existing regular files inside the project can be saved/);
assert.match(rpc, /MAX_FILE_RESPONSE_BYTES/);
assert.match(rpc, /"remote-file-written"/);
assert.doesNotMatch(rpc, /legacy_write|files\.write_file|record_snapshot_before_write/);
assert.doesNotMatch(rpc, /files\.create_file|files\.delete_path/);

// ---- native writer stays a distinct policy ----
const native = fn(read("core/crates/aeroric-core/src/files/read_write.rs"), "write_file_content");
assert.match(native, /validate_content_size\(content\)\?/);
assert.match(native, /resolve_file_write_destination\(root, relative\)\?/);
assert.doesNotMatch(native, /record_snapshot_before_write|validate_path_within|to_string_lossy|legacy_write/);

// ---- map citations and wire stay as declared ----
const map = JSON.parse(read("docs/native/command-map.json"));
assert.equal(map.modules.fs.core_service, "files");
const row = map.commands.find((c) => c.command === "write_file_content");
assert(row, "write_file_content mapped");
assert.equal(row.rust_path, "fs::write_file_content");
assert.equal(row.core_service, "files");
assert.equal(row.rpc_method, "files.write_file_content");
assert.equal(row.registered_at, "src-tauri/src/lib.rs:526");
assert.equal(row.definition_resolved, "src-tauri/src/fs.rs:123");
assert.equal(row.reachability.rust_internal_refs, 2);
assert.deepEqual(row.params, ["path: String", "content: String", "project_path: String"]);

// ---- frozen actual cases and direct core cases ----
assert.match(
  desktop,
  /^#\[cfg\(test\)\]\n#\[cfg\(unix\)\]\nmod legacy_write_tests;$/m,
);
const actual = read("src-tauri/src/fs/legacy_write_tests.rs");
assert.equal(actual.match(/#\[test\]/g)?.length, 5);
assert.match(actual, /super::write_file_content\(/);
assert.match(actual, /Command::new\(std::env::current_exe\(\)\.unwrap\(\)\)/);
assert.match(actual, /\.env_clear\(\)/);
assert.match(actual, /\.current_dir\(&root\.0\)/);
assert.match(actual, /stdout\.contains\("1 passed; 0 failed; 0 ignored"\)/);
assert.match(actual, /child\.kill\(\)\.unwrap\(\)/);
assert.match(actual, /child\.wait_with_output\(\)\.unwrap\(\)/);
assert.match(actual, /\.env\("HOME", &root\.0\)/);
assert.doesNotMatch(actual, /\.env\("HOME", "/);
assert.equal(actual.match(/\.env\(/g)?.length, 8, "sanctioned child env set only");
assert.match(actual, /!project\.join\("\.aeroric"\)\.exists\(\),\s*"no history side effects"/);
assert.doesNotMatch(actual, /#\[ignore\]|trash::|formatter::|read_clipboard|delete_path\(/);
const direct = read("core/crates/aeroric-core/tests/legacy_write.rs");
assert.equal(direct.match(/#\[test\]/g)?.length, 5);
assert.match(direct, /use aeroric_core::files::legacy_write::write_file_content;/);
assert.match(direct, /scratch::Scratch::new\(/);
assert.match(direct, /!project\.join\("\.aeroric"\)\.exists\(\),\s*"no history side effects"/);
assert.doesNotMatch(direct, /#\[ignore\]|Command::|trash::|home_dir/);
console.log(
  "legacy-write-invariants: one core composition, thin desktop command, unchanged mobile/native writers, frozen actual5/direct5",
);
