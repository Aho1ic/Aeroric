/**
 * 跨端共享的 wire-shaped 词表。桌面 `src/types.ts` 与 mobile 从这里 re-export，
 * 禁止再在两端各自复制一份字面量联合。
 *
 * Rust 侧不强制 codegen；parity 测试锁关键字段名（见 src/test/contracts-parity.test.ts）。
 */

export type BuiltInAgentType = "claude" | "claude_gpt55" | "codex" | "dsh" | "omp";
export type AgentType = BuiltInAgentType | (string & {});

/** 协议族:决定启动参数、会话格式与配置文件形态。 */
export type ProtocolFamily = "claude" | "codex" | "dsh" | "omp";

export type PermissionMode = "ask" | "auto_edit" | "full_access";

export type TaskStatus =
  | "todo"
  | "pending"
  | "running"
  | "input_required"
  | "detached"
  | "interrupted"
  | "done"
  | "failed"
  | "cancelled";

export type ProjectLocation =
  | { kind: "local"; path: string }
  | { kind: "ssh"; connectionId: string; remotePath: string }
  | { kind: "wsl"; distribution: string; linuxPath: string };

/**
 * 项目头像定制。`color` 是调色板键名而非色值，换主题时可整体重调。
 */
export interface ProjectAvatarOverride {
  color?: string;
  emoji?: string;
  label?: string;
}
