// watcher-discipline 的负例集（照 terminal-invariants.test.mjs 的先例：判定纯函数
// 直接喂字符串，不需要临时树；每一条都必须是「改坏就红」而不是装饰）。
import test from "node:test";
import assert from "node:assert/strict";
import { watcherDisciplineProblems, loadWatcherCorpus } from "./watcher-discipline.mjs";

const SESSION = "src-tauri/src/session.rs";
const WATCHER = "src-tauri/src/event_watcher.rs";
const LIB = "src-tauri/src/lib.rs";

function good() {
  return [
    {
      path: SESSION,
      text: [
        "pub(crate) fn spawn_claude_lazy_session_attach() {",
        '    let watcher_name = format!("watch:claude-lazy-attach:{}:{}", task_id, gen);',
        "    let spawn_result = supervisor.spawn_supervised(&watcher_name, move |token| {",
        "        if token.is_cancelled() {",
        "            return;",
        "        }",
        "        for _ in 0..240 {",
        "            if token.is_cancelled() {",
        "                break;",
        "            }",
        "        }",
        "    });",
        '    if let Err(error) = spawn_result { eprintln!("lazy session attach registration failed"); }',
        "}",
        "pub(crate) fn spawn_resume_session_watcher() {",
        '    let watcher_name = format!("watch:resume-session:{}:{}", task_id, gen);',
        "    let spawn_result = supervisor.spawn_supervised(&watcher_name, move |token| {",
        "        if token.is_cancelled() {",
        "            return;",
        "        }",
        "        register_and_watch_session();",
        "    });",
        '    if let Err(error) = spawn_result { eprintln!("session watcher registration failed"); }',
        "}",
      ].join("\n"),
    },
    {
      path: WATCHER,
      text: [
        "pub fn start(app: AppHandle) {",
        '    supervisor.spawn_supervised("watch:hook-events", move |token| run_loop(app, token))',
        "}",
        "fn run_loop(token: ShutdownToken) {",
        "    loop {",
        "        if token.is_cancelled() {",
        "            return;",
        "        }",
        "    }",
        "}",
        "fn dispatch() {}",
      ].join("\n"),
    },
    {
      path: LIB,
      text: [
        "if let tauri::RunEvent::Exit = event {",
        "    let supervisor = Arc::clone(&state.supervisor);",
        "    let report = supervisor.shutdown();",
        "    let manager = app_handle.state::<local_router_commands::LocalRouterManager>();",
        "    block_on(manager.shutdown());",
        "}",
      ].join("\n"),
    },
  ];
}

const mutate = (path, fn) => {
  const files = good();
  const file = files.find((entry) => entry.path === path);
  file.text = fn(file.text);
  return files;
};

test("the shipping corpus itself passes (positive anchor)", () => {
  assert.deepEqual(watcherDisciplineProblems(loadWatcherCorpus()), []);
});

test("an unread corpus file is an error, not silence (D12)", () => {
  const files = good().filter((entry) => entry.path !== LIB);
  const problems = watcherDisciplineProblems(files);
  assert.equal(problems.length, 1);
  assert.match(problems[0], /was not read/);
});

test("a reintroduced bare thread::spawn in session.rs fails by name", () => {
  const problems = watcherDisciplineProblems(
    mutate(SESSION, (text) => text + "\nfn leak() { thread::spawn(|| {}); }\n"),
  );
  assert.equal(problems.length, 1);
  assert.match(problems[0], /bare thread::spawn returned \(1\)/);
});

test("a thread::spawn mention inside a comment does not count", () => {
  const problems = watcherDisciplineProblems(
    mutate(SESSION, (text) => "// thread::spawn( 只是历史叙述\n" + text),
  );
  assert.deepEqual(problems, []);
});

test("move |_token| (token received but never read) fails", () => {
  const problems = watcherDisciplineProblems(
    mutate(SESSION, (text) =>
      text.replace("move |token| {\n        if token.is_cancelled() {\n            return;\n        }\n        register_and_watch_session", "move |_token| {\n        register_and_watch_session"),
    ),
  );
  assert.ok(problems.some((p) => p.includes("move |_token|")), JSON.stringify(problems));
});

test("removing the resume entry check fails the region rule", () => {
  const problems = watcherDisciplineProblems(
    mutate(SESSION, (text) =>
      text.replace(
        "if token.is_cancelled() {\n            return;\n        }\n        register_and_watch_session",
        "register_and_watch_session",
      ),
    ),
  );
  assert.ok(problems.some((p) => p.includes("resume-watcher")), JSON.stringify(problems));
});

test("dropping the in-loop check of lazy attach fails the 2x minimum", () => {
  const problems = watcherDisciplineProblems(
    mutate(SESSION, (text) =>
      text.replace(
        "        for _ in 0..240 {\n            if token.is_cancelled() {\n                break;\n            }\n        }",
        "        for _ in 0..240 {\n        }",
      ),
    ),
  );
  assert.ok(problems.some((p) => p.includes("lazy-attach")), JSON.stringify(problems));
});

test("a missing supervisor.shutdown() in the Exit branch fails", () => {
  const problems = watcherDisciplineProblems(
    mutate(LIB, (text) => text.replace("    let report = supervisor.shutdown();\n", "")),
  );
  assert.ok(problems.some((p) => p.includes("supervisor.shutdown() missing")), JSON.stringify(problems));
});

test("silencing watchers AFTER LocalRouter fails the order rule", () => {
  const problems = watcherDisciplineProblems(
    mutate(LIB, (text) => {
      const lines = text.split("\n");
      const sd = lines.findIndex((l) => l.includes("supervisor.shutdown()"));
      const line = lines.splice(sd, 1)[0];
      lines.splice(lines.length - 1, 0, line);
      return lines.join("\n");
    }),
  );
  assert.ok(
    problems.some((p) => p.includes("before LocalRouter")),
    JSON.stringify(problems),
  );
});

test("a duplicated watch:hook-events registration name fails the exact-count rule", () => {
  const problems = watcherDisciplineProblems(
    mutate(WATCHER, (text) => text + '\nfn ghost() { supervisor.spawn_supervised("watch:hook-events", move |token| run_loop2(token)); }\n'),
  );
  assert.ok(problems.some((p) => p.includes('"watch:hook-events" expected 1x, found 2')), JSON.stringify(problems));
});

test("a run_loop that never reads the token fails its region rule", () => {
  const problems = watcherDisciplineProblems(
    mutate(WATCHER, (text) =>
      text.replace("        if token.is_cancelled() {\n            return;\n        }\n", ""),
    ),
  );
  assert.ok(problems.some((p) => p.includes("run_loop")), JSON.stringify(problems));
});
