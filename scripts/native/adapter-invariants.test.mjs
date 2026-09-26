// Negative controls for scripts/native/adapter-invariants.mjs.
//
// An absence-style guard that cannot be made to fire is worse than no guard: it reports "clean"
// for an empty corpus, a renamed file or a broken scanner. Each case here mutates one fact and
// requires the specific problem text, and one case proves the scan refuses to judge what it could
// not read.
import assert from "node:assert/strict";
import test from "node:test";

import {
  adapterInvariantProblems,
  codeOnly,
  enclosingFunction,
  rustFiles,
  snapshotWriteProblems,
} from "./adapter-invariants.mjs";

const PTY = "src-tauri/src/pty.rs";
const LIB = "src-tauri/src/lib.rs";
const ADAPTER = "src-tauri/src/core_adapter.rs";
const SESSION = "src-tauri/src/session.rs";
const EXECUTOR = "src-tauri/src/legacy_task_executor.rs";
/** Names the no-recursion rule is judged against; the real run loads them from the inventory. */
const COMMANDS = ["run_task", "resume_task", "save_project_tasks"];

/** A corpus that satisfies wiring §12's presence controls, with one file free for mutations. */
function webCorpus(mutate = null) {
  const files = [
    { name: "src/state/app/taskIntentAdoption.ts", text: "export function sendTaskIntent() { replaceProjectRows(prev, id, rows); }" },
    { name: "src/taskFlush.ts", text: "await Promise.all([flushAllProjectIntents(), flushProjects()]);" },
    ...Array.from({ length: 60 }, (_, i) => ({ name: `src/mod/file${i}.ts`, text: "export const ok = true;" })),
  ];
  return mutate ? files.map((file) => (file.name === mutate.name ? { ...file, text: mutate.text } : file)) : files;
}

test("wiring §12: a retired snapshot call site is named by file and line", () => {
  const problems = snapshotWriteProblems(webCorpus({ name: "src/mod/file0.ts", text: "persistProjectTasks(pid, allTasks);" }), null);
  assert.equal(problems.length, 1, problems.join("\n"));
  assert.match(problems[0], /src\/mod\/file0\.ts:1 still names the retired snapshot write path/);
});

test("wiring §12: each retired spelling is its own problem — the flush helpers count too", () => {
  for (const text of ["flushProjectTasks(pid);", "flushAllProjectTasks();", "PERSISTENCE_COMMANDS.saveProjectTasks", 'invoke("save_project_tasks")']) {
    const problems = snapshotWriteProblems(webCorpus({ name: "src/mod/file0.ts", text }), null);
    assert.equal(problems.length, 1, `${text}: ${problems.join("\n")}`);
  }
});

test("wiring §12: test files are exempt, by a stated rule rather than a silent exclusion", () => {
  const problems = snapshotWriteProblems(webCorpus({ name: "src/test/task-persistence.test.ts", text: "persistProjectTasks(pid, rows);" }), null);
  assert.deepEqual(problems, [], "the corpus itself carries the exclusion rule: /\.test\.|^src\/test\//");
});

test("wiring §12: the registration must be gone from lib.rs, not merely uncalled", () => {
  const problems = snapshotWriteProblems(webCorpus(), "storage::save_project_tasks,");
  assert.equal(problems.length, 1, problems.join("\n"));
  assert.match(problems[0], /lib\.rs still registers save_project_tasks/);
});

test("wiring §12: the presence controls refuse an unreadable or thin corpus", () => {
  assert.match(snapshotWriteProblems([], null)[0], /no web sources scanned/);
  assert.match(
    snapshotWriteProblems([{ name: "src/a.ts", text: "" }], null)[0],
    /too thin to claim absence/,
  );
  const noBridge = webCorpus().filter((file) => file.name !== "src/state/app/taskIntentAdoption.ts");
  assert.match(snapshotWriteProblems(noBridge, null)[0], /adoption bridge was not found/);
  const noFlush = webCorpus().map((file) => (file.name === "src/taskFlush.ts" ? { ...file, text: "" } : file));
  assert.match(snapshotWriteProblems(noFlush, null)[0], /exit flush no longer drains the intent lane/);
});

/** A source tree that satisfies all three rules, so every mutation below is attributable to the
 *  one line it changes rather than to scaffolding. */
