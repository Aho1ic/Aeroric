import { test } from "node:test";
import assert from "node:assert/strict";
import { cpSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { check, extractSignature, freeze, parseCommandDefinitions, parseRegistry, renderParams } from "./inventory.mjs";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..");
const mapPath = path.join(repoRoot, "docs/native/command-map.json");

function tempTree(mutateMap) {
  // Operates on a throwaway copy so a negative test can never damage the real
  // frozen inventory (P1-04 Action 7).
  const dir = mkdtempSync(path.join(os.tmpdir(), "aeroric-inventory-"));
  mkdirSync(path.join(dir, "docs/native"), { recursive: true });
  mkdirSync(path.join(dir, "scripts/native"), { recursive: true });
  mkdirSync(path.join(dir, "src-tauri/src"), { recursive: true });
  cpSync(path.join(repoRoot, "src-tauri/src/lib.rs"), path.join(dir, "src-tauri/src/lib.rs"));
  cpSync(path.join(repoRoot, "docs/native/feature-matrix.json"), path.join(dir, "docs/native/feature-matrix.json"));
  cpSync(path.join(repoRoot, "scripts/native/checks.json"), path.join(dir, "scripts/native/checks.json"));
  const map = JSON.parse(readFileSync(mapPath, "utf8"));
  mutateMap?.(map);
  writeFileSync(path.join(dir, "docs/native/command-map.json"), JSON.stringify(map));
  return { dir, cleanup: () => rmSync(dir, { recursive: true, force: true }) };
}

test("the committed inventory matches the live registration site", () => {
  const result = check({ repoRoot });
  assert.equal(result.ok, true, result.problems.slice(0, 5).join("\n"));
  assert.equal(result.registered, 660);
  assert.equal(result.mapped, 660);
  assert.equal(result.features, 26);
  assert.equal(result.events, 38);
  assert.equal(result.panels, 19);
  assert.equal(result.definitionsResolved, 660);
  assert.equal(result.dynamicSites, 5);
});

test("a definition resolved to the wrong file must fail", () => {
  const { dir, cleanup } = tempTree((map) => {
    const victim = map.commands.find((c) => c.rust_path === "git::git_status");
    victim.definition_resolved = "src-tauri/src/lsp.rs:1";
  });
  try {
    const result = check({ repoRoot: dir, sourceRoot: repoRoot });
    assert.equal(result.ok, false);
    assert.ok(result.problems.some((p) => p.includes("not that fn's declaration") || p.includes("resolved file missing")), result.problems.join("|"));
  } finally { cleanup(); }
});

test("an unenumerated dynamic command name must fail", () => {
  const { dir, cleanup } = tempTree((map) => {
    const site = map.dynamic_invoke_sites.find((s) => s.file === "src/lib/api/git.ts");
    site.possible_commands.push("remote_git_totally_new_thing");
  });
  try {
    const result = check({ repoRoot: dir, sourceRoot: repoRoot });
    assert.equal(result.ok, false);
    assert.ok(result.problems.some((p) => p.includes("unregistered names drifted")), result.problems.join("|"));
  } finally { cleanup(); }
});

test("a dynamic site whose declared gap silently disappears must also fail", () => {
  const { dir, cleanup } = tempTree((map) => {
    map.dynamic_invoke_sites.find((s) => s.file === "src/lib/api/git.ts").known_unregistered = [];
  });
  try {
    const result = check({ repoRoot: dir, sourceRoot: repoRoot });
    assert.equal(result.ok, false, "shrinking a disclosed gap must not pass quietly");
    assert.ok(result.problems.some((p) => p.includes("unregistered names drifted")));
  } finally { cleanup(); }
});

test("a vanished dynamic site file must fail", () => {
  const { dir, cleanup } = tempTree((map) => {
    map.dynamic_invoke_sites.find((s) => s.file === "src/lib/api/lsp.ts").file = "src/lib/api/does-not-exist.ts";
  });
  try {
    const result = check({ repoRoot: dir, sourceRoot: repoRoot });
    assert.equal(result.ok, false);
    assert.ok(result.problems.some((p) => p.includes("file gone")), result.problems.join("|"));
  } finally { cleanup(); }
});

test("a new event channel that was never frozen must fail", () => {
  const { dir, cleanup } = tempTree((map) => {
    map.events = map.events.filter((e) => e.name !== "task-status");
    map.meta.counts.events = map.events.length;
  });
  try {
    const result = check({ repoRoot: dir, sourceRoot: repoRoot });
    assert.equal(result.ok, false);
    assert.ok(result.problems.some((p) => p.includes("event emitted/listened but not frozen")), result.problems.join("|"));
  } finally { cleanup(); }
});

test("an orphan event left untriaged must fail", () => {
  const { dir, cleanup } = tempTree((map) => {
    const orphan = map.events.find((e) => e.name === "dsh-host-stream-error");
    assert.equal(orphan.orphan, "emitted-without-listener");
    orphan.disposition = "untriaged";
  });
  try {
    const result = check({ repoRoot: dir, sourceRoot: repoRoot });
    assert.equal(result.ok, false);
    assert.ok(result.problems.some((p) => p.includes("no disposition")), result.problems.join("|"));
  } finally { cleanup(); }
});

test("a settings panel missing from the frozen list must fail", () => {
  const { dir, cleanup } = tempTree((map) => {
    map.panels = map.panels.filter((p) => p.key !== "dsh-plugins");
    map.meta.counts.panels = map.panels.length;
  });
  try {
    const result = check({ repoRoot: dir, sourceRoot: repoRoot });
    assert.equal(result.ok, false);
    assert.ok(result.problems.some((p) => p.includes("settings panel not frozen: dsh-plugins")), result.problems.join("|"));
  } finally { cleanup(); }
});

test("a panel with no native target must fail", () => {
  const { dir, cleanup } = tempTree((map) => { map.panels.find((p) => p.key === "theme").target_scene = null; });
  try {
    const result = check({ repoRoot: dir, sourceRoot: repoRoot });
    assert.equal(result.ok, false);
    assert.ok(result.problems.some((p) => p.includes("missing target_scene")), result.problems.join("|"));
  } finally { cleanup(); }
});

test("drift in the recorded dynamic emit sites must fail", () => {
  const { dir, cleanup } = tempTree((map) => { map.event_dynamic_emit_sites = []; });
  try {
    const result = check({ repoRoot: dir, sourceRoot: repoRoot });
    assert.equal(result.ok, false);
    assert.ok(result.problems.some((p) => p.includes("dynamic emit sites drifted")), result.problems.join("|"));
  } finally { cleanup(); }
});

test("dropping one mapping from a temp copy must fail", () => {
  const { dir, cleanup } = tempTree((map) => {
    const index = map.commands.findIndex((c) => c.rust_path === "git::git_status");
    assert.ok(index > 0, "fixture expected git::git_status to exist");
    map.commands.splice(index, 1);
    map.meta.counts.registered_entries = map.commands.length;
  });
  try {
    const result = check({ repoRoot: dir, sourceRoot: repoRoot });
    assert.equal(result.ok, false, "a mapped-away command must not pass silently");
    assert.ok(result.problems.some((p) => p.includes("registered but not mapped")), result.problems.join("|"));
  } finally { cleanup(); }
});

test("an invented mapping with no registration must fail", () => {
  const { dir, cleanup } = tempTree((map) => {
    map.commands.push({ rust_path: "ghost::does_not_exist", check_module: "skeleton", feature_ids: ["F01"], category: "domain_rpc", definition_candidates: [] });
    map.meta.counts.registered_entries = map.commands.length;
  });
  try {
    const result = check({ repoRoot: dir, sourceRoot: repoRoot });
    assert.equal(result.ok, false);
    assert.ok(result.problems.some((p) => p.includes("mapped but not registered")));
  } finally { cleanup(); }
});

test("a check module absent from checks.json must fail", () => {
  const { dir, cleanup } = tempTree((map) => { map.commands[0].check_module = "not-registered-anywhere"; });
  try {
    const result = check({ repoRoot: dir, sourceRoot: repoRoot });
    assert.equal(result.ok, false);
    assert.ok(result.problems.some((p) => p.includes("registered in neither checks.json.modules nor plannedModules")));
  } finally { cleanup(); }
});

test("an unknown feature id must fail", () => {
  const { dir, cleanup } = tempTree((map) => { map.commands[3].feature_ids = ["F99"]; });
  try {
    const result = check({ repoRoot: dir, sourceRoot: repoRoot });
    assert.equal(result.ok, false);
    assert.ok(result.problems.some((p) => p.includes("absent from feature-matrix")));
  } finally { cleanup(); }
});

test("parseRegistry stops at the block end and ignores comment lines", () => {
  const lib = readFileSync(path.join(repoRoot, "src-tauri/src/lib.rs"), "utf8");
  const parsed = parseRegistry(lib);
  assert.equal(parsed.entries.length, 660);
  assert.ok(parsed.entries.every((e) => e.rustPath && !e.rustPath.startsWith("//")));
  assert.throws(() => parseRegistry("no macro here"), /generate_handler/);
});

test("an unregistered frontend invoke literal must fail", () => {
  // The literal surface is clean today (422 sites, 0 unknown); this proves the
  // guard fires rather than merely reporting zero.
  const { dir, cleanup } = tempTree((map) => {
    map.commands.find((c) => c.rust_path === "git::git_status").reachability = { frontend_literal: false, other_package_literal: false, rust_internal_refs: 0, registered: true };
    map.commands.find((c) => c.rust_path === "git::git_status").reachable = false;
  });
  try {
    const result = check({ repoRoot: dir, sourceRoot: repoRoot });
    assert.equal(result.ok, false);
    assert.ok(result.problems.some((p) => p.includes("reachability.reachable drifted")), result.problems.join("|"));
  } finally { cleanup(); }
});

test("a changed unreachable total must fail", () => {
  const { dir, cleanup } = tempTree((map) => { map.meta.counts.unreachable = 12; });
  try {
    const result = check({ repoRoot: dir, sourceRoot: repoRoot });
    assert.equal(result.ok, false);
    assert.ok(result.problems.some((p) => p.includes("meta.counts.unreachable")), result.problems.join("|"));
  } finally { cleanup(); }
});

test("attribute pairing beats a raw grep count, which overcounts", () => {
  // Raw `#[tauri::command]` line matches include comments, doc links and the
  // guard test's own assertion strings; pairing them to fn signatures does not.
  const readFile = (p) => readFileSync(p, "utf8");
  const definitions = parseCommandDefinitions(path.join(repoRoot, "src-tauri", "src"), readFile);
  assert.equal(definitions.size, 660, "pairing must recover exactly the registered set");

  const scan = (dir) => readdirSync(dir, { withFileTypes: true }).reduce((total, entry) => {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) return total + scan(full);
    if (!entry.name.endsWith(".rs")) return total;
    return total + readFileSync(full, "utf8").split("\n").filter((line) => line.includes("#[tauri::command]")).length;
  }, 0);
  const rawLines = scan(path.join(repoRoot, "src-tauri", "src"));
  assert.ok(rawLines > definitions.size, `expected raw matches (${rawLines}) to overcount real commands (${definitions.size})`);
});

