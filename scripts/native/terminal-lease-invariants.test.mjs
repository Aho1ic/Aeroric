import { test } from "node:test";
import assert from "node:assert/strict";
import { leaseChannelProblems, loadLeaseCorpus } from "./terminal-lease-invariants.mjs";

// 计数守卫自己必须先被证明会红。`hostFiles`/`cliFiles`/`coreFiles` 是纯输入,
// 所以这些反例不需要临时 git 树。

const GOOD_CORE = [
  {
    path: "core/crates/aeroric-core/src/terminal/lease.rs",
    text: "pub fn write_gate(&self, handle: &LeaseHandle) -> Result<(), LeaseError> {\n    self.check(handle)\n}\n",
  },
];

const EXECUTOR_PATH = "core/crates/aeroric-host/src/executor.rs";
const SERVICES_PATH = "core/crates/aeroric-host/src/services.rs";
const REMOTE_PATH = "core/crates/aeroric-host/src/remote_headless.rs";

/** 标准通道文件:input 与 resize 各一闸一配对,resize + kill_run 各读一次 master。 */
function channelFile(overrides = {}) {
  const text = [
    "impl CliPtyExecutor {",
    "    pub fn input(&self, task_id: &str, bytes: &[u8]) -> Result<(), String> {",
    "        let writer = self.registry.pty_writers.lock().get(&run).cloned().unwrap();",
    "        let mut leases = self.leases.lock().expect(\"lock\");",
    "        let lease = leases.entry(task_id.to_string()).or_default();",
    "        let handle = lease.acquire();",
    "        let written = lease.write_gate(&handle).map_err(|e| format!(\"lease gate refused the write: {e:?}\"))",
    "            .and_then(|()| { let mut w = writer.lock(); w.write_all(bytes).map_err(|e| e.to_string()); });",
    "        let _ = lease.release(&handle);",
    "        written",
    "    }",
    "    pub fn resize(&self, task_id: &str, cols: u16, rows: u16) -> Result<(), String> {",
    "        let master = self.registry.master_for(task_id).ok_or_else(|| \"no master\")?;",
    "        let mut leases = self.leases.lock().expect(\"lock\");",
    "        let lease = leases.entry(task_id.to_string()).or_default();",
    "        let handle = lease.acquire();",
    "        let resized = lease.write_gate(&handle).map_err(|e| format!(\"lease gate refused the resize: {e:?}\"))",
    "            .and_then(|()| master.lock().resize(portable_pty::PtySize { rows, cols, pixel_width: 0, pixel_height: 0 }).map_err(|e| e.to_string()));",
    "        let _ = lease.release(&handle);",
    "        resized",
    "    }",
    "    pub fn kill_run(&self, task_id: &str) {",
    "        if let Some(master) = self.registry.master_for(task_id) {",
    "            let _ = master.lock().process_group_leader();",
    "        }",
    "    }",
    "}",
  ].join("\n");
  return { path: overrides.path ?? EXECUTOR_PATH, text: overrides.text ?? text };
}

function hostWith(...files) {
  const services = { path: SERVICES_PATH, text: "fn task_input() { executor.input(t, &p); } fn task_resize() { executor.resize(t, 80, 24); }" };
  const remote = { path: REMOTE_PATH, text: "async fn task_input(&self, task_id: &str, text: &str) { self.executor.input(task_id, data.as_bytes())?; }" };
  return [services, remote, ...files];
}

function cliDummy() {
  return [{ path: "core/crates/aeroric-cli/src/main.rs", text: "fn main() { println!(\"cli\"); }" }];
}

const GOOD_HOST = hostWith(channelFile());
const GOOD_CLI = cliDummy();

test("the real corpus is a pass", () => {
  const { hostFiles, cliFiles, coreFiles } = loadLeaseCorpus();
  assert.deepEqual(leaseChannelProblems(hostFiles, cliFiles, coreFiles), []);
});

test("a synthetic single-channel corpus is a pass", () => {
  assert.deepEqual(leaseChannelProblems(GOOD_HOST, GOOD_CLI, GOOD_CORE), []);
});

