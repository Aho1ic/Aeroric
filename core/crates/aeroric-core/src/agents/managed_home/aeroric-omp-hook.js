// Aeroric omp hook bridge — managed by the Aeroric desktop app.
//
// omp 跑在真 PTY 里(原生交互式 TUI),Aeroric 拿不到任何结构化帧,任务状态只能
// 靠 omp 自己的 hook 上报。这里写出的 events.jsonl 行格式与 aeroric-hook.mjs
// (claude/codex)完全一致,由 event_watcher.rs 统一消费:
//   agent_start          → UserPromptSubmit → running
//   tool_execution_start → PostToolUse      → running
//   agent_end            → Stop             → input_required
//
// 与 claude/codex hook 的差别:那两个是 agent 每次事件拉起一个 node 进程、从
// stdin 读 payload;omp hook 是 Bun 在 agent 进程内 import 的 ESM 模块。
//
// 关键:工厂函数不是只跑一次。子代理会话继承父的 preloadedPreparedExtensions
// (除非它带 restrictToolNames),每个子代理都会重新绑定这个工厂、重新注册
// handler。而 agent_end 的载荷是 {type, messages, willContinue?},不带
// sessionId/agentId/depth,单看事件无从分辨主会话与子代理。子代理终态若也写
// Stop,主任务会在还在跑的时候被标成"等待输入"。唯一可靠的判别式是 handler
// 第二参 ExtensionContext 上的 hasUI:交互式 TUI 为 true,子代理创建时写死
// false。所以每个 handler 都必须先过这道门。
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

  // 同一状态重复落盘没有意义(event_watcher 侧本来也会去重),这层只是把磁盘
  // 写入省掉。变量是每会话的——工厂每会话重跑,正合主会话的语义。
  let lastWritten = null;
  const writeStatus = (event) => {
    if (lastWritten === event) return;
    lastWritten = event;
    write(event);
  };

  // 一次 agent loop = 用户的一条 prompt。turn_start/turn_end 是 loop 内的分轮,
  // 对"是否需要用户关注"没有额外信息量,不上报。
  pi.on("agent_start", async (_event, ctx) => {
    if (ctx?.hasUI !== true) return;
    writeStatus("UserPromptSubmit");
  });
  // 压缩路径上的 deferredHandoff / automaticContinuationBlocked 会在主会话上发出
  // willContinue 未设的 agent_end,hasUI 门挡不住它。工具一旦再跑起来就说明上一条
  // Stop 不是终态,用这条把粘住的 input_required 冲掉。tool_execution_* 是纯观测
  // 事件,改不了 agent 行为(tool_call 是 fail-closed 控制钩子,不能用)。
  pi.on("tool_execution_start", async (_event, ctx) => {
    if (ctx?.hasUI !== true) return;
    writeStatus("PostToolUse");
  });
  pi.on("agent_end", async (event, ctx) => {
    if (ctx?.hasUI !== true) return;
    // willContinue = 会话已自行安排自动续跑(auto-retry / 空回复重试),
    // 不是用户可见的终态,报了会让角标闪一下又消失。
    if (event?.willContinue === true) return;
    writeStatus("Stop");
  });
}
