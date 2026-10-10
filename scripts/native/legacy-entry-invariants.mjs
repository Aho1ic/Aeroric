#!/usr/bin/env node
// Slice47: exact old name/new-path policy and exclusive final-entry creation.
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

const core = read("core/crates/aeroric-core/src/files/legacy_entry.rs");
assert.match(core, /#\[cfg\(target_os = "windows"\)\]\s*const WINDOWS_RESERVED_STEMS: &\[&str\] = &\[/);
const table = core.match(/const WINDOWS_RESERVED_STEMS: &\[&str\] = &\[([\s\S]*?)\];/);
assert(table);
assert.deepEqual([...table[1].matchAll(/"([^"]+)"/g)].map((match) => match[1]), [
  "CON", "PRN", "AUX", "NUL", ...Array.from({ length: 10 }, (_, i) => `COM${i}`),
  ...Array.from({ length: 10 }, (_, i) => `LPT${i}`),
]);
assert.equal(core.match(/\bconst WINDOWS_RESERVED_STEMS\b/g)?.length, 1);
const name = fn(core, "validate_entry_name");
assert.match(name, /^pub fn validate_entry_name\(name: &str\) -> Result<\(\), String>/);
assert.match(name, /if name\.is_empty\(\) \{\s*return Err\("File name cannot be empty"\.to_string\(\)\);/);
assert.match(name, /if name\.len\(\) > 255 \{\s*return Err\("File name is too long \(max 255 bytes\)"\.to_string\(\)\);/);
assert.match(name, /if name == "\." \|\| name == "\.\." \{\s*return Err\("Invalid file name"\.to_string\(\)\);/);
assert(name.includes(String.raw`if name.contains('/') || name.contains('\\') || name.contains('\0')`));
assert.match(name, /#\[cfg\(target_os = "windows"\)\]\s*\{\s*for ch in name\.chars\(\)/);
assert(name.includes(`matches!(ch, '<' | '>' | ':' | '"' | '|' | '?' | '*')`));
assert.match(name, /if \(ch as u32\) < 0x20 \{\s*return Err\("File name contains control characters"\.to_string\(\)\);/);
assert.match(name, /if name\.ends_with\(' '\) \|\| name\.ends_with\('\.'\) \{\s*return Err\("File name cannot end with a space or a dot"\.to_string\(\)\);/);
assert.match(name, /let stem = name\.split_once\('\.'\)\.map\(\|\(s, _\)\| s\)\.unwrap_or\(name\);/);
assert.match(name, /if !stem\.is_empty\(\) \{\s*let stem_upper = stem\.to_ascii_uppercase\(\);/);
assert.match(name, /if WINDOWS_RESERVED_STEMS\.iter\(\)\.any\(\|r\| \*r == stem_upper\) \{\s*return Err\(format!\("File name '\{\}' is reserved on Windows", stem\)\);/);
ordered(name, ["if name.is_empty()", "if name.len() > 255", 'if name == "."',
  "if name.contains('/')", '#[cfg(target_os = "windows")]', "for ch in name.chars()",
  "if (ch as u32) < 0x20", "if name.ends_with(' ')", "let stem =", "if !stem.is_empty()", "Ok(())"]);
assert.equal(name.match(/"File name contains forbidden characters"/g)?.length, 2);

const policy = fn(core, "validate_new_path_within");
assert.match(policy, /^pub fn validate_new_path_within\(\s*target: &str,\s*allowed_root: &str,\s*\) -> Result<\(std::path::PathBuf, String\), String>/);
assert.match(policy, /let target_path = Path::new\(target\);/);
assert.match(policy, /if !target_path\.is_absolute\(\) \{\s*return Err\("Path must be absolute"\.to_string\(\)\);/);
assert.match(policy, /let file_name = target_path\s*\.file_name\(\)\s*\.and_then\(\|s\| s\.to_str\(\)\)\s*\.ok_or_else\(\|\| "Invalid file name"\.to_string\(\)\)\?\s*\.to_string\(\);/);
assert.match(policy, /validate_entry_name\(&file_name\)\?;/);
assert.match(policy, /let parent = target_path\s*\.parent\(\)\s*\.ok_or_else\(\|\| "Cannot resolve parent directory"\.to_string\(\)\)\?;/);
assert.match(policy, /let canonical_parent = parent\s*\.canonicalize\(\)\s*\.map_err\(\|e\| format!\("Cannot resolve parent directory: \{\}", e\)\)\?;/);
assert.match(policy, /let canonical_root = Path::new\(allowed_root\)\s*\.canonicalize\(\)\s*\.map_err\(\|e\| format!\("Cannot resolve root directory: \{\}", e\)\)\?;/);
assert.match(policy, /if !canonical_parent\.starts_with\(&canonical_root\) \{\s*return Err\("Path is outside the allowed directory"\.to_string\(\)\);/);
assert.match(policy, /Ok\(\(canonical_parent, file_name\)\)\s*}$/);
assert.equal(policy.match(/\.canonicalize\(\)/g)?.length, 2);
ordered(policy, ["let target_path", "if !target_path.is_absolute()", "let file_name",
  "validate_entry_name(&file_name)", "let parent =", "let canonical_parent",
  "let canonical_root", "if !canonical_parent.starts_with", "Ok((canonical_parent, file_name))"]);
for (const code of [name, policy]) {
  assert.doesNotMatch(code, /\.trim\(|\.exists\(|\.is_dir\(|std::fs::|host_dispatch|load_settings|std::env|home_dir|Command::|path_policy::|read_write::|require_write|ensure_not_protected_path|chars\(\)\.count/);
}

for (const service of ["create_file", "create_directory"]) {
  const code = fn(core, service);
  assert.match(code, new RegExp(`^pub fn ${service}\\(target: &Path\\) -> Result<\\(\\), String>`));
  assert.match(code, /\.map_err\(\|e\| match e\.kind\(\) \{\s*std::io::ErrorKind::AlreadyExists => \{\s*"A file or folder with that name already exists"\.to_string\(\)\s*}\s*_ => e\.to_string\(\),\s*}\)/);
  assert.doesNotMatch(code, /\.exists\(|create_dir_all|\.truncate\(|\.create\(true\)|set_permissions|std::env|home_dir|load_settings|local_history|path_policy::|read_write::|host_dispatch/);
}
const file = fn(core, "create_file");
ordered(file, ["std::fs::OpenOptions::new()", ".write(true)", ".create_new(true)", ".open(target)", ".map(|_| ())", ".map_err("]);
assert.match(fn(core, "create_directory"), /std::fs::create_dir\(target\)\.map_err/);
assert.match(read("core/crates/aeroric-core/src/files/mod.rs"), /^pub mod legacy_entry;$/m);

const desktop = read("src-tauri/src/fs.rs");
assert.match(desktop, /^#\[cfg\(all\(test, unix\)\)\]\nuse aeroric_core::files::legacy_entry::validate_entry_name;$/m);
assert.match(desktop, /^use aeroric_core::files::legacy_entry::validate_new_path_within;$/m);
assert.doesNotMatch(desktop, /^(?:pub(?:\([^)]*\))?\s+)?fn (?:validate_entry_name|validate_new_path_within)\b|const WINDOWS_RESERVED_STEMS\b/m);
for (const service of ["create_file", "create_directory"]) {
  const code = fn(desktop, service);
  ordered(code, ["tauri::async_runtime::spawn_blocking(move || {",
    "let (parent, file_name) = validate_new_path_within(&path, &project_path)?;",
    "let target = parent.join(&file_name);",
    `aeroric_core::files::legacy_entry::${service}(&target)`, ".await"]);
  assert.equal(code.match(/validate_new_path_within\(/g)?.length, 1);
  assert.doesNotMatch(code, /std::fs|\.exists\(|\.trim\(|host_dispatch|read_write::|require_write/);
}
const renameSource = read("core/crates/aeroric-core/src/files/legacy_modify.rs");
assert.match(renameSource, /^use super::legacy_entry::validate_entry_name;$/m);
assert.doesNotMatch(renameSource, /^(?:pub(?:\([^)]*\))?\s+)?fn validate_entry_name\b/m);
ordered(fn(renameSource, "rename_path"), ["let name = new_name.trim();", "validate_entry_name(name)?;",
  'validate_existing_path_for_modify(&path, &project_path, "rename")?']);
const renameAdapter = fn(desktop, "rename_path");
ordered(renameAdapter, ["tauri::async_runtime::spawn_blocking(move || {",
  "aeroric_core::files::legacy_modify::rename_path(path, new_name, project_path)", ".await"]);
assert.equal(renameAdapter.match(/legacy_modify::rename_path\(/g)?.length, 1);
assert.doesNotMatch(renameAdapter, /\.trim\(|validate_entry_name|std::fs::|host_dispatch/);
const copySource = read("core/crates/aeroric-core/src/files/legacy_copy.rs");
assert.match(copySource, /^use super::legacy_entry::validate_entry_name;$/m);
assert.doesNotMatch(copySource, /^(?:pub(?:\([^)]*\))?\s+)?fn validate_entry_name\b/m);
const copy = fn(copySource, "copy_paths_to_directory");
ordered(copy, ["let name = source", "validate_entry_name(name)?;", "let destination = target.join(name);", "copy_path_recursive("]);
assert.doesNotMatch(copy, /\.trim\(/);
const copyAdapter = fn(desktop, "copy_paths_to_directory");
ordered(copyAdapter, ["tauri::async_runtime::spawn_blocking(move || {",
  "if source_paths.is_empty()", "return Ok(());",
  "let target = validate_path_within(&target_directory, &project_path, false)?;",
  "aeroric_core::files::legacy_copy::copy_paths_to_directory(source_paths, target)", ".await"]);
assert.equal(copyAdapter.match(/legacy_copy::copy_paths_to_directory\(/g)?.length, 1);
assert.doesNotMatch(copyAdapter, /for source_path|validate_entry_name|\.trim\(|std::fs::|host_dispatch/);
const native = read("core/crates/aeroric-core/src/files/read_write.rs");
for (const service of ["create_file", "create_directory", "rename_path"]) {
  assert.doesNotMatch(fn(native, service), /legacy_entry/);
}
const mapped = JSON.parse(read("docs/native/command-map.json")).commands;
const registry = read("src-tauri/src/lib.rs");
for (const service of ["create_file", "create_directory"]) {
  assert.match(registry, new RegExp(`^\\s*fs::${service},$`, "m"));
  const row = mapped.find((entry) => entry.command === service);
  assert.equal(row?.rust_path, `fs::${service}`);
  assert.equal(row?.rpc_method, `files.${service}`);
  assert.deepEqual(row?.params, ["path: String", "project_path: String"]);
}
const tests = read("src-tauri/src/fs/legacy_entry_tests.rs");
assert.equal(tests.match(/#\[test\]/g)?.length, 8);
for (const service of ["validate_entry_name", "validate_new_path_within", "create_file",
  "create_directory", "rename_path", "copy_paths_to_directory"]) {
  assert.match(tests, new RegExp(`super::${service}\\(`));
}
assert.match(tests, /\.env_clear\(\)/);
assert.match(tests, /\.current_dir\(&root\.0\)/);
assert.match(tests, /stdout\.contains\("1 passed; 0 failed; 0 ignored"\)/);
assert.match(tests, /child\.kill\(\)\.unwrap\(\)/);
assert.match(tests, /child\.wait_with_output\(\)\.unwrap\(\)/);
console.log("legacy-entry-invariants: exact shared name/new-path/exclusive IO, guarded workers and unchanged rename/copy/native boundaries");
