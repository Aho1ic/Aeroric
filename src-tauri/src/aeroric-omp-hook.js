// Aeroric omp hook bridge — managed by the Aeroric desktop app.
//
// omp 跑在真 PTY 里(原生交互式 TUI),Aeroric 拿不到任何结构化帧,任务状态只能
// 靠 omp 自己的 hook 上报。这里写出的 events.jsonl 行格式与 aeroric-hook.mjs
// (claude/codex)完全一致,由 event_watcher.rs 统一消费:
//   agent_start → UserPromptSubmit → running
//   agent_end   → Stop             → input_required
//
// 与 claude/codex hook 的差别:那两个是 agent 每次事件拉起一个 node 进程、从
// stdin 读 payload;omp hook 是 Bun 在 agent 进程内 import 的 ESM 模块,工厂函数
// 只在启动时跑一次,handler 常驻。
//
// 仅在 AERORIC_TASK_ID + AERORIC_EVENT_DIR 同时存在时注册 handler;用户手动跑
// omp(即使用的是同一个托管 home)时不注册任何东西,零副作用。

import { appendFileSync, mkdirSync } from "node:fs";
import { join } from "node:path";

export default function aeroricOmpHook(pi) {
  const taskId = process.env.AERORIC_TASK_ID;
  const eventDir = process.env.AERORIC_EVENT_DIR;
  if (!taskId || !eventDir) return;
  const agent = process.env.AERORIC_AGENT || "";
  const eventFile = join(eventDir, "events.jsonl");

  // event_watcher 的 HookEvent 反序列化要求这些键存在;omp 的事件载荷不带
  // session/transcript 信息(SessionStartEvent 只有 {type}),故留空——会话发现
  // 由 session_omp.rs 的目录扫描 watcher 负责。
  const write = (event) => {
    try {
      mkdirSync(eventDir, { recursive: true });
      appendFileSync(
        eventFile,
        `${JSON.stringify({
          ts: Date.now(),
          task_id: taskId,
          agent,
          codex_like: false,
          event,
          session_id: "",
          transcript_path: "",
          cwd: process.cwd(),
          tool_name: "",
          permission_mode: "",
        })}\n`,
      );
    } catch {
      // 绝不让 hook 失败影响 agent。
    }
  };

  // 一次 agent loop = 用户的一条 prompt。turn_start/turn_end 是 loop 内的分轮,
  // 对"是否需要用户关注"没有额外信息量,不上报(event_watcher 侧虽有状态去重,
  // 但没必要每轮都写一行盘)。
  pi.on("agent_start", async () => {
    write("UserPromptSubmit");
  });
  pi.on("agent_end", async (event) => {
    // willContinue = 会话已自行安排自动续跑(auto-retry / 空回复重试),
    // 不是用户可见的终态,报了会让角标闪一下又消失。
    if (event?.willContinue === true) return;
    write("Stop");
  });
}
