#!/usr/bin/env node
// Slice32: ownership/order ratchet, not a Rust parser or replacement for UDS tests.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");
const host = "core/crates/aeroric-host/";
const read = (file) => readFileSync(path.join(root, file), "utf8");
function body(source, name) {
  const start = source.search(new RegExp(`^\\s*(?:pub(?:\\([^)]*\\))?\\s+)?(?:async\\s+)?fn\\s+${name}\\b`, "m"));
  assert(start >= 0, `missing function ${name}`);
  const open = source.indexOf("{", start);
  let depth = 0;
  for (let i = open; i < source.length; i += 1) {
    if (source[i] === "{") depth += 1;
    if (source[i] === "}" && --depth === 0) return source.slice(open + 1, i);
  }
  assert.fail(`unclosed body ${name}`);
}

const owner = read(`${host}src/file_access.rs`);
const guard = body(owner, "write_refusal");
assert.match(guard, /context\s*\.require_write\(\)/);
assert.match(guard, /ErrorCode::IoError/);
assert.doesNotMatch(guard, /\.lease\(\)|acquire\(|request\.params|std::fs|std::env/);
assert.match(owner, /trash:\s*Arc::new\(aeroric_core::files::read_write::delete_path\)/);
assert.match(read(`${host}src/lib.rs`), /pub mod file_access;/);
const assembly = read(`${host}src/dispatcher.rs`);
assert.match(assembly, /pub files: crate::file_access::FilePorts/);
assert.match(assembly, /register_file_services\([\s\S]*?wiring\.files\.clone\(\)/);

const first = read(`${host}src/services.rs`);
const second = read(`${host}src/file_services2.rs`);
const directGuard = /if let Some\(frame\) = write_refusal\(&request, &context\) \{\s*return frame;\s*\}/;
const methods = [
  [first, "files_write_file", "find_project_root(", ["files.write_file"]],
  [first, "files_create", "find_project_root(", ["files.create_file", "files.create_directory"]],
  [first, "files_delete", "find_project_root(", ["files.delete_path"]],
  [first, "files_rename", "find_project_root(", ["files.rename_path"]],
  [second, "files_format_file", "project_root(", ["files.format_file"]],
  [second, "files_copy_paths", "project_root(", ["files.copy_paths_to_directory"]],
];
for (const [source, handler, lookup, names] of methods) {
  const code = body(source, handler);
  const check = code.indexOf("write_refusal(");
  assert.match(code, directGuard, `${handler} must return its refusal`);
  assert(check >= 0 && check < code.indexOf(lookup), `${handler}: check before root IO`);
  for (const name of names) assert(source.includes(`"${name}"`), `${name} must stay registered`);
}
const history = body(second, "files_local_history");
assert.match(history, /if matches!\(op, HistoryOp::Restore\) \{\s*if let Some\(frame\) = write_refusal\(&request, &context\) \{\s*return frame;\s*\}\s*\}/);
assert(history.indexOf("write_refusal(") < history.indexOf("project_root("));
assert(second.includes('"files.restore_local_history_entry"'));
for (const query of ["files_read_dir", "files_read_file"]) {
  assert.doesNotMatch(body(first, query), /write_refusal|require_write/);
}
assert.match(body(first, "files_delete"), /match trash\(&root, &relative\)/);
assert.match(body(first, "register_file_services"), /files_delete\(request, context, trash\)/);
const tests = read(`${host}tests/file_write_authority.rs`);
assert.match(tests, /start_read_only_with_wiring/);
assert.match(tests, /delete_path_with_trash/);
assert.match(tests, /fn each_mutation_rechecks_a_replaced_lease_on_the_same_connection/);
assert.match(tests, /fn read_only_format_cannot_even_create_a_snapshot/);
assert.match(tests, /fn read_only_file_directory_and_history_queries_remain_available/);
// Slice34: the project-scoped copy loop lives once in core. Its directory
// and leaf checks are distinct (exists() would miss a dangling leaf).
const fileCore = read("core/crates/aeroric-core/src/files/read_write.rs");
const policy = read("core/crates/aeroric-core/src/files/path_policy.rs");
const copy = body(fileCore, "copy_project_paths_to_directory");
assert.doesNotMatch(second, /\bfn copy_into\b|std::fs::copy\(|std::fs::create_dir_all\(/);
assert.match(body(second, "files_copy_paths"), /read_write::copy_project_paths_to_directory\(/);
for (const [check, write] of [
  ["validate_directory_destination(", "std::fs::create_dir_all("],
  ["std::fs::symlink_metadata(&to)", "std::fs::copy("],
]) {
  const offset = copy.indexOf(check);
  assert(offset >= 0 && offset < copy.indexOf(write), `copy: ${check} must precede ${write}`);
}
assert.match(copy, /Err\(error\) if error\.kind\(\) == std::io::ErrorKind::NotFound/);
assert.doesNotMatch(copy, /to\.exists\(\)/);
const destination = body(policy, "validate_directory_destination");
assert.match(destination, /std::fs::symlink_metadata\(ancestor\)/);
assert.match(destination, /resolved\.starts_with\(&canonical_root\)/);
assert.match(destination, /ancestor\s*\.parent\(\)/);
assert.doesNotMatch(destination, /create_dir|fs::write|fs::copy|\.exists\(\)/);
// Slice35: writing an existing leaf needs its own containment check; creation
// must reserve the directory entry, not follow exists() with a truncating write.
const write = body(fileCore, "write_file_content");
const size = write.indexOf("validate_content_size(");
const leaf = write.indexOf("resolve_file_write_destination(");
assert(size >= 0 && leaf > size && write.indexOf("std::fs::write(") > leaf);
assert.match(write, /std::fs::write\(&target, content\)/);
const writeDestination = body(policy, "resolve_file_write_destination");
assert.match(writeDestination, /resolve_parent_within\(root, relative\)/);
assert.match(writeDestination, /std::fs::symlink_metadata\(&target\)/);
assert.match(writeDestination, /canonical\.starts_with\(&canonical_root\)/);
assert.match(writeDestination, /Err\(error\) if error\.kind\(\) == std::io::ErrorKind::NotFound => Ok\(target\)/);
assert.doesNotMatch(writeDestination, /\.exists\(\)|fs::write|OpenOptions/);
const create = body(fileCore, "create_file");
assert.match(create, /OpenOptions::new\(\)\s*\.write\(true\)\s*\.create_new\(true\)\s*\.open\(&target\)/);
assert.match(create, /std::io::ErrorKind::AlreadyExists/);
assert.match(create, /"file already exists"/);
assert.doesNotMatch(create, /\.exists\(\)|fs::write/);
// Slice36: explicit remote/bad targets cannot be silently treated as local.
// This is projectId/canonical-root policy, not the definition exact-string rule.
const targetDecode = body(owner, "decode");
assert.match(targetDecode, /request\.params\.get\("target"\)/);
assert.match(targetDecode, /serde_json::from_value::<ProjectTarget>/);
assert.match(targetDecode, /ErrorCode::UnsupportedCapability/);
assert.match(targetDecode, /ErrorCode::InvalidArgument/);
assert.doesNotMatch(targetDecode, /std::fs|std::env|context|require_write|validate_project_root/);
const targetRoot = body(owner, "root_refusal");
assert.match(targetRoot, /validate_project_root\(local\)/);
assert.match(targetRoot, /Ok\(path\) if path == canonical_root => None/);
assert.doesNotMatch(targetRoot, /require_write|load_projects|create_dir|fs::write/);
for (const [source, handler, lookup, business] of [
  [first, "files_read_dir", "find_project_root(", "read_write::read_dir_entries("],
  [first, "files_read_file", "find_project_root(", "read_write::read_file_content("],
  [first, "files_write_file", "find_project_root(", "read_write::write_file_content("],
  [first, "files_create", "find_project_root(", "let result = if directory"],
  [first, "files_delete", "find_project_root(", "match trash("],
  [first, "files_rename", "find_project_root(", "read_write::rename_path("],
  [second, "files_format_file", "project_root(", "let snapshot ="],
  [second, "files_local_history", "project_root(", "path_policy::resolve_within("],
  [second, "files_copy_paths", "project_root(", "read_write::copy_project_paths_to_directory("],
]) {
  const code = body(source, handler);
  const decode = code.indexOf("FileTarget::decode(");
  const root = code.indexOf(lookup);
  const check = code.indexOf("target.root_refusal(");
  assert(decode >= 0 && root > decode && check > root && code.indexOf(business) > check,
    `${handler}: target decode before root, root assertion before business`);
  assert.match(code, /FileTarget::decode\(&request\) \{\s*Ok\(target\) => target,\s*Err\(frame\) => return \*frame,/);
  assert.match(code, /if let Some\(frame\) = target\.root_refusal\(&request, &(?:root|root_path)\) \{\s*return frame;/);
  const writer = code.indexOf("write_refusal(");
  if (writer >= 0) assert(decode < writer && writer < root, `${handler}: target before writer before root`);
}
// Slice37: params.target cannot override a stored remote/unaddressable location.
// Use the established core policy, never resolve_location's fallback on its own.
const recordGuard = body(owner, "require_local_record");
assert.match(recordGuard, /if !mutations::is_addressable\(record\) \{\s*return Err\(FileProjectError::InvalidLocation\);/);
assert.match(recordGuard, /if !mutations::resolve_location\(record\)\.is_local\(\) \{\s*return Err\(FileProjectError::UnsupportedLocation\);/);
const addressable = recordGuard.indexOf("mutations::is_addressable(");
const location = recordGuard.indexOf("mutations::resolve_location(");
assert(addressable >= 0 && location > addressable);
assert.doesNotMatch(recordGuard, /std::fs|std::env|load_projects|serde_json|request\.params|validate_project_root/);
const recordError = body(owner, "into_frame");
assert.match(recordError, /Self::NotFound\(message\) => RpcError::new\(ErrorCode::NotFound, message\)/);
assert.match(recordError, /ErrorCode::InvalidArgument/);
assert.match(recordError, /ErrorCode::UnsupportedCapability/);
assert.match(recordError, /\.with_error_detail\(&ErrorDetail \{\s*field: Some\("location"\.into\(\)\)/);
const resolverStart = second.indexOf("impl ProjectRootResolver for RecordRootResolver");
assert(resolverStart >= 0, "retain the production root-resolver port");
for (const lookup of [body(first, "find_project_root"), body(second.slice(resolverStart), "resolve")]) {
  const admission = lookup.indexOf("require_local_record(&record)?");
  const rawPath = lookup.indexOf('text(&record, "path")');
  assert(admission >= 0 && rawPath > admission, "stored location before raw path extraction");
}
for (const handler of ["files_read_dir", "files_read_file", "files_write_file", "files_create", "files_delete", "files_rename"]) {
  assert.match(body(first, handler), /Err\(error\) => return error\.into_frame\(&request\)/);
}
assert.match(body(second, "project_root"), /\.map_err\(\|error\| Box::new\(error\.into_frame\(request\)\)\)/);
assert.match(second, /Result<String, FileProjectError>/);
console.log("file-write-invariants: writer/explicit-target/stored-location admission, typed root errors and retained copy/write/create/query/trash behavior");