// ---- freeze() merge semantics, on a synthetic tree with injected extraction ----

function freezeFixture(overrides = {}) {
  const dir = mkdtempSync(path.join(os.tmpdir(), "aeroric-freeze-"));
  mkdirSync(path.join(dir, "docs/native"), { recursive: true });
  const map = {
    meta: { counts: { events: 1, panels: 1 } },
    events: [{
      name: "agent-tool-install-progress",
      emitters: ["src-tauri/src/agent_ops.rs:88"],
      listeners: [],
      orphan: "emitted-without-listener",
      disposition: "deferred-to-P3-02",
      gap_id: "G12",
      note: "P3-02 must either give it a consumer or drop the emit",
      review_owner: "18502553797",
      ...overrides.event,
    }],
    panels: [{
      key: "general",
      via: "literal",
      at: "src/components/app-settings/panelRegistry.tsx:12",
      target_scene: "SettingsGeneral",
      maps_commands: ["get_settings"],
      ...overrides.panel,
    }],
    commands: [],
  };
  writeFileSync(path.join(dir, "docs/native/command-map.json"), JSON.stringify(map, null, 2) + "\n");
  return { dir, path: path.join(dir, "docs/native/command-map.json") };
}

function runFreeze(dir, live) {
  return freeze({
    repoRoot: dir,
    live: {
      // Mirrors extractEvents()'s real output shape, including the null triage
      // placeholders it seeds — a stub without them let a data-losing merge pass.
      events: [{ name: "agent-tool-install-progress", emitters: ["src-tauri/src/agent_ops.rs:90"], listeners: [], orphan: null, rust_consts: [], ts_consts: [], dynamic_emitters: [], dynamic_listeners: [], disposition: null, gap_id: null }],
      panels: [{ key: "general", via: "literal", at: "src/components/app-settings/panelRegistry.tsx:14" }],
      unresolved: { dynamic_emitters: [], dynamic_listeners: [] },
      ...live,
    },
  });
}

