#!/usr/bin/env node
// Slice53: equal DTO/selector/readonly validation, explicitly separate runners.
// Exact source probes complement frozen actual tests; they are not Rust mutants.
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");
const read = (file) => readFileSync(path.join(root, file), "utf8");
function fn(source, name) {
  const matches = [...source.matchAll(new RegExp(
    `^(?:pub(?:\\([^\\n)]*\\))?\\s+)?(?:async\\s+)?fn\\s+${name}\\b`, "gm",
  ))];
  assert.equal(matches.length, 1, `one actual ${name}`);
  const tail = source.slice(matches[0].index);
  const end = tail.search(/^}/m);
  assert(end >= 0, `closed ${name}`);
  return tail.slice(0, end + 1).replace(/^\s*\/\/[^\n]*\n/gm, "");
}
function struct(source, name) {
  const matches = [...source.matchAll(new RegExp(`^pub struct ${name} \\{`, "gm"))];
  assert.equal(matches.length, 1, `one shared ${name}`);
  const tail = source.slice(matches[0].index);
  const end = tail.search(/^}/m);
  assert(end >= 0);
  return tail.slice(0, end + 1);
}

const expectedCore = {
  "validate_file_path": "pub fn validate_file_path(project_root: &Path, file_path: &str) -> Result<PathBuf, String> {\n    let path = Path::new(file_path);\n    if !path.is_absolute() {\n        return Err(\"File path must be absolute\".to_string());\n    }\n    let canonical = path\n        .canonicalize()\n        .map_err(|e| format!(\"Cannot resolve file path: {e}\"))?;\n    if !canonical.starts_with(project_root) {\n        return Err(\"File path is outside the project\".to_string());\n    }\n    if !canonical.is_file() {\n        return Err(\"File path is not a file\".to_string());\n    }\n    Ok(canonical)\n}",
  "formatter_for_extension": "pub fn formatter_for_extension(extension: &str, file_path: &str) -> Option<FormatterCommand> {\n    let ext = extension.trim_start_matches('.').to_ascii_lowercase();\n    match ext.as_str() {\n        \"js\" | \"jsx\" | \"ts\" | \"tsx\" | \"css\" | \"scss\" | \"json\" | \"jsonc\" | \"md\" | \"mdx\" | \"html\"\n        | \"yaml\" | \"yml\" => Some(FormatterCommand {\n            program: \"pnpm\".to_string(),\n            args: vec![\n                \"exec\".to_string(),\n                \"prettier\".to_string(),\n                \"--write\".to_string(),\n                file_path.to_string(),\n            ],\n        }),\n        \"rs\" => Some(FormatterCommand {\n            program: \"rustfmt\".to_string(),\n            args: vec![file_path.to_string()],\n        }),\n        \"go\" => Some(FormatterCommand {\n            program: \"gofmt\".to_string(),\n            args: vec![\"-w\".to_string(), file_path.to_string()],\n        }),\n        \"py\" => Some(FormatterCommand {\n            program: \"ruff\".to_string(),\n            args: vec![\"format\".to_string(), file_path.to_string()],\n        }),\n        _ => None,\n    }\n}",
  "format_file": "pub fn format_file(\n    project_root: &Path,\n    file_path: &str,\n    snapshot_before: Option<&dyn Fn(&str) -> Result<(), String>>,\n) -> Result<FormatFileResult, String> {\n    let file = validate_file_path(project_root, file_path)?;\n    let file_string = file.to_string_lossy().into_owned();\n    if let Some(snapshot) = snapshot_before {\n        snapshot(&file_string)?;\n    }\n    let extension = file\n        .extension()\n        .and_then(|ext| ext.to_str())\n        .ok_or_else(|| \"No formatter is configured for this file type.\".to_string())?;\n    let formatter = formatter_for_extension(extension, &file_string)\n        .ok_or_else(|| \"No formatter is configured for this file type.\".to_string())?;\n    let mut cmd = Command::new(&formatter.program);\n    cmd.args(&formatter.args).current_dir(project_root);\n    let output = cmd\n        .output()\n        .map_err(|e| format!(\"Failed to run formatter: {e}\"))?;\n    if !output.status.success() {\n        let stderr = String::from_utf8_lossy(&output.stderr);\n        return Err(format!(\"Formatter failed: {stderr}\"));\n    }\n    Ok(FormatFileResult {\n        file_path: file_string,\n        command: format!(\"{} {}\", formatter.program, formatter.args.join(\" \")),\n    })\n}",
  "validate_size": "pub fn validate_size(file: &Path) -> Result<(), String> {\n    let metadata = std::fs::metadata(file).map_err(|e| e.to_string())?;\n    if metadata.len() > path_policy::MAX_FILE_BYTES {\n        return Err(format!(\n            \"file exceeds the {} byte limit\",\n            path_policy::MAX_FILE_BYTES\n        ));\n    }\n    Ok(())\n}"
};
const expectedWorker = "pub async fn format_file(\n    project_path: String,\n    file_path: String,\n) -> Result<FormatFileResult, String> {\n    tauri::async_runtime::spawn_blocking(move || {\n        let root = validate_project_root(&project_path)?;\n        let file = validate_file_path(&root, &file_path)?;\n        let file_string = file.to_string_lossy().into_owned();\n        crate::local_history::record_snapshot_before_write(&project_path, &file_string, \"\")?;\n        let extension = file\n            .extension()\n            .and_then(|ext| ext.to_str())\n            .ok_or_else(|| \"No formatter is configured for this file type.\".to_string())?;\n        let formatter = formatter_for_extension(extension, &file_string)\n            .ok_or_else(|| \"No formatter is configured for this file type.\".to_string())?;\n        let mut cmd = Command::new(&formatter.program);\n        crate::subprocess::configure_background_command(&mut cmd);\n        let output = cmd\n            .args(&formatter.args)\n            .current_dir(&root)\n            .envs(crate::app_settings::get_login_shell_env().iter().cloned())\n            .output()\n            .map_err(|e| format!(\"Failed to run formatter: {e}\"))?;\n        if !output.status.success() {\n            let stderr = String::from_utf8_lossy(&output.stderr);\n            return Err(format!(\"Formatter failed: {stderr}\"));\n        }\n        Ok(FormatFileResult {\n            file_path: file_string,\n            command: format!(\"{} {}\", formatter.program, formatter.args.join(\" \")),\n        })\n    })\n    .await\n    .map_err(|e| e.to_string())?\n}";
const expectedHostWorker = "fn files_format_file(\n    request: Request,\n    context: Arc<CoreContext>,\n    resolver: Arc<dyn ProjectRootResolver>,\n) -> Frame {\n    let path = match param(&request, \"path\") {\n        Ok(value) => value,\n        Err(frame) => return *frame,\n    };\n    let target = match FileTarget::decode(&request) {\n        Ok(target) => target,\n        Err(frame) => return *frame,\n    };\n    if let Some(frame) = write_refusal(&request, &context) {\n        return frame;\n    }\n    let root_path = match project_root(&request, &context, &resolver) {\n        Ok(root) => root,\n        Err(frame) => return *frame,\n    };\n    if let Some(frame) = target.root_refusal(&request, &root_path) {\n        return frame;\n    }\n    let snapshot = |absolute: &str| -> Result<(), String> {\n        let file = std::path::PathBuf::from(absolute);\n        history::force_snapshot(\n            history::CODE_LAYOUT,\n            &root_path,\n            &file,\n            history::now_ms_u64(),\n        )\n        .map(|_| ())\n    };\n    let result = file_format::format_file(&root_path, &path, Some(&snapshot))\n        .and_then(|result| serde_json::to_value(result).map_err(|error| error.to_string()));\n    value_frame(&request, result)\n}";
const oldTestsSha256 = "1a078f86f84b50f8f404efd9db981829c61b42cfe4ce15610495f1d6bbe19e27";

