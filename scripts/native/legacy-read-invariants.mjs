#!/usr/bin/env node
// Slice46: preserve the actual legacy query contracts, not native/frame aliases.
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

const core = read("core/crates/aeroric-core/src/files/legacy_read.rs");
assert.equal(core.match(/pub struct ImagePreviewData\b/g)?.length, 1);
assert.match(core, /#\[derive\(serde::Serialize\)\]\s*#\[serde\(rename_all = "camelCase"\)\]\s*pub struct ImagePreviewData/);
const modelStart = core.indexOf("pub struct ImagePreviewData");
const model = core.slice(modelStart, core.indexOf("\n}", modelStart) + 2);
assert.deepEqual(
  [...model.matchAll(/pub (\w+): ([^,\n]+),/g)].map((m) => [m[1], m[2]]),
  [["data_url", "String"], ["mime_type", "String"], ["byte_length", "u64"]],
);
assert.doesNotMatch(model, /#\[serde/);
assert.match(core, /pub const MAX_IMAGE_PREVIEW_BYTES: u64 = 10 \* 1024 \* 1024;/);
const mime = fn(core, "previewable_image_mime_type");
assert.match(mime, /path: &Path\) -> Option<&'static str>/);
assert.match(mime, /path\.extension\(\)\?\.to_str\(\)\?\.to_ascii_lowercase\(\)/);
assert.deepEqual(
  [...mime.matchAll(/"([^"]+)"/g)].map((m) => m[1]),
  ["png", "image/png", "jpg", "jpeg", "image/jpeg", "gif", "image/gif",
    "webp", "image/webp", "bmp", "image/bmp", "svg", "image/svg+xml"],
);
assert.match(mime, /_ => None/);

const text = fn(core, "read_file_content");
assert.match(text, /^pub fn read_file_content\(validated_path: &Path\) -> Result<String, String>/);
assert.match(text, /let file = std::fs::File::open\(validated_path\)\.map_err\(\|e\| e\.to_string\(\)\)\?;/);
assert.match(text, /let meta = file\.metadata\(\)\.map_err\(\|e\| e\.to_string\(\)\)\?;/);
assert.match(text, /if meta\.len\(\) > 2 \* 1024 \* 1024/);
assert.match(text, /"File too large \(\{:\.1\} MB\)",\s*meta\.len\(\) as f64 \/ 1024\.0 \/ 1024\.0/);
assert.match(text, /let mut buf = String::with_capacity\(meta\.len\(\) as usize\)/);
assert.match(text, /std::io::BufReader::new\(file\)\s*\.read_to_string\(&mut buf\)\s*\.map_err\(\|e\| e\.to_string\(\)\)\?;/);
ordered(text, ["let file =", "let meta =", "if meta.len()", "let mut buf =", "BufReader::new(file)", ".read_to_string(&mut buf)", "Ok(buf)"]);

const image = fn(core, "read_image_preview");
assert.match(image, /^pub fn read_image_preview\(validated_path: &Path\) -> Result<ImagePreviewData, String>/);
assert.match(image, /let mime_type = previewable_image_mime_type\(validated_path\)\s*\.ok_or_else\(\|\| "Unsupported image format"\.to_string\(\)\)\?;/);
assert.match(image, /let file = std::fs::File::open\(validated_path\)\.map_err\(\|e\| e\.to_string\(\)\)\?;/);
assert.match(image, /let meta = file\.metadata\(\)\.map_err\(\|e\| e\.to_string\(\)\)\?;/);
assert.match(image, /if meta\.len\(\) > MAX_IMAGE_PREVIEW_BYTES/);
assert.match(image, /"Image too large \(\{:\.1\} MB\)",\s*meta\.len\(\) as f64 \/ 1024\.0 \/ 1024\.0/);
assert.match(image, /let mut bytes = Vec::with_capacity\(meta\.len\(\) as usize\)/);
assert.match(image, /std::io::BufReader::new\(file\)\s*\.read_to_end\(&mut bytes\)\s*\.map_err\(\|e\| e\.to_string\(\)\)\?;/);
assert.match(image, /data_url: format!\(\s*"data:\{\};base64,\{\}",\s*mime_type,\s*base64::engine::general_purpose::STANDARD\.encode\(bytes\)/);
assert.match(image, /mime_type: mime_type\.to_string\(\),\s*byte_length: meta\.len\(\),/);
ordered(image, ["let mime_type =", "let file =", "let meta =", "if meta.len()", "let mut bytes =", ".read_to_end(&mut bytes)", "Ok(ImagePreviewData", "byte_length: meta.len()"]);
for (const code of [mime, text, image]) {
  assert.doesNotMatch(code, /host_dispatch|load_settings|home_dir|std::env|Command::|\.take\(|\.truncate\(|\.trim\(|preview_decision|read_write::|validate_path_within|\.canonicalize\(|MAX_FRAME/);
}
assert.match(read("core/crates/aeroric-core/src/files/mod.rs"), /^pub mod legacy_read;$/m);

const desktop = read("src-tauri/src/fs.rs");
assert.equal(desktop.match(/^pub\(crate\) use aeroric_core::files::legacy_read::ImagePreviewData;$/gm)?.length, 1);
assert.doesNotMatch(desktop, /struct ImagePreviewData\b|const MAX_IMAGE_PREVIEW_BYTES\b|^fn previewable_image_mime_type\b/m);
for (const [caller, service, policy] of [
  ["read_file_content_with_policy", "read_file_content", "allow_symlink_escape"],
  ["read_image_preview", "read_image_preview", "true"],
]) {
  const code = fn(desktop, caller);
  const guard = `let validated_path = validate_path_within(&path, &project_path, ${policy})?;`;
  ordered(code, [guard, "tauri::async_runtime::spawn_blocking(move || {",
    `aeroric_core::files::legacy_read::${service}(&validated_path)`, ".await"]);
  assert.match(code, new RegExp(`spawn_blocking\\(move \\|\\| \\{\\s*aeroric_core::files::legacy_read::${service}\\(&validated_path\\)\\s*\\}\\)`));
  assert.equal(code.match(/validate_path_within\(/g)?.length, 1);
  assert.doesNotMatch(code, /std::fs|BufReader|\.metadata\(|base64::|host_dispatch|\.trim\(|read_write::|preview_decision/);
}
assert.match(fn(desktop, "read_file_content"), /read_file_content_with_policy\(path, project_path, true\)\.await/);
assert.match(fn(desktop, "read_project_file_content"), /read_file_content_with_policy\(path, project_path, false\)\.await/);
assert.doesNotMatch(fn(read("core/crates/aeroric-core/src/files/read_write.rs"), "read_file_content"), /legacy_read/);
assert.doesNotMatch(fn(read("core/crates/aeroric-core/src/files/preview.rs"), "preview_decision"), /legacy_read/);

const mapped = JSON.parse(read("docs/native/command-map.json")).commands;
const registration = read("src-tauri/src/lib.rs");
for (const name of ["read_file_content", "read_image_preview"]) {
  assert.match(registration, new RegExp(`^\\s*fs::${name},$`, "m"));
  const row = mapped.find((entry) => entry.command === name);
  assert.equal(row?.rust_path, `fs::${name}`);
  assert.equal(row?.rpc_method, `files.${name}`);
  assert.deepEqual(row?.params, ["path: String", "project_path: String"]);
}
const tests = read("src-tauri/src/fs/legacy_read_tests.rs");
for (const name of ["read_file_content", "read_project_file_content", "read_image_preview"]) {
  assert.match(tests, new RegExp(`super::${name}\\(`));
}
assert.equal(tests.match(/#\[test\]/g)?.length, 7);
assert.match(tests, /\.env_clear\(\)/);
assert.match(tests, /stdout\.contains\("1 passed; 0 failed; 0 ignored"\)/);
assert.match(tests, /child\.kill\(\)\.unwrap\(\)/);
assert.match(tests, /child\.wait_with_output\(\)\.unwrap\(\)/);
console.log("legacy-read-invariants: exact shared legacy IO/MIME/wire/limits, unchanged guarded workers and native contracts");
