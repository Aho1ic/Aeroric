import { test } from "node:test";
import assert from "node:assert/strict";
import { settlementProblems } from "./terminal-invariants.mjs";

// 计数守卫自己必须先被证明会红。`hostFiles`/`coreFiles` 是纯输入,所以这些反例不需要临时树。

const GOOD_CORE = [{ path: "core/crates/aeroric-core/src/terminal/exit.rs", text: "pub fn settle_exit(facts: &ExitFacts) -> ExitDecision {\n    NoStatus\n}\n" }];

function hostWith(body) {
  return [{ path: "src-tauri/src/pty.rs", text: body }];
}

const ONE_SITE = [
  "fn finalize_task_exit() {",
  "    let settlement = aeroric_core::terminal::settle_exit(&aeroric_core::terminal::ExitFacts {",
  "        family,",
  "    });",
  "}",
].join("\n");

test("exactly one host call inside finalize_task_exit is a pass", () => {
  assert.deepEqual(settlementProblems(hostWith(ONE_SITE), GOOD_CORE), []);
});

test("a second host call site is refused and both are named", () => {
  const files = [
    { path: "src-tauri/src/pty.rs", text: ONE_SITE },
    {
      path: "src-tauri/src/ssh.rs",
      text: ["fn other() {", "    let d = settle_exit(&facts);", "}"].join("\n"),
    },
  ];
  const problems = settlementProblems(files, GOOD_CORE);
  assert.equal(problems.length, 1, `expected one problem, got ${JSON.stringify(problems)}`);
  assert.match(problems[0], /found 2/);
  assert.match(problems[0], /src-tauri\/src\/pty\.rs:2/);
  assert.match(problems[0], /src-tauri\/src\/ssh\.rs:2/);
});

test("zero host call sites is refused — the host decides by itself again", () => {
  const problems = settlementProblems(
    hostWith("fn finalize_task_exit() {\n    // 结算已上收 core\n}\n"),
    GOOD_CORE,
  );
  assert.equal(problems.length, 1);
  assert.match(problems[0], /found 0 \(none\)/);
  assert.match(problems[0], /the host decides exit status by itself again/);
});

test("a mention inside a comment does not count as a call site", () => {
  // 正控:若扫描不剥注释,这条会被数成 1 处而"通过"。它必须报零处。
  const files = hostWith("fn finalize_task_exit() {\n    // settle_exit(&facts) 走 core\n}\n");
  const problems = settlementProblems(files, GOOD_CORE);
  assert.equal(problems.length, 1);
  assert.match(problems[0], /found 0/);
});

test("a mention inside a string literal does not count as a call site", () => {
  const files = hostWith('fn finalize_task_exit() {\n    let hint = "settle_exit(&facts)";\n}\n');
  const problems = settlementProblems(files, GOOD_CORE);
  assert.equal(problems.length, 1);
  assert.match(problems[0], /found 0/);
});

test("the single call must sit in finalize_task_exit, not elsewhere", () => {
  const problems = settlementProblems(
    hostWith("fn report_something_else() {\n    let d = settle_exit(&facts);\n}\n"),
    GOOD_CORE,
  );
  assert.equal(problems.length, 1);
  assert.match(problems[0], /must live in finalize_task_exit/);
  assert.match(problems[0], /found it in report_something_else/);
});

test("a missing core definition is refused", () => {
  const problems = settlementProblems(
    hostWith(ONE_SITE),
    [{ path: "core/crates/aeroric-core/src/terminal/exit.rs", text: "pub fn something_else() {}\n" }],
  );
  assert.equal(problems.length, 1);
  assert.match(problems[0], /core must define settle_exit exactly once, found 0/);
});

test("two core definitions are refused — one decision face is the whole point", () => {
  const problems = settlementProblems(hostWith(ONE_SITE), [
    GOOD_CORE[0],
    { path: "core/crates/aeroric-core/src/terminal/other.rs", text: GOOD_CORE[0].text },
  ]);
  assert.equal(problems.length, 1);
  assert.match(problems[0], /found 2/);
});

test("an unread host corpus is an error, never an absence", () => {
  const problems = settlementProblems([], GOOD_CORE);
  assert.equal(problems.length, 1);
  assert.match(problems[0], /an unread corpus is not absence/);
});

test("an unread core file is an error, never a pass", () => {
  const problems = settlementProblems(hostWith(ONE_SITE), []);
  assert.equal(problems.length, 1);
  assert.match(problems[0], /refusing to certify a count against an empty corpus/);
});