function cleanFiles() {
  return [
    {
      name: PTY,
      text: `pub(crate) static STARTUP_SIGNAL_VERSION: AtomicU64 = AtomicU64::new(1);
pub(crate) struct StartupSignalRegistration { version: u64, sender: Sender }
pub(crate) fn register_initial_input_signal(task_manager: &TaskManager, run: &RunKey) -> u64 {
    let version = STARTUP_SIGNAL_VERSION.fetch_add(1, Ordering::Relaxed);
    version
}
pub(crate) fn clear_initial_input_signal_if_current(signals: &StartupSignalRegistry) {
    let _ = STARTUP_SIGNAL_VERSION.load(Ordering::Relaxed);
}
`,
    },
    {
      name: LIB,
      text: `impl TaskManager {
    pub(crate) fn release_session_path_if_unreferenced(&self, path: &str) -> bool {
        let referenced = false;
        if referenced {
            return false;
        }
        self.claimed_session_paths.lock().remove(&path.to_string())
    }
}
`,
    },
    {
      name: ADAPTER,
      text: `pub(crate) fn writer_handover_allowed() -> bool {
    cfg!(target_os = "macos")
}
impl TaskEventSink for TauriEventSink {
    fn notify(&self, notification: TaskNotification) {
        let _ = self.app.emit(
            "task-status",
            task_status_payload(task_id, status, None),
        );
        let _ = self.app.emit("task-session", task_session_payload(facts));
    }
}
`,
    },
    { name: SESSION, text: `fn claim_session_path(app: &AppHandle, path: &str) -> bool {\n    true\n}\n` },
    {
      // The module doc names TaskService on purpose: prose must not satisfy or trip the rule, only
      // a reference in code may. Every method here delegates, so mutations below are attributable.
      name: EXECUTOR,
      text: `//! The fixed direction is command -> TaskService -> this file -> *_core launch bodies.
impl<R: tauri::Runtime> TaskExecutor for LegacyTaskExecutor<R> {
    fn launch(&self, request: &LaunchRequest) -> Execution<()> {
        if request.family == ProtocolFamily::Dsh {
            return Err(ExecutorFailure::Unsupported {
                reason: dsh_refusal_reason(),
            });
        }
        run_task_core(app, task_id)
    }
    fn resume(&self, request: &ResumeRequest) -> Execution<()> {
        resume_task_core(app, task_id)
    }
}
`,
    },
  ];
}

function problemText(files, commands = COMMANDS) {
  return adapterInvariantProblems(files, commands).join("\n");
}

/**
 * For a one-fact mutation, "some problem mentions it" is not enough: a fixture that breaks in two
 * ways at once would let a rule look like it understood the wrong thing. Assert a single problem.
 */
function onlyProblemText(files, commands = COMMANDS) {
  const problems = adapterInvariantProblems(files, commands);
  assert.equal(problems.length, 1, `expected exactly one problem, got:\n  ${problems.join("\n  ")}`);
  return problems[0];
}

test("the reference tree satisfies every rule", () => {
  // Doubles as the near-miss control: the fixture calls `run_task_core(`, and a command name that
  // is a prefix of the body it delegates to must not read as calling the command wrapper.
  assert.deepEqual(adapterInvariantProblems(cleanFiles(), COMMANDS), []);
});

test("a second run-identity source is refused", () => {
  const files = cleanFiles();
  files[0].text += "\nstatic STARTUP_SIGNAL_GENERATION: AtomicU64 = AtomicU64::new(1);\n";
  assert.match(problemText(files), /STARTUP_SIGNAL_GENERATION is back/);
});

test("the version stamp cannot leak into another file", () => {
  const files = cleanFiles();
  files[3].text += "\nlet _ = STARTUP_SIGNAL_VERSION.load(Ordering::Relaxed);\n";
  assert.match(problemText(files), /read outside pty\.rs/);
});

test("and cannot be read by a third function in pty.rs either", () => {
  const files = cleanFiles();
  files[0].text += `pub(crate) fn some_other_thing() -> u64 {
    STARTUP_SIGNAL_VERSION.load(Ordering::Relaxed)
}
`;
  assert.match(problemText(files), /also read by some_other_thing/);
});

