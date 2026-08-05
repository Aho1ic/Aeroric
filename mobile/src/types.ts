/**
 * 与桌面端 remote 模块共享的领域类型。
 * 字段与 Rust 侧 serde 序列化(camelCase)严格对应:
 * 桌面 src-tauri/src/storage.rs 的 Project/Task、remote/mod.rs 的配对 offer。
 */

export interface PairedHost {
  /** 本地生成的主机记录 id(uuid) */
  id: string;
  /** 桌面静态密钥派生的稳定身份;旧 M1 记录可能缺失。 */
  hostId?: string;
  name: string;
  /** 候选连接地址,按优先级排列(并行竞速拨号;含 LAN/自定义/relay) */
  endpoints: string[];
  /** 配对时 pin 的桌面静态公钥(E2EE 信任根);旧 M1 记录缺失 → 需重新配对 */
  publicKey?: string;
  /** Optional wire selection for Orca-compatible hosts; legacy records stay Aeroric. */
  protocol?: "aeroric" | "orca";
  deviceId: string;
  deviceToken: string;
  pairedAt: number;
}

export interface PairingOffer {
  v: number;
  endpoints: string[];
  invite: string;
  hostName: string;
  hostId?: string;
  /** v2 起必填:E2EE 握手的信任根 */
  publicKey: string;
}

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

export interface Project {
  id: string;
  name: string;
  path: string;
  group?: string;
  lastOpenedAt: number;
  orderIndex?: number;
  hiddenFromRail?: boolean;
  /** 置顶:在各自分组内排最前,分组折叠时仍露出。桌面与手机共享同一份状态。 */
  pinned?: boolean;
}

export interface Task {
  id: string;
  projectId: string;
  name?: string;
  prompt: string;
  agent: string;
  selectedModel?: string;
  reasoningEffort?: string;
  speed?: string;
  status: TaskStatus;
  createdAt: number;
  attentionRequestedAt?: number;
  starred?: boolean;
  failureReason?: string;
  worktreeBranch?: string;
  approval?: ApprovalRequest;
}

/** task.create/task.resume 的远程确认结果。task 为桌面端权威快照,可为空以兼容旧桌面。 */
export interface RemoteTaskActionResult {
  accepted: boolean;
  taskId?: string;
  task?: Task;
}

export interface HostInfo {
  name: string;
  version: string;
  platform: string;
}

/**
 * 连接建立后由 `hello` 带回的实时主机身份(桌面 remote/mod.rs::live_identity)。
 * 手机端据此把已保存记录的 LAN 地址刷新到当前网段,而不是新建一条记录。
 */
export interface HostIdentity {
  /** 桌面静态密钥派生的稳定身份;同一台电脑换网段后不变 */
  hostId?: string;
  hostName?: string;
  /** 全部候选地址(LAN → 自定义公网 → relay) */
  endpoints?: string[];
  /** endpoints 中属于内网直连的子集;只有这些会替换本地旧 LAN 地址 */
  lanEndpoints?: string[];
}

/** 服务端推送帧(events_bridge 白名单事件) */
export interface RemotePush {
  push: string;
  data: unknown;
}

export interface TaskStatusPush {
  task_id: string;
  status: TaskStatus;
  approval?: ApprovalRequest;
}

export interface ApprovalRequest {
  requestId: string;
  kind: string;
  toolName?: string;
}

// ── M3:结构化会话与任务操作 ─────────────────────────────────────────────────

/** 与桌面 src-tauri/src/session.rs 的 SessionContent(serde tag="type")对应。 */
export type SessionContent =
  | { type: "text"; text: string }
  | { type: "tool_use"; id: string; name: string; input: string }
  | { type: "thinking"; thinking: string };

export interface SessionMessage {
  role: "user" | "assistant";
  content: SessionContent[];
}

/** RPC session.messages 的响应。available=false 时手机端引导切终端 tab。 */
export interface SessionMessagesResult {
  available: boolean;
  reason?: "ssh" | "no_session";
  isCodex?: boolean;
  messages: SessionMessage[];
}

/** 推送 session.appended 的负载(watcher 增量批)。 */
export interface SessionAppendedPush {
  task_id: string;
  messages: SessionMessage[];
}

/** RPC agents.list 的条目(桌面内置 + 自定义 agent 的窄面视图)。 */
export interface AgentChoice {
  id: string;
  label: string;
  codexLike: boolean;
}

/**
 * RPC agentConfig.list 的条目。API Key 永不回传明文，只报告是否已配置。
 */
export interface AgentConfigEntry {
  id: string;
  label: string;
  codexLike: boolean;
  editable: boolean;
  baseUrl?: string;
  apiKeyConfigured?: boolean;
  models?: string[];
  enable1mContext?: boolean;
  enableChatCompletionsProxy?: boolean;
  proxyEnabled?: boolean;
}

export type PermissionMode = "ask" | "auto_edit" | "full_access";

export const PERMISSION_MODE_VALUES: PermissionMode[] = ["ask", "auto_edit", "full_access"];

/** 这些状态下 PTY 仍在(或即将启动),可以发 prompt / 审批。 */
export function taskAcceptsInput(status: TaskStatus | string): boolean {
  return status === "running" || status === "input_required" || status === "pending";
}

// ── M5:只读 diff 与文件浏览 ─────────────────────────────────────────────────

/** 与桌面 git.rs GitFileChange 对应。 */
export interface GitFileChange {
  path: string;
  status: string;
  staged: boolean;
}

export interface GitChangesResult {
  available: boolean;
  reason?: "ssh" | "wsl";
  changes?: GitFileChange[];
}

export interface GitDiffResult {
  available: boolean;
  reason?: "ssh" | "wsl";
  diff?: string;
}

/** 与桌面 fs.rs FsEntry 对应(serde 无 rename,保持 snake_case)。 */
export interface FsEntryView {
  name: string;
  path: string;
  is_dir: boolean;
  extension?: string | null;
  is_gitignored?: boolean;
}

export interface ProjectFilesResult {
  available: boolean;
  reason?: "ssh" | "wsl";
  root?: string;
  entries?: FsEntryView[];
}

export interface ReadFileResult {
  available: boolean;
  reason?: "ssh" | "wsl";
  content?: string;
  truncated?: boolean;
  totalBytes?: number;
}

/** `project.writeFile` 的返回:SSH/WSL 项目 available=false。 */
export interface WriteFileResult {
  available: boolean;
  reason?: "ssh" | "wsl";
  ok?: boolean;
}