test("a re-freeze must never erase a human event note", () => {
  const { dir } = freezeFixture();
  runFreeze(dir);
  const frozen = JSON.parse(readFileSync(path.join(dir, "docs/native/command-map.json"), "utf8"));
  assert.equal(frozen.events[0].note, "P3-02 must either give it a consumer or drop the emit");
  assert.equal(frozen.events[0].disposition, "deferred-to-P3-02");
  assert.equal(frozen.events[0].gap_id, "G12");
  assert.equal(frozen.events[0].emitters[0], "src-tauri/src/agent_ops.rs:90", "derived fields must still update");
  assert.equal(frozen.events[0].orphan, null);
  rmSync(dir, { recursive: true, force: true });
});

test("any unrecognised human field survives, not only whitelisted ones", () => {
  // The bug this guards: the merge listed the human keys it knew about, so every
  // other annotation was deleted by a freeze that `--check` still called consistent.
  const { dir } = freezeFixture();
  runFreeze(dir);
  const frozen = JSON.parse(readFileSync(path.join(dir, "docs/native/command-map.json"), "utf8"));
  assert.equal(frozen.events[0].review_owner, "18502553797");
  assert.equal(frozen.panels[0].maps_commands[0], "get_settings");
  assert.equal(frozen.panels[0].at, "src/components/app-settings/panelRegistry.tsx:14");
  rmSync(dir, { recursive: true, force: true });
});