test("a stamp nothing reads means the rule is not being checked", () => {
  const files = cleanFiles();
  files[0].text = files[0].text
    .replace("    let version = STARTUP_SIGNAL_VERSION.fetch_add(1, Ordering::Relaxed);\n", "")
    .replace("    let _ = STARTUP_SIGNAL_VERSION.load(Ordering::Relaxed);\n", "");
  assert.match(problemText(files), /no function reads STARTUP_SIGNAL_VERSION/);
});

test("a second claim-removal site is refused", () => {
  const files = cleanFiles();
  files[3].text = files[3].text.replace(
    "    true\n}",
    `    let mut claimed = tm.claimed_session_paths.lock();
    claimed.remove(&path);
    true
}`,
  );
  assert.match(problemText(files), /claim removal outside the reverse-lookup helper: src-tauri\/src\/session\.rs:claim_session_path/);
});

test("a helper that no longer removes anything is caught, not praised", () => {
  const files = cleanFiles();
  files[1].text = files[1].text.replace(
    "        self.claimed_session_paths.lock().remove(&path.to_string())",
    "        let _ = path;\n        false",
  );
  assert.match(problemText(files), /contains no claim removal/);
});

test("handover must stay macOS-only", () => {
  const always = cleanFiles();
  always[2].text = always[2].text.replace('    cfg!(target_os = "macos")\n', "    true\n");
  assert.match(problemText(always), /no longer returns cfg!\(target_os = "macos"\)/);

  const widened = cleanFiles();
  widened[2].text = widened[2].text.replace(
    '    cfg!(target_os = "macos")\n',
    '    cfg!(any(target_os = "macos", target_os = "windows"))\n',
  );
  assert.match(problemText(widened), /names windows\/linux|no longer returns/);
});

test("an unreadable guard fails instead of passing", () => {
  const noHelper = cleanFiles();
  noHelper[1].text = "impl TaskManager {}\n";
  assert.match(problemText(noHelper), /the scan read nothing/);

  const noAdapter = cleanFiles();
  noAdapter[2].text = "// rewritten\n";
  assert.match(problemText(noAdapter), /the scan read nothing/);
});

test("methods are attributed to their own function, not to the last free fn", () => {
  const lib = cleanFiles()[1].text.split("\n");
  assert.equal(enclosingFunction(lib, 1), "release_session_path_if_unreferenced");
  const withPrecedingFreeFn = `pub(crate) fn authorize_app_exit() {}\n\nimpl TaskManager {\n    pub(crate) fn helper(&self) {}\n}\n`.split("\n");
  assert.equal(enclosingFunction(withPrecedingFreeFn, 3), "helper");
});

test("the real tree is scanned, not an empty stand-in", () => {
  const names = rustFiles("src-tauri/src");
  assert.ok(names.length > 100, `expected the real src-tauri tree, got ${names.length} files`);
  assert.ok(names.includes(PTY) && names.includes(LIB) && names.includes(ADAPTER));
});

test("a second emitter of a handed-over family is a problem", () => {
  const files = cleanFiles();
  files[0].text += `
pub(crate) fn rogue(app: &AppHandle<Wry>, task_id: &str) {
    let _ = app.emit("task-status", json!({ "task_id": task_id }));
}
`;
  assert.match(problemText(files), /ADR-023: task-status still has legacy emitters outside the sink/);
});

test("the sink itself must be in view, or ADR-023 is unprovable", () => {
  const files = cleanFiles();
  files[2].text = `pub(crate) fn writer_handover_allowed() -> bool {
    cfg!(target_os = "macos")
}
`;
  assert.match(
    problemText(files),
    /ADR-023: task-status must be emitted by exactly one line of core_adapter\.rs, found 0/,
  );
});

test("emitting through the event-name constant is still counted", () => {
  const files = cleanFiles();
  files[0].text += `
pub(crate) fn via_constant(app: &AppHandle<Wry>, payload: Value) {
    let _ = app.emit(crate::event_names::TASK_STATUS, payload);
}
`;
  assert.match(problemText(files), /ADR-023: task-status still has legacy emitters/);
});

