#!/usr/bin/env node
// `pnpm native:inventory --check` — mechanical consistency between the live
// Tauri surface and the frozen planning artifacts.
//
// Fails on: a registered command absent from command-map, a mapped command no
// longer registered, a duplicated definition, a source path that no longer
// exists, a command whose check module is neither implemented nor planned, and a
// feature id missing from feature-matrix. It recomputes from source rather than
// trusting any count produced earlier.
import { existsSync, readFileSync, readdirSync, writeFileSync } from "node:fs";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";
import { carrierCrossCheck, freezeFeatureMatrix } from "./task-status.mjs";

const defaultRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..");

export function parseRegistry(libSource) {
  const lines = libSource.split("\n");
  const start = lines.findIndex((l) => l.includes("generate_handler!["));
  if (start === -1) throw new Error("no generate_handler! found — the registration site moved");
  let end = -1;
  for (let i = start; i < lines.length; i += 1) {
    if (lines[i].trim().startsWith("])")) { end = i; break; }
  }
  if (end === -1) throw new Error("generate_handler! block is unterminated");
  const entries = [];
  for (let i = start + 1; i < end; i += 1) {
    const line = lines[i].trim();
    if (!line || line.startsWith("//")) continue;
    entries.push({ rustPath: line.replace(/,$/, ""), line: i + 1 });
  }
  return { entries, startLine: start + 1, endLine: end + 1 };
}

// Attribute → fn pairing, mirroring src-tauri's own guard test. Counting raw
// matches is not acceptable: `rg '#\[tauri::command\]'` over this repo returns
// 677 hits for 660 real commands because the token also appears in comments,
// doc links and in the guard test's own assertion strings.
export function parseCommandDefinitions(sourceDir, readFile) {
  const found = new Map();
  const files = listRustFiles(sourceDir, readFile);
  for (const file of files) {
    const lines = readFile(file).split("\n");
    for (let i = 0; i < lines.length; i += 1) {
      const trimmed = lines[i].trim();
      if (trimmed !== "#[tauri::command]") continue;
      let name = null;
      for (let j = i + 1; j <= Math.min(i + 8, lines.length - 1); j += 1) {
        const m = lines[j].match(/^\s*(?:pub(?:\([^)]*\))?\s+)?(?:default\s+)?(?:async\s+)?(?:unsafe\s+)?fn\s+([A-Za-z_]\w*)/);
        if (m) { name = m[1]; break; }
        if (lines[j].trim() && !lines[j].trim().startsWith("#") && !lines[j].trim().startsWith("//")) continue;
      }
      if (!name) throw new Error(`${file}:${i + 1} attribute has no fn signature within 8 lines`);
      if (found.has(name)) throw new Error(`duplicate command definition '${name}' at ${file}:${i + 1} and ${found.get(name)}`);
      found.set(name, `${file}:${i + 1}`);
    }
  }
  return found;
}

function listRustFiles(dir, readFile) {
  if (readFile.listRustFiles) return readFile.listRustFiles(dir);
  const out = [];
  const stack = [dir];
  while (stack.length) {
    const current = stack.pop();
    for (const dirent of readdirSync(current, { withFileTypes: true })) {
      const full = path.join(current, dirent.name);
      if (dirent.isDirectory()) stack.push(full);
      else if (dirent.name.endsWith(".rs")) out.push(full);
    }
  }
  return out.sort();
}

