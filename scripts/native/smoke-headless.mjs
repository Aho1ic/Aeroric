#!/usr/bin/env node
// `scripts/native/smoke-headless.mjs` — P3-07 Action 5: Headless CLI lifecycle smoke test.
// Creates temp data-dir, fake project, fake agent, exercises serve, run, input,
// cancel, resume, history, approval, and cleanly terminates and removes all resources in `finally`.

import assert from "node:assert/strict";
import { spawn, spawnSync } from "node:child_process";
import { chmodSync, existsSync, linkSync, lstatSync, mkdirSync, readFileSync, readdirSync, readlinkSync, realpathSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
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
            label: "Fake fast",
            path: fastAgentScript,
            args: [],
            family: "claude",
            codex_like: false,
            username: "synthetic-legacy-user",
            password: "synthetic-legacy-pass",
            future_profile_field: { username: "not-a-legacy-field" },
          },
          {
            id: "fake-long",
            label: "Fake long",
            path: longAgentScript,
            args: [],
            family: "claude",
            codex_like: false,
          },
        ],
        future_smoke_field: { retained: true },
        agent_proxy_enabled: { claude: false, " OMP ": true },
        agent_proxy_overrides: {
          " CLAUDE ": { enabled: true, url: "legacy.invalid:7890" },
        },
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
      env: {
        ...process.env, HOME: scratch, XDG_CONFIG_HOME: path.join(scratch, "config"),
        AERORIC_HOME: path.join(scratch, "unused-global-data"),
        GIT_CONFIG_GLOBAL: path.join(scratch, "empty-gitconfig"), GIT_CONFIG_NOSYSTEM: "1",
      },
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

    // G43: exercise the real generic CLI -> authenticated host -> core search
    // path, not a second test-only dispatcher. All inputs remain synthetic.
    const searchFile = path.join(projDir, "search-smoke.txt");
    writeFileSync(searchFile, "İ title\n");
    const searchParams = path.join(scratch, "search-params.json");
    const searchRpc = (method, params, expectSuccess = true) => {
      writeFileSync(searchParams, JSON.stringify(params));
      const reply = runCli(["--data-dir", dataDir, "rpc", method, "--params-file", searchParams], scratch);
      if ((reply.code === 0) !== expectSuccess) {
        throw new Error(`${method} unexpected exit ${reply.code}: ${reply.stderr}`);
      }
      return expectSuccess ? JSON.parse(reply.stdout) : reply;
    };
    // Slice 21: the intrinsic object wire and the frozen legacy bare-array
    // wire are distinct contracts over the same core read, not shape aliases.
    const projectsPath = path.join(dataDir, "projects.json");
    const projectsBeforeQuery = readFileSync(projectsPath, "utf8");
    const listedProjects = searchRpc("project.list", {});
    assert(Array.isArray(listedProjects.projects), "project.list must retain its object wrapper");
    assert.deepEqual(listedProjects.projects.map((project) => project.path), [projDir]);
    assert.equal(readFileSync(projectsPath, "utf8"), projectsBeforeQuery);
    const legacyProjects = searchRpc("project.load_projects", {});
    assert(Array.isArray(legacyProjects), "project.load_projects must return a bare array");
    assert.deepEqual(legacyProjects, listedProjects.projects);
    assert.equal(readFileSync(projectsPath, "utf8"), projectsBeforeQuery);
    console.log("Both project-list CLI wires and unchanged project file verified.");

    // Slice39: all agent-config calls are scratch-only, including the old
    // malformed-settings branch (claude_gpt55 has no default config path).
    const agentConfigSettingsBefore = readFileSync(settingsPath);
    const agentConfigFile = path.join(scratch, "agent config 中文.txt");
    const agentConfigParams = { agent: "claude_gpt55", content: "raw\n配置\0\r\n" };
    const agentConfigMethods = [
      "settings.get_agent_config_file_path",
      "settings.read_agent_config_file",
      "settings.write_agent_config_file",
    ];
    const agentConfigSettings = {
      ...JSON.parse(agentConfigSettingsBefore.toString()),
      claude_gpt55_config_path: agentConfigFile,
    };
    try {
      writeFileSync(settingsPath, JSON.stringify(agentConfigSettings));
      assert.equal(searchRpc(agentConfigMethods[0], agentConfigParams), agentConfigFile);
      assert.equal(searchRpc(agentConfigMethods[1], agentConfigParams), null);
      assert(!existsSync(agentConfigFile), "read must not initialize an agent config");
      assert.equal(searchRpc(agentConfigMethods[2], agentConfigParams), null);
      assert.equal(readFileSync(agentConfigFile, "utf8"), agentConfigParams.content);
      assert.equal(searchRpc(agentConfigMethods[1], agentConfigParams), agentConfigParams.content);
      const entriesBefore = readdirSync(scratch).sort();
      for (const patch of [
        { custom_agents: false }, { proxy_settings: null }, { send_shortcut: 42 },
      ]) {
        writeFileSync(settingsPath, JSON.stringify({ ...agentConfigSettings, ...patch }));
        const settingsBefore = readFileSync(settingsPath);
        for (const method of agentConfigMethods) {
          const refused = searchRpc(method, agentConfigParams, false);
          assert.match(refused.stderr, /io.?error/i);
          assert.match(refused.stderr, /invalid stored app settings/);
          assert.deepEqual(readFileSync(settingsPath), settingsBefore);
          assert.equal(readFileSync(agentConfigFile, "utf8"), agentConfigParams.content);
          assert.deepEqual(readdirSync(scratch).sort(), entriesBefore);
        }
      }
      // Params fail before stored-settings decoding, even while it is corrupt.
      for (const method of agentConfigMethods) {
        const refused = searchRpc(method, { agent: 42, content: "ignored" }, false);
        assert.match(refused.stderr, /invalid.?argument/i);
      }
      writeFileSync(settingsPath, JSON.stringify(agentConfigSettings));
      assert.equal(searchRpc(agentConfigMethods[1], agentConfigParams), agentConfigParams.content);
      assert.equal(searchRpc(agentConfigMethods[2], { ...agentConfigParams, content: "" }), null);
      assert.equal(readFileSync(agentConfigFile, "utf8"), "");
      // Slice41: the shipping atomic writer replaces the named leaf, not its
      // symlink/hardlink target. These targets are all independent scratch data.
      for (const kind of ["file", "dangling", "directory", "hardlink"]) {
        const root = path.join(scratch, "agent-config-parity", kind);
        mkdirSync(root, { recursive: true });
        const target = path.join(root, "target");
        const leaf = path.join(root, "config");
        if (kind === "directory") {
          mkdirSync(target);
          writeFileSync(path.join(target, "sentinel"), "keep directory");
        } else if (kind !== "dangling") {
          writeFileSync(target, "keep target");
        }
        if (kind === "hardlink") linkSync(target, leaf);
        else symlinkSync(target, leaf);
        const fields = JSON.stringify({ ...agentConfigSettings, claude_gpt55_config_path: leaf });
        writeFileSync(settingsPath, fields);
        assert.equal(searchRpc(agentConfigMethods[2], agentConfigParams), null);
        assert.equal(readFileSync(leaf, "utf8"), agentConfigParams.content);
        assert(lstatSync(leaf).isFile() && !lstatSync(leaf).isSymbolicLink());
        if (kind === "directory") {
          assert.equal(readFileSync(path.join(target, "sentinel"), "utf8"), "keep directory");
        } else if (kind === "dangling") {
          assert(!existsSync(target), "atomic config write must not create a dangling target");
        } else {
          assert.equal(readFileSync(target, "utf8"), "keep target");
        }
        assert.deepEqual(readdirSync(root).sort(), kind === "dangling" ? ["config"] : ["config", "target"]);
        assert.equal(readFileSync(settingsPath, "utf8"), fields);
      }
    } finally {
      writeFileSync(settingsPath, agentConfigSettingsBefore);
    }
    assert.deepEqual(readFileSync(settingsPath), agentConfigSettingsBefore);
    console.log("Agent-config CLI typed refusal, raw wire, legacy leaf replacement, no repair and recovery verified.");

    // Slice42: the CLI supplies only a config-home port, while managed config
    // roots follow --data-dir. All old ambient/global alternatives are decoys.
    const rootConfigSettingsBefore = readFileSync(settingsPath);
    const defaultConfigs = [
      ["claude", path.join(scratch, ".claude", "settings.json")],
      ["codex", path.join(scratch, ".codex", "config.toml")],
      ["dsh", path.join(dataDir, "agent-homes", "local_dsh", "settings.yaml")],
      ["omp", path.join(dataDir, "agent-homes", "omp", "config.yml")],
    ];
    const rootDecoys = [
      path.join(scratch, ".aeroric", "agent-homes", "omp", "config.yml"),
      path.join(scratch, "unused-global-data", "agent-homes", "local_dsh", "settings.yaml"),
    ];
    const rootFixtureDirectories = [
      path.join(scratch, ".claude"), path.join(scratch, ".codex"),
      path.join(dataDir, "agent-homes"), path.join(scratch, ".aeroric"),
      path.join(scratch, "unused-global-data"),
    ];
    for (const dir of rootFixtureDirectories) assert(!existsSync(dir), `root fixture already exists: ${dir}`);
    try {
      writeFileSync(settingsPath, "{}");
      for (const file of rootDecoys) {
        mkdirSync(path.dirname(file), { recursive: true });
        writeFileSync(file, "DECOY: unchanged");
      }
      for (const [, file] of defaultConfigs) {
        mkdirSync(path.dirname(file), { recursive: true });
        writeFileSync(file, "CONTEXT default");
      }
      // Do not read or write until all paths match scratch: a partially wired
      // CLI must not turn its unrelated placeholder home into /.claude IO.
      for (const [agent, file] of defaultConfigs) {
        assert.equal(searchRpc(agentConfigMethods[0], { agent }), file);
      }
      for (const [agent, file] of defaultConfigs) {
        assert.equal(searchRpc(agentConfigMethods[1], { agent }), "CONTEXT default");
        assert.equal(searchRpc(agentConfigMethods[2], { agent, content: "root raw\0配置\r\n" }), null);
        assert.equal(readFileSync(file, "utf8"), "root raw\0配置\r\n");
      }
      assert.equal(readFileSync(settingsPath, "utf8"), "{}");
      for (const file of rootDecoys) assert.equal(readFileSync(file, "utf8"), "DECOY: unchanged");
      assert(!existsSync(path.join(dataDir, "agent-homes", "dsh")));
      assert(!existsSync(path.join(dataDir, "agent-homes", "local_omp")));
    } finally {
      writeFileSync(settingsPath, rootConfigSettingsBefore);
      // Restore this stage's fixture layout too: the later preference checks
      // must still prove that no agent-home directory is created by a query.
      for (const dir of rootFixtureDirectories) {
        if (existsSync(dir)) rmSync(dir, { recursive: true });
      }
    }
    assert.deepEqual(readFileSync(settingsPath), rootConfigSettingsBefore);
    console.log("Agent-config CLI injected official/managed defaults and untouched global decoys verified.");

    // Slice28: history-only trim parity and sequential, potentially partial
    // deletion. These orphan fixtures are not the fake tasks launched below.
    const historyDirectory = path.join(dataDir, "terminal-history");
    mkdirSync(historyDirectory, { recursive: true });
    const historyEntriesBefore = readdirSync(historyDirectory).sort();
    const historyFirst = path.join(historyDirectory, "cleanup-first.log");
    const historyLater = path.join(historyDirectory, "cleanup-later.log");
    const historyDecoy = path.join(historyDirectory, " cleanup-first .log");
    const historyBlocked = path.join(historyDirectory, "cleanup-blocked.log");
    writeFileSync(historyFirst, "canonical cleanup bytes\r\n");
    writeFileSync(historyLater, "later cleanup bytes");
    writeFileSync(historyDecoy, "padded-name decoy stays");
    assert.equal(searchRpc("project.read_task_terminal_history", {
      taskId: " cleanup-first ",
    }), "canonical cleanup bytes\r\n");
    const malformedHistoryDelete = searchRpc("project.delete_task_terminal_histories", {
      taskIds: ["cleanup-first", 42, "cleanup-later"],
    }, false);
    assert.match(malformedHistoryDelete.stderr, /invalid.?argument/i);
    assert.equal(readFileSync(historyFirst, "utf8"), "canonical cleanup bytes\r\n");
    const partialHistoryDelete = searchRpc("project.delete_task_terminal_histories", {
      taskIds: ["cleanup-first", "../invalid", "cleanup-later"],
    }, false);
    assert.match(partialHistoryDelete.stderr, /invalid.?argument/i);
    assert(!existsSync(historyFirst), "a later invalid ID does not undo earlier deletion");
    assert.equal(readFileSync(historyLater, "utf8"), "later cleanup bytes");
    writeFileSync(historyFirst, "canonical cleanup bytes\r\n");
    assert.equal(searchRpc("project.delete_task_terminal_histories", {
      taskIds: [" cleanup-first ", "cleanup-first", "cleanup-missing"],
    }), null);
    assert.equal(searchRpc("project.read_task_terminal_history", { taskId: "cleanup-first" }), "");
    assert.equal(readFileSync(historyDecoy, "utf8"), "padded-name decoy stays");
    mkdirSync(historyBlocked);
    writeFileSync(path.join(historyBlocked, "sentinel"), "keep directory contents");
    const directoryHistoryDelete = searchRpc("project.delete_task_terminal_histories", {
      taskIds: ["cleanup-blocked", "cleanup-later"],
    }, false);
    assert.match(directoryHistoryDelete.stderr, /io.?error/i);
    assert.equal(readFileSync(path.join(historyBlocked, "sentinel"), "utf8"), "keep directory contents");
    assert.equal(readFileSync(historyLater, "utf8"), "later cleanup bytes");
    assert.equal(searchRpc("project.delete_task_terminal_histories", { taskIds: [] }), null);
    assert.equal(readFileSync(projectsPath, "utf8"), projectsBeforeQuery);
    // Only remove our synthetic fixtures; never clean the host's other logs.
    for (const fixture of [historyLater, historyDecoy, historyBlocked]) rmSync(fixture, { recursive: true });
    assert.deepEqual(readdirSync(historyDirectory).sort(), historyEntriesBefore);
    console.log("History CLI trim parity, deletion/null wire, strict params and partial failures verified.");

    // Slice 26: read a project-local config through the actual frozen RPC.
    // Keep these fixtures separate from the fake task's cwd and agent settings.
    const missingConfigProject = path.join(scratch, "missing-config-project");
    const defaultConfig = searchRpc("settings.read_project_config", {
      projectPath: missingConfigProject,
    });
    assert.equal(defaultConfig.agent.default, "claude");
    assert.equal(defaultConfig.agent.default_permission_mode, "full_access");
    assert.equal(defaultConfig.editor.format_on_save, false);
    assert(!existsSync(missingConfigProject), "read must not initialize a project");
    const configProject = path.join(scratch, " config fixture ");
    const configDirectory = path.join(configProject, ".aeroric");
    const configFile = path.join(configDirectory, "config.toml");
    mkdirSync(configDirectory, { recursive: true });
    const configText = '[agent]\ndefault = "fixture-agent"\nprompt_prefix = "只读"\n[git]\ncommit_prompt = "fixture prompt"\ncommit_message_timeout_secs = 42\n';
    writeFileSync(configFile, configText);
    const configRead = searchRpc("settings.read_project_config", { projectPath: configProject });
    assert.deepEqual(configRead, {
      agent: { default: "fixture-agent", default_permission_mode: "full_access", prompt_prefix: "只读" },
      git: { commit_prompt: "fixture prompt", commit_message_timeout_secs: 42 },
      editor: {
        format_on_save: false,
        file_browser_sort: { field: "modified", direction: "desc" },
        sftp_sort: { field: "modified", direction: "desc" },
      },
    });
    assert.equal(readFileSync(configFile, "utf8"), configText);
    assert(!existsSync(path.join(configDirectory, "attachments")));
    writeFileSync(configFile, "[malformed");
    assert.deepEqual(
      searchRpc("settings.read_project_config", { projectPath: configProject }),
      defaultConfig,
    );
    assert.equal(readFileSync(configFile, "utf8"), "[malformed");
    const invalidConfigParam = searchRpc("settings.read_project_config", { projectPath: 42 }, false);
    assert.match(invalidConfigParam.stderr, /invalid.?argument/i);
    writeFileSync(configFile, Buffer.from([0xff, 0xfe]));
    const invalidConfigUtf8 = searchRpc("settings.read_project_config", { projectPath: configProject }, false);
    assert.match(invalidConfigUtf8.stderr, /io.?error/i);
    assert.deepEqual(readFileSync(configFile), Buffer.from([0xff, 0xfe]));
    console.log("Project config CLI wire, defaults, no-write reads and refusal verified.");

    // Slice27: real frozen mutations in separate scratch projects, never settings/HOME.
    assert.deepEqual(searchRpc("settings.init_project_config", {
      projectPath: missingConfigProject,
    }), defaultConfig);
    const initializedDirectory = path.join(missingConfigProject, ".aeroric");
    const initializedFile = path.join(initializedDirectory, "config.toml");
    assert.deepEqual(readdirSync(initializedDirectory).sort(), ["attachments", "config.toml"]);
    const templateBytes = readFileSync(initializedFile);
    assert.deepEqual(searchRpc("settings.init_project_config", {
      projectPath: missingConfigProject,
    }), defaultConfig);
    assert.deepEqual(readFileSync(initializedFile), templateBytes, "init must not replace existing content");
    const savedConfigProject = path.join(scratch, " CLI 保存  ");
    const savedConfigDirectory = path.join(savedConfigProject, ".aeroric");
    const savedConfig = {
      ...configRead,
      agent: { ...configRead.agent, prompt_prefix: "保存\nsecond line" },
      editor: { ...configRead.editor, format_on_save: true },
    };
    assert.equal(searchRpc("settings.write_project_config", {
      projectPath: savedConfigProject, config: savedConfig,
    }), null);
    assert.deepEqual(searchRpc("settings.read_project_config", {
      projectPath: savedConfigProject,
    }), savedConfig);
    assert.deepEqual(readdirSync(savedConfigDirectory), ["config.toml"], "write does not create attachments");
    assert(!existsSync(savedConfigProject.trimEnd()), "the original path is not normalized");
    const savedBytes = readFileSync(path.join(savedConfigDirectory, "config.toml"));
    const refusedConfigWrite = searchRpc("settings.write_project_config", {
      projectPath: savedConfigProject, config: { agent: 4 },
    }, false);
    assert.match(refusedConfigWrite.stderr, /invalid.?argument/i);
    assert.deepEqual(readFileSync(path.join(savedConfigDirectory, "config.toml")), savedBytes);
    const refusedInit = searchRpc("settings.init_project_config", { projectPath: 42 }, false);
    assert.match(refusedInit.stderr, /invalid.?argument/i);
    console.log("Project config CLI init/write, roundtrip, original paths and refused writes verified.");

    // Slice30: project-config parents also take ADR-006 targets. None may
    // read/write/init local data on behalf of an unsupported remote target.
    const configTargetAbsent = path.join(scratch, "config target must stay absent ");
    for (const method of ["settings.read_project_config", "settings.init_project_config", "settings.write_project_config"]) {
      for (const target of [
        { kind: "ssh", connectionId: "synthetic", remotePath: "/srv/fake" },
        { kind: "wsl", distribution: "synthetic", linuxPath: "/srv/fake" },
      ]) {
        const refusedTarget = searchRpc(method, {
          projectPath: savedConfigProject, config: savedConfig, target,
        }, false);
        assert.match(refusedTarget.stderr, /unsupported.?capability/i);
        const refusedAbsent = searchRpc(method, {
          projectPath: configTargetAbsent, config: savedConfig, target,
        }, false);
        assert.match(refusedAbsent.stderr, /unsupported.?capability/i);
        assert(!existsSync(configTargetAbsent));
      }
      for (const target of [null, { kind: "local", path: savedConfigProject.trimEnd() }]) {
        const refusedTarget = searchRpc(method, {
          projectPath: savedConfigProject, config: savedConfig, target,
        }, false);
        assert.match(refusedTarget.stderr, /invalid.?argument/i);
      }
      assert.deepEqual(readFileSync(path.join(savedConfigDirectory, "config.toml")), savedBytes);
      assert.deepEqual(readdirSync(savedConfigDirectory), ["config.toml"], "refused init must not create attachments");
    }
    assert.deepEqual(searchRpc("settings.read_project_config", {
      projectPath: savedConfigProject, target: { kind: "local", path: savedConfigProject },
    }), savedConfig);
    console.log("Project config CLI remote/invalid/conflicting-target refusals and exact-local control verified.");

    // Slice29: definition IO only, separate from the fake lifecycle project's cwd.
    const runConfigProject = path.join(scratch, "run config 中文 ");
    const runConfigDirectory = path.join(runConfigProject, ".aeroric");
    const runConfigFile = path.join(runConfigDirectory, "run-configs.json");
    const mustNotExecute = path.join(runConfigProject, "must-not-execute");
    mkdirSync(runConfigProject);
    assert.deepEqual(searchRpc("ide.read_run_configs", { projectPath: runConfigProject }), {
      version: 1, configs: [],
    });
    assert(!existsSync(runConfigDirectory), "definition query must not initialize the directory");
    const runConfigInput = {
      version: 99, configs: [
        { type: "shell", id: "same", name: "fixture", command: `touch '${mustNotExecute}'`, cwd: ".", env: {} },
        { type: "debug", id: "same", name: "debug", debugType: "python", program: "missing.py", cwd: ".",
          args: [], env: {}, breakpoints: [{ file: "missing.py", line: 3, column: 1 }] },
      ],
    };
    const runConfigSaved = { ...runConfigInput, version: 1 };
    assert.deepEqual(searchRpc("ide.write_run_configs", {
      projectPath: runConfigProject, document: runConfigInput,
    }), runConfigSaved, "write returns the normalized document, not null");
    const runConfigBytes = readFileSync(runConfigFile);
    assert.deepEqual(searchRpc("ide.read_run_configs", {
      projectPath: runConfigProject, target: { kind: "local", path: runConfigProject },
    }), runConfigSaved);
    assert.deepEqual(readFileSync(runConfigFile), runConfigBytes);
    assert.deepEqual(readdirSync(runConfigDirectory), ["run-configs.json"]);
    assert(!existsSync(runConfigProject.trimEnd()), "root must preserve original spaces");
    assert(!existsSync(mustNotExecute) && !existsSync(path.join(runConfigProject, "missing.py")));
    for (const version of [0, 99]) {
      const raw = JSON.stringify({ ...runConfigInput, version });
      writeFileSync(runConfigFile, raw);
      assert.deepEqual(searchRpc("ide.read_run_configs", { projectPath: runConfigProject }),
        { ...runConfigInput, version: version || 1 });
      assert.equal(readFileSync(runConfigFile, "utf8"), raw, "normalization is returned, not persisted");
    }
    writeFileSync(runConfigFile, runConfigBytes);
    const invalidRunConfig = searchRpc("ide.write_run_configs", {
      projectPath: runConfigProject,
      document: { version: 99, configs: [{ ...runConfigInput.configs[0], command: " " }] },
    }, false);
    assert.match(invalidRunConfig.stderr, /invalid.?argument/i);
    const invalidRunShape = searchRpc("ide.write_run_configs", {
      projectPath: runConfigProject, document: null,
    }, false);
    assert.match(invalidRunShape.stderr, /invalid.?argument/i);
    for (const method of ["ide.read_run_configs", "ide.write_run_configs"]) {
      const refusedTarget = searchRpc(method, {
        projectPath: runConfigProject, document: runConfigInput,
        target: { kind: "ssh", connectionId: "synthetic", remotePath: "/srv/fake" },
      }, false);
      assert.match(refusedTarget.stderr, /unsupported.?capability/i);
      const refusedRoot = searchRpc(method, { projectPath: "relative", document: runConfigInput }, false);
      assert.match(refusedRoot.stderr, /invalid.?argument/i);
      assert.deepEqual(readFileSync(runConfigFile), runConfigBytes);
    }
    writeFileSync(runConfigFile, Buffer.from([0xff, 0xfe]));
    const runConfigPoison = searchRpc("ide.read_run_configs", { projectPath: runConfigProject }, false);
    assert.match(runConfigPoison.stderr, /io.?error/i);
    assert.deepEqual(readFileSync(runConfigFile), Buffer.from([0xff, 0xfe]));
    assert.deepEqual(searchRpc("ide.write_run_configs", {
      projectPath: runConfigProject, document: {},
    }), { version: 1, configs: [] });
    assert(!existsSync(mustNotExecute), "definition IO must never execute the configured shell");
    console.log("Run-config CLI definition read/write, lowercase wire, defaults/versions and target/error refusals verified.");

    // Slice31: debug definitions are not a request to start/attach a debugger.
    const debugProject = path.join(scratch, "debug config 中文 ");
    const debugDirectory = path.join(debugProject, ".aeroric");
    const debugFile = path.join(debugDirectory, "debug-configs.json");
    mkdirSync(debugProject);
    assert.deepEqual(searchRpc("ide.read_debug_configs", { projectPath: debugProject }),
      { version: 1, configs: [] });
    assert(!existsSync(debugDirectory));
    const debugInput = {
      version: 99,
      configs: [
        { id: "same", name: "node launch", type: "node", request: "launch",
          program: "missing.js", cwd: "missing-dir/..", attachHost: "127.0.0.1",
          args: [], env: { LANGUAGE: "中文" }, breakpoints: [] },
        { id: "same", name: "python attach", type: "python", request: "attach",
          program: "", cwd: ".", attachHost: " synthetic.local ", attachPort: 5678,
          args: [], env: {}, breakpoints: [
            { file: "missing.py", line: 3, column: 1, condition: "count > 0", logMessage: "中文 {count}" },
          ] },
      ],
    };
    const debugSaved = { ...debugInput, version: 1 };
    assert.deepEqual(searchRpc("ide.write_debug_configs", {
      projectPath: debugProject, document: debugInput,
    }), debugSaved, "debug write returns the normalized document, not null");
    const debugBytes = readFileSync(debugFile);
    assert.deepEqual(searchRpc("ide.read_debug_configs", {
      projectPath: debugProject, target: { kind: "local", path: debugProject },
    }), debugSaved);
    for (const version of [0, 99]) {
      const raw = JSON.stringify({ ...debugInput, version });
      writeFileSync(debugFile, raw);
      assert.deepEqual(searchRpc("ide.read_debug_configs", { projectPath: debugProject }),
        { ...debugInput, version: version || 1 });
      assert.equal(readFileSync(debugFile, "utf8"), raw);
    }
    writeFileSync(debugFile, debugBytes);
    for (const document of [
      null,
      { configs: [{ ...debugInput.configs[0], type: "Node" }] },
      { configs: [{ ...debugInput.configs[1], attachPort: 0 }] },
      { configs: [{ ...debugInput.configs[0], cwd: "../outside" }] },
    ]) {
      assert.match(searchRpc("ide.write_debug_configs", {
        projectPath: debugProject, document,
      }, false).stderr, /invalid.?argument/i);
    }
    for (const method of ["ide.read_debug_configs", "ide.write_debug_configs"]) {
      for (const target of [
        { kind: "ssh", connectionId: "synthetic", remotePath: "/srv/fake" },
        { kind: "wsl", distribution: "synthetic", linuxPath: "/srv/fake" },
      ]) {
        assert.match(searchRpc(method, {
          projectPath: debugProject, document: debugInput, target,
        }, false).stderr, /unsupported.?capability/i);
      }
      for (const target of [null, { kind: "local", path: debugProject.trimEnd() }]) {
        assert.match(searchRpc(method, {
          projectPath: debugProject, document: debugInput, target,
        }, false).stderr, /invalid.?argument/i);
      }
      assert.deepEqual(readFileSync(debugFile), debugBytes);
    }
    writeFileSync(debugFile, Buffer.from([0xff, 0xfe]));
    assert.match(searchRpc("ide.read_debug_configs", {
      projectPath: debugProject,
    }, false).stderr, /io.?error/i);
    assert.deepEqual(readFileSync(debugFile), Buffer.from([0xff, 0xfe]));
    assert.deepEqual(searchRpc("ide.write_debug_configs", {
      projectPath: debugProject, document: {},
    }), { version: 1, configs: [] });
    assert.deepEqual(readdirSync(debugDirectory), ["debug-configs.json"]);
    assert(!existsSync(debugProject.trimEnd()) && !existsSync(path.join(debugProject, "missing-dir")));
    assert(!existsSync(path.join(debugProject, "missing.js")) && !existsSync(path.join(debugProject, "missing.py")));
    console.log("Debug-config CLI definition wire, launch/attach defaults, versions and target/validation refusals verified.");

    // Slice32: current-writer controls for the existing file RPCs. No OS
    // Trash (covered with injected quarantine in real-host tests), and .txt
    // formatting cannot execute a formatter even on the old failure path.
    const fileProjectId = legacyProjects[0].id;
    const fileFixture = "file-authority-smoke";
    const fileParams = (extra) => ({ projectId: fileProjectId, ...extra });
    const filePath = `${fileFixture}/fixture.txt`;
    assert.deepEqual(searchRpc("files.create_directory", fileParams({ path: fileFixture })), { created: true });
    assert.deepEqual(searchRpc("files.create_file", fileParams({ path: filePath })), { created: true });
    assert.deepEqual(searchRpc("files.write_file", fileParams({
      path: filePath, content: "file authority original\n",
    })), { written: true });
    assert.deepEqual(searchRpc("files.read_file", fileParams({ path: filePath })),
      { content: "file authority original\n" });
    const fileHistory = path.join(projDir, ".aeroric/local-history", filePath);
    mkdirSync(fileHistory, { recursive: true });
    writeFileSync(path.join(fileHistory, "1700000000000.txt"), "file authority older\n");
    assert.equal(searchRpc("files.restore_local_history_entry", fileParams({
      path: filePath, entryId: "1700000000000",
    })).content, "file authority older\n");
    assert.equal(searchRpc("files.list_local_history", fileParams({ path: filePath })).length, 2);
    assert.equal(searchRpc("files.read_local_history_entry", fileParams({
      path: filePath, entryId: "1700000000000",
    })).content, "file authority older\n");
    assert.deepEqual(searchRpc("files.rename_path", fileParams({
      path: filePath, newName: "renamed.txt",
    })), { renamed: true });
    const renamedFile = `${fileFixture}/renamed.txt`;
    const copiedFiles = searchRpc("files.copy_paths_to_directory", fileParams({
      paths: [renamedFile], targetDir: `${fileFixture}/copies`,
    }));
    assert.deepEqual(copiedFiles, {
      copied: [realpathSync(path.join(projDir, fileFixture, "copies/renamed.txt"))],
    });
    // Slice34: "outside" is only another scratch directory. Both linked
    // ancestors and dangling destination leaves must refuse before writing.
    const copyOutside = path.join(scratch, "copy-outside");
    mkdirSync(copyOutside);
    writeFileSync(path.join(copyOutside, "sentinel"), "outside unchanged");
    symlinkSync(copyOutside, path.join(projDir, fileFixture, "outside-link"), "dir");
    for (const targetDir of [`${fileFixture}/outside-link`, `${fileFixture}/outside-link/new/nested`]) {
      assert.match(searchRpc("files.copy_paths_to_directory", fileParams({
        paths: [renamedFile], targetDir,
      }), false).stderr, /invalid.?argument/i);
      assert.deepEqual(readdirSync(copyOutside), ["sentinel"]);
    }
    const danglingDirectory = path.join(projDir, fileFixture, "dangling-output");
    mkdirSync(danglingDirectory);
    const outsideLeaf = path.join(copyOutside, "must-not-exist");
    symlinkSync(outsideLeaf, path.join(danglingDirectory, "renamed.txt"));
    assert.match(searchRpc("files.copy_paths_to_directory", fileParams({
      paths: [renamedFile], targetDir: `${fileFixture}/dangling-output`,
    }), false).stderr, /Refusing to overwrite/);
    assert.equal(readlinkSync(path.join(danglingDirectory, "renamed.txt")), outsideLeaf);
    assert.deepEqual(readdirSync(copyOutside), ["sentinel"]);
    assert.equal(readFileSync(path.join(copyOutside, "sentinel"), "utf8"), "outside unchanged");
    const canonicalFileFixture = realpathSync(path.join(projDir, fileFixture));
    symlinkSync(path.join(canonicalFileFixture, "copies"), path.join(projDir, fileFixture, "inside-link"), "dir");
    assert.deepEqual(searchRpc("files.copy_paths_to_directory", fileParams({
      paths: [renamedFile], targetDir: `${fileFixture}/inside-link/new/nested`,
    })), { copied: [path.join(canonicalFileFixture, "inside-link/new/nested/renamed.txt")] });
    assert.equal(readFileSync(path.join(projDir, renamedFile), "utf8"), "file authority older\n");
    assert.match(searchRpc("files.format_file", fileParams({
      path: path.join(projDir, renamedFile),
    }), false).stderr, /No formatter is configured/);
    assert.equal(readFileSync(path.join(projDir, renamedFile), "utf8"), "file authority older\n");
    assert.equal(searchRpc("files.list_local_history", fileParams({ path: renamedFile })).length, 1);
    assert.deepEqual(searchRpc("files.read_dir", fileParams({ path: fileFixture }))
      .entries.map((entry) => entry.name).sort(), ["copies", "dangling-output", "inside-link", "outside-link", "renamed.txt"]);
    assert.equal(readFileSync(projectsPath, "utf8"), projectsBeforeQuery);
    rmSync(path.join(projDir, fileFixture), { recursive: true });
    rmSync(path.join(projDir, ".aeroric/local-history", fileFixture), { recursive: true });
    console.log("File mutation CLI writer controls, copy symlink refusals, history/read wire and safe formatter refusal verified.");

    // Slice35: only scratch siblings are used for outside referents. An
    // existing/dangling leaf must not bypass the parent-only path check.
    const leafFixture = "file-leaf-smoke";
    const leafDirectory = path.join(projDir, leafFixture);
    const leafOutside = path.join(scratch, "leaf-outside");
    mkdirSync(leafDirectory);
    mkdirSync(leafOutside);
    writeFileSync(path.join(leafOutside, "sentinel.txt"), "outside leaf unchanged\n");
    writeFileSync(path.join(leafDirectory, "target.txt"), "original\n");
    const leafTargets = [
      ["outside-link.txt", path.join(leafOutside, "sentinel.txt")],
      ["outside-dangling.txt", path.join(leafOutside, "must-not-exist")],
      ["inside-dangling.txt", path.join(leafDirectory, "must-not-exist")],
    ];
    for (const [name, target] of leafTargets) {
      symlinkSync(target, path.join(leafDirectory, name));
      assert.match(searchRpc("files.write_file", fileParams({
        path: `${leafFixture}/${name}`, content: "must not write",
      }), false).stderr, /io.?error/i);
      assert.equal(readlinkSync(path.join(leafDirectory, name)), target);
      assert.match(searchRpc("files.create_file", fileParams({
        path: `${leafFixture}/${name}`,
      }), false).stderr, /file already exists/);
    }
    assert.deepEqual(readdirSync(leafOutside), ["sentinel.txt"]);
    assert.equal(readFileSync(path.join(leafOutside, "sentinel.txt"), "utf8"), "outside leaf unchanged\n");
    assert(!existsSync(path.join(leafDirectory, "must-not-exist")));
    symlinkSync("target.txt", path.join(leafDirectory, "inside-link.txt"));
    assert.deepEqual(searchRpc("files.write_file", fileParams({
      path: `${leafFixture}/inside-link.txt`, content: "\uFEFF你好\r\n",
    })), { written: true });
    assert.equal(readFileSync(path.join(leafDirectory, "target.txt"), "utf8"), "\uFEFF你好\r\n");
    assert.equal(readlinkSync(path.join(leafDirectory, "inside-link.txt")), "target.txt");
    assert.deepEqual(searchRpc("files.create_file", fileParams({
      path: `${leafFixture}/new.txt`,
    })), { created: true });
    assert.equal(readFileSync(path.join(leafDirectory, "new.txt"), "utf8"), "");
    assert.equal(readFileSync(projectsPath, "utf8"), projectsBeforeQuery);
    rmSync(leafDirectory, { recursive: true });
    console.log("File write/create CLI leaf refusals, resolved alias and exclusive-create wire verified.");

    // Slice36: exercise explicit targets without ever using CLI OS Trash.
    // The real-host suite injects deletion; .txt here cannot start a formatter.
    const fileTargetFixture = "file-target-smoke";
    const fileTargetDirectory = path.join(projDir, fileTargetFixture);
    const fileTargetOther = path.join(scratch, "file-target-other");
    mkdirSync(fileTargetDirectory);
    mkdirSync(fileTargetOther);
    writeFileSync(path.join(fileTargetOther, "sentinel"), "other target unchanged\n");
    for (const name of ["query", "write", "format", "copy"]) {
      writeFileSync(path.join(fileTargetDirectory, `${name}.txt`), `${name} original\n`);
    }
    const fileTargetHistory = path.join(projDir, ".aeroric/local-history", fileTargetFixture);
    for (const [target, error] of [
      [{ kind: "ssh", connectionId: "fixture-only", remotePath: projDir }, /unsupported.?capability/i],
      [{ kind: "wsl", distribution: "fixture-only", linuxPath: projDir }, /unsupported.?capability/i],
      [null, /invalid.?argument/i],
      [{ kind: "local", path: fileTargetOther }, /invalid.?argument/i],
    ]) {
      for (const [method, params] of [
        ["files.read_dir", { path: fileTargetFixture }],
        ["files.read_file", { path: `${fileTargetFixture}/query.txt` }],
        ["files.write_file", { path: `${fileTargetFixture}/write.txt`, content: "must not write" }],
        ["files.format_file", { path: path.join(fileTargetDirectory, "format.txt") }],
        ["files.copy_paths_to_directory", { paths: [`${fileTargetFixture}/copy.txt`], targetDir: `${fileTargetFixture}/copies` }],
      ]) {
        assert.match(searchRpc(method, fileParams({ ...params, target }), false).stderr, error);
        assert(!existsSync(fileTargetHistory), `${method} must not snapshot for a refused target`);
        assert.deepEqual(readdirSync(fileTargetDirectory).sort(), ["copy.txt", "format.txt", "query.txt", "write.txt"]);
        for (const name of ["query", "write", "format", "copy"]) {
          assert.equal(readFileSync(path.join(fileTargetDirectory, `${name}.txt`), "utf8"), `${name} original\n`);
        }
      }
    }
    assert.deepEqual(searchRpc("files.read_file", fileParams({
      path: `${fileTargetFixture}/query.txt`, target: { kind: "local", path: projDir },
    })), { content: "query original\n" });
    const fileTargetAlias = path.join(scratch, "file-target-root-alias");
    symlinkSync(realpathSync(projDir), fileTargetAlias, "dir");
    assert.deepEqual(searchRpc("files.write_file", fileParams({
      path: `${fileTargetFixture}/write.txt`, content: "matching alias\n",
      target: { kind: "local", path: fileTargetAlias },
    })), { written: true });
    assert.equal(readFileSync(path.join(fileTargetDirectory, "write.txt"), "utf8"), "matching alias\n");
    assert.deepEqual(readdirSync(fileTargetOther), ["sentinel"]);
    assert.equal(readFileSync(path.join(fileTargetOther, "sentinel"), "utf8"), "other target unchanged\n");
    assert.equal(readFileSync(projectsPath, "utf8"), projectsBeforeQuery);

    // Slice37: these are scratch saved records, not production migration.
    // A remote saved location must not use its coincidentally local raw path.
    writeFileSync(path.join(fileTargetDirectory, "write.txt"), "write original\n");
    const fileRecordFixture = JSON.parse(projectsBeforeQuery);
    assert(fileRecordFixture.some((record) => record.id === fileProjectId));
    try {
      for (const [location, error] of [
        [{ kind: "ssh", connectionId: "fixture-only", remotePath: projDir }, /unsupported.?capability/i],
        [{ kind: "wsl", distribution: "fixture-only", linuxPath: projDir }, /unsupported.?capability/i],
        [null, /invalid.?argument/i],
        [{ kind: "future-transport", path: projDir }, /invalid.?argument/i],
      ]) {
        const recordBytes = JSON.stringify(fileRecordFixture.map((record) =>
          record.id === fileProjectId ? { ...record, location } : record));
        writeFileSync(projectsPath, recordBytes);
        for (const assertion of [{}, { target: { kind: "local", path: fileTargetAlias } }]) {
          for (const [method, params] of [
            ["files.read_dir", { path: fileTargetFixture }],
            ["files.read_file", { path: `${fileTargetFixture}/query.txt` }],
            ["files.write_file", { path: `${fileTargetFixture}/write.txt`, content: "must not write" }],
            ["files.format_file", { path: path.join(fileTargetDirectory, "format.txt") }],
            ["files.copy_paths_to_directory", { paths: [`${fileTargetFixture}/copy.txt`], targetDir: `${fileTargetFixture}/copies` }],
          ]) {
            assert.match(searchRpc(method, fileParams({ ...params, ...assertion }), false).stderr, error);
            assert(!existsSync(fileTargetHistory), `${method} must not snapshot for a stored remote/bad location`);
            assert.deepEqual(readdirSync(fileTargetDirectory).sort(), ["copy.txt", "format.txt", "query.txt", "write.txt"]);
            for (const name of ["query", "write", "format", "copy"]) {
              assert.equal(readFileSync(path.join(fileTargetDirectory, `${name}.txt`), "utf8"), `${name} original\n`);
            }
            assert.equal(readFileSync(projectsPath, "utf8"), recordBytes);
          }
        }
      }
      writeFileSync(projectsPath, JSON.stringify(fileRecordFixture.map((record) =>
        record.id === fileProjectId ? { ...record, location: { kind: "local", path: fileTargetAlias } } : record)));
      assert.deepEqual(searchRpc("files.read_file", fileParams({
        path: `${fileTargetFixture}/query.txt`,
      })), { content: "query original\n" });
    } finally {
      writeFileSync(projectsPath, projectsBeforeQuery);
    }
    assert.deepEqual(searchRpc("files.read_file", fileParams({
      path: `${fileTargetFixture}/query.txt`,
    })), { content: "query original\n" });
    assert.equal(readFileSync(projectsPath, "utf8"), projectsBeforeQuery);
    console.log("File CLI stored-location refusals, no-snapshot boundary and local/recovery controls verified.");

    rmSync(fileTargetDirectory, { recursive: true });
    console.log("File CLI target refusals, no-snapshot boundary and matching/canonical-local controls verified.");

    // Slice38: stored search candidates, including task worktrees, are not
    // local authority when their saved project location is remote or invalid.
    const searchRecordFixture = "search-record-smoke";
    const searchRecordDirectory = path.join(projDir, searchRecordFixture);
    const searchRecordFile = path.join(searchRecordDirectory, "record.txt");
    const searchRecordWorktree = path.join(scratch, "search-record-worktree");
    const searchRecordWorktreeFile = path.join(searchRecordWorktree, "record.txt");
    mkdirSync(searchRecordDirectory);
    mkdirSync(searchRecordWorktree);
    writeFileSync(searchRecordFile, "record-marker\n");
    writeFileSync(searchRecordWorktreeFile, "record-marker\n");
    const searchRecordTasks = path.join(dataDir, "projects", fileProjectId, "tasks.json");
    const searchRecordTasksBefore = existsSync(searchRecordTasks) ? readFileSync(searchRecordTasks) : null;
    const searchRecordTaskBytes = JSON.stringify([{ id: "scratch-search-worktree", worktreePath: searchRecordWorktree }]);
    mkdirSync(path.dirname(searchRecordTasks), { recursive: true });
    writeFileSync(searchRecordTasks, searchRecordTaskBytes);
    try {
      for (const [location, identifiedError] of [
        [{ kind: "ssh", connectionId: "fixture-only", remotePath: projDir }, /unsupported.?capability/i],
        [{ kind: "wsl", distribution: "fixture-only", linuxPath: projDir }, /unsupported.?capability/i],
        [null, /invalid.?argument/i],
        [{ kind: "future-transport", path: projDir }, /invalid.?argument/i],
      ]) {
        const recordBytes = JSON.stringify(fileRecordFixture.map((record) =>
          record.id === fileProjectId ? { ...record, location } : record));
        writeFileSync(projectsPath, recordBytes);
        for (const route of ["id", "path", "worktree"]) {
          const selected = route === "worktree" ? searchRecordWorktree : projDir;
          const file = route === "worktree" ? searchRecordWorktreeFile : searchRecordFile;
          const relative = route === "worktree" ? "record.txt" : `${searchRecordFixture}/record.txt`;
          const address = route === "id" ? { projectId: fileProjectId } : { projectPath: selected };
          for (const [method, params] of [
            ["files.search_text", { query: "record-marker" }],
            ["files.list_project_files", {}],
            ["files.apply_text_replacements", { replacements: [{
              path: file, start: 0, end: 13, matchText: "record-marker", replacementText: "changed",
            }] }],
            ["files.apply_replacements", { path: relative, replacement: "changed",
              matches: [{ start: 0, end: 13, matchText: "record-marker" }] }],
          ]) {
            assert.match(searchRpc(method, { ...address, ...params }, false).stderr,
              route === "id" ? identifiedError : /not.?found/i);
            assert.equal(readFileSync(searchRecordFile, "utf8"), "record-marker\n");
            assert.equal(readFileSync(searchRecordWorktreeFile, "utf8"), "record-marker\n");
            assert.deepEqual(readdirSync(searchRecordDirectory), ["record.txt"]);
            assert.deepEqual(readdirSync(searchRecordWorktree), ["record.txt"]);
            assert.equal(readFileSync(projectsPath, "utf8"), recordBytes);
            assert.equal(readFileSync(searchRecordTasks, "utf8"), searchRecordTaskBytes);
          }
        }
      }
      writeFileSync(projectsPath, JSON.stringify(fileRecordFixture.map((record) =>
        record.id === fileProjectId ? { ...record, location: { kind: "local", path: projDir } } : record)));
      assert.equal(searchRpc("files.search_text", {
        projectPath: searchRecordWorktree, query: "record-marker",
      }).length, 1);
    } finally {
      writeFileSync(projectsPath, projectsBeforeQuery);
      if (searchRecordTasksBefore === null) rmSync(searchRecordTasks);
      else writeFileSync(searchRecordTasks, searchRecordTasksBefore);
    }
    assert.equal(searchRpc("files.search_text", { projectId: fileProjectId, query: "record-marker" }).length, 1);
    rmSync(searchRecordDirectory, { recursive: true });
    rmSync(searchRecordWorktree, { recursive: true });
    console.log("Search CLI saved-record id/path/worktree refusals, no-write boundary and local/recovery controls verified.");

    const searchHits = searchRpc("files.search_text", {
      projectPath: projDir, query: "title", options: {},
    });
    if (searchHits.length !== 1 || searchHits[0].matchText !== "title") {
      throw new Error(`search CLI payload mismatch: ${JSON.stringify(searchHits)}`);
    }
    const searchPreview = searchRpc("files.replace_text_preview", {
      projectPath: projDir, query: "title", replacement: "name", options: {},
    });
    const searchMatches = searchPreview.files.flatMap((file) => file.matches);
    if (searchMatches.length !== 1 || searchMatches[0].start !== 3 || searchMatches[0].end !== 8) {
      throw new Error("search preview lost original UTF-8 byte offsets");
    }
    const searchApplied = searchRpc("files.apply_text_replacements", {
      projectPath: projDir, replacements: searchMatches,
    });
    if (searchApplied.replacementsApplied !== 1 || readFileSync(searchFile, "utf8") !== "İ name\n") {
      throw new Error("search apply did not use the preview's rich matches");
    }
    searchRpc("files.search_text", { projectPath: scratch, query: "title" }, false);
    searchRpc("files.apply_text_replacements", {
      projectPath: projDir, replacements: searchMatches,
      target: { kind: "ssh", connectionId: "not-a-real-connection", remotePath: "/srv/synthetic" },
    }, false);
    // Slice33: both separately registered apply wires must still work for the
    // current writer; read-only/lost-inode refusals use real UDS harness tests.
    const nativeSearchPreview = searchRpc("files.replace_preview", {
      projectId: fileProjectId, path: "search-smoke.txt", query: "name", replacement: "done",
    });
    const nativeSearchMatches = nativeSearchPreview.files.flatMap((file) => file.matches);
    assert.equal(nativeSearchMatches.length, 1);
    assert.equal(nativeSearchMatches[0].start, 3);
    assert.equal(nativeSearchMatches[0].matchText, "name");
    assert.deepEqual(searchRpc("files.apply_replacements", {
      projectId: fileProjectId, path: "search-smoke.txt",
      replacement: "done", matches: nativeSearchMatches,
    }), { filesChanged: 1, replacementsApplied: 1, replacementsSkipped: 0 });
    assert.equal(readFileSync(searchFile, "utf8"), "İ done\n");
    console.log("Search CLI legacy/native roundtrips, root refusal and unsupported-target refusal verified.");

    // Slices 15–18: real CLI preference wire, normalization and refusal. The fake
    // profiles above are valid AppSettings DTOs, but are never refreshed here.
    const shortcut = searchRpc("settings.save_send_shortcut", { sendShortcut: "invalid" });
    const newline = searchRpc("settings.save_shift_enter_newline", { enabled: false });
    const cleanup = searchRpc("settings.update_auto_cleanup_settings", {
      autoCleanupSettings: { enabled: false, weekday: 99, hour: 99, last_run_at: 123 },
    });
    const reportDir = path.join(scratch, "report-not-created");
    const weekly = searchRpc("settings.update_weekly_report_settings", {
      weeklyReportSettings: { week_start_day: 99, output_dir: reportDir },
    });
    const embedding = searchRpc("settings.update_notebook_embedding_settings", {
      notebookEmbeddingSettings: {
        provider: "ollama", base_url: " ", model: " ",
        api_key: "synthetic-key-must-not-be-stored",
      },
    });
    if (shortcut.send_shortcut !== "mod_enter" || newline.terminal_shift_enter_newline !== false ||
        cleanup.auto_cleanup_settings.weekday !== 6 || cleanup.auto_cleanup_settings.hour !== 23 ||
        cleanup.auto_cleanup_settings.last_run_at !== 123 || weekly.weekly_report_settings.week_start_day !== 6 ||
        weekly.future_smoke_field.retained !== true ||
        embedding.notebook_embedding_settings.base_url !== "http://127.0.0.1:11434" ||
        embedding.notebook_embedding_settings.model !== "nomic-embed-text" || existsSync(reportDir) ||
        existsSync(path.join(dataDir, "agent-homes"))) {
      throw new Error("preference normalization/preservation/side-effect boundary failed");
    }
    for (const [effort, normalized] of [[" OFF ", "off"], ["high", "high"], ["MAX", "max"]]) {
      const dsh = searchRpc("settings.update_dsh_reasoning_effort", { agent: "dsh", effort });
      if (dsh.dsh_reasoning_efforts.dsh !== normalized || dsh.future_smoke_field.retained !== true) {
        throw new Error("DSH effort preference normalization/preservation failed");
      }
    }
    const profilesBeforeProxy = JSON.parse(readFileSync(settingsPath, "utf8")).custom_agents;
    const expectedProfiles = profilesBeforeProxy.map(({ username: _username, password: _password, ...profile }) => profile);
    const proxy = searchRpc("settings.update_proxy_settings", {
      proxySettings: { url: " 127.0.0.1:7890 " },
    });
    assert.deepEqual(proxy.proxy_settings, {
      url: "http://127.0.0.1:7890", no_proxy: "", username: "", password: "",
    }, "URL-only replacement must not resurrect legacy credentials");
    assert.deepEqual(proxy.agent_proxy_enabled, { claude: true, omp: true });
    assert.deepEqual(proxy.custom_agents, expectedProfiles);
    assert.equal(Object.hasOwn(proxy, "agent_proxy_overrides"), false);
    const authenticatedProxy = searchRpc("settings.update_proxy_settings", {
      proxySettings: {
        url: " socks5://never-contact.invalid:7890 ",
        no_proxy: " localhost, ,example.invalid,localhost ",
        username: " synthetic-request-user ", password: " synthetic-request-pass ",
      },
    });
    assert.deepEqual(authenticatedProxy.proxy_settings, {
      url: "socks5://never-contact.invalid:7890", no_proxy: "localhost,example.invalid,localhost",
      username: "synthetic-request-user", password: "synthetic-request-pass",
    });
    const clearedProxy = searchRpc("settings.update_proxy_settings", { proxySettings: {} });
    assert.deepEqual(clearedProxy.proxy_settings, { url: "", no_proxy: "", username: "", password: "" });
    assert.deepEqual(clearedProxy.custom_agents, expectedProfiles);
    assert.deepEqual(clearedProxy.future_smoke_field, { retained: true });
    const proxyOnDisk = JSON.parse(readFileSync(settingsPath, "utf8"));
    assert.deepEqual(proxyOnDisk.proxy_settings, clearedProxy.proxy_settings);
    assert.deepEqual(proxyOnDisk.custom_agents, expectedProfiles);
    assert.equal(Object.hasOwn(proxyOnDisk, "agent_proxy_overrides"), false);
    assert.equal(existsSync(path.join(dataDir, "agent-homes")), false);
    const settingsBeforeRefusal = readFileSync(settingsPath, "utf8");
    if (settingsBeforeRefusal.includes("synthetic-key-must-not-be-stored")) {
      throw new Error("embedding inline key entered settings.json");
    }
    searchRpc("settings.save_shift_enter_newline", { enabled: "false" }, false);
    searchRpc("settings.update_notebook_embedding_settings", {
      notebookEmbeddingSettings: { provider: "invalid-provider" },
    }, false);
    for (const params of [
      { agent: "dsh", effort: "low" },
      { agent: "codex", effort: "high" },
      { agent: "custom-dsh", effort: "max" },
      { agent: "dsh", effort: false },
    ]) {
      searchRpc("settings.update_dsh_reasoning_effort", params, false);
    }
    for (const params of [{}, { proxySettings: null }, { proxySettings: { password: false } }]) {
      searchRpc("settings.update_proxy_settings", params, false);
    }
    if (readFileSync(settingsPath, "utf8") !== settingsBeforeRefusal) {
      throw new Error("invalid preference request modified settings");
    }
    console.log("Preference CLI wire, normalization and no-write refusal verified.");

    // Slice 20: the default CLI deliberately has no MaintenanceHost. A pure
    // pending query fix must not silently enable download/install capabilities.
    const maintenancePaths = [
      path.join(dataDir, "updates"),
      path.join(dataDir, "updates", "pending-release-update.json"),
      path.join(dataDir, "notifications.json"),
    ];
    for (const entry of maintenancePaths) assert.equal(existsSync(entry), false);
    const pendingRefusal = searchRpc("maintenance.get_pending_release_update", {}, false);
    assert.match(pendingRefusal.stderr,
      /host answered UnsupportedCapability: this host does not offer the requested method/);
    for (const entry of maintenancePaths) assert.equal(existsSync(entry), false);
    assert.equal(readFileSync(settingsPath, "utf8"), settingsBeforeRefusal);
    console.log("Default CLI pending-query capability refusal and no-write boundary verified.");

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

    // Slice 22: exact projectId file query, not task.list's registered-project
    // aggregation. The fake run above has settled before snapshots are taken.
    const projectId = legacyProjects[0].id;
    const projectTasksPath = path.join(dataDir, "projects", projectId, "tasks.json");
    const tasksBeforeQuery = readFileSync(projectTasksPath, "utf8");
    const projectRegistryBeforeTasksQuery = readFileSync(projectsPath, "utf8");
    const projectTasks = searchRpc("project.load_project_tasks", { projectId });
    assert(Array.isArray(projectTasks), "project.load_project_tasks must return a bare array");
    assert(projectTasks.some((task) => task.id === taskId), "the settled fake task must be present");
    assert.deepEqual(projectTasks, JSON.parse(tasksBeforeQuery));
    assert.equal(readFileSync(projectTasksPath, "utf8"), tasksBeforeQuery);
    const invalidTasks = searchRpc("project.load_project_tasks", { projectId: "../not-a-project" }, false);
    assert.match(invalidTasks.stderr, /host answered InvalidArgument:/);
    const missingProjectId = `missing-list-${process.pid}`;
    const missingProjectDir = path.join(dataDir, "projects", missingProjectId);
    assert.equal(existsSync(missingProjectDir), false);
    assert.deepEqual(searchRpc("project.load_project_tasks", { projectId: missingProjectId }), []);
    assert.equal(existsSync(missingProjectDir), false);
    assert.equal(readFileSync(projectTasksPath, "utf8"), tasksBeforeQuery);
    assert.equal(readFileSync(projectsPath, "utf8"), projectRegistryBeforeTasksQuery);
    console.log("Frozen projectId task-list CLI read/refusals and no-write boundary verified.");

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

    // Slice 19: query the real host registry while the existing fake task is
    // live. This is not the desktop process's potentially empty child table.
    let observedActive = false;
    for (let i = 0; i < 50; i++) {
      const activeIds = searchRpc("terminal.get_active_task_ids", {});
      assert(Array.isArray(activeIds) && activeIds.every((id) => typeof id === "string"));
      assert.deepEqual(activeIds, [...new Set(activeIds)].sort());
      if (activeIds.includes(longTaskId)) {
        observedActive = true;
        break;
      }
      await sleep(50);
    }
    assert(observedActive, "the running fake task must appear in the host's active list");

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
    let observedRetired = false;
    for (let i = 0; i < 50; i++) {
      const activeIds = searchRpc("terminal.get_active_task_ids", {});
      if (!activeIds.includes(longTaskId)) {
        observedRetired = true;
        break;
      }
      await sleep(50);
    }
    assert(observedRetired, "a retired fake task must disappear from the host's active list");
    console.log(`Task ${longTaskId} cancelled cleanly.`);
    console.log("Hosted active-task list observed the live run and its retirement.");

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
