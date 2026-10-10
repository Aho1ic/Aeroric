#!/usr/bin/env node
// Slice44: one legacy listing owner, not a native/legacy wire alias.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");
const read = (file) => readFileSync(path.join(root, file), "utf8");

// Bounded rustfmt top-level functions, not a Rust parser. Neighbours must not
// accidentally inherit this reader's placement or side-effect assertions.
function fn(source, name) {
  const matches = [...source.matchAll(new RegExp(
    `^(?:pub(?:\\([^\\n)]*\\))?\\s+)?(?:async\\s+)?fn\\s+${name}\\b`, "gm",
  ))];
  assert.equal(matches.length, 1, `one real ${name}`);
  const tail = source.slice(matches[0].index);
  const end = tail.search(/^}/m);
  assert(end >= 0, `closed ${name}`);
  return tail.slice(0, end + 1);
}

const core = read("core/crates/aeroric-core/src/files/legacy_directory.rs");
assert.equal(core.match(/pub struct FsEntry\b/g)?.length, 1);
assert.match(core, /#\[derive\(serde::Serialize\)\]\s*pub struct FsEntry/);
const model = core.slice(core.indexOf("pub struct FsEntry"), core.indexOf("\n}", core.indexOf("pub struct FsEntry")) + 2);
assert.deepEqual(
  [...model.matchAll(/pub (\w+): ([^,\n]+),/g)].map((match) => [match[1], match[2]]),
  [["name", "String"], ["path", "String"], ["is_dir", "bool"],
    ["extension", "Option<String>"], ["modified_at_ms", "Option<u64>"],
    ["is_gitignored", "bool"], ["is_symlink", "bool"]],
);
assert.doesNotMatch(model, /#\[serde/);
const ignored = core.match(/const IGNORED_DIRS:.*?=\s*&\[([\s\S]*?)\];/)?.[1];
assert(ignored);
assert.deepEqual(
  [...ignored.matchAll(/"([^"]+)"/g)].map((match) => match[1]),
  [".git", "node_modules", ".next", ".nuxt", "dist", "build", "target",
    "__pycache__", ".cache", "coverage", ".turbo", ".expo", "out", ".output",
    ".venv", "venv", ".tox"],
);
const code = fn(core, "read_dir_entries");
assert.match(code, /directory: &Path,\s*project_path: &str/);
assert.match(code, /std::fs::read_dir\(directory\)\.map_err\(\|e\| e\.to_string\(\)\)/);
assert.match(code, /\.flatten\(\)\s*\.filter/);
assert.match(code, /if p\.is_dir\(\) \{[\s\S]*?!IGNORED_DIRS\.contains\(&name_str\.as_ref\(\)\)/);
assert.match(code, /let is_dir = p\.is_dir\(\)/);
assert.match(code, /let name = entry\.file_name\(\)\.to_string_lossy\(\)\.into_owned\(\)/);
assert.match(code, /path: p\.to_string_lossy\(\)\.into_owned\(\)/);
assert.match(code, /\.extension\(\)\s*\.and_then\(\|e\| e\.to_str\(\)\)\s*\.map\(\|s\| s\.to_lowercase\(\)\)/);
assert.match(code, /let modified_at_ms = entry\s*\.metadata\(\)/);
assert.match(code, /modified\.duration_since\(UNIX_EPOCH\)/);
assert.match(code, /duration\.as_millis\(\) as u64/);
assert.match(code, /let is_symlink = entry\s*\.file_type\(\)\s*\.map\(\|file_type\| file_type\.is_symlink\(\)\)\s*\.unwrap_or\(false\)/);
assert.match(code, /\(true, false\) => std::cmp::Ordering::Less/);
assert.match(code, /\(false, true\) => std::cmp::Ordering::Greater/);
assert.match(code, /a\.name\.to_lowercase\(\)\.cmp\(&b\.name\.to_lowercase\(\)\)/);
assert.match(code, /if !result\.is_empty\(\) \{\s*let ignored_set/);
assert.equal(code.match(/Command::new\(/g)?.length, 1);
assert.match(code, /Command::new\("git"\)/);
assert.match(code, /crate::runtime::subprocess::configure_background_command\(&mut cmd\)/);
assert.match(code, /cmd\.args\(\["check-ignore", "--stdin"\]\)\s*\.current_dir\(project_path\)/);
assert.match(code, /\.stdin\(std::process::Stdio::piped\(\)\)\s*\.stdout\(std::process::Stdio::piped\(\)\)\s*\.stderr\(std::process::Stdio::null\(\)\)/);
assert.match(code, /writeln!\(stdin, "\{\}", entry\.path\)/);
assert.match(code, /match child\.wait_with_output\(\)/);
assert.equal(code.match(/Err\(_\) => std::collections::HashSet::new\(\)/g)?.length, 2);
assert.match(code, /entry\.is_gitignored = ignored_set\.contains\(&entry\.path\)/);
assert.doesNotMatch(code, /output\.status|\.success\(\)|load_settings|detect_path|home_dir|require_write|host_dispatch|eq_ignore_ascii_case/);
assert.match(read("core/crates/aeroric-core/src/files/mod.rs"), /pub mod legacy_directory;/);

const desktop = read("src-tauri/src/fs.rs");
assert.match(desktop, /pub\(crate\) use aeroric_core::files::legacy_directory::FsEntry;/);
assert.doesNotMatch(desktop, /struct FsEntry\b|const IGNORED_DIRS\b/);
const adapter = fn(desktop, "read_dir_entries");
assert.match(adapter, /path: String,\s*project_path: String/);
assert.match(adapter, /Result<Vec<FsEntry>, String>/);
assert.match(adapter, /tauri::async_runtime::spawn_blocking\(move \|\| \{\s*let directory = validate_path_within\(&path, &project_path, false\)\?;\s*aeroric_core::files::legacy_directory::read_dir_entries\(&directory, &project_path\)\s*\}\)/);
assert.equal(adapter.match(/validate_path_within\(/g)?.length, 1);
assert.equal(adapter.match(/legacy_directory::read_dir_entries\(/g)?.length, 1);
assert.doesNotMatch(adapter, /\.trim\(|std::fs|Command::|\.sort_by|host_dispatch/);
assert.match(read("src-tauri/src/lib.rs"), /^\s*fs::read_dir_entries,/m);
const mapped = JSON.parse(read("docs/native/command-map.json")).commands.find((row) => row.command === "read_dir_entries");
assert.equal(mapped?.rust_path, "fs::read_dir_entries");
assert.equal(mapped?.rpc_method, "files.read_dir_entries");

const native = read("core/crates/aeroric-core/src/files/read_write.rs");
assert.match(native, /#\[serde\(rename_all = "camelCase"\)\]\s*pub struct FileEntry/);
assert.doesNotMatch(fn(native, "read_dir_entries"), /legacy_directory|Command::|IGNORED_DIRS/);
const tests = read("src-tauri/src/fs/legacy_directory_tests.rs");
assert.match(tests, /super::read_dir_entries\(/);
assert.match(tests, /\.env_clear\(\)/);
assert.match(tests, /stdout\.contains\("1 passed; 0 failed; 0 ignored"\)/);
assert.equal(tests.match(/#\[test\]/g)?.length, 6);
console.log("legacy-directory-invariants: shared legacy model/algorithm, unchanged strict worker adapter and distinct native wire");