export function check({ repoRoot = defaultRoot, sourceRoot, readFile, names } = {}) {
  // Frozen artifacts may be checked from a throwaway copy while source scanning still
  // reads the real tree; without this the temp-tree negative tests would report
  // every source path as stale.
  const scanRoot = sourceRoot ?? repoRoot;
  const read = readFile ?? ((p) => readFileSync(p, "utf8"));
  const root = repoRoot;
  const libPath = names?.lib ?? "src-tauri/src/lib.rs";
  const registry = parseRegistry(read(path.join(root, libPath)));
  const map = JSON.parse(read(path.join(root, "docs/native/command-map.json")));
  const matrix = JSON.parse(read(path.join(root, "docs/native/feature-matrix.json")));
  const checks = JSON.parse(read(path.join(root, "scripts/native/checks.json")));

  const problems = [];
  const registeredNames = registry.entries.map((e) => e.rustPath.split("::").pop());
  const seen = new Map();
  for (const name of registeredNames) {
    seen.set(name, (seen.get(name) ?? 0) + 1);
  }
  for (const [name, count] of seen) if (count > 1) problems.push(`command '${name}' registered ${count} times`);

  const mapped = new Map(map.commands.map((c) => [c.rust_path, c]));
  for (const entry of registry.entries) {
    if (!mapped.has(entry.rustPath)) problems.push(`registered but not mapped: ${entry.rustPath}`);
  }
  for (const entry of mapped.values()) {
    if (!registry.entries.some((r) => r.rustPath === entry.rust_path)) problems.push(`mapped but not registered: ${entry.rust_path}`);
  }

  for (const command of mapped.values()) {
    if (!command.check_module) problems.push(`${command.rust_path}: no check module`);
    else if (!checks.modules[command.check_module] && !checks.plannedModules?.[command.check_module]) {
      problems.push(`${command.rust_path}: check module '${command.check_module}' is registered in neither checks.json.modules nor plannedModules`);
    }
    for (const feature of command.feature_ids ?? []) {
      if (feature.startsWith("F") && !matrix.features.some((f) => f.id === feature)) {
        problems.push(`${command.rust_path}: feature '${feature}' absent from feature-matrix`);
      }
    }
    for (const candidate of command.definition_candidates ?? []) {
      const file = candidate.split(":")[0];
      if (!existsSync(path.join(root, file))) problems.push(`${command.rust_path}: stale source path ${file}`);
    }
    if (!command.category) problems.push(`${command.rust_path}: uncategorised`);
  }

  if (map.meta?.counts?.registered_entries !== registry.entries.length) {
    problems.push(`command-map meta says ${map.meta?.counts?.registered_entries} entries, registry has ${registry.entries.length}`);
  }

  // ---- events (P1-04 Action 7: 注册命令/事件/面板逐项映射) ----
  const live = extractEvents({ repoRoot: scanRoot });
  const liveEvents = new Map(live.events.map((e) => [e.name, e]));
  const frozenEvents = map.events ?? [];
  if (!frozenEvents.length) {
    problems.push("command-map.json has no `events` section — run `node scripts/native/inventory.mjs --freeze`");
  }
  const frozenEventNames = new Set(frozenEvents.map((e) => e.name));
  for (const name of liveEvents.keys()) if (!frozenEventNames.has(name)) problems.push(`event emitted/listened but not frozen: ${name}`);
  for (const frozen of frozenEvents) {
    const current = liveEvents.get(frozen.name);
    if (!current) { problems.push(`frozen event no longer exists in source: ${frozen.name}`); continue; }
    for (const at of [...current.emitters, ...current.listeners]) {
      if (!existsSync(path.join(scanRoot, at.split(":")[0]))) problems.push(`${frozen.name}: stale source path ${at}`);
    }
    if (current.emitters.length && !current.listeners.length && (frozen.disposition === "untriaged" || !frozen.disposition)) {
      problems.push(`${frozen.name}: emitted with no listener and no disposition — triage it or register a gap`);
    }
    if (frozen.gap_id && !matrix.gaps.some((g) => g.id === frozen.gap_id)) {
      problems.push(`${frozen.name}: gap_id '${frozen.gap_id}' absent from feature-matrix`);
    }
    if (frozen.orphan !== current.orphan) problems.push(`${frozen.name}: orphan state drifted (frozen ${frozen.orphan}, live ${current.orphan})`);
  }
  problems.push(...dynamicSiteProblems({
    kind: "dynamic emit sites",
    live: live.unresolved.dynamic_emitters,
    frozen: map.event_dynamic_emit_sites,
  }));
  problems.push(...dynamicSiteProblems({
    kind: "dynamic listen sites",
    live: live.unresolved.dynamic_listeners,
    frozen: map.event_dynamic_listen_sites,
  }));

  // ---- settings panels ----
  const livePanels = extractPanels({ repoRoot: scanRoot });
  const livePanelKeys = new Set(livePanels.map((p) => p.key));
  const frozenPanels = map.panels ?? [];
  if (!frozenPanels.length) {
    problems.push("command-map.json has no `panels` section — run `node scripts/native/inventory.mjs --freeze`");
  }
  const frozenPanelKeys = new Map(frozenPanels.map((p) => [p.key, p]));
  for (const key of livePanelKeys) if (!frozenPanelKeys.has(key)) problems.push(`settings panel not frozen: ${key}`);
  for (const key of frozenPanelKeys.keys()) if (!livePanelKeys.has(key)) problems.push(`frozen settings panel gone from registry: ${key}`);
  for (const panel of frozenPanels) {
    if (!panel.target_scene || !panel.task_ids) problems.push(`panel ${panel.key}: missing target_scene or task_ids`);
  }
  // A frozen `at` is a citation. Before this line the count and the keys were checked
  // but not the citation, so a panel could keep "pointing at" a line it no longer is.
  problems.push(...panelDriftProblems(livePanels, frozenPanels));
  problems.push(...moduleTableProblems({
    commands: map.commands ?? [],
    table: map.modules,
    categoryResolution: map.meta?.category_resolution,
  }));

  // ---- G10: every command must resolve to one real definition in its module ----
  for (const command of mapped.values()) {
    const resolved = command.definition_resolved;
    if (!resolved) { problems.push(`${command.rust_path}: definition not resolved (${command.resolution ?? "none"})`); continue; }
    const [file, lineText] = resolved.split(":");
    const line = Number(lineText);
    // G10 reads source, so it reads `scanRoot` — the same root every other live
    // extractor uses. `repoRoot` may be a throwaway copy of the frozen artifacts.
    const definition = path.join(scanRoot, file);
    if (!existsSync(definition)) problems.push(`${command.rust_path}: resolved file missing ${file}`);
    else {
      const decl = read(definition).split("\n")[line - 1] ?? "";
      if (!new RegExp(`\\bfn\\s+${command.command}\\b`).test(decl)) {
        problems.push(`${command.rust_path}: definition_resolved points at ${resolved}, which is not that fn's declaration`);
      }
      // ADR-006 depends on these: a twin may only collapse into its parent when the
      // argument lists still relate, so a source signature that moved must fail here
      // rather than at the first cross-language call.
      const live = extractSignature({ file: definition, line, command: command.command });
      const frozen = command.params;
      if (!("params" in command)) {
        problems.push(`${command.rust_path}: has no \`params\` key — run \`node scripts/native/inventory.mjs --freeze\``);
      } else if (command.definition_unique === false) {
        if (frozen !== null) problems.push(`${command.rust_path}: ambiguous definition must record params as null`);
      } else if (!live) {
        problems.push(`${command.rust_path}: signature no longer extractable from ${resolved}`);
      } else if (JSON.stringify(renderParams(live)) !== JSON.stringify(frozen)) {
        problems.push(`${command.rust_path}: params drifted from source — frozen ${JSON.stringify(frozen)}, live ${JSON.stringify(renderParams(live))}`);
      }
    }
  }
  const skipped = [...mapped.values()].filter((c) => c.definition_resolved && !("params" in c));
  if (skipped.length) {
    problems.push(`${skipped.length} resolved command(s) carry no params (silent extraction gap): ${skipped.slice(0, 3).map((c) => c.command).join(", ")}`);
  }
  if (map.meta?.definition_resolution?.needs_human_resolution?.length) {
    const list = map.meta.definition_resolution.needs_human_resolution;
    problems.push(`definition_resolution lists ${list.length} unresolved command(s): ${list.slice(0, 5).join(", ")}`);
  }

  // ---- G08: dynamic command-name sites must stay closed enumerations ----
  const registeredCommandNames = new Set(registry.entries.map((e) => e.rustPath.split("::").pop()));
  const sites = map.dynamic_invoke_sites ?? [];
  if (!sites.length) problems.push("command-map.json has no `dynamic_invoke_sites` — every variable command name must be enumerated");
  for (const site of sites) {
    const siteFile = path.join(scanRoot, site.file);
    if (!existsSync(siteFile)) { problems.push(`dynamic site ${site.file}:${site.line}: file gone`); continue; }
    if (!Number(site.line) || site.line > read(siteFile).split("\n").length) problems.push(`dynamic site ${site.file}:${site.line}: line out of range`);
    if (!Array.isArray(site.possible_commands) || !site.possible_commands.length) { problems.push(`dynamic site ${site.file}: no closed enumeration`); continue; }
    const missing = site.possible_commands.filter((n) => !registeredCommandNames.has(n)).sort();
    const declared = (site.known_unregistered ?? []).slice().sort();
    if (missing.join("|") !== declared.join("|")) {
      problems.push(`dynamic site ${site.file}:${site.line}: unregistered names drifted — generated ${JSON.stringify(missing)}, declared ${JSON.stringify(declared)}. A prefix helper quietly producing an unknown command is a runtime failure; register the command or record the gap.`);
    }
  }

  // ---- category accountability ----
  // `category` decides host routing (the ADR forbids host_local_os_ui in a mutation
  // allow-list and requires platform_specific to say "unsupported + why"), and it was
  // produced by a session table that was never stored — so the field had no derivation
  // at all. It is now derived here from meta.category_resolution or the check fails.
  const catRes = map.meta?.category_resolution;
  if (!catRes || typeof catRes.module_default !== "object" || Object.keys(catRes.module_default ?? {}).length === 0) {
    problems.push("meta.category_resolution.module_default is missing — `category` would be an unaccountable hand-written field");
  } else {
    const modulesSeen = new Set();
    for (const command of map.commands) {
      const mod = command.rust_path.split("::")[0];
      modulesSeen.add(mod);
      const override = catRes.overrides?.[command.rust_path];
      const def = catRes.module_default[mod];
      if (def === undefined) {
        problems.push(`${command.rust_path}: module "${mod}" has no category default — classify it explicitly; a new module must not silently inherit domain_rpc`);
        continue;
      }
      const expected = override?.category ?? def;
      if (command.category !== expected) {
        problems.push(`${command.rust_path}: category "${command.category}" does not match the derived "${expected}" (module default${override ? " + named override" : ""})`);
      }
      if (override && command.category === def) {
        problems.push(`${command.rust_path}: listed as an override but equals the module default — delete the stale override`);
      }
      if (override && !String(override.why ?? "").trim()) {
        problems.push(`${command.rust_path}: override has no recorded reason`);
      }
    }
    for (const mod of Object.keys(catRes.module_default)) {
      if (!modulesSeen.has(mod)) problems.push(`category_resolution.module_default lists "${mod}" but no command lives there any more`);
    }
    for (const key of Object.keys(catRes.overrides ?? {})) {
      if (!map.commands.some((c) => c.rust_path === key)) problems.push(`category_resolution.overrides lists "${key}" which is not a frozen command`);
    }
  }

  // The plan lives in the real tree; a temp copy under test has no copy of it.
  const planRoot = sourceRoot ?? repoRoot;

  // ---- method surface accountability (ADR-002 / ADR-003 / ADR-005) ----
  const rpcRes = map.meta?.rpc_resolution;
  if (!rpcRes || typeof rpcRes.module_to_service !== "object" || !Object.keys(rpcRes.module_to_service ?? {}).length) {
    problems.push("meta.rpc_resolution.module_to_service is missing — rpc_method would be an unaccountable hand-written field");
  } else {
    const svcOf = (c) => rpcRes.module_to_service[c.rust_path.split("::")[0]];
    for (const command of map.commands) {
      if (command.category === "host_local_os_ui") {
        if (command.rpc_method) {
          problems.push(`${command.rust_path}: host_local_os_ui must have no wire name (ADR-003) — it is implemented by the client adapter, and a method here would let a host accept work it cannot do`);
        }
        continue;
      }
      if (!command.rpc_method) {
        problems.push(`${command.rust_path}: ${command.category} command has no rpc_method — the method surface would silently omit it`);
        continue;
      }
      const svc = svcOf(command);
      if (!svc) {
        problems.push(`${command.rust_path}: module "${command.rust_path.split("::")[0]}" has no service mapping — classify it in meta.rpc_resolution; a new module must not inherit a prefix by accident`);
        continue;
      }
      const expected = `${svc}.${command.rust_path.split("::").pop()}`;
      if (command.rpc_method !== expected) {
        problems.push(`${command.rust_path}: rpc_method "${command.rpc_method}" does not match the derived "${expected}"`);
      }
      if (command.rust_path.split("::").pop() !== command.command) {
        problems.push(`${command.rust_path}: wire leaf is not the legacy command name "${command.command}" (ADR-002 keeps them verbatim equal)`);
      }
    }
    // Read the plan ONCE for every task-heading lookup below. The loops that follow run over
    // ~660 command rows and used to `read()` the whole plan (≈1 MB today) per `pending:` row, which made
    // `inventory-unit` swing 5–16 s on FS-cache warmth and time out at the 300 s case budget.
    // Headings are matched at line start: prose that quotes "### Task P#-#" must not satisfy the check.
    const taskHeadings = new Set(
      [...read(path.join(planRoot, "MIGRATION_PLAN.md"), "utf8").matchAll(/^### Task (P\d+-\d+) /gm)].map((m) => m[1]),
    );
    const seenModules = new Set(map.commands.map((c) => c.rust_path.split("::")[0]));
    for (const mod of Object.keys(rpcRes.module_to_service)) {
      if (!seenModules.has(mod)) problems.push(`rpc_resolution maps module "${mod}" that no longer has commands`);
    }
    // ADR-005: every DSH typed method must say whether anyone calls it.
    for (const command of map.commands) {
      if (!command.rust_path.startsWith("dsh_webui::")) continue;
      const evidence = String(command.caller_evidence ?? "");
      if (!evidence) {
        problems.push(`${command.rust_path}: missing caller_evidence — ADR-005 requires proven:/pending: for every DSH typed method`);
      } else if (evidence.startsWith("pending:")) {
        const task = evidence.slice("pending:".length).trim();
        if (!/^P\d+-\d+$/.test(task)) problems.push(`${command.rust_path}: pending caller evidence must name a task id, got "${task}"`);
        else if (!taskHeadings.has(task)) {
          problems.push(`${command.rust_path}: pending evidence points at ${task}, which is not a Task in MIGRATION_PLAN.md`);
        }
      } else if (!evidence.startsWith("proven:")) {
        problems.push(`${command.rust_path}: caller_evidence must start with proven: or pending:, got "${evidence.slice(0, 24)}"`);
      }
    }
    // ADR-005's rule, widened to the whole method surface (Q10): a name may enter a
    // three-language wire contract only with a reason — a caller literal that exists,
    // or `caller_evidence` naming who will decide. "Nobody calls this" with no owner
    // is not a reason.
    for (const command of map.commands.filter((c) => c.rpc_method)) {
      const evidence = String(command.caller_evidence ?? "");
      const reach = command.reachability ?? {};
      const alias = reach.prefix_derived_of && reach.prefix_derived_parent_reachable === true;
      if (!evidence) {
        // A caller literal *is* evidence, and 450 rows have one. What may not happen
        // is a name with neither a caller nor a stated reason riding into the
        // contract. Prefix twins are not in the contract at all — ADR-006 made them
        // aliases — so the same exemption the exporter applies has to apply here.
        if (command.reachable !== true && !alias) {
          problems.push(`${command.rust_path}: neither a caller literal nor caller_evidence — this name has no reason to be on the wire`);
        }
        continue;
      }
      if (evidence.startsWith("pending:")) {
        const task = evidence.slice("pending:".length).trim().split(/\s/)[0];
        if (!/^P\d+-\d+$/.test(task)) problems.push(`${command.rust_path}: pending evidence must name a task id, got "${task}"`);
        else if (!taskHeadings.has(task)) {
          problems.push(`${command.rust_path}: pending evidence points at ${task}, which is not a Task in MIGRATION_PLAN.md`);
        }
      } else if (!evidence.startsWith("proven:")) {
        problems.push(`${command.rust_path}: caller_evidence must start with proven: or pending:, got "${evidence.slice(0, 24)}"`);
      }
    }

    const named = map.commands.filter((c) => c.rpc_method).map((c) => c.rpc_method);
    if (new Set(named).size !== named.length) {
      problems.push(`rpc_method collision in the frozen surface: ${named.length - new Set(named).size} duplicate names`);
    }
    if (map.meta?.counts?.methods !== undefined && map.meta.counts.methods !== named.length) {
      problems.push(`meta.counts.methods is ${map.meta.counts.methods} but ${named.length} commands carry a method`);
    }
  }

  // ---- Q2: test-case floors cannot drift from minCases, and may not be orphaned ----
  // Honest scope: this makes "silently lower the bar" a two-edit, reviewable act and
  // catches deleting a whole test case (its floor becomes an orphan). It cannot detect a
  // deliberate simultaneous edit of both numbers — that is what §3.1 rule 5 diff review is for.
  const floors = checks.runner?.floors ?? {};
  const seenFloorKeys = new Set();
  for (const [moduleName, moduleEntry] of Object.entries(checks.modules ?? {})) {
    for (const testCase of moduleEntry.cases ?? []) {
      if (testCase.kind !== "test") continue;
      const key = `${testCase.name}@${moduleName}`;
      seenFloorKeys.add(key);
      if (!(key in floors)) problems.push(`${key}: has no runner.floors entry — declare a floor so the bar cannot drift`);
      else if (floors[key] !== testCase.minCases) problems.push(`${key}: minCases ${testCase.minCases} != frozen floor ${floors[key]} — lower the floor deliberately in the same reviewed edit, never by accident`);
      else if (floors[key] < 1) problems.push(`${key}: floor must be at least 1 test case`);
    }
  }
  for (const key of Object.keys(floors)) {
    if (!seenFloorKeys.has(key)) problems.push(`runner.floors has an orphan entry ${key} — its test case was deleted or renamed; re-verify the acceptance it belonged to`);
  }

  // ---- frontend invoke literals must all be registered commands ----
  const invokeLiterals = extractInvokeLiterals({ repoRoot: scanRoot });
  for (const site of invokeLiterals) {
    if (!registeredCommandNames.has(site.command)) problems.push(`frontend invokes "${site.command}" at ${site.at} but that command is not registered`);
  }

  // ---- frozen reachability table must match a recomputation ----
  const liveReach = extractReachability({ repoRoot: scanRoot });
  let drift = 0;
  for (const command of mapped.values()) {
    const frozen = { ...(command.reachability ?? {}), reachable: command.reachable };
    const live = liveReach.get(command.rust_path);
    if (!live) { problems.push(`${command.rust_path}: no recomputed reachability`); continue; }
    if (!frozen) { problems.push(`${command.rust_path}: reachability not frozen`); continue; }
    for (const key of ["frontend_literal", "other_package_literal", "rust_internal_refs", "test_only_literal", "reachable"]) {
      if (frozen[key] !== live[key]) { problems.push(`${command.rust_path}: reachability.${key} drifted (frozen ${frozen[key]}, live ${live[key]})`); drift += 1; }
    }
  }
  const liveUnreachable = [...liveReach.values()].filter((r) => !r.reachable).length;
  if (map.meta?.counts?.unreachable !== liveUnreachable) {
    problems.push(`meta.counts.unreachable is ${map.meta?.counts?.unreachable} but recomputed ${liveUnreachable}`);
  }
  void drift;

  // ---- task-status carrier ⇄ §进度总表 ⇄ feature-matrix derived columns (F3, closed by Q7) ----
  // The matrix's delivery columns used to be hand-written and read by nobody, so 26 rows sat on
  // `unstarted` long after three of their Tasks were accepted. They are derived now, and both sides
  // of the mirror are checked: the plan table against the JSON carrier, the carrier against the
  // committed columns.
  let taskStates = null;
  let taskStatuses = null;
  let taskUniverseIds = null;
  try {
    const bundle = carrierCrossCheck({
      repoRoot,
      read,
      matrix,
      planText: read(path.join(planRoot, "MIGRATION_PLAN.md"), "utf8"),
    });
    problems.push(...bundle.problems);
    taskStates = { tasks: bundle.stats.tasks, accepted: bundle.stats.accepted, rows: bundle.stats.rows };
    taskStatuses = bundle.carrier.statuses;
    taskUniverseIds = new Set(bundle.table.universe.map((u) => u.task));
  } catch (error) {
    problems.push(`task-status carrier unreadable: ${error.message}`);
  }

  // ---- gap ledger: an `open` gap may not be owned only by Tasks that already closed (T1 round, 2026-09-28) ----
  // Same failure class as F3: the ledger recorded an owner, the owner's Task was accepted, and nothing
  // re-read the ledger afterwards, so the risk simply stopped being anyone's.
  if (taskStatuses && taskUniverseIds) {
    for (const gap of matrix.gaps ?? []) {
      const raw = String(gap.task_ids ?? "").trim();
      const ids = raw.match(/P\d+-\d+/g) ?? [];
      if (!raw) {
        problems.push(`${gap.id}: gap with no task_ids — §1.2 forbids an entry without an owner`);
        continue;
      }
      if (raw.includes("~")) {
        problems.push(`${gap.id}: task_ids "${raw}" is a range; a range cannot answer "who still owes this" — name the open Tasks`);
      }
      for (const id of ids) {
        if (!taskUniverseIds.has(id)) problems.push(`${gap.id}: task_ids names ${id}, which is not a Task heading in MIGRATION_PLAN.md`);
      }
      if (gap.status === "open" && ids.length) {
        const pending = ids.filter((id) => taskStatuses.get(id) !== "accepted");
        if (!pending.length && !gap.accepted_by) {
          problems.push(`${gap.id}: status open but every named Task (${ids.join(", ")}) is accepted — 改派给真正欠它的 Task，或写明 accepted_by 谁接受这笔风险`);
        }
      }
    }
    problems.push(...panelTaskIdProblems(map.panels ?? [], taskUniverseIds));
  }

  return {
    ok: problems.length === 0,
    registered: registry.entries.length,
    mapped: mapped.size,
    features: matrix.features.length,
    gaps: matrix.gaps.length,
    events: frozenEvents.length,
    panels: frozenPanels.length,
    definitionsResolved: [...mapped.values()].filter((c) => c.definition_resolved).length,
    dynamicSites: sites.length,
    invokeLiterals: invokeLiterals.length,
    unreachable: liveUnreachable,
    taskStates,
    problems,
  };
}

const RUST_STRING_CONST = /(?:pub(?:\([^)]*\))?\s+)?const\s+([A-Z][A-Z0-9_]*)\s*:\s*&str\s*=\s*"([^"]+)"/g;
const TS_STRING_CONST = /(?:export\s+)?const\s+([A-Z][A-Z0-9_]*)\s*=\s*"([^"]+)"\s*(?:as\s+const)?\s*;/g;
const EMIT_START = /\.emit(?:_to|_all)?/g;
const LISTEN_START = /(?<![A-Za-z0-9_$])listen(?:\.any)?(?![A-Za-z0-9_$])/g;