test("a second gate site is refused and both are named", () => {
  const text = [
    "impl CliPtyExecutor {",
    "    pub fn input(&self) {",
    "        let lease = self.leases.entry(\"t\".into()).or_default();",
    "        let handle = lease.acquire();",
    "        let _ = lease.write_gate(&handle);",
    "        let _ = lease.write_gate(&handle);",
    "        let _ = lease.release(&handle);",
    "    }",
    "}",
  ].join("\n");
  const problems = leaseChannelProblems(hostWith({ path: EXECUTOR_PATH, text }), GOOD_CLI, GOOD_CORE);
  assert.ok(
    problems.some((p) => p.includes("exactly 2") || p.includes("exactly the channel functions")),
    problems.join("\n"),
  );
});

test("zero gate sites is refused — writes bypass the lease", () => {
  const text = [
    "impl CliPtyExecutor {",
    "    pub fn input(&self) {",
    "        let mut w = writer.lock();",
    "        w.write_all(bytes);",
    "    }",
    "}",
  ].join("\n");
  const problems = leaseChannelProblems(hostWith({ path: EXECUTOR_PATH, text }), GOOD_CLI, GOOD_CORE);
  assert.ok(problems.some((p) => p.includes("exactly 2")), problems.join("\n"));
});

test("a gate site outside the channel file is refused", () => {
  const elsewhere = { path: "core/crates/aeroric-host/src/elsewhere.rs", text: "fn other() { lease.write_gate(&h); }" };
  const problems = leaseChannelProblems(hostWith(channelFile(), elsewhere), GOOD_CLI, GOOD_CORE);
  assert.ok(
    problems.some((p) => p.includes("exactly 2") || p.includes("must both live in")),
    problems.join("\n"),
  );
});

test("a gate call in a non-channel fn is refused", () => {
  const text = [
    "impl CliPtyExecutor {",
    "    pub fn input(&self) {",
    "        let lease = self.leases.entry(\"t\".into()).or_default();",
    "        let handle = lease.acquire();",
    "        let _ = lease.write_gate(&handle);",
    "        let _ = lease.release(&handle);",
    "    }",
    "    pub fn prompt(&self) {",
    "        let lease = self.leases.entry(\"t\".into()).or_default();",
    "        let handle = lease.acquire();",
    "        let _ = lease.write_gate(&handle);",
    "        let _ = lease.release(&handle);",
    "    }",
    "}",
  ].join("\n");
  const problems = leaseChannelProblems(hostWith({ path: EXECUTOR_PATH, text }), GOOD_CLI, GOOD_CORE);
  assert.ok(problems.some((p) => p.includes("exactly the channel functions")), problems.join("\n"));
});

test("input without acquire is refused", () => {
  const text = [
    "impl CliPtyExecutor {",
    "    pub fn input(&self) {",
    "        let writer = self.registry.pty_writers.lock().get(&run).cloned().unwrap();",
    "        let lease = self.leases.entry(\"t\".into()).or_default();",
    "        let _ = lease.write_gate(&handle);",
    "        let _ = lease.release(&handle);",
    "    }",
    "    pub fn resize(&self) {",
    "        let master = self.registry.master_for(\"t\").unwrap();",
    "        let lease = self.leases.entry(\"t\".into()).or_default();",
    "        let handle = lease.acquire();",
    "        let _ = lease.write_gate(&handle);",
    "        let _ = lease.release(&handle);",
    "    }",
    "    pub fn kill_run(&self) {",
    "        if let Some(master) = self.registry.master_for(\"t\") { let _ = master.lock().process_group_leader(); }",
    "    }",
    "}",
  ].join("\n");
  const problems = leaseChannelProblems(hostWith({ path: EXECUTOR_PATH, text }), GOOD_CLI, GOOD_CORE);
  assert.ok(problems.some((p) => p.includes("lease.acquire must appear exactly once per channel fn")), problems.join("\n"));
});

