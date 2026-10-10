#!/usr/bin/env node
// Slices49–50: shared helpers/loop, retained actual clipboard target admission.
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

const core = read("core/crates/aeroric-core/src/files/legacy_copy.rs");
assert.match(core, /^use super::legacy_entry::validate_entry_name;$/m);
assert.match(core, /^use std::path::\{Path, PathBuf\};$/m);
assert.doesNotMatch(core, /^(?:pub(?:\([^)]*\))?\s+)?fn validate_entry_name\b/m);
const copy = fn(core, "copy_path_recursive");
assert.match(copy, /^pub fn copy_path_recursive\(source: &Path, destination: &Path\) -> Result<\(\), String>/);
assert.match(copy, /let metadata = source\.symlink_metadata\(\)\.map_err\(\|e\| e\.to_string\(\)\)\?;/);
assert.match(copy, /if metadata\.file_type\(\)\.is_symlink\(\) \{\s*let target = std::fs::read_link\(source\)\.map_err\(\|e\| e\.to_string\(\)\)\?;/);
assert.match(copy, /#\[cfg\(unix\)\]\s*\{\s*std::os::unix::fs::symlink\(target, destination\)\.map_err\(\|e\| e\.to_string\(\)\)\?;\s*}/);
assert.match(copy, /#\[cfg\(windows\)\]\s*\{\s*if source\.is_dir\(\) \{\s*std::os::windows::fs::symlink_dir\(target, destination\)\s*\.map_err\(\|e\| e\.to_string\(\)\)\?;\s*} else \{\s*std::os::windows::fs::symlink_file\(target, destination\)\s*\.map_err\(\|e\| e\.to_string\(\)\)\?;\s*}\s*}\s*return Ok\(\(\)\);/);
assert.match(copy, /if metadata\.is_dir\(\) \{\s*std::fs::create_dir\(destination\)\.map_err/);
const errorMapping = /\.map_err\(\|e\| match e\.kind\(\) \{\s*std::io::ErrorKind::AlreadyExists => \{\s*"A file or folder with that name already exists"\.to_string\(\)\s*}\s*_ => e\.to_string\(\),\s*}\)/g;
assert.equal([...copy.matchAll(errorMapping)].length, 2, "directory/file collision mapping, not symlink");
assert.match(copy, /for entry in std::fs::read_dir\(source\)\.map_err\(\|e\| e\.to_string\(\)\)\? \{\s*let entry = entry\.map_err\(\|e\| e\.to_string\(\)\)\?;\s*copy_path_recursive\(&entry\.path\(\), &destination\.join\(entry\.file_name\(\)\)\)\?;\s*}\s*return Ok\(\(\)\);/);
assert.match(copy, /std::fs::copy\(source, destination\)\s*\.map\(\|_\| \(\)\)\s*\.map_err/);
assert.equal(copy.match(/return Ok\(\(\)\);/g)?.length, 2);
ordered(copy, ["source.symlink_metadata()", "metadata.file_type().is_symlink()",
  "std::fs::read_link(source)", "#[cfg(unix)]", "#[cfg(windows)]",
  "if metadata.is_dir()", "std::fs::create_dir(destination)",
  "std::fs::read_dir(source)", "copy_path_recursive(&entry.path()", "std::fs::copy(source, destination)"]);
assert.doesNotMatch(copy, /\.canonicalize\(|\.exists\(|\.sort|\.collect|create_dir_all|set_permissions|remove_file|remove_dir|\.trim\(|OpenOptions/);

const guard = fn(core, "ensure_copy_target_not_inside_source");
assert.match(guard, /^pub fn ensure_copy_target_not_inside_source\(source: &Path, target: &Path\) -> Result<\(\), String>/);
assert.match(guard, /if !source\.is_dir\(\) \{\s*return Ok\(\(\)\);\s*}/);
assert.match(guard, /let canonical_source = source\s*\.canonicalize\(\)\s*\.map_err\(\|e\| format!\("Cannot resolve source directory: \{\}", e\)\)\?;/);
assert.match(guard, /let canonical_target = target\s*\.canonicalize\(\)\s*\.map_err\(\|e\| format!\("Cannot resolve paste target: \{\}", e\)\)\?;/);
assert.match(guard, /if canonical_target\.starts_with\(&canonical_source\) \{\s*return Err\("Cannot copy a folder into itself"\.to_string\(\)\);\s*}\s*Ok\(\(\)\)/);
assert.equal(guard.match(/\.canonicalize\(\)/g)?.length, 2);
ordered(guard, ["if !source.is_dir()", "return Ok(())", "let canonical_source",
  "let canonical_target", "if canonical_target.starts_with", "return Err("]);
assert.doesNotMatch(guard, /std::fs::|\.exists\(|is_absolute|symlink_metadata|\.trim\(/);
for (const code of [copy, guard]) {
  assert.doesNotMatch(code, /std::env|home_dir|Command::|trash::|host_dispatch|require_write|load_settings|local_history|path_policy::|read_write::/);
}
assert.match(read("core/crates/aeroric-core/src/files/mod.rs"), /^pub mod legacy_copy;$/m);

const desktop = read("src-tauri/src/fs.rs");
assert.match(desktop, /^#\[cfg\(all\(test, unix\)\)\]\nuse aeroric_core::files::legacy_copy::copy_path_recursive;$/m);
assert.match(desktop, /^#\[cfg\(test\)\]\nuse aeroric_core::files::legacy_copy::ensure_copy_target_not_inside_source;$/m);
assert.match(desktop, /^#\[cfg\(test\)\]\nuse std::path::PathBuf;$/m);
assert.doesNotMatch(desktop, /^(?:pub(?:\([^)]*\))?\s+)?fn (?:copy_path_recursive|ensure_copy_target_not_inside_source)\b/m);
assert.equal(fn(core, "copy_paths_to_directory"), `pub fn copy_paths_to_directory(source_paths: Vec<String>, target: PathBuf) -> Result<(), String> {
    if !target.is_dir() {
        return Err("Paste target must be a directory".to_string());
    }
    for source_path in source_paths {
        let source = PathBuf::from(&source_path);
        if !source.is_absolute() {
            return Err("Source path must be absolute".to_string());
        }
        if !source.exists() {
            return Err(format!("Source path does not exist: {}", source_path));
        }
        let name = source
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or_else(|| "Invalid source file name".to_string())?;
        validate_entry_name(name)?;
        let destination = target.join(name);
        if destination.exists() {
            return Err("A file or folder with that name already exists".to_string());
        }
        ensure_copy_target_not_inside_source(&source, &target)?;
        copy_path_recursive(&source, &destination)?;
    }
    Ok(())
}`);
assert.equal(fn(desktop, "copy_paths_to_directory"), `pub async fn copy_paths_to_directory(
    source_paths: Vec<String>,
    target_directory: String,
    project_path: String,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        if source_paths.is_empty() {
            return Ok(());
        }
        let target = validate_path_within(&target_directory, &project_path, false)?;
        aeroric_core::files::legacy_copy::copy_paths_to_directory(source_paths, target)
    })
    .await
    .map_err(|e| e.to_string())?
}`);
const native = fn(read("core/crates/aeroric-core/src/files/read_write.rs"), "copy_project_paths_to_directory");
assert.doesNotMatch(native, /legacy_copy/);
assert.match(native, /std::fs::symlink_metadata\(&to\)/);
const row = JSON.parse(read("docs/native/command-map.json")).commands
  .find((entry) => entry.command === "copy_paths_to_directory");
assert.equal(row?.rust_path, "fs::copy_paths_to_directory");
assert.equal(row?.rpc_method, "files.copy_paths_to_directory");
assert.deepEqual(row?.params, ["source_paths: Vec<String>", "target_directory: String", "project_path: String"]);
assert.match(read("src-tauri/src/lib.rs"), /^\s*fs::copy_paths_to_directory,$/m);
const tests = read("src-tauri/src/fs/legacy_copy_tests.rs");
assert.equal(tests.match(/#\[test\]/g)?.length, 6);
assert.match(desktop, /#\[cfg\(test\)\]\s*#\[cfg\(unix\)\]\s*mod legacy_copy_tests;/);
for (const service of ["copy_path_recursive", "ensure_copy_target_not_inside_source", "copy_paths_to_directory"]) {
  assert.match(tests, new RegExp(`super::${service}\\(`));
}
assert.doesNotMatch(tests, /\b(?:delete_path|read_clipboard_file_paths)\s*\(|trash::|#\[ignore\b/);
assert.match(tests, /\.env_clear\(\)/);
assert.match(tests, /\.current_dir\(&root\.0\)/);
assert.match(tests, /stdout\.contains\("1 passed; 0 failed; 0 ignored"\)/);
assert.match(tests, /child\.kill\(\)\.unwrap\(\)/);
assert.match(tests, /child\.wait_with_output\(\)\.unwrap\(\)/);
assert.match(tests, /assert_copied_only\(/);
assert.match(tests, /outside-project-but-still-scratch/);
const coreTests = read("core/crates/aeroric-core/tests/legacy_copy.rs");
assert.equal(coreTests.match(/#\[test\]/g)?.length, 8);
assert.match(coreTests, /legacy_copy::copy_paths_to_directory\(/);
assert.match(coreTests, /assert_loop_copied_only\(/);
assert.match(coreTests, /outside-project-but-still-scratch/);
assert.doesNotMatch(coreTests, /\b(?:delete_path|read_clipboard_file_paths)\s*\(|trash::|#\[ignore\b/);
console.log("legacy-copy-invariants: exact shared helpers/ordered loop, retained clipboard target admission, isolated effects and distinct native policy");