/**
 * Locate `emit(...)` / `listen(...)["<generics>"]` call sites and return their
 * first argument. A regex cannot do this: TypeScript generic parameters nest
 * (`listen<{ questions: Array<{ id: string }> }>(NAME, …)` spans lines and
 * brackets), and every regex approximation tried so far either lost those
 * subscriptions or captured an unrelated later argument.
 */
function callSiteArgs(text, startRegex) {
  const found = [];
  for (const match of text.matchAll(startRegex)) {
    let i = match.index + match[0].length;
    while (i < text.length && /\s/.test(text[i])) i += 1;
    if (text[i] === "<") {
      let depth = 0;
      while (i < text.length) {
        const ch = text[i];
        if (ch === '"' || ch === "'" || ch === "`") {
          const quote = ch;
          i += 1;
          while (i < text.length && text[i] !== quote) i += text[i] === "\\" ? 2 : 1;
        } else if (ch === "<") depth += 1;
        else if (ch === ">") { depth -= 1; if (depth === 0) { i += 1; break; } }
        else if (ch === ";" || ch === "}") { if (depth === 0) break; }
        i += 1;
      }
      while (i < text.length && /\s/.test(text[i])) i += 1;
    }
    if (text[i] !== "(") continue;
    i += 1;
    while (i < text.length && /\s/.test(text[i])) i += 1;
    const argStart = i;
    let depth = 0;
    while (i < text.length) {
      const ch = text[i];
      if (ch === '"' || ch === "'" || ch === "`") {
        const quote = ch;
        i += 1;
        while (i < text.length && text[i] !== quote) i += text[i] === "\\" ? 2 : 1;
        i += 1;
        continue;
      }
      if ("([{".includes(ch)) depth += 1;
      else if (")]}".includes(ch)) { if (depth === 0 && ch === ")") break; depth -= 1; }
      else if (ch === "," && depth === 0) break;
      i += 1;
    }
    const arg = text.slice(argStart, i).trim();
    if (arg) found.push({ arg, line: text.slice(0, match.index).split("\n").length });
  }
  return found;
}