test("resize without release is refused", () => {
  const text = [
    "impl CliPtyExecutor {",
    "    pub fn input(&self) {",
    "        let writer = self.registry.pty_writers.lock().get(&run).cloned().unwrap();",
    "        let lease = self.leases.entry(\"t\".into()).or_default();",
    "        let handle = lease.acquire();",
    "        let _ = lease.write_gate(&handle);",
    "        let _ = lease.release(&handle);",
    "    }",
    "    pub fn resize(&self) {",
    "        let master = self.registry.master_for(\"t\").unwrap();",
    "        let lease = self.leases.entry(\"t\".into()).or_default();",
    "        let handle = lease.acquire();",
    "        let _ = lease.write_gate(&handle);",
    "    }",
    "    pub fn kill_run(&self) {",
    "        if let Some(master) = self.registry.master_for(\"t\") { let _ = master.lock().process_group_leader(); }",
    "    }",
    "}",
  ].join("\n");
  const problems = leaseChannelProblems(hostWith({ path: EXECUTOR_PATH, text }), GOOD_CLI, GOOD_CORE);
  assert.ok(problems.some((p) => p.includes("lease.release must appear exactly once per channel fn")), problems.join("\n"));
});

test("a second pty_writers sink is refused", () => {
  const text = [
    "impl CliPtyExecutor {",
    "    pub fn input(&self) {",
    "        let writer = self.registry.pty_writers.lock().get(&run).cloned().unwrap();",
    "        let lease = self.leases.entry(\"t\".into()).or_default();",
    "        let handle = lease.acquire();",
    "        let _ = lease.write_gate(&handle);",
    "        let _ = lease.release(&handle);",
    "    }",
    "    pub fn resize(&self) {",
    "        let master = self.registry.master_for(\"t\").unwrap();",
    "        let lease = self.leases.entry(\"t\".into()).or_default();",
    "        let handle = lease.acquire();",
    "        let _ = lease.write_gate(&handle);",
    "        let _ = lease.release(&handle);",
    "    }",
    "    pub fn kill_run(&self) {",
    "        if let Some(master) = self.registry.master_for(\"t\") { let _ = master.lock().process_group_leader(); }",
    "        let w2 = self.registry.pty_writers.lock().get(&run).cloned().unwrap();",
    "        let _ = w2.lock().write_all(b\"x\");",
    "    }",
    "}",
  ].join("\n");
  const problems = leaseChannelProblems(hostWith({ path: EXECUTOR_PATH, text }), GOOD_CLI, GOOD_CORE);
  assert.ok(problems.some((p) => p.includes("pty_writers must be read exactly once")), problems.join("\n"));
});

test("a third master_for consumer is refused", () => {
  const text = [
    "impl CliPtyExecutor {",
    "    pub fn input(&self) {",
    "        let writer = self.registry.pty_writers.lock().get(&run).cloned().unwrap();",
    "        let lease = self.leases.entry(\"t\".into()).or_default();",
    "        let handle = lease.acquire();",
    "        let _ = lease.write_gate(&handle);",
    "        let _ = lease.release(&handle);",
    "    }",
    "    pub fn resize(&self) {",
    "        let master = self.registry.master_for(\"t\").unwrap();",
    "        let lease = self.leases.entry(\"t\".into()).or_default();",
    "        let handle = lease.acquire();",
    "        let _ = lease.write_gate(&handle);",
    "        let _ = lease.release(&handle);",
    "    }",
    "    pub fn kill_run(&self) {",
    "        if let Some(master) = self.registry.master_for(\"t\") { let _ = master.lock().process_group_leader(); }",
    "        let extra = self.registry.master_for(\"t2\");",
    "        let _ = extra.unwrap().lock().resize(portable_pty::PtySize { rows: 1, cols: 1, pixel_width: 0, pixel_height: 0 });",
    "    }",
    "}",
  ].join("\n");
  const problems = leaseChannelProblems(hostWith({ path: EXECUTOR_PATH, text }), GOOD_CLI, GOOD_CORE);
  assert.ok(problems.some((p) => p.includes("master_for must be used exactly twice")), problems.join("\n"));
});