test("freeze is idempotent: a second run changes no bytes", () => {
  const { dir } = freezeFixture();
  runFreeze(dir);
  const once = readFileSync(path.join(dir, "docs/native/command-map.json"), "utf8");
  runFreeze(dir);
  const twice = readFileSync(path.join(dir, "docs/native/command-map.json"), "utf8");
  assert.equal(twice, once, "a no-op freeze must not churn the frozen checklist");
  rmSync(dir, { recursive: true, force: true });
});

test("a newly discovered orphan event starts untriaged rather than silently in-use", () => {
  const { dir } = freezeFixture();
  runFreeze(dir, {
    events: [{ name: "brand-new-channel", emitters: ["x.rs:1"], listeners: [], orphan: "emitted-without-listener" }],
  });
  const frozen = JSON.parse(readFileSync(path.join(dir, "docs/native/command-map.json"), "utf8"));
  const added = frozen.events.find((e) => e.name === "brand-new-channel");
  assert.equal(added.disposition, "untriaged");
  rmSync(dir, { recursive: true, force: true });
});

// ---- category accountability (meta.category_resolution) ----

function expectCategoryProblem(map, mutate, pattern, label) {
  const { dir, cleanup } = tempTree((m) => { mutate(m); map = m; });
  try {
    const result = check({ repoRoot: dir, sourceRoot: repoRoot });
    assert.equal(result.ok, false, `${label}: expected a failure`);
    assert.ok(result.problems.some((p) => pattern.test(p)), `${label} not caught; saw: ${result.problems.slice(0, 3).join(" | ")}`);
  } finally { cleanup(); }
}

test("a category that contradicts the recorded derivation fails", () => {
  expectCategoryProblem(null, (map) => {
    const victim = map.commands.find((c) => c.rust_path === "pty::run_task");
    victim.category = "host_local_os_ui";
  }, /pty::run_task: category "host_local_os_ui" does not match the derived/, "flipped category");
});

test("a command in a module nobody classified fails instead of defaulting", () => {
  expectCategoryProblem(null, (map) => {
    const victim = map.commands.find((c) => c.rust_path === "pty::run_task");
    victim.rust_path = "brand_new_module::run_task";
    delete map.meta.category_resolution.module_default.brand_new_module;
  }, /module "brand_new_module" has no category default/, "unclassified module");
});

test("an override without a recorded reason fails", () => {
  expectCategoryProblem(null, (map) => {
    const key = "fs::open_in_system_file_manager";
    map.meta.category_resolution.overrides[key] = { category: "host_local_os_ui", why: "" };
    map.commands.find((c) => c.rust_path === key).category = "host_local_os_ui";
  }, /override has no recorded reason/, "reasonless override");
});