function occurrences(files, read, startRegex, consts) {
  const hits = [];
  const dynamic = [];
  for (const file of files) {
    const text = read(file);
    for (const { arg, line } of callSiteArgs(text, startRegex)) {
      const at = `${file}:${line}`;
      const lineText = text.split("\n")[line - 1] ?? "";
      if (lineText.trimStart().startsWith("//") || lineText.trimStart().startsWith("///")) continue;
      const literal = arg.match(/^"([^"]+)"$/);
      if (literal) { hits.push({ event: literal[1], at }); continue; }
      const symbol = arg.split("::").pop().split(".").pop();
      const value = resolveSymbol(symbol, file, consts);
      if (value) hits.push({ event: value, at, viaConst: symbol });
      else if (consts.bySymbol.get(symbol)?.ambiguous) dynamic.push({ identifier: `${symbol} (ambiguous: ${consts.bySymbol.get(symbol).values.join(" | ")})`, at });
      else dynamic.push({ identifier: arg, at });
    }
  }
  return { hits, dynamic };
}
/// Files whose string constants are event names by declaration; elsewhere a
/// constant is only a name→value lookup, never an event in its own right.
/// P3-05 第一片（补记二百五十）：`DSH_HOST_*` 十个常量随 10 臂映射迁入 core 的
/// dsh_host dispatcher，故声明文件与 Rust 语料都要能看见 core 侧真身——
/// 宿主 `event_names.rs` 只剩 re-export（不再有字符串常量）。
const EVENT_CONSTANT_FILES = ["src-tauri/src/event_names.rs", "src/tauriEvents.ts", "src/appRemoteEvents.ts"];
const CORE_EVENT_CONSTANT_FILES = [
  "core/crates/aeroric-core/src/agents/dsh_host/dispatcher.rs",
  // 补记二百七十一次：waterfall 四常量（approval/question 的 requested/resolved）
  // 声明随第八片入 approvals.rs，清单漏登记——宿主 emit 点解析不到符号后掉进
  // dynamic 桶，contracts 的 emitters 1→0、orphan 翻成 listened-without-emitter。
  // 断点在第八片，266 前没人跑 protocol/projects/tasks 的 contracts-current 所以一直没红。
  "core/crates/aeroric-core/src/agents/dsh_host/approvals.rs",
];