test("an ungated second write_all inside executor.rs is refused", () => {
  const text = [
    "impl CliPtyExecutor {",
    "    pub fn input(&self) {",
    "        let writer = self.registry.pty_writers.lock().get(&run).cloned().unwrap();",
    "        let lease = self.leases.entry(\"t\".into()).or_default();",
    "        let handle = lease.acquire();",
    "        let _ = lease.write_gate(&handle);",
    "        let _ = lease.release(&handle);",
    "    }",
    "    pub fn resize(&self) {",
    "        let master = self.registry.master_for(\"t\").unwrap();",
    "        let lease = self.leases.entry(\"t\".into()).or_default();",
    "        let handle = lease.acquire();",
    "        let _ = lease.write_gate(&handle);",
    "        let _ = lease.release(&handle);",
    "    }",
    "    pub fn kill_run(&self) {",
    "        if let Some(master) = self.registry.master_for(\"t\") { let _ = master.lock().process_group_leader(); }",
    "    }",
    "    pub fn inject_prompt(&self) {",
    "        let writer = self.registry.pty_writers.lock().get(&run).cloned().unwrap();",
    "        let mut w = writer.lock();",
    "        let _ = w.write_all(b\"prompt\");",
    "    }",
    "}",
  ].join("\n");
  const problems = leaseChannelProblems(hostWith({ path: EXECUTOR_PATH, text }), GOOD_CLI, GOOD_CORE);
  assert.ok(problems.some((p) => p.includes("the only pty byte write inside")), problems.join("\n"));
});

test("a second executor.input funnel is refused", () => {
  const extra = {
    path: SERVICES_PATH,
    text: "fn a() { executor.input(t, &p); executor.input(t, &q); executor.resize(t, 80, 24); }",
  };
  const problems = leaseChannelProblems(hostWith(channelFile(), extra), GOOD_CLI, GOOD_CORE);
  assert.ok(problems.some((p) => p.includes("the host funnel must be exactly")), problems.join("\n"));
});

test("an unread core lease source is refused", () => {
  assert.ok(
    leaseChannelProblems(GOOD_HOST, GOOD_CLI, []).some((p) => p.includes("lease source was not read")),
  );
});

test("two core write_gate definitions are refused — one gate is the whole point", () => {
  const core = [
    {
      path: "core/crates/aeroric-core/src/terminal/lease.rs",
      text: "pub fn write_gate(&self, h: &LeaseHandle) -> Result<(), LeaseError> { Ok(()) }\npub fn write_gate(&self, h: &LeaseHandle) -> Result<(), LeaseError> { Ok(()) }\n",
    },
  ];
  const problems = leaseChannelProblems(GOOD_HOST, GOOD_CLI, core);
  assert.ok(problems.some((p) => p.includes("exactly once")), problems.join("\n"));
});

test("a mention inside a comment does not count as a call site", () => {
  const text = [
    "// lease.write_gate(&handle) is the one gate",
    "impl CliPtyExecutor {",
    "    pub fn input(&self) {",
    "        let lease = self.leases.entry(\"t\".into()).or_default();",
    "        let handle = lease.acquire();",
    "        let _ = lease.write_gate(&handle);",
    "        let _ = lease.release(&handle);",
    "    }",
    "}",
  ].join("\n");
  // 注释里的 write_gate 不算调用点——但缺 resize 通道的语料必须仍然红(计数不变量没有消失)。
  const problems = leaseChannelProblems(hostWith({ path: EXECUTOR_PATH, text }), GOOD_CLI, GOOD_CORE);
  assert.ok(problems.some((p) => p.includes("exactly 2")), problems.join("\n"));
});

test("an empty cli corpus is an error, never an absence", () => {
  assert.ok(
    leaseChannelProblems(GOOD_HOST, [], GOOD_CORE).some((p) => p.includes("no CLI .rs file was read")),
  );
});