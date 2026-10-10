#!/usr/bin/env node
// Slice52: exact legacy hex history, equal types only; no native/host policy alias.
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

const expectedLegacy = {
  "record_snapshot_before_write": "pub fn record_snapshot_before_write(\n    project_path: &str,\n    file_path: &str,\n    next_content: &str,\n) -> Result<Option<LocalHistoryEntry>, String> {\n    let root = validate_project_root(project_path)?;\n    let file = validate_file_path(&root, file_path)?;\n    record_snapshot_for_file(CODE_LAYOUT, &root, &file, Some(next_content))\n}",
  "list_entries": "pub fn list_entries(project_path: &str, file_path: &str) -> Result<Vec<LocalHistoryEntry>, String> {\n    let root = validate_project_root(project_path)?;\n    let file = validate_file_path(&root, file_path)?;\n    list_entries_in(CODE_LAYOUT, &root, &file)\n}",
  "list_entries_in": "pub fn list_entries_in(\n    layout: HistoryLayout,\n    root: &Path,\n    file: &Path,\n) -> Result<Vec<LocalHistoryEntry>, String> {\n    let relative_path = relative_file_path(root, file)?;\n    let history_dir = history_dir_for_relative_path(layout, root, &relative_path);\n    if !history_dir.exists() {\n        return Ok(Vec::new());\n    }\n\n    let mut entries = Vec::new();\n    for entry in fs::read_dir(history_dir).map_err(|e| e.to_string())? {\n        let entry = entry.map_err(|e| e.to_string())?;\n        let path = entry.path();\n        if path.extension().and_then(|ext| ext.to_str()) != Some(SNAPSHOT_EXTENSION) {\n            continue;\n        }\n        let Some(id) = path.file_stem().and_then(|stem| stem.to_str()) else {\n            continue;\n        };\n        let Some(created_at_ms) = entry_id_timestamp_ms(id) else {\n            continue;\n        };\n        let metadata = entry.metadata().map_err(|e| e.to_string())?;\n        if !metadata.is_file() {\n            continue;\n        }\n        entries.push(LocalHistoryEntry {\n            id: id.to_string(),\n            file_path: file.to_string_lossy().into_owned(),\n            relative_path: relative_path.clone(),\n            created_at_ms,\n            size: metadata.len(),\n        });\n    }\n\n    entries.sort_by(|a, b| {\n        b.created_at_ms\n            .cmp(&a.created_at_ms)\n            .then_with(|| entry_id_sequence(&b.id).cmp(&entry_id_sequence(&a.id)))\n    });\n    entries.truncate(layout.max_entries);\n    Ok(entries)\n}",
  "read_entry": "pub fn read_entry(\n    project_path: &str,\n    file_path: &str,\n    entry_id: &str,\n) -> Result<LocalHistorySnapshot, String> {\n    let root = validate_project_root(project_path)?;\n    let file = validate_file_path(&root, file_path)?;\n    read_entry_in(CODE_LAYOUT, &root, &file, entry_id)\n}",
  "read_entry_in": "pub fn read_entry_in(\n    layout: HistoryLayout,\n    root: &Path,\n    file: &Path,\n    entry_id: &str,\n) -> Result<LocalHistorySnapshot, String> {\n    let entry = entry_for_id(layout, root, file, entry_id)?;\n    let content = fs::read_to_string(entry_path(layout, root, &entry.relative_path, entry_id))\n        .map_err(|e| e.to_string())?;\n    Ok(LocalHistorySnapshot { entry, content })\n}",
  "restore_entry": "pub fn restore_entry(\n    project_path: &str,\n    file_path: &str,\n    entry_id: &str,\n) -> Result<LocalHistorySnapshot, String> {\n    let root = validate_project_root(project_path)?;\n    let file = validate_file_path(&root, file_path)?;\n    let snapshot = read_entry(project_path, file_path, entry_id)?;\n    let _ = record_snapshot_for_file(CODE_LAYOUT, &root, &file, Some(&snapshot.content))?;\n    fs::write(&file, snapshot.content.as_bytes()).map_err(|e| e.to_string())?;\n    Ok(snapshot)\n}",
  "record_snapshot_for_file": "fn record_snapshot_for_file(\n    layout: HistoryLayout,\n    root: &Path,\n    file: &Path,\n    skip_if_content_matches: Option<&str>,\n) -> Result<Option<LocalHistoryEntry>, String> {\n    if is_inside_history_dir(layout, root, file) {\n        return Ok(None);\n    }\n    let metadata = fs::metadata(file).map_err(|e| e.to_string())?;\n    if !metadata.is_file() || metadata.len() > MAX_SNAPSHOT_BYTES {\n        return Ok(None);\n    }\n    let current_content = match fs::read_to_string(file) {\n        Ok(content) => content,\n        Err(_) => return Ok(None),\n    };\n    if skip_if_content_matches.is_some_and(|next| next == current_content) {\n        return Ok(None);\n    }\n    let relative_path = relative_file_path(root, file)?;\n    let history_dir = history_dir_for_relative_path(layout, root, &relative_path);\n    if within_min_interval(layout, &history_dir) {\n        return Ok(None);\n    }\n    create_snapshot(layout, root, file, &current_content)\n}",
  "within_min_interval": "fn within_min_interval(layout: HistoryLayout, history_dir: &Path) -> bool {\n    if layout.min_interval_ms == 0 {\n        return false;\n    }\n    let Some(latest) = latest_entry_timestamp_ms(history_dir) else {\n        return false;\n    };\n    now_ms_u64().saturating_sub(latest) < layout.min_interval_ms\n}",
  "latest_entry_timestamp_ms": "fn latest_entry_timestamp_ms(history_dir: &Path) -> Option<u64> {\n    fs::read_dir(history_dir)\n        .ok()?\n        .filter_map(Result::ok)\n        .filter_map(|entry| {\n            let path = entry.path();\n            if path.extension().and_then(|ext| ext.to_str()) != Some(SNAPSHOT_EXTENSION) {\n                return None;\n            }\n            entry_id_timestamp_ms(path.file_stem()?.to_str()?)\n        })\n        .max()\n}",
  "create_snapshot": "fn create_snapshot(\n    layout: HistoryLayout,\n    root: &Path,\n    file: &Path,\n    content: &str,\n) -> Result<Option<LocalHistoryEntry>, String> {\n    let relative_path = relative_file_path(root, file)?;\n    let history_dir = history_dir_for_relative_path(layout, root, &relative_path);\n    fs::create_dir_all(&history_dir).map_err(|e| e.to_string())?;\n    let base_id = now_ms_u64().to_string();\n\n    for suffix in 0..1000 {\n        let id = if suffix == 0 {\n            base_id.clone()\n        } else {\n            format!(\"{base_id}-{suffix}\")\n        };\n        let path = entry_path(layout, root, &relative_path, &id);\n        let mut file_handle = match fs::OpenOptions::new()\n            .write(true)\n            .create_new(true)\n            .open(&path)\n        {\n            Ok(handle) => handle,\n            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => continue,\n            Err(err) => return Err(err.to_string()),\n        };\n        file_handle\n            .write_all(content.as_bytes())\n            .map_err(|e| e.to_string())?;\n        let size = file_handle.metadata().map_err(|e| e.to_string())?.len();\n        let entry = LocalHistoryEntry {\n            id,\n            file_path: file.to_string_lossy().into_owned(),\n            relative_path,\n            created_at_ms: entry_id_timestamp_ms(&base_id).unwrap_or_else(now_ms_u64),\n            size,\n        };\n        prune_history_dir(&history_dir, layout.max_entries)?;\n        return Ok(Some(entry));\n    }\n\n    Err(\"Could not create a unique local history snapshot\".to_string())\n}",
  "entry_for_id": "fn entry_for_id(\n    layout: HistoryLayout,\n    root: &Path,\n    file: &Path,\n    entry_id: &str,\n) -> Result<LocalHistoryEntry, String> {\n    validate_entry_id(entry_id)?;\n    let relative_path = relative_file_path(root, file)?;\n    let path = entry_path(layout, root, &relative_path, entry_id);\n    let metadata = fs::metadata(&path).map_err(|e| e.to_string())?;\n    if !metadata.is_file() {\n        return Err(\"Local history entry is not a file\".to_string());\n    }\n    let created_at_ms = entry_id_timestamp_ms(entry_id)\n        .ok_or_else(|| \"Invalid local history entry id\".to_string())?;\n    Ok(LocalHistoryEntry {\n        id: entry_id.to_string(),\n        file_path: file.to_string_lossy().into_owned(),\n        relative_path,\n        created_at_ms,\n        size: metadata.len(),\n    })\n}",
  "validate_file_path": "fn validate_file_path(root: &Path, file_path: &str) -> Result<PathBuf, String> {\n    let path = Path::new(file_path);\n    if !path.is_absolute() {\n        return Err(\"File path must be absolute\".to_string());\n    }\n    let canonical = path\n        .canonicalize()\n        .map_err(|e| format!(\"Cannot resolve file path: {e}\"))?;\n    if !canonical.starts_with(root) {\n        return Err(\"File path is outside the project\".to_string());\n    }\n    if !canonical.is_file() {\n        return Err(\"File path is not a file\".to_string());\n    }\n    Ok(canonical)\n}",
  "validate_entry_id": "fn validate_entry_id(entry_id: &str) -> Result<(), String> {\n    if entry_id.is_empty() || !entry_id.chars().all(|ch| ch.is_ascii_digit() || ch == '-') {\n        return Err(\"Invalid local history entry id\".to_string());\n    }\n    Ok(())\n}",
  "relative_file_path": "fn relative_file_path(root: &Path, file: &Path) -> Result<String, String> {\n    let relative = file\n        .strip_prefix(root)\n        .map_err(|_| \"File path is outside the project\".to_string())?;\n    let parts = relative\n        .components()\n        .map(|component| component.as_os_str().to_string_lossy().into_owned())\n        .collect::<Vec<_>>();\n    Ok(parts.join(\"/\"))\n}",
  "history_dir_for_relative_path": "fn history_dir_for_relative_path(\n    layout: HistoryLayout,\n    root: &Path,\n    relative_path: &str,\n) -> PathBuf {\n    root.join(layout.dir)\n        .join(hex_encode(relative_path.as_bytes()))\n}",
  "entry_path": "fn entry_path(layout: HistoryLayout, root: &Path, relative_path: &str, entry_id: &str) -> PathBuf {\n    history_dir_for_relative_path(layout, root, relative_path).join(format!(\"{entry_id}.txt\"))\n}",
  "is_inside_history_dir": "fn is_inside_history_dir(layout: HistoryLayout, root: &Path, file: &Path) -> bool {\n    file.starts_with(root.join(layout.dir))\n}",
  "entry_id_timestamp_ms": "fn entry_id_timestamp_ms(entry_id: &str) -> Option<u64> {\n    entry_id.split('-').next()?.parse().ok()\n}",
  "entry_id_sequence": "fn entry_id_sequence(entry_id: &str) -> u16 {\n    entry_id\n        .split_once('-')\n        .and_then(|(_, suffix)| suffix.parse().ok())\n        .unwrap_or(0)\n}",
  "prune_history_dir": "pub fn prune_history_dir(history_dir: &Path, max_entries: usize) -> Result<(), String> {\n    let mut entries = fs::read_dir(history_dir)\n        .map_err(|e| e.to_string())?\n        .filter_map(Result::ok)\n        .filter_map(|entry| {\n            let path = entry.path();\n            if path.extension().and_then(|ext| ext.to_str()) != Some(SNAPSHOT_EXTENSION) {\n                return None;\n            }\n            let id = path.file_stem()?.to_str()?.to_string();\n            let timestamp = entry_id_timestamp_ms(&id)?;\n            Some((timestamp, entry_id_sequence(&id), path))\n        })\n        .collect::<Vec<_>>();\n    entries.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| right.1.cmp(&left.1)));\n    for (_, _, path) in entries.into_iter().skip(max_entries) {\n        fs::remove_file(path).map_err(|e| e.to_string())?;\n    }\n    Ok(())\n}",
  "hex_encode": "fn hex_encode(bytes: &[u8]) -> String {\n    const HEX: &[u8; 16] = b\"0123456789abcdef\";\n    let mut encoded = String::with_capacity(bytes.len() * 2);\n    for byte in bytes {\n        encoded.push(HEX[(byte >> 4) as usize] as char);\n        encoded.push(HEX[(byte & 0x0f) as usize] as char);\n    }\n    encoded\n}"
};