function collectConsts(files, read, regex) {
  const perFile = new Map();
  const bySymbol = new Map();
  for (const file of files) {
    const text = read(file);
    for (const match of text.matchAll(regex)) {
      const entry = { value: match[2], at: file };
      if (!perFile.has(file)) perFile.set(file, new Map());
      perFile.get(file).set(match[1], entry);
      const prior = bySymbol.get(match[1]);
      if (prior && prior.value !== entry.value) {
        bySymbol.set(match[1], { ...prior, ambiguous: true, values: [...new Set([...(prior.values ?? [prior.value]), entry.value])] });
      } else if (!prior) {
        bySymbol.set(match[1], entry);
      }
    }
  }
  return { perFile, bySymbol };
}

function resolveSymbol(symbol, file, consts) {
  const local = consts.perFile.get(file)?.get(symbol);
  if (local) return local.value;
  const global = consts.bySymbol.get(symbol);
  // An imported name that two files define with two different values cannot be
  // resolved by scanning alone. Recording it as ambiguous beats picking whichever
  // definition the file walk happened to reach last.
  if (!global || global.ambiguous) return null;
  return global.value;
}


function isTestFile(file) {
  return /(^|\/)(tests?|__tests__)(\/|\.)/.test(file) || /\.test\.[cm]tsx?$/.test(file)
    || /_tests?\.rs$/.test(file) || /\/tests\//.test(file);
}

/**
 * Event channels as the source actually declares them. Anything that cannot be
 * attributed statically is kept in `dynamic*` with its location rather than being
 * guessed at — an unresolved name is a finding, not a blank.
 */
export function extractEvents({ repoRoot = defaultRoot, readFileSync: read = readFileSync } = {}) {
  const rustFiles = listRustFiles(path.join(repoRoot, "src-tauri", "src"), (dir) => listRustFiles(dir, { readFileSync: read })).filter((f) => !isTestFile(f));
  // P3-05: the dsh_host core module hosts event constants and (later) typed sinks;
  // its non-test .rs files join the Rust corpus so symbol→value resolution works.
  for (const rel of CORE_EVENT_CONSTANT_FILES) {
    const abs = path.join(repoRoot, rel);
    if (existsSync(abs) && !isTestFile(abs)) rustFiles.push(abs);
  }
  const readText = (f) => read(f, "utf8");
  const rel = (f) => path.relative(repoRoot, f).split(path.sep).join("/");
  const rustSources = [...new Set(rustFiles.map((f) => rel(f)))];
  const webFiles = listSourceFiles(path.join(repoRoot, "src"), [".ts", ".tsx"], read).filter((f) => !isTestFile(f)).map(rel);

  const rustConsts = collectConsts(rustSources, readText, RUST_STRING_CONST);
  const tsConsts = collectConsts(webFiles, readText, TS_STRING_CONST);
  const emits = occurrences(rustSources, readText, EMIT_START, rustConsts);
  const listens = occurrences(webFiles, readText, LISTEN_START, tsConsts);

  const byEvent = new Map();
  const touch = (name) => {
    if (!byEvent.has(name)) byEvent.set(name, { name, rust_consts: [], ts_consts: [], emitters: [], listeners: [], disposition: null, gap_id: null });
    return byEvent.get(name);
  };
  // The event set is what source actually sends or receives, plus anything
  // declared in a dedicated event-constants file (a defined-but-unused constant
  // is a finding worth seeing, not something to silently drop).
  for (const hit of emits.hits) touch(hit.event).emitters.push(hit.at);
  for (const hit of listens.hits) touch(hit.event).listeners.push(hit.at);
  const declaredConsts = (file, consts) => {
    const bucket = file.startsWith("src/") ? "ts_consts" : "rust_consts";
    for (const [symbol, entry] of consts.perFile.get(file) ?? []) touch(entry.value)[bucket].push({ symbol, at: entry.at });
  };
  for (const file of [...EVENT_CONSTANT_FILES, ...CORE_EVENT_CONSTANT_FILES]) declaredConsts(file, file.startsWith("src/") ? tsConsts : rustConsts);

  const dynamic = { dynamic_emitters: [], dynamic_listeners: [] };
  for (const hit of emits.dynamic) dynamic.dynamic_emitters.push(`${hit.identifier} @ ${hit.at}`);
  for (const hit of listens.dynamic) dynamic.dynamic_listeners.push(`${hit.identifier} @ ${hit.at}`);

  const events = [...byEvent.values()]
    .sort((a, b) => a.name.localeCompare(b.name))
    .map((e) => ({
      ...e,
      orphan: e.emitters.length > 0 && e.listeners.length === 0
        ? "emitted-without-listener"
        : (e.listeners.length > 0 && e.emitters.length === 0 ? "listened-without-emitter" : null),
    }));
  return { events, unresolved: dynamic };
}

const IGNORED_SCAN_DIRS = new Set(["node_modules", ".git", ".worktrees", ".build", ".artifacts", "target", "dist", "build"]);

function walkAny(dir, extensions, out = []) {
  if (!existsSync(dir)) return out;
  for (const dirent of readdirSync(dir, { withFileTypes: true })) {
    const full = path.join(dir, dirent.name);
    if (dirent.isDirectory()) { if (!IGNORED_SCAN_DIRS.has(dirent.name)) walkAny(full, extensions, out); }
    else if (extensions.some((ext) => dirent.name.endsWith(ext))) out.push(full);
  }
  return out.sort();
}

const INVOKE_START = /(?<![A-Za-z0-9_$])(?:invoke|invokeCancellable)(?![A-Za-z0-9_$])/g;

/** Command-name string literals passed straight to invoke() from the frontend. */
/**
 * The three accountability helpers below exist as pure functions on purpose: the
 * negative tests must be able to feed them a contradiction directly. A throwaway
 * tree cannot reproduce "the live scan found 29 sites and the frozen file lists 29
 * different ones" — that is exactly the state a count-only check passes.
 */
export function dynamicSiteProblems({ kind, live, frozen }) {
  const problems = [];
  const l = [...new Set(live ?? [])];
  const f = [...new Set(frozen ?? [])];
  if (!f.length && l.length) return [`${kind}: no frozen sites recorded — run --freeze`];
  const missing = l.filter((s) => !f.includes(s));
  const extra = f.filter((s) => !l.includes(s));
  if (missing.length) {
    problems.push(`${kind}: ${missing.length} live site(s) absent from the frozen list — ${missing.slice(0, 3).join(" | ")}`);
  }
  if (extra.length) {
    problems.push(`${kind}: ${extra.length} frozen site(s) not reproduced by the live scan — ${extra.slice(0, 3).join(" | ")} (a drifted file:line inside one of these strings reads as "recorded verbatim" while pointing nowhere)`);
  }
  return problems;
}

/** Panels: `key`/`via`/`at` are derived, the rest is the human routing decision. */
export function panelDriftProblems(livePanels, frozenPanels) {
  const live = new Map((livePanels ?? []).map((p) => [p.key, p]));
  const problems = [];
  for (const panel of frozenPanels ?? []) {
    const found = live.get(panel.key);
    if (!found) continue;
    for (const field of ["via", "at"]) {
      if (panel[field] !== found[field]) {
        problems.push(`panel ${panel.key}: ${field} drifted (frozen ${JSON.stringify(panel[field])}, live ${JSON.stringify(found[field])}) — re-run --freeze`);
      }
    }
  }
  return problems;
}