test("the second family is guarded too, not just the one that was deleted first", () => {
  const files = cleanFiles();
  files[0].text += `
pub(crate) fn rogue_session(app: &AppHandle<Wry>, facts: Value) {
    let _ = app.emit("task-session", facts);
}
`;
  assert.match(onlyProblemText(files), /ADR-023: task-session still has legacy emitters outside the sink/);
});

// ---- Action 4: the call direction. Each case mutates one fact about the executor fixture.

test("a call back up into a command wrapper is refused, and named by its function", () => {
  const files = cleanFiles();
  files[4].text += `
fn retry_after_failure(app: &AppHandle<Wry>, task_id: &str) {
    run_task(app, task_id.to_string());
}
`;
  assert.match(
    onlyProblemText(files),
    /Action 4: src-tauri\/src\/legacy_task_executor\.rs:retry_after_failure calls the command wrapper run_task\(\)/,
  );
});

test("a port that reaches no launch body is caught rather than praised", () => {
  const files = cleanFiles();
  files[4].text = files[4].text.replace(
    "        resume_task_core(app, task_id)\n",
    "        Ok(Execution::ready(()))\n",
  );
  assert.match(
    onlyProblemText(files),
    /Action 4: LegacyTaskExecutor::resume reaches no \*_core launch body/,
  );
});

test("reporting success before delegating is the silent no-op the plan names", () => {
  const files = cleanFiles();
  files[4].text = files[4].text.replace(
    "        resume_task_core(app, task_id)",
    "        if request.detached {\n            return Ok(Execution::ready(()));\n        }\n        resume_task_core(app, task_id)",
  );
  assert.match(
    onlyProblemText(files),
    /Action 4: LegacyTaskExecutor::resume returns Ok before delegating/,
  );
});

test("a refusal that does not travel as an Err is not a refusal", () => {
  const files = cleanFiles();
  files[4].text = files[4].text.replace(
    "            return Err(ExecutorFailure::Unsupported {",
    "            let parked = ExecutorFailure::Unsupported {",
  );
  assert.match(
    onlyProblemText(files),
    /Action 4: an ExecutorFailure::Unsupported is built outside an Err/,
  );
});

test("an executor that cannot be found is a failure, not an empty clean sweep", () => {
  const files = cleanFiles();
  files[4].text = files[4].text.replace("impl<R: tauri::Runtime> TaskExecutor for LegacyTaskExecutor<R> {", "impl<R: tauri::Runtime> SomethingElse for LegacyTaskExecutor<R> {");
  assert.match(
    onlyProblemText(files),
    /Action 4: the `impl … TaskExecutor for …` block in .* could not be read/,
  );
});

test("prose naming TaskService is invisible to the rule; a code reference is not", () => {
  // The fixture's module doc already names TaskService and stays clean (see the reference-tree
  // case), which is what makes this direction meaningful.
  const files = cleanFiles();
  files[4].text = files[4].text.replace(
    "        run_task_core(app, task_id)",
    "        let service: Arc<TaskService> = self.service.clone();\n        run_task_core(app, task_id)",
  );
  assert.match(onlyProblemText(files), /Action 4: src-tauri\/src\/legacy_task_executor\.rs names TaskService/);
});

// ---- codeOnly: the blanker that makes the two directions above possible.

test("blanking comments preserves length and line numbers", () => {
  const text = `//! prose about TaskService and run_task()
fn real() {
    // another prose line
    run_task_core()
}
`;
  const blanked = codeOnly(text);
  assert.equal(blanked.length, text.length, "byte length must survive so line numbers still point at code");
  assert.equal(blanked.split("\n").length, text.split("\n").length);
  assert.ok(!blanked.includes("TaskService"), "doc-comment prose must not survive as content");
  assert.ok(!blanked.includes("another prose line"), "line comments must not survive either");
  assert.match(blanked, /fn real\(\) \{/);
  assert.match(blanked, /run_task_core\(\)/);
});

test("a slash-slash inside a string is content, not a comment", () => {
  const text = 'let url = "https://example/a"; let after = 1;\n';
  const blanked = codeOnly(text);
  assert.equal(blanked.length, text.length);
  assert.ok(!blanked.includes("https"), "string contents are blanked out");
  assert.match(
    blanked,
    /let after = 1;/,
    "if `//` inside the string were read as a comment, the rest of the line would be blanked too",
  );
});
