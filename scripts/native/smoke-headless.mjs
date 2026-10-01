#!/usr/bin/env node
// `scripts/native/smoke-headless.mjs` — P3-07 Action 5: Headless CLI lifecycle smoke test.
// Creates temp data-dir, fake project, fake agent, exercises serve, run, input,
// cancel, resume, history, approval, and cleanly terminates and removes all resources in `finally`.

import { spawn, spawnSync } from "node:child_process";
import { chmodSync, existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..");
const cliBin = path.join(repoRoot, "core", "target", "debug", "aeroric-cli");

function ensureCliBinary() {
  if (!existsSync(cliBin)) {
    console.log("Compiling aeroric-cli binary...");
    const res = spawnSync("cargo", ["build", "--manifest-path", "core/Cargo.toml", "--locked", "-p", "aeroric-cli"], {
      cwd: repoRoot,
      stdio: "inherit",
    });
    if (res.status !== 0) {
      throw new Error(`Failed to build aeroric-cli (exit code: ${res.status})`);
    }
  }
}

async function sleep(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

function runCli(args, cwd = repoRoot) {
  const res = spawnSync(cliBin, args, {
    cwd,
    encoding: "utf8",
    env: { ...process.env },
    timeout: 30000,
  });
  return {
    code: res.status ?? -1,
    stdout: res.stdout ?? "",
    stderr: res.stderr ?? "",
  };
}

async function main() {
  ensureCliBinary();

  const scratch = `/tmp/ah-smoke-${process.pid}-${Date.now()}`;
  mkdirSync(scratch, { recursive: true });

  const dataDir = path.join(scratch, "data");
  mkdirSync(dataDir, { recursive: true });

  const projDir = path.join(scratch, "proj");
  mkdirSync(projDir, { recursive: true });

  // 1. 创建 fake-fast 和 fake-long 脚本
  const fastAgentScript = path.join(scratch, "fake-fast.sh");
  writeFileSync(
    fastAgentScript,
    "#!/bin/sh\necho 'HELLO FROM FAKE FAST AGENT'\nexit 0\n",
    { mode: 0o755 }
  );
  chmodSync(fastAgentScript, 0o755);

  const longAgentScript = path.join(scratch, "fake-long.sh");
  writeFileSync(
    longAgentScript,
    "#!/bin/sh\necho 'HELLO FROM FAKE LONG AGENT'\ntrap 'exit 130' TERM INT\nsleep 60 &\nwait $!\n",
    { mode: 0o755 }
  );
  chmodSync(longAgentScript, 0o755);

  // 2. 写入 settings.json
  const settingsPath = path.join(dataDir, "settings.json");
  writeFileSync(
    settingsPath,
    JSON.stringify(
      {
        custom_agents: [
          {
            id: "fake-fast",
            path: fastAgentScript,
            args: [],
            family: "claude",
            codex_like: false,
          },
          {
            id: "fake-long",
            path: longAgentScript,
            args: [],
            family: "claude",
            codex_like: false,
          },
        ],
      },
      null,
      2
    )
  );

  let serveProcess = null;

  try {
    console.log(`Starting headless host in ${dataDir}...`);
    serveProcess = spawn(cliBin, ["--data-dir", dataDir, "serve"], {
      cwd: repoRoot,
      stdio: ["ignore", "pipe", "pipe"],
      env: { ...process.env, HOME: scratch },
    });

    serveProcess.stderr.on("data", (chunk) => {
      // 保留诊断，调试时可用
    });

    const sockPath = path.join(dataDir, "host.sock");
    const tokenPath = path.join(dataDir, "host.token");
    let ready = false;
    for (let i = 0; i < 100; i++) {
      if (existsSync(sockPath) && existsSync(tokenPath)) {
        ready = true;
        break;
      }
      if (serveProcess.exitCode !== null) {
        throw new Error(`Host exited prematurely with code ${serveProcess.exitCode}`);
      }
      await sleep(50);
    }

    if (!ready) {
      throw new Error("Headless host did not become ready within 5 seconds");
    }
    console.log("Headless host is ready.");

    // A. project open
    console.log("Exercising `project open`...");
    const openRes = runCli(["--data-dir", dataDir, "project", "open", projDir]);
    if (openRes.code !== 0) {
      throw new Error(`project open failed (code ${openRes.code}): ${openRes.stderr}`);
    }
    console.log("Project opened successfully.");

    // B. run 到 settled (done)
    console.log("Exercising `run` with fake-fast agent...");
    const promptFile = path.join(scratch, "prompt.txt");
    writeFileSync(promptFile, "hello from smoke test\n");
    const runRes = runCli([
      "--data-dir",
      dataDir,
      "run",
      "--project",
      projDir,
      "--agent",
      "fake-fast",
      "--prompt-file",
      promptFile,
      "--json",
    ]);
    if (runRes.code !== 0) {
      throw new Error(`run failed (code ${runRes.code}): ${runRes.stderr}`);
    }
    const runOutput = JSON.parse(runRes.stdout.trim().split("\n")[0]);
    const taskId = runOutput.taskId;
    if (!taskId) {
      throw new Error(`Expected taskId in run output: ${runRes.stdout}`);
    }
    console.log(`Task ${taskId} completed to done.`);

    // C. task input
    console.log("Exercising `task input` (must handle retired/missing run gracefully)...");
    const inputFile = path.join(scratch, "input.txt");
    writeFileSync(inputFile, "user input\n");
    const inputRes = runCli([
      "--data-dir",
      dataDir,
      "task",
      "input",
      taskId,
      "--text-file",
      inputFile,
    ]);
    // 已经 done 的任务没有 live run 句柄，返回非零并带错误提示，绝不在无租约时假装写入
    if (inputRes.code === 0) {
      throw new Error("task input on completed task should not succeed without a live PTY");
    }
    console.log("Task input control verified.");

    // D. task resume
    console.log(`Exercising \`task resume\` for task ${taskId}...`);
    const resumeRes = runCli(["--data-dir", dataDir, "task", "resume", taskId]);
    if (resumeRes.code !== 0) {
      throw new Error(`task resume failed (code ${resumeRes.code}): ${resumeRes.stderr}`);
    }
    console.log("Task resumed successfully.");

    // E. run long task and cancel
    console.log("Exercising `task cancel` on long running task...");
    const rpcRunParamsFile = path.join(scratch, "run-long-params.json");
    const longTaskId = "long-task-" + Date.now();
    writeFileSync(
      rpcRunParamsFile,
      JSON.stringify({
        projectPath: projDir,
        agent: "fake-long",
        prompt: "stay running",
        taskId: longTaskId,
      })
    );
    const rpcStartRes = runCli([
      "--data-dir",
      dataDir,
      "rpc",
      "task.run",
      "--params-file",
      rpcRunParamsFile,
    ]);
    if (rpcStartRes.code !== 0) {
      throw new Error(`rpc task.run failed: ${rpcStartRes.stderr}`);
    }

    // 给进程起跑一点时间
    await sleep(300);

    const cancelRes = runCli(["--data-dir", dataDir, "task", "cancel", longTaskId]);
    if (cancelRes.code !== 0) {
      throw new Error(`task cancel failed (code ${cancelRes.code}): ${cancelRes.stderr}`);
    }

    // 等待状态变为 cancelled
    let cancelled = false;
    for (let i = 0; i < 50; i++) {
      const getRes = runCli(["--data-dir", dataDir, "task", "get", longTaskId]);
      if (getRes.code === 0) {
        const info = JSON.parse(getRes.stdout);
        if (info.task && info.task.status === "cancelled") {
          cancelled = true;
          break;
        }
      }
      await sleep(50);
    }
    if (!cancelled) {
      throw new Error(`Task ${longTaskId} was not observed as cancelled`);
    }
    console.log(`Task ${longTaskId} cancelled cleanly.`);

    // F. session list / history
    console.log("Exercising `session list` and `session history`...");
    const sessionListRes = runCli(["--data-dir", dataDir, "session", "list"]);
    if (sessionListRes.code !== 0) {
      throw new Error(`session list failed: ${sessionListRes.stderr}`);
    }
    // 未生成 transcript 的任务查询 history 明确返回 NotFound
    const sessionHistRes = runCli(["--data-dir", dataDir, "session", "history", taskId]);
    if (sessionHistRes.code === 0) {
      throw new Error("session history without on-disk transcript should return non-zero exit code");
    }
    console.log("Session operations verified.");

    // G. approval list
    console.log("Exercising `approval list`...");
    const appListRes = runCli(["--data-dir", dataDir, "approval", "list"]);
    if (appListRes.code !== 0) {
      throw new Error(`approval list failed: ${appListRes.stderr}`);
    }
    const appListJson = JSON.parse(appListRes.stdout);
    if (!Array.isArray(appListJson.approvals)) {
      throw new Error("approval list output missing 'approvals' array");
    }
    console.log("Approval operations verified.");

    console.log("Headless smoke checks passed cleanly.");
  } finally {
    if (serveProcess && serveProcess.exitCode === null) {
      console.log("Terminating serve process...");
      serveProcess.kill("SIGTERM");
      await new Promise((resolve) => {
        const timer = setTimeout(() => {
          serveProcess.kill("SIGKILL");
          resolve();
        }, 3000);
        serveProcess.once("exit", () => {
          clearTimeout(timer);
          resolve();
        });
      });
    }

    try {
      rmSync(scratch, { recursive: true, force: true });
    } catch {
      // ignore
    }
  }
}

main().catch((err) => {
  console.error("smoke-headless FAILED:", err);
  process.exit(1);
});