test("a stale override or stale module default fails", () => {
  expectCategoryProblem(null, (map) => {
    const key = "fs::open_in_system_file_manager";
    // make the command match its module default while keeping the override listed
    map.commands.find((c) => c.rust_path === key).category = "domain_rpc";
  }, /listed as an override but equals the module default|does not match the derived/, "stale override");
  expectCategoryProblem(null, (map) => {
    map.meta.category_resolution.module_default.ghost_module = "domain_rpc";
  }, /lists "ghost_module" but no command lives there/, "stale module default");
});

test("deleting the derivation table fails loudly", () => {
  expectCategoryProblem(null, (map) => { delete map.meta.category_resolution; },
    /unaccountable hand-written field/, "missing category_resolution");
});

// ---- method-surface accountability (ADR-002 / 003 / 005) ----

function expectMethodProblem(mutate, pattern, label) {
  const { dir, cleanup } = tempTree(mutate);
  try {
    const result = check({ repoRoot: dir, sourceRoot: repoRoot });
    assert.equal(result.ok, false, `${label}: expected failure`);
    assert.ok(result.problems.some((p) => pattern.test(p)), `${label} not caught: ${result.problems.slice(0, 3).join(" | ")}`);
  } finally { cleanup(); }
}

test("a platform_specific command without a wire name is refused", () => {
  expectMethodProblem((map) => {
    map.commands.find((c) => c.category === "platform_specific").rpc_method = null;
  }, /has no rpc_method/, "missing method");
});

test("a client-only command given a wire name is refused", () => {
  expectMethodProblem((map) => {
    map.commands.find((c) => c.category === "host_local_os_ui").rpc_method = "git.hack";
  }, /host_local_os_ui must have no wire name/, "UI command on the wire");
});

test("a hand-edited prefix that no longer matches the table is refused", () => {
  expectMethodProblem((map) => {
    map.commands.find((c) => c.rpc_method === "git.git_status").rpc_method = "vcs.git_status";
  }, /does not match the derived "git\.git_status"/, "hand-edited method");
});

test("a duplicate method name is refused", () => {
  expectMethodProblem((map) => {
    const rows = map.commands.filter((c) => c.rpc_method);
    rows[1].rpc_method = rows[0].rpc_method;
  }, /rpc_method collision/, "duplicate wire name");
});

test("DSH typed methods must state their caller evidence", () => {
  expectMethodProblem((map) => { delete map.commands.find((c) => c.rust_path.startsWith("dsh_webui::")).caller_evidence; },
    /missing caller_evidence/, "no evidence");
  expectMethodProblem((map) => {
    map.commands.find((c) => c.rust_path.startsWith("dsh_webui::")).caller_evidence = "pending: P99-99";
  }, /not a Task in MIGRATION_PLAN/, "evidence pointing at a nonexistent task");
  expectMethodProblem((map) => {
    map.commands.find((c) => c.rust_path.startsWith("dsh_webui::")).caller_evidence = "maybe someone calls it";
  }, /must start with proven: or pending:/, "unparseable evidence");
});

test("a method name with neither a caller nor a stated reason is refused", () => {
  // Q10: this is the whole point of widening the rule — a dead name must carry the
  // task that decides its fate, or it may not sit in a three-language contract.
  expectMethodProblem((map) => {
    const row = map.commands.find((c) => c.rpc_method && c.reachable === true && !c.caller_evidence);
    row.reachable = false;
  }, /neither a caller literal nor caller_evidence/, "uncalled name without evidence");
});

test("an alias is exempt from the evidence rule because it is not on the wire", () => {
  const { dir, cleanup } = tempTree((map) => {
    const twin = map.commands.find(
      (c) => c.reachability?.prefix_derived_of && c.reachability.prefix_derived_parent_reachable === true && !c.caller_evidence,
    );
    twin.reachable = false;
  });
  try {
    const problems = check({ repoRoot: dir, sourceRoot: repoRoot }).problems
      .filter((p) => p.includes("neither a caller literal"));
    assert.deepEqual(problems, [], "ADR-006 aliases must not be demanded a reason they already show");
  } finally {
    cleanup();
  }
});

test("a stale module mapping in the derivation table is refused", () => {
  expectMethodProblem((map) => { map.meta.rpc_resolution.module_to_service.ghost_module = "git"; },
    /maps module "ghost_module" that no longer has commands/, "stale module mapping");
});

// ADR-006 needs argument lists that survive real Rust signatures: lifetimes,
// qualified paths and generics with commas. Each shape below has already broken an
// implementation once in this session.
const fakeRead = (files) => (file) => {
  const key = file.split("/").slice(-2).join("/");
  if (!(key in files)) throw Object.assign(new Error(`ENOENT ${key}`), { code: "ENOENT" });
  return files[key];
};