export function panelTaskIdProblems(panels, universe) {
  const problems = [];
  for (const panel of panels ?? []) {
    for (const id of String(panel.task_ids ?? "").match(/P\d+-\d+/g) ?? []) {
      if (!universe.has(id)) {
        problems.push(`panel ${panel.key}: task_ids names ${id}, which is not a Task heading in MIGRATION_PLAN.md`);
      }
    }
  }
  return problems;
}

/**
 * The `modules` rollup is a summary of the command rows, so every column of it must be
 * a value its own rows actually carry. Deliberately convention-free: a root-level
 * command (no `::`) is `(crate_root)` in this table but its own key in
 * `category_resolution.module_default`, and that disagreement is an open question —
 * so those modules are skipped rather than forced into either answer.
 */
export function moduleTableProblems({ commands, table, categoryResolution }) {
  const problems = [];
  const byModule = new Map();
  for (const command of commands ?? []) {
    const mod = String(command.rust_path ?? "").split("::")[0];
    if (!String(command.rust_path ?? "").includes("::")) continue;
    if (!byModule.has(mod)) byModule.set(mod, []);
    byModule.get(mod).push(command);
  }
  for (const [mod, entry] of Object.entries(table ?? {})) {
    const rows = byModule.get(mod);
    if (!rows) continue;
    for (const field of ["category", "core_service", "swift_scene", "check_module"]) {
      const claimed = entry?.[field];
      if (claimed === undefined || claimed === null) {
        problems.push(`modules.${mod}: column "${field}" is missing although ${rows.length} command row(s) carry it`);
        continue;
      }
      const actual = new Set(rows.map((r) => r[field]).filter((v) => v !== undefined && v !== null));
      if (!actual.has(claimed)) {
        problems.push(`modules.${mod}.${field} ${JSON.stringify(claimed)} is not a value any of its ${rows.length} command rows carry (seen: ${[...actual].slice(0, 4).map((v) => JSON.stringify(v)).join(", ")})`);
      }
    }
    const def = categoryResolution?.module_default?.[mod];
    if (def !== undefined && entry?.category !== def) {
      problems.push(`modules.${mod}.category ${JSON.stringify(entry?.category)} contradicts the audited module_default ${JSON.stringify(def)}`);
    }
  }
  return problems;
}

export function extractInvokeLiterals({ repoRoot = defaultRoot } = {}) {
  const out = [];
  for (const file of walkAny(path.join(repoRoot, "src"), [".ts", ".tsx"]).filter((f) => !isTestFile(relOf(repoRoot, f)))) {
    const text = readFileSync(file, "utf8");
    for (const { arg, line } of callSiteArgs(text, INVOKE_START)) {
      const literal = arg.match(/^"([a-z0-9_]+)"$/);
      if (literal) out.push({ command: literal[1], at: `${relOf(repoRoot, file)}:${line}` });
    }
  }
  return out;
}

function relOf(root, file) { return path.relative(root, file).split(path.sep).join("/"); }

/** Recompute the frozen reachability verdict for every command. Single pass:
 *  scanning 660 names against every file separately is O(names x files) and made
 *  the gate take minutes, which in practice means nobody runs it. */
const reachCache = new Map();

