#!/usr/bin/env node
// Slice45: exact legacy absolute-target policy; no native guard substitution.
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
function ordered(code, tokens) {
  let last = -1;
  for (const token of tokens) {
    const next = code.indexOf(token);
    assert(next > last, `present and ordered: ${token}`);
    last = next;
  }
}

const core = read("core/crates/aeroric-core/src/files/legacy_path.rs");
const code = fn(core, "validate_path_within");
assert.match(code, /^pub fn validate_path_within\(\s*target: &str,\s*allowed_root: &str,\s*allow_symlink_escape: bool,\s*\) -> Result<std::path::PathBuf, String>/);
assert.match(code, /let target_path = Path::new\(target\);/);
assert.match(code, /if !target_path\.is_absolute\(\) \{\s*return Err\("Path must be absolute"\.to_string\(\)\);/);
assert.match(code, /let canonical_root = Path::new\(allowed_root\)\s*\.canonicalize\(\)\s*\.map_err\(\|e\| format!\("Cannot resolve root directory: \{\}", e\)\)\?;/);
assert.match(code, /if let Ok\(canonical_target\) = target_path\.canonicalize\(\) \{\s*if canonical_target\.starts_with\(&canonical_root\) \{\s*return Ok\(canonical_target\);/);
assert.match(code, /if !allow_symlink_escape \{\s*return Err\("Path is outside the allowed directory"\.to_string\(\)\);/);
assert.match(code, /let file_name = target_path\s*\.file_name\(\)\s*\.and_then\(\|s\| s\.to_str\(\)\)\s*\.ok_or_else\(\|\| "Invalid file name"\.to_string\(\)\)\?;/);
assert.match(code, /let parent = target_path\s*\.parent\(\)\s*\.ok_or_else\(\|\| "Cannot resolve parent directory"\.to_string\(\)\)\?;/);
assert.match(code, /let canonical_parent = parent\s*\.canonicalize\(\)\s*\.map_err\(\|e\| format!\("Cannot resolve path: \{\}", e\)\)\?;/);
assert.match(code, /if !canonical_parent\.starts_with\(&canonical_root\) \{\s*return Err\("Path is outside the allowed directory"\.to_string\(\)\);/);
assert.match(code, /Ok\(canonical_parent\.join\(file_name\)\)\s*}$/);
ordered(code, ["if !target_path.is_absolute()", "let canonical_root", "if let Ok(canonical_target)",
  "if !allow_symlink_escape", "let file_name", "let parent =", "let canonical_parent",
  "if !canonical_parent.starts_with", "Ok(canonical_parent.join(file_name))"]);
assert.equal(code.match(/\.canonicalize\(\)/g)?.length, 3);
assert.doesNotMatch(code, /\.trim\(|\.is_dir\(|\.exists\(|\.is_symlink\(|validate_project_root|validate_relative_path|std::env|home_dir|load_settings|host_dispatch|Command::|std::fs::|require_write/);
assert.match(read("core/crates/aeroric-core/src/files/mod.rs"), /^pub mod legacy_path;$/m);

const desktop = read("src-tauri/src/fs.rs");
assert.equal(desktop.match(/^use aeroric_core::files::legacy_path::validate_path_within;$/gm)?.length, 1);
assert.doesNotMatch(desktop, /^(?:pub(?:\([^)]*\))?\s+)?fn validate_path_within\b/m);
const callers = [
  ["open_in_system_file_manager", "let target = validate_path_within(&path, &project_path, true)?;", true],
  ["read_dir_entries", "let directory = validate_path_within(&path, &project_path, false)?;", true],
  ["read_file_content_with_policy", "let validated_path = validate_path_within(&path, &project_path, allow_symlink_escape)?;", false],
  ["read_image_preview", "let validated_path = validate_path_within(&path, &project_path, true)?;", false],
  ["copy_paths_to_directory", "let target = validate_path_within(&target_directory, &project_path, false)?;", true],
];
for (const [name, expression, workerFirst] of callers) {
  const caller = fn(desktop, name);
  assert.equal(caller.match(/validate_path_within\(/g)?.length, 1, name);
  const worker = "tauri::async_runtime::spawn_blocking(move ||";
  ordered(caller, workerFirst ? [worker, expression] : [expression, worker]);
  assert.doesNotMatch(caller, /legacy_path::|path_policy::|\.trim\(\)|host_dispatch/);
}
// Slice54 moved the write composition (strict guard call included) into core
// files::legacy_write; the desktop save command only delegates to that owner.
const writeCaller = fn(desktop, "write_file_content");
ordered(writeCaller, ["tauri::async_runtime::spawn_blocking(move ||",
  "aeroric_core::files::legacy_write::write_file_content(&project_path, &path, &content)"]);
assert.doesNotMatch(writeCaller, /validate_path_within|record_snapshot_before_write|std::fs::write/);
assert.equal(
  fn(read("core/crates/aeroric-core/src/files/legacy_write.rs"), "write_file_content")
    .match(/validate_path_within\(/g)?.length,
  1,
);
assert.match(read("src-tauri/src/lib.rs"), /^\s*fs::write_file_content,$/m);
assert.match(fn(desktop, "read_file_content"), /read_file_content_with_policy\(path, project_path, true\)\.await/);
assert.match(fn(desktop, "read_project_file_content"), /read_file_content_with_policy\(path, project_path, false\)\.await/);
const native = read("core/crates/aeroric-core/src/files/path_policy.rs");
for (const name of ["validate_project_root", "validate_legacy_project_root", "validate_relative_path", "resolve_within"]) {
  assert.doesNotMatch(fn(native, name), /legacy_path::|allow_symlink_escape/);
}
const registration = read("src-tauri/src/lib.rs");
for (const [name] of callers.filter(([name]) => name !== "read_file_content_with_policy")) {
  assert.match(registration, new RegExp(`^\\s*fs::${name},$`, "m"));
}
const tests = read("src-tauri/src/fs/legacy_path_tests.rs");
assert.match(tests, /super::validate_path_within\(/);
for (const name of ["read_file_content", "read_project_file_content", "read_image_preview",
  "read_dir_entries", "write_file_content", "copy_paths_to_directory"]) {
  assert.match(tests, new RegExp(`super::${name}\\(`));
}
assert.equal(tests.match(/#\[test\]/g)?.length, 7);
assert.match(tests, /\.env_clear\(\)/);
assert.match(tests, /\.current_dir\(&root\.0\)/);
assert.match(tests, /stdout\.contains\("1 passed; 0 failed; 0 ignored"\)/);
assert.match(tests, /child\.kill\(\)\.unwrap\(\)/);
assert.match(tests, /child\.wait_with_output\(\)\.unwrap\(\)/);
console.log("legacy-path-invariants: one legacy guard, unchanged caller modes/order and distinct native policies");