const expectedCommands = {
  "list_local_history": "pub async fn list_local_history(\n    project_path: String,\n    file_path: String,\n) -> Result<Vec<LocalHistoryEntry>, String> {\n    tauri::async_runtime::spawn_blocking(move || list_entries(&project_path, &file_path))\n        .await\n        .map_err(|e| e.to_string())?\n}",
  "read_local_history_entry": "pub async fn read_local_history_entry(\n    project_path: String,\n    file_path: String,\n    entry_id: String,\n) -> Result<LocalHistorySnapshot, String> {\n    tauri::async_runtime::spawn_blocking(move || read_entry(&project_path, &file_path, &entry_id))\n        .await\n        .map_err(|e| e.to_string())?\n}",
  "restore_local_history_entry": "pub async fn restore_local_history_entry(\n    project_path: String,\n    file_path: String,\n    entry_id: String,\n) -> Result<LocalHistorySnapshot, String> {\n    tauri::async_runtime::spawn_blocking(move || {\n        restore_entry(&project_path, &file_path, &entry_id)\n    })\n    .await\n    .map_err(|e| e.to_string())?\n}"
};

// Slice54 moved the write composition into core files::legacy_write; this
// gate keeps the save consumer's delegation and the history seam, while the
// composition body is pinned by legacy-write-invariants.
const expectedWriter = "pub async fn write_file_content(\n    path: String,\n    content: String,\n    project_path: String,\n) -> Result<(), String> {\n    tauri::async_runtime::spawn_blocking(move || {\n        aeroric_core::files::legacy_write::write_file_content(&project_path, &path, &content)\n    })\n    .await\n    .map_err(|e| e.to_string())?\n}";