function buildReachIndex(repoRoot) {
  const stamp = `${Date.now()}`;
  const cached = reachCache.get(repoRoot);
  if (cached && cached.valid) return cached;
  const webAll = walkAny(path.join(repoRoot, "src"), [".ts", ".tsx"]);
  const web = webAll.filter((f) => !isTestFile(relOf(repoRoot, f)));
  // Test sources are excluded from the *reachable* verdict, but a test that asserts
  // `invoke` was called with "run_tests" is evidence the command is wired. Dropping
  // that signal classified live commands as unwired, so it is kept as a weak flag.
  const webTests = webAll.filter((f) => isTestFile(relOf(repoRoot, f)));
  const rust = walkAny(path.join(repoRoot, "src-tauri", "src"), [".rs"]).filter((f) => !isTestFile(relOf(repoRoot, f)));
  const other = [
    ...walkAny(path.join(repoRoot, "mobile"), [".ts", ".tsx", ".js", ".json"]),
    ...walkAny(path.join(repoRoot, "packages"), [".ts", ".tsx", ".json"]),
  ].filter((f) => !isTestFile(relOf(repoRoot, f)));
  const literalSet = (files) => {
    const set = new Set();
    for (const f of files) for (const m of readFileSync(f, "utf8").matchAll(/"([a-z0-9_]+)"/g)) set.add(m[1]);
    return set;
  };
  const webLiterals = literalSet(web);
  const webTestLiterals = literalSet(webTests);
  const otherLiterals = literalSet(other);
  // One pass over Rust sources, counting per LINE (not per occurrence) and
  // excluding the line that declares the command itself, so the numbers mean the
  // same thing as the frozen table it is compared against.
  const rustRefs = new Map();
  for (const f of rust) {
    if (relOf(repoRoot, f) === "src-tauri/src/lib.rs") continue;
    for (const line of readFileSync(f, "utf8").split("\n")) {
      const t = line.trimStart();
      if (t.startsWith("//") || t.startsWith("///") || t.startsWith("*")) continue;
      const declared = new Set([...line.matchAll(/(?:async\s+)?fn\s+([a-z][a-z0-9_]*)/g)].map((m) => m[1]));
      const tokens = new Set([...line.matchAll(/\b([a-z][a-z0-9_]{2,})\b/g)].map((m) => m[1]));
      for (const tok of tokens) {
        if (declared.has(tok)) continue;
        rustRefs.set(tok, (rustRefs.get(tok) ?? 0) + 1);
      }
    }
  }
  const registration = readFileSync(path.join(repoRoot, "src-tauri/src/lib.rs"), "utf8");
  const registeredNames = new Set([...registration.matchAll(/[\s(]([a-z0-9_]+),/g)].map((m) => m[1]));
  const index = { webLiterals, webTestLiterals, otherLiterals, rustRefs, registeredNames, valid: true, stamp };
  reachCache.set(repoRoot, index);
  return index;
}

export function extractReachability({ repoRoot = defaultRoot } = {}) {
  const index = buildReachIndex(repoRoot);
  const commands = JSON.parse(readFileSync(path.join(repoRoot, "docs/native/command-map.json"), "utf8")).commands;
  const result = new Map();
  for (const command of commands) {
    const name = command.command;
    const frontendLiteral = index.webLiterals.has(name);
    const otherPackageLiteral = !frontendLiteral && index.otherLiterals.has(name);
    const rustInternalRefs = index.rustRefs.get(name) ?? 0;
    const testOnlyLiteral = !frontendLiteral && !otherPackageLiteral && index.webTestLiterals.has(name);
    // `prefixCommand` derives `remote_X` / `wsl_X` from a logical name at runtime, so a
    // missing literal for such a name is expected, not evidence of a dead command.
    const derived = /^(remote|wsl)_(.+)$/.exec(name);
    result.set(command.rust_path, {
      frontend_literal: frontendLiteral,
      other_package_literal: otherPackageLiteral,
      rust_internal_refs: rustInternalRefs,
      registered: index.registeredNames.has(name),
      test_only_literal: testOnlyLiteral,
      prefix_derived_of: derived ? derived[2] : null,
      reachable: frontendLiteral || otherPackageLiteral || rustInternalRefs > 0,
    });
  }
  for (const [path_, row] of result) {
    if (row.prefix_derived_of) {
      const parentPath = [...result.keys()].find((k) => k.split("::").pop() === row.prefix_derived_of);
      row.prefix_derived_parent_reachable = parentPath ? result.get(parentPath).reachable : null;
    }
  }
  return result;
}

export function invalidateInventoryCaches(repoRoot) {
  if (repoRoot) reachCache.delete(repoRoot); else reachCache.clear();
}

/** Settings panels straight out of the registry, including const-valued keys. */
/**
 * Client-visible parameters of one command, read from its resolved definition
 * (ADR-006 needs them: a `remote_X` twin may only collapse into `X` when the extra
 * arguments are exactly the transport target). Rust injection (`State`, `AppHandle`,
 * windows) is marked, not dropped, so "we filtered a parameter" stays auditable.
 */
export function extractSignature({ file, line, command, read = readFileSync }) {
  const text = read(file, "utf8");
  const start = text.split("\n").slice(line - 1).join("\n");
  const decl = new RegExp(`\\bfn\\s+${command}\\b`).exec(start);
  if (!decl) return null;
  const opened = start.indexOf("(", decl.index);
  if (opened < 0) return null;
  let depth = 0, angle = 0, end = -1;
  for (let i = opened; i < start.length; i += 1) {
    const ch = start[i];
    // Only `"` is skipped: a `'` here is a lifetime (`State<'_, X>`), and treating it
    // as an opening quote made the scanner swallow the rest of the signature.
    if (ch === '"') {
      i += 1;
      while (i < start.length && start[i] !== '"') i += (start[i] === "\\" ? 2 : 1);
      continue;
    }
    if (ch === "(" || ch === "[" || ch === "{") depth += 1;
    else if (ch === ")" || ch === "]" || ch === "}") depth -= 1;
    else if (ch === "<" && !/[=!<>]/.test(start[i + 1] ?? "")) angle += 1;
    else if (ch === ">" && angle > 0) angle -= 1;
    if (ch === ")" && depth === 0) { end = i; break; }
  }
  if (end < 0) return null;
  const body = start.slice(opened + 1, end);
  const parts = [];
  let current = "", d = 0, a = 0;
  for (const ch of body) {
    if (ch === "(" || ch === "[" || ch === "{") d += 1;
    else if (ch === ")" || ch === "]" || ch === "}") d -= 1;
    else if (ch === "<") a += 1;
    else if (ch === ">" && a > 0) a -= 1;
    if (ch === "," && d === 0 && a === 0) { parts.push(current); current = ""; }
    else current += ch;
  }
  if (current.trim()) parts.push(current);
  // A Rust type may be qualified (`tauri::State<'_, X>`), so the injected-runtime names
  // are matched at any path boundary rather than only at the start of the type.
  const injected = /(^|[^\w>])(State|AppHandle|Manager|Window|WebviewWindow|Webview)\b/;
  return parts.map((part) => {
    const raw = part.trim().replace(/\s+/g, " ");
    const colon = raw.indexOf(":");
    if (colon < 0) return { name: raw.replace(/&/, ""), type: null, injected: raw === "self" || raw.startsWith("&self") };
    return {
      name: raw.slice(0, colon).trim().replace(/&mut\s+/, "").replace(/&/, ""),
      type: raw.slice(colon + 1).trim(),
      injected: injected.test(raw.slice(colon + 1)),
    };
  }).filter((entry) => entry.name && entry.name !== "self");
}

/** How a parameter is written into the frozen map: `name: Type`, `!` for injected. */
export function renderParams(params) {
  return params.map((p) => `${p.name}: ${p.type}${p.injected ? "!" : ""}`);
}

/** Same, for every command the map has already resolved to a single definition. */
export function extractSignatures({ repoRoot = defaultRoot, commands, read = readFileSync }) {
  const out = new Map();
  for (const command of commands) {
    const resolved = command.definition_resolved;
    if (!resolved || command.definition_unique === false) { out.set(command.rust_path, null); continue; }
    const [file, line] = resolved.split(":");
    const params = extractSignature({ file: path.join(repoRoot, file), line: Number(line), command: command.command, read });
    out.set(command.rust_path, params);
  }
  return out;
}

export function extractPanels({ repoRoot = defaultRoot, readFileSync: read = readFileSync } = {}) {
  const file = "src/components/app-settings/panelRegistry.tsx";
  const text = read(path.join(repoRoot, file), "utf8");
  const consts = new Map([...text.matchAll(TS_STRING_CONST)].map((m) => [m[1], m[2]]));
  const panels = [];
  text.split("\n").forEach((line, index) => {
    const m = line.match(/^\s*key:\s*(?:"([^"]+)"|([A-Z][A-Z0-9_]*))\s*,/);
    if (!m) return;
    const key = m[1] ?? consts.get(m[2]);
    if (!key) return;
    panels.push({ key, via: m[1] ? "literal" : `const ${m[2]}`, at: `${file}:${index + 1}` });
  });
  return panels;
}

function listSourceFiles(dir, extensions, read) {
  void read;
  const out = [];
  const stack = [dir];
  while (stack.length) {
    const current = stack.pop();
    for (const dirent of readdirSync(current, { withFileTypes: true })) {
      if (dirent.name === "node_modules" || dirent.name.startsWith(".")) continue;
      const full = path.join(current, dirent.name);
      if (dirent.isDirectory()) stack.push(full);
      else if (extensions.some((ext) => dirent.name.endsWith(ext))) out.push(full);
    }
  }
  return out.sort();
}

const isMain = process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url);

// Keys the extractors derive from source. Anything not listed here is treated as
// human-authored and survives a re-freeze untouched.
const EVENT_SOURCE_KEYS = ["name", "rust_consts", "ts_consts", "emitters", "listeners", "dynamic_emitters", "dynamic_listeners", "orphan"];
const PANEL_SOURCE_KEYS = ["key", "via", "at"];

/**
 * Write the frozen `events` and `panels` sections into command-map.json.
 *
 * The merge is deliberately a **blacklist of derived keys**, not a whitelist of
 * human keys: a whitelist silently deletes every human field that is not named
 * in it, and that is exactly how one `--freeze` run once erased all 14 event
 * `note` annotations while `--check` still reported the inventory as consistent.
 * Key order of the existing file is preserved so a no-op freeze stays a no-op.
 */
