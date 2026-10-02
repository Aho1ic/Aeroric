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

    // H. 终端重放面/租约面（P5-01，ADR-038）：plan_replay 承接回放与活流、
    // acquire/release 租约（过期票据 no-op）。
    console.log("Exercising terminal replay and lease rpc...");
    const getGenRes = runCli(["--data-dir", dataDir, "task", "get", taskId]);
    if (getGenRes.code !== 0) {
      throw new Error(`task get for generation failed: ${getGenRes.stderr}`);
    }
    // generation 在协议里是十进制字符串（Quantity 约定）。
    const generation = Number(JSON.parse(getGenRes.stdout)?.task?.generation);
    if (!Number.isFinite(generation) || generation <= 0) {
      throw new Error(`task.get did not report a usable generation: ${getGenRes.stdout}`);
    }
    const replayParamsFile = path.join(scratch, "plan-replay-params.json");
    writeFileSync(
      replayParamsFile,
      JSON.stringify({
        taskId,
        generation,
        cursor: { kind: "bytes", value: 0 },
      })
    );
    const replayRes = runCli([
      "--data-dir", dataDir, "rpc", "terminal.plan_replay",
      "--params-file", replayParamsFile,
    ]);
    if (replayRes.code !== 0) {
      throw new Error(`terminal.plan_replay failed: ${replayRes.stderr}`);
    }
    const replayJson = JSON.parse(replayRes.stdout);
    if (replayJson.kind !== "seamless") {
      throw new Error(`plan_replay kind = ${replayJson.kind}, expected seamless`);
    }
    const replayedBytes = Buffer.from(replayJson.bytes ?? "", "base64");
    if (replayedBytes.length === 0) {
      throw new Error("plan_replay returned no bytes for a run that produced output");
    }
    if (!Number.isFinite(Number(replayJson.resumeAfter)) || Number(replayJson.resumeAfter) <= 0) {
      throw new Error(`plan_replay resumeAfter unusable: ${replayJson.resumeAfter}`);
    }
    console.log(`plan_replay seamless: ${replayedBytes.length} byte(s), resumeAfter ${replayJson.resumeAfter}.`);

    // 活流尾巴：从 resumeAfter 续读——空尾巴也是合法应答（无新字节）。
    const tailParamsFile = path.join(scratch, "plan-replay-tail.json");
    writeFileSync(
      tailParamsFile,
      JSON.stringify({
        taskId,
        generation,
        cursor: { kind: "bytes", value: Number(replayJson.resumeAfter) },
      })
    );
    const tailRes = runCli([
      "--data-dir", dataDir, "rpc", "terminal.plan_replay",
      "--params-file", tailParamsFile,
    ]);
    if (tailRes.code !== 0) {
      throw new Error(`plan_replay tail failed: ${tailRes.stderr}`);
    }
    const tailJson = JSON.parse(tailRes.stdout);
    if (tailJson.kind !== "seamless") {
      throw new Error(`plan_replay tail kind = ${tailJson.kind}, expected seamless`);
    }
    console.log("plan_replay tail from resumeAfter verified.");

    // 租约：acquire 拿票据 → release 成功 → 再 release 同票据＝no-op false。
    const acquireRes = runCli([
      "--data-dir", dataDir, "rpc", "terminal.acquire_control",
      "--params-file", JSON.stringify({ taskId, generation }) && (() => {
        const p = path.join(scratch, "acquire-params.json");
        writeFileSync(p, JSON.stringify({ taskId, generation }));
        return p;
      })(),
    ]);
    if (acquireRes.code !== 0) {
      throw new Error(`terminal.acquire_control failed: ${acquireRes.stderr}`);
    }
    const ticket = JSON.parse(acquireRes.stdout)?.ticket;
    if (typeof ticket !== "number" || ticket <= 0) {
      throw new Error(`acquire_control did not mint a ticket: ${acquireRes.stdout}`);
    }
    const releaseParamsFile = path.join(scratch, "release-params.json");
    writeFileSync(releaseParamsFile, JSON.stringify({ taskId, generation, ticket }));
    const releaseRes = runCli([
      "--data-dir", dataDir, "rpc", "terminal.release_control",
      "--params-file", releaseParamsFile,
    ]);
    if (releaseRes.code !== 0 || JSON.parse(releaseRes.stdout)?.released !== true) {
      throw new Error(`release_control failed: ${releaseRes.stdout} ${releaseRes.stderr}`);
    }
    const releaseAgain = runCli([
      "--data-dir", dataDir, "rpc", "terminal.release_control",
      "--params-file", releaseParamsFile,
    ]);
    if (releaseAgain.code !== 0 || JSON.parse(releaseAgain.stdout)?.released !== false) {
      throw new Error("releasing a stale ticket should succeed with released=false");
    }
    console.log("Terminal lease acquire/release verified (stale ticket is a no-op).");

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