test("extractSignature reads a multi-line signature and marks injected state", () => {
  const files = {
    "src/pty.rs": `pub async fn kill_shell(
    task_manager: State<'_, TaskManager>,
    shell_id: String,
) -> Result<(), String> {
    todo!()
}
`,
  };
  const params = extractSignature({ file: "/x/src/pty.rs", line: 1, command: "kill_shell", read: fakeRead(files) });
  assert.deepEqual(params, [
    { name: "task_manager", type: "State<'_, TaskManager>", injected: true },
    { name: "shell_id", type: "String", injected: false },
  ]);
  assert.deepEqual(renderParams(params), ["task_manager: State<'_, TaskManager>!", "shell_id: String"]);
});

test("a qualified runtime handle is injected too, not a client parameter", () => {
  // The regression: `'` in a lifetime was once treated as an opening quote, so the
  // scanner swallowed the rest of the signature and this row silently had no params.
  const files = {
    "src/usage.rs": "pub async fn read_usage_snapshot(\n    state: tauri::State<'_, crate::TaskManager>,\n) -> Result<UsageSnapshot, String> {\n    todo!()\n}\n",
  };
  const params = extractSignature({ file: "/x/src/usage.rs", line: 1, command: "read_usage_snapshot", read: fakeRead(files) });
  assert.deepEqual(renderParams(params), ["state: tauri::State<'_, crate::TaskManager>!"]);
});

test("generic commas do not split a parameter", () => {
  const files = {
    "src/db.rs": "pub fn q(map: HashMap<String, Vec<u8>>, page: u32) -> i32 { 0 }\n",
  };
  const params = extractSignature({ file: "/x/src/db.rs", line: 1, command: "q", read: fakeRead(files) });
  assert.deepEqual(params.map((p) => p.name), ["map", "page"]);
  assert.equal(params[0].type, "HashMap<String, Vec<u8>>");
});

test("a name that is not the requested function yields nothing rather than a guess", () => {
  const files = { "src/a.rs": "pub fn other(x: u8) -> u8 { x }\n" };
  assert.equal(extractSignature({ file: "/x/src/a.rs", line: 1, command: "wanted", read: fakeRead(files) }), null);
});

test("frozen params must match the source, and an absent key is a failure", () => {
  const dir = mkdtempSync(path.join(os.tmpdir(), "aeroric-params-"));
  try {
    mkdirSync(path.join(dir, "docs/native"), { recursive: true });
    // The registry (lib.rs) is read from `repoRoot`, source signatures from
    // `sourceRoot`: copying only the registration site keeps this a drift test
    // rather than a missing-file test.
    mkdirSync(path.join(dir, "src-tauri/src"), { recursive: true });
    cpSync(path.join(repoRoot, "src-tauri/src/lib.rs"), path.join(dir, "src-tauri/src/lib.rs"));
    cpSync(path.join(repoRoot, "docs/native/feature-matrix.json"), path.join(dir, "docs/native/feature-matrix.json"));
    cpSync(path.join(repoRoot, "scripts/native/checks.json"), path.join(dir, "scripts/native/checks.json"));
    const map = JSON.parse(readFileSync(mapPath, "utf8"));
    const row = map.commands.find((c) => c.command === "read_usage_snapshot");
    assert.equal(row.definition_resolved, "src-tauri/src/usage.rs:280");
    const write = (mutate) => {
      const copy = structuredClone(map);
      mutate(copy.commands.find((c) => c.command === "read_usage_snapshot"));
      writeFileSync(path.join(dir, "docs/native/command-map.json"), JSON.stringify(copy));
      return check({ repoRoot: dir, sourceRoot: repoRoot }).problems
        .filter((p) => p.includes("read_usage_snapshot") && /params/.test(p));
    };
    assert.ok(write((c) => { c.params = ["shell_id: String"]; }).some((p) => /params drifted/.test(p)),
      "a source signature that moved must fail the gate");
    assert.ok(write((c) => { delete c.params; }).some((p) => /has no `params` key/.test(p)),
      "a row never annotated must not read as 'nothing to check'");
    // Unmutated: no params complaint. Other problems (event paths that only exist in
    // a full tree) are out of scope for this assertion.
    assert.deepEqual(write((c) => c).filter((p) => /params/.test(p)), [],
      "the committed map must be self-consistent about params");
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});