export function freeze({
  repoRoot = defaultRoot,
  sourceRoot,
  readFileSync: read = readFileSync,
  writeFileSync: put = writeFileSync,
  live,
  dryRun = false,
} = {}) {
  const mapPath = path.join(repoRoot, "docs/native/command-map.json");
  const original = read(mapPath, "utf8");
  const map = JSON.parse(original);
  const source = live ?? (() => {
    const { events, unresolved } = extractEvents({ repoRoot });
    return { events, unresolved, panels: extractPanels({ repoRoot }) };
  })();
  const { events, unresolved, panels } = source;
  const existing = new Map((map.events ?? []).map((e) => [e.name, e]));
  const existingPanels = new Map((map.panels ?? []).map((p) => [p.key, p]));

  // Only keys the extractor *derives from source* may be overwritten. `disposition`
  // and `gap_id` are triage a human records, but extractEvents seeds them as null
  // placeholders, so overlaying every emitted key reset real triage to "untriaged"
  // even though the note annotations survived.
  const overlay = (prior, derived, sourceKeys, defaults) => {
    const out = { ...(prior ?? {}) };
    for (const key of sourceKeys) if (key in derived) out[key] = derived[key];
    for (const [key, fallback] of Object.entries(defaults)) out[key] = out[key] ?? fallback;
    return out;
  };
  const merged = events.map((e) => overlay(existing.get(e.name), e, EVENT_SOURCE_KEYS, {
    disposition: e.orphan ? "untriaged" : "in-use",
    gap_id: null,
    target: null,
  }));
  map.events = merged;
  map.event_dynamic_emit_sites = unresolved.dynamic_emitters ?? [];
  map.event_dynamic_listen_sites = unresolved.dynamic_listeners ?? [];
  map.panels = panels.map((p) => overlay(existingPanels.get(p.key), p, PANEL_SOURCE_KEYS, {
    target_scene: null,
    task_ids: null,
    maps_commands: null,
  }));
  // Commands are annotated, never regenerated: `--freeze` has no extractor for the
  // human keys on a command row, so only `params` may be written here.
  const signatures = extractSignatures({ repoRoot, commands: map.commands });
  let annotated = 0;
  for (const command of map.commands) {
    const params = signatures.get(command.rust_path);
    // `null` is recorded rather than omitted: an ambiguous definition is a known gap
    // in the contract's evidence, and "key absent" would be indistinguishable from
    // "the extractor was never run".
    command.params = params ? renderParams(params) : null;
    if (params) annotated += 1;
  }
  map.meta.counts.command_params_annotated = annotated;
  map.meta.counts.events = merged.length;
  map.meta.counts.panels = panels.length;

  // The feature matrix's delivery columns are derived, never hand-written (F3/Q7). `--freeze-check`
  // compares them byte for byte as well, so a status flip that was recorded in one artifact but not
  // the other cannot pass as "nothing changed".
  const matrixPath = path.join(repoRoot, "docs/native/feature-matrix.json");
  const matrixDoc = JSON.parse(read(matrixPath, "utf8"));
  const bundle = carrierCrossCheck({
    repoRoot,
    read: (p) => read(p, "utf8"),
    matrix: matrixDoc,
    planText: read(path.join(sourceRoot ?? repoRoot, "MIGRATION_PLAN.md"), "utf8"),
  });
  if (bundle.inputProblems.length) {
    throw new Error(`refusing to freeze while the mirror inputs disagree:\n  ${bundle.inputProblems.slice(0, 10).join("\n  ")}`);
  }
  const matrixFreeze = freezeFeatureMatrix({
    repoRoot,
    read: (p) => read(p, "utf8"),
    dryRun: true,
    derived: bundle.derived,
  });

  const extraFields = (entries, derivedKeys) =>
    [...new Set(entries.flatMap((e) => Object.keys(e)).filter((k) => !derivedKeys.has(k)))].sort().join(", ") || "none";
  const serialized = `${JSON.stringify(map, null, 2)}\n`;
  if (dryRun) {
    const changed = serialized !== original || matrixFreeze.changed;
    console.log(`freeze --check: the frozen file ${changed ? "DIFFERS from what --freeze would write" : "is exactly what --freeze would write"}`);
    if (matrixFreeze.changed) console.log(`  docs/native/feature-matrix.json derived columns also differ (${matrixFreeze.rows} row(s)).`);
    if (changed) {
      console.log(`  events[${extraFields(merged, new Set(EVENT_SOURCE_KEYS))}] panels[${extraFields(map.panels, new Set(PANEL_SOURCE_KEYS))}] are human-owned and were carried through;`);
      console.log(`  run \`node scripts/native/inventory.mjs --freeze\` to refresh, and inspect the diff before accepting it.`);
    }
    return { changed, events: merged.length, panels: panels.length, matrixChanged: matrixFreeze.changed };
  }
  put(mapPath, serialized);
  if (matrixFreeze.changed) put(matrixPath, matrixFreeze.serialized);
  console.log(`froze ${merged.length} event channel(s), ${panels.length} settings panel(s) into docs/native/command-map.json`);
  if (matrixFreeze.changed) console.log(`re-derived ${matrixFreeze.rows} feature-matrix row(s) from docs/native/task-status.json`);
  console.log(`  human-authored fields carried through: events[${extraFields(merged, new Set(EVENT_SOURCE_KEYS))}] panels[${extraFields(map.panels, new Set(PANEL_SOURCE_KEYS))}]`);
  const dyn = (unresolved.dynamic_emitters ?? []).length;
  if (dyn) console.log(`  ${dyn} dynamic emit site(s) recorded verbatim, not guessed`);
  return { map, events: merged.length, panels: panels.length, matrixChanged: matrixFreeze.changed };
}

/** Freeze reachability using the same index the checker recomputes. */
function freezeReachability() {
  const mapPath = path.join(defaultRoot, "docs/native/command-map.json");
  const map = JSON.parse(readFileSync(mapPath, "utf8"));
  const live = extractReachability({ repoRoot: defaultRoot });
  for (const command of map.commands) {
    const r = live.get(command.rust_path);
    if (!r) throw new Error(`no reachability computed for ${command.rust_path}`);
    command.reachability = {
      frontend_literal: r.frontend_literal,
      other_package_literal: r.other_package_literal,
      rust_internal_refs: r.rust_internal_refs,
      test_only_literal: r.test_only_literal,
      prefix_derived_of: r.prefix_derived_of,
      prefix_derived_parent_reachable: r.prefix_derived_parent_reachable ?? null,
      registered: r.registered,
    };
    command.reachable = r.reachable;
  }
  const unreachable = map.commands.filter((c) => !c.reachable);
  const byModule = {};
  for (const c of unreachable) {
    const m = c.rust_path.split("::")[0];
    byModule[m] = (byModule[m] ?? 0) + 1;
  }
  map.meta.counts.reachable = map.commands.length - unreachable.length;
  map.meta.counts.unreachable = unreachable.length;
  map.meta.reachability = {
    rule: "reachable = 名字以字符串字面量出现在 src/ 或 mobile//packages/，或在 src-tauri/src 内除自身 fn 声明行与 lib.rs 注册之外仍被按行引用。计数单位为引用行数。",
    excluded_dirs: [...IGNORED_SCAN_DIRS].filter((d) => existsSync(path.join(defaultRoot, d))),
    unreachable_by_module: byModule,
    note: "不可达不等于可删：remote_/wsl_ 镜像变体可能由 prefixCommand 运行时拼接，也可能只被外部客户端使用。删除任何一条之前必须先完成 G16 的逐项定性。",
  };
  writeFileSync(mapPath, `${JSON.stringify(map, null, 2)}\n`);
  console.log(`froze reachability: reachable=${map.meta.counts.reachable} unreachable=${map.meta.counts.unreachable}`);
}

if (isMain) {
  if (process.argv.includes("--freeze-reachability")) {
    try { freezeReachability(); } catch (error) { console.error(`inventory --freeze-reachability: ${error.message}`); process.exit(1); }
    process.exit(0);
  }
  if (process.argv.includes("--freeze-check")) {
    try { process.exit(freeze({ dryRun: true }).changed ? 1 : 0); }
    catch (error) { console.error(`inventory --freeze-check: ${error.message}`); process.exit(1); }
  }
  if (process.argv.includes("--freeze")) {
    try { freeze(); } catch (error) { console.error(`inventory --freeze: ${error.message}`); process.exit(1); }
    process.exit(0);
  }
  if (!process.argv.includes("--check")) {
    console.error("usage: inventory.mjs --check | --freeze | --freeze-check | --freeze-reachability");
    process.exit(2);
  }
  let result;
  try {
    result = check({});
  } catch (error) {
    console.error(`inventory: ${error.message}`);
    process.exit(1);
  }
  console.log(`inventory: registered=${result.registered} mapped=${result.mapped} features=${result.features} gaps=${result.gaps} events=${result.events} panels=${result.panels} definitions=${result.definitionsResolved} dynamicSites=${result.dynamicSites} invokeLiterals=${result.invokeLiterals} unreachable=${result.unreachable} taskStates=${result.taskStates ? `${result.taskStates.tasks}/${result.taskStates.accepted}accepted` : "unreadable"}`);
  if (!result.ok) {
    console.error(result.problems.slice(0, 40).map((p) => `  - ${p}`).join("\n"));
    if (result.problems.length > 40) console.error(`  … ${result.problems.length - 40} more`);
    console.error(`\ninventory: ${result.problems.length} inconsistency(ies). A new entry with no mapping must fail here, not pass silently.`);
    process.exit(1);
  }
  console.log("inventory: consistent");
}