const core = read("core/crates/aeroric-core/src/files/format.rs");
for (const [name, expected] of Object.entries(expectedCore)) assert.equal(fn(core, name), expected, `native/shared ${name}`);
assert.equal(struct(core, "FormatterCommand"), "pub struct FormatterCommand {\n    pub program: String,\n    pub args: Vec<String>,\n}");
assert.equal(struct(core, "FormatFileResult"), "pub struct FormatFileResult {\n    pub file_path: String,\n    pub command: String,\n}");
assert.match(core, /#\[derive\(Debug, Clone, PartialEq, Eq\)\]\npub struct FormatterCommand/);
assert.match(core, /#\[derive\(Debug, Clone, serde::Serialize\)\]\n#\[serde\(rename_all = "camelCase"\)\]\npub struct FormatFileResult/);
assert.match(core, /^use super::path_policy;$/m);
assert.match(core, /^use std::path::\{Path, PathBuf\};$/m);
assert.match(core, /^use std::process::Command;$/m);
assert.doesNotMatch(core, /tauri::|AppHandle|host_dispatch|load_settings|home_dir/);

const desktop = read("src-tauri/src/formatter.rs");
assert.equal(fn(desktop, "format_file"), expectedWorker);
assert.equal(fn(desktop, "formatter_for_extension"), "pub fn formatter_for_extension(extension: &str, file_path: &str) -> Option<FormatterCommand> {\n    aeroric_core::files::format::formatter_for_extension(extension, file_path)\n}");
assert.match(desktop, /^use crate::path_guard::validate_project_root;$/m);
assert.match(desktop, /^use aeroric_core::files::format::validate_file_path;$/m);
assert.match(desktop, /^pub use aeroric_core::files::format::\{FormatFileResult, FormatterCommand\};$/m);
assert.match(desktop, /^use std::process::Command;$/m);
assert.match(desktop, /^#\[tauri::command\]\npub async fn format_file\(/m);
assert.doesNotMatch(desktop, /^(?:pub\s+)?(?:struct (?:FormatFileResult|FormatterCommand)|fn validate_file_path)\b/m);
assert.doesNotMatch(desktop, /trim_start_matches|to_ascii_lowercase|host_dispatch/);
const originalTests = desktop.match(/#\[cfg\(test\)\]\nmod tests \{[\s\S]*?\n}/)?.[0];
assert(originalTests, "old three tests remain at their original names");
assert.equal(createHash("sha256").update(originalTests).digest("hex"), oldTestsSha256);
assert.match(desktop, /^#\[cfg\(test\)\]\n#\[cfg\(unix\)\]\nmod legacy_formatter_tests;$/m);

const host = read("core/crates/aeroric-host/src/file_services2.rs");
const hostWorker = fn(host, "files_format_file");
assert.equal(hostWorker, expectedHostWorker);
assert.match(host, /use aeroric_core::files::\{format as file_format, local_history as history\};/);
assert.match(hostWorker, /history::force_snapshot\(/);
assert.match(hostWorker, /file_format::format_file\(&root_path, &path, Some\(&snapshot\)\)/);
assert.doesNotMatch(hostWorker, /legacy_history|get_login_shell_env/);

const tests = read("src-tauri/src/formatter/legacy_formatter_tests.rs");
assert.equal(tests.match(/#\[test\]/g)?.length, 8);
for (const name of [
  "selects_complete_extension_vocabulary_and_literal_argv", "format_result_keeps_exact_camelcase_wire_and_type_traits",
  "validator_preserves_error_order_and_canonical_path", "validator_keeps_alias_and_containment_policy",
  "command_rejects_invalid_roots_and_files_before_history", "command_records_legacy_snapshot_before_extension_refusal",
  "command_preserves_history_skips_before_extension_refusal", "command_history_failure_precedes_extension_and_process_work",
]) assert(tests.includes(`fn ${name}()`), name);
assert.match(tests, /\.block_on\(super::format_file\(/);
assert.match(tests, /super::formatter_for_extension\(/);
assert.match(tests, /super::validate_file_path\(/);
assert.match(tests, /serde_json::to_value\(result\.clone\(\)\)/);
assert.match(tests, /Command::new\(std::env::current_exe\(\)\.unwrap\(\)\)/);
assert.match(tests, /\.env_clear\(\)/);
assert.match(tests, /\.env\("HOME", &root\.0\)/);
assert.match(tests, /\.env\("SHELL", root\.0\.join\("bin\/no-shell"\)\)/);
assert.match(tests, /\.current_dir\(&root\.0\)/);
assert.match(tests, /stdout\.contains\("1 passed; 0 failed; 0 ignored"\)/);
assert.match(tests, /child\.kill\(\)\.unwrap\(\)/);
assert.match(tests, /child\.wait_with_output\(\)\.unwrap\(\)/);
assert.match(tests, /assert_snapshot\(root, before\)/);
assert.match(tests, /assert_only_snapshot_added\(/);
assert.doesNotMatch(tests, /#\[ignore\]|trash::|get_login_shell_env\(|(?:crate|super)::local_history::|Command::new\("/);

const direct = read("core/crates/aeroric-core/tests/formatter_policy.rs");
assert.equal(direct.match(/#\[test\]/g)?.length, 5);
assert.match(direct, /^use aeroric_core::files::format;$/m);
assert.match(direct, /scratch::Scratch::new\(/);
for (const name of ["formatter_for_extension", "validate_file_path", "format_file"]) assert(direct.includes(`format::${name}(`));
assert.match(direct, /assert_unchanged\(root, before\)/);
assert.doesNotMatch(direct, /#\[ignore\]|Command::|trash::|home_dir/);

const map = JSON.parse(read("docs/native/command-map.json"));
const row = map.commands.find((row) => row.command === "format_file");
assert.equal(row?.rust_path, "formatter::format_file");
assert.equal(row?.core_service, "files::format");
assert.equal(row?.rpc_method, "files.format_file");
assert.deepEqual(row?.params, ["project_path: String", "file_path: String"]);
assert.equal(map.modules.formatter.core_service, "files::format");
assert.match(read("src-tauri/src/lib.rs"), /^\s*formatter::format_file,$/m);
console.log("formatter-policy-invariants: equal shared DTO/validator/selector, unchanged actual/native workers, old3/frozen8/direct5 and distinct host/history/environment policy");