const core = read("core/crates/aeroric-core/src/files/legacy_history.rs");
for (const [name, expected] of Object.entries(expectedLegacy)) assert.equal(fn(core, name), expected, name);
assert.match(core, /^use super::local_history::\{\s*HistoryLayout, LocalHistoryEntry, LocalHistorySnapshot, CODE_LAYOUT, MAX_SNAPSHOT_BYTES,\s*SNAPSHOT_EXTENSION,\s*\};$/m);
assert.match(core, /^use super::path_policy::validate_legacy_project_root as validate_project_root;$/m);
assert.match(core, /^use crate::clock::now_ms_u64;$/m);
assert.match(read("src-tauri/src/clock.rs"), /^#\[cfg\(test\)\]\npub\(crate\) fn now_ms_u64\(\) -> u64/m);
assert.match(core, /^use std::fs;$/m);
assert.match(core, /^use std::io::Write;$/m);
assert.match(core, /^use std::path::\{Path, PathBuf\};$/m);
assert.doesNotMatch(core, /\b(?:struct (?:HistoryLayout|LocalHistoryEntry|LocalHistorySnapshot)|const (?:CODE_LAYOUT|MAX_SNAPSHOT_BYTES|SNAPSHOT_EXTENSION))\b/);
assert.doesNotMatch(core, /tauri::|AppHandle|host_dispatch|load_settings|Command::|std::env|home_dir|force_snapshot\(/);
assert.match(read("core/crates/aeroric-core/src/files/mod.rs"), /^pub mod legacy_history;$/m);

const desktop = read("src-tauri/src/local_history.rs");
for (const [name, expected] of Object.entries(expectedCommands)) assert.equal(fn(desktop, name), expected, name);
for (const name of Object.keys(expectedLegacy)) assert.doesNotMatch(desktop, new RegExp(`^(?:pub(?:\\([^\\n)]*\\))?\\s+)?fn\\s+${name}\\b`, "m"));
assert.match(desktop, /^pub\(crate\) use aeroric_core::files::legacy_history::record_snapshot_before_write;$/m);
assert.match(desktop, /^use aeroric_core::files::legacy_history::\{list_entries, read_entry, restore_entry\};$/m);
assert.match(desktop, /^#\[cfg\(test\)\]\nuse aeroric_core::files::legacy_history::prune_history_dir;$/m);
assert.match(desktop, /^#\[cfg\(test\)\]\nuse aeroric_core::files::local_history::HISTORY_DIR;$/m);
assert.match(desktop, /^pub use aeroric_core::files::local_history::\{LocalHistoryEntry, LocalHistorySnapshot\};$/m);
assert.doesNotMatch(desktop, /\bstruct (?:HistoryLayout|LocalHistoryEntry|LocalHistorySnapshot)\b|const (?:CODE_LAYOUT|MAX_SNAPSHOT_BYTES|SNAPSHOT_EXTENSION)\b|host_dispatch/);
assert.equal(fn(read("src-tauri/src/fs.rs"), "write_file_content"), expectedWriter);
const formatter = fn(read("src-tauri/src/formatter.rs"), "format_file");
assert(formatter.indexOf("crate::local_history::record_snapshot_before_write(") < formatter.indexOf("let extension ="));
assert.match(formatter, /crate::local_history::record_snapshot_before_write\(&project_path, &file_string, ""\)\?;/);

const native = read("core/crates/aeroric-core/src/files/local_history.rs");
for (const [name, fields] of [
  ["HistoryLayout", [["dir", "&'static str"], ["max_entries", "usize"], ["min_interval_ms", "u64"]]],
  ["LocalHistoryEntry", [["id", "String"], ["file_path", "String"], ["relative_path", "String"], ["created_at_ms", "u64"], ["size", "u64"]]],
  ["LocalHistorySnapshot", [["entry", "LocalHistoryEntry"], ["content", "String"]]],
]) {
  const start = native.indexOf(`pub struct ${name} {`);
  assert(start >= 0);
  const code = native.slice(start, native.indexOf("\n}", start) + 2);
  assert.deepEqual([...code.matchAll(/pub (\w+): ([^,\n]+),/g)].map((m) => [m[1], m[2]]), fields);
  assert.doesNotMatch(code, /#\[serde/);
  if (name !== "HistoryLayout") assert.match(native.slice(Math.max(0, start - 140), start), /#\[derive\(Debug, Clone, PartialEq, Eq, serde::Serialize\)\]\n#\[serde\(rename_all = "camelCase"\)\]\n$/);
}
for (const [name, value] of [
  ["HISTORY_DIR", '".aeroric/local-history"'], ["SNAPSHOT_EXTENSION", '"txt"'],
  ["MAX_SNAPSHOT_BYTES", "2 * 1024 * 1024"], ["MAX_LIST_ENTRIES", "100"],
]) assert(native.includes(`pub const ${name}: ${name.startsWith("MAX_") ? (name === "MAX_LIST_ENTRIES" ? "usize" : "u64") : "&str"} = ${value};`));
assert.match(native, /pub const CODE_LAYOUT: HistoryLayout = HistoryLayout \{\s*dir: HISTORY_DIR,\s*max_entries: MAX_LIST_ENTRIES,\s*min_interval_ms: 0,\s*\};/);
assert.match(fn(native, "history_dir_for_relative_path"), /root\.join\(layout\.dir\)\.join\(relative\)/);
for (const name of ["record_snapshot", "list_entries", "read_entry", "restore_entry", "discard_history"]) assert.doesNotMatch(fn(native, name), /legacy_history/);
assert.match(read("core/crates/aeroric-host/src/file_services2.rs"), /use aeroric_core::files::\{format as file_format, local_history as history\};/);
assert.match(read("core/crates/aeroric-core/src/notebook/snapshots.rs"), /use crate::files::local_history::\{self, HistoryLayout, LocalHistoryEntry, LocalHistorySnapshot\};/);

const mapping = JSON.parse(read("docs/native/command-map.json"));
assert.equal(mapping.modules.local_history.core_service, "files::legacy_history");
const mapped = mapping.commands;
const registration = read("src-tauri/src/lib.rs");
for (const name of Object.keys(expectedCommands)) {
  const row = mapped.find((row) => row.command === name);
  assert.equal(row?.rust_path, `local_history::${name}`);
  assert.equal(row?.core_service, "files::legacy_history");
  assert.equal(row?.rpc_method, `files.${name}`);
  assert.deepEqual(row?.params, ["project_path: String", "file_path: String", ...(name === "list_local_history" ? [] : ["entry_id: String"])]);
  assert.match(registration, new RegExp(`^\\s*local_history::${name},$`, "m"));
}
const tests = read("src-tauri/src/local_history/legacy_history_tests.rs");
assert.match(desktop, /^#\[cfg\(test\)\]\n#\[cfg\(unix\)\]\nmod legacy_history_tests;$/m);
assert.equal(tests.match(/#\[test\]/g)?.length, 8);
for (const name of Object.keys(expectedCommands)) assert.match(tests, new RegExp(`super::${name}\\(`));
assert.match(tests, /crate::fs::write_file_content\(/);
assert.match(tests, /super::record_snapshot_before_write\(/);
assert.match(tests, /Command::new\(std::env::current_exe\(\)\.unwrap\(\)\)/);
assert.match(tests, /\.env_clear\(\)/);
assert.match(tests, /\.current_dir\(&root\.0\)/);
assert.match(tests, /stdout\.contains\("1 passed; 0 failed; 0 ignored"\)/);
assert.match(tests, /child\.kill\(\)\.unwrap\(\)/);
assert.match(tests, /child\.wait_with_output\(\)\.unwrap\(\)/);
assert.match(tests, /assert_eq!\(snapshot\(root\), before/);
assert.doesNotMatch(tests, /#\[ignore\]|trash::|(?:super|crate::fs)::delete_path\(|(?:super|crate::formatter)::format_file\(/);
for (const name of ["records_snapshots_before_text_changes", "restores_snapshot_and_records_current_content", "rejects_history_entry_traversal", "prunes_snapshot_files_beyond_retention_limit"]) assert(desktop.includes(`fn ${name}()`));
const direct = read("core/crates/aeroric-core/tests/legacy_history.rs");
assert.equal(direct.match(/#\[test\]/g)?.length, 8);
assert.match(direct, /use aeroric_core::files::\{legacy_history as history, local_history as native\};/);
for (const name of ["record_snapshot_before_write", "list_entries", "list_entries_in", "read_entry", "read_entry_in", "restore_entry", "prune_history_dir"]) assert(direct.includes(`history::${name}(`));
assert.match(direct, /scratch::Scratch::new\(/);
assert.doesNotMatch(direct, /#\[ignore\]|Command::|trash::|home_dir/);
console.log("legacy-history-invariants: exact21 legacy functions, shared DTO/layout/root/clock, unchanged actual workers/save and distinct native/host/notebook policy");
