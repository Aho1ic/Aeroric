#!/usr/bin/env node
// Slices48/51: shared existing-leaf guards/rename, unchanged actual OS Trash boundary.
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
  let previous = -1;
  for (const token of tokens) {
    const next = code.indexOf(token);
    assert(next > previous, `present and ordered: ${token}`);
    previous = next;
  }
}

const core = read("core/crates/aeroric-core/src/files/legacy_modify.rs");
assert.match(core, /^use super::legacy_entry::validate_entry_name;$/m);
assert.doesNotMatch(core, /^(?:pub(?:\([^)]*\))?\s+)?fn validate_entry_name\b/m);
assert.match(core, /^const PROTECTED_FIRST_SEGMENTS: &\[&str\] = &\["\.git", "\.aeroric"\];$/m);
assert.equal(core.match(/\bconst PROTECTED_FIRST_SEGMENTS\b/g)?.length, 1);
const protection = fn(core, "ensure_not_protected_path");
assert.match(protection, /^fn ensure_not_protected_path\(path: &Path, root: &Path, action: &str\) -> Result<\(\), String>/);
assert.match(protection, /if path == root \{\s*return Err\(format!\("Cannot \{\} the project root", action\)\);/);
assert.match(protection, /if let Ok\(rel\) = path\.strip_prefix\(root\) \{\s*if let Some\(first\) = rel\.components\(\)\.next\(\) \{\s*if let Some\(name\) = first\.as_os_str\(\)\.to_str\(\)/);
assert.match(protection, /if PROTECTED_FIRST_SEGMENTS\s*\.iter\(\)\s*\.any\(\|protected\| protected\.eq_ignore_ascii_case\(name\)\)/);
assert.match(protection, /return Err\(format!\("Cannot \{\} protected directory: \{\}", action, name\)\);/);
assert.match(protection, /Ok\(\(\)\)\s*}$/);
ordered(protection, ["if path == root", "path.strip_prefix(root)", "rel.components().next()",
  "first.as_os_str().to_str()", "PROTECTED_FIRST_SEGMENTS", "Ok(())"]);

for (const name of ["validate_existing_path_for_delete", "validate_existing_path_for_modify"]) {
  const code = fn(core, name);
  assert.match(code, new RegExp(`^pub fn ${name}\\(\\s*target: &str,\\s*allowed_root: &str,`));
  assert.match(code, /let target_path = Path::new\(target\);/);
  assert.match(code, /if !target_path\.is_absolute\(\) \{\s*return Err\("Path must be absolute"\.to_string\(\)\);/);
  assert.match(code, /let file_name = target_path\s*\.file_name\(\)\s*\.and_then\(\|s\| s\.to_str\(\)\)\s*\.ok_or_else\(\|\| "Invalid file name"\.to_string\(\)\)\?;/);
  assert.match(code, /let parent = target_path\s*\.parent\(\)\s*\.ok_or_else\(\|\| "Cannot resolve parent directory"\.to_string\(\)\)\?;/);
  assert.match(code, /let canonical_parent = parent\s*\.canonicalize\(\)\s*\.map_err\(\|e\| format!\("Cannot resolve parent directory: \{\}", e\)\)\?;/);
  assert.match(code, /let canonical_root = Path::new\(allowed_root\)\s*\.canonicalize\(\)\s*\.map_err\(\|e\| format!\("Cannot resolve root directory: \{\}", e\)\)\?;/);
  assert.match(code, /if !canonical_parent\.starts_with\(&canonical_root\) \{\s*return Err\("Path is outside the allowed directory"\.to_string\(\)\);/);
  assert.match(code, /let resolved = canonical_parent\.join\(file_name\);/);
  assert.match(code, /if resolved\.symlink_metadata\(\)\.is_err\(\) \{\s*return Err\("Path does not exist"\.to_string\(\)\);/);
  assert.equal(code.match(/\.canonicalize\(\)/g)?.length, 2);
  assert.equal(code.match(/\.symlink_metadata\(\)/g)?.length, 1);
  assert.equal(code.match(/ensure_not_protected_path\(/g)?.length, 1);
  ordered(code, ["let target_path", "if !target_path.is_absolute()", "let file_name",
    "let parent =", "let canonical_parent", "let canonical_root",
    "if !canonical_parent.starts_with", "let resolved =", "resolved.symlink_metadata()",
    "ensure_not_protected_path(", "Ok("]);
}
const deletion = fn(core, "validate_existing_path_for_delete");
assert.match(deletion, /\) -> Result<std::path::PathBuf, String>/);
assert.match(deletion, /ensure_not_protected_path\(&resolved, &canonical_root, "delete"\)\?;\s*Ok\(resolved\)/);
const modification = fn(core, "validate_existing_path_for_modify");
assert.match(modification, /action: &str,\s*\) -> Result<\(PathBuf, PathBuf\), String>/);
assert.match(modification, /ensure_not_protected_path\(&resolved, &canonical_root, action\)\?;\s*Ok\(\(resolved, canonical_root\)\)/);
for (const code of [protection, deletion, modification]) {
  assert.doesNotMatch(code, /\.trim\(|\.exists\(|\.is_dir\(|std::fs::|std::env|home_dir|Command::|trash::|local_history|load_settings|require_write|host_dispatch|validate_entry_name|validate_new_path_within|validate_path_within|path_policy::|read_write::/);
}
assert.match(read("core/crates/aeroric-core/src/files/mod.rs"), /^pub mod legacy_modify;$/m);

const desktop = read("src-tauri/src/fs.rs");
assert.match(desktop, /^use aeroric_core::files::legacy_modify::validate_existing_path_for_delete;$/m);
assert.match(desktop, /^#\[cfg\(all\(test, unix\)\)\]\nuse aeroric_core::files::legacy_modify::validate_existing_path_for_modify;$/m);
assert.doesNotMatch(desktop, /^(?:pub(?:\([^)]*\))?\s+)?fn (?:ensure_not_protected_path|validate_existing_path_for_delete|validate_existing_path_for_modify)\b|const PROTECTED_FIRST_SEGMENTS\b/m);
// The actual delete caller is unchanged; tests never invoke its OS boundary.
assert.equal(fn(desktop, "delete_path"), `pub async fn delete_path(path: String, project_path: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let resolved = validate_existing_path_for_delete(&path, &project_path)?;
        trash::delete(&resolved).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}`);
assert.equal(fn(core, "rename_path"), `pub fn rename_path(path: String, new_name: String, project_path: String) -> Result<(), String> {
    let name = new_name.trim();
    validate_entry_name(name)?;
    let (source, _) = validate_existing_path_for_modify(&path, &project_path, "rename")?;
    let parent = source
        .parent()
        .ok_or_else(|| "Cannot resolve parent directory".to_string())?;
    let destination = parent.join(name);
    if destination.exists() {
        return Err("A file or folder with that name already exists".to_string());
    }
    std::fs::rename(&source, &destination).map_err(|e| e.to_string())
}`);
assert.equal(fn(desktop, "rename_path"), `pub async fn rename_path(
    path: String,
    new_name: String,
    project_path: String,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        aeroric_core::files::legacy_modify::rename_path(path, new_name, project_path)
    })
    .await
    .map_err(|e| e.to_string())?
}`);
const native = read("core/crates/aeroric-core/src/files/read_write.rs");
for (const service of ["delete_path", "delete_path_with_trash", "rename_path"]) {
  assert.doesNotMatch(fn(native, service), /legacy_modify/);
}
const mapped = JSON.parse(read("docs/native/command-map.json")).commands;
const registry = read("src-tauri/src/lib.rs");
for (const [service, params] of [
  ["delete_path", ["path: String", "project_path: String"]],
  ["rename_path", ["path: String", "new_name: String", "project_path: String"]],
]) {
  assert.match(registry, new RegExp(`^\\s*fs::${service},$`, "m"));
  const row = mapped.find((entry) => entry.command === service);
  assert.equal(row?.rust_path, `fs::${service}`);
  assert.equal(row?.rpc_method, `files.${service}`);
  assert.deepEqual(row?.params, params);
}
const tests = read("src-tauri/src/fs/legacy_modify_tests.rs");
assert.equal(tests.match(/#\[test\]/g)?.length, 7);
assert.match(desktop, /#\[cfg\(test\)\]\s*#\[cfg\(unix\)\]\s*mod legacy_modify_tests;/);
for (const service of ["validate_existing_path_for_delete", "validate_existing_path_for_modify", "rename_path"]) {
  assert.match(tests, new RegExp(`super::${service}\\(`));
}
assert.doesNotMatch(tests, /\bdelete_path\s*\(|trash::|#\[ignore\b/);
assert.match(tests, /\.env_clear\(\)/);
assert.match(tests, /\.current_dir\(&root\.0\)/);
assert.match(tests, /stdout\.contains\("1 passed; 0 failed; 0 ignored"\)/);
assert.match(tests, /child\.kill\(\)\.unwrap\(\)/);
assert.match(tests, /child\.wait_with_output\(\)\.unwrap\(\)/);
assert.match(tests, /assert_renamed_only\(root, before, &source, &destination, true\)/);
const renameTests = read("core/crates/aeroric-core/tests/legacy_rename.rs");
assert.equal(renameTests.match(/#\[test\]/g)?.length, 4);
assert.match(renameTests, /legacy_modify::rename_path\(/);
assert.match(renameTests, /assert_renamed_only\(/);
assert.match(renameTests, /outside-project-but-still-scratch/);
assert.match(renameTests, /#\[cfg\(unix\)\]\nuse aeroric_core::files::read_write;/);
assert.doesNotMatch(renameTests, /\b(?:delete_path|read_clipboard_file_paths)\s*\(|trash::|#\[ignore\b/);
console.log("legacy-modify-invariants: exact shared guards/rename, thin actual rename worker, unchanged delete boundary and no test Trash");
