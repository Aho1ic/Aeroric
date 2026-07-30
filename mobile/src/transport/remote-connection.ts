/**
 * 远程连接核心:E2EE 握手、请求-响应关联、心跳、指数退避重连、
 * 多 endpoint 并行竞速拨号、推送分发。
 * 协议见桌面端 src-tauri/src/remote/protocol.rs(envelope v2)与 crypto.rs(E2EE)。
 *
 * 连接流程:对全部 endpoints 同时开 WS,最先 onopen 者胜出(其余立即关闭)
 * → 明文 hello/hello_ack 握手派生会话密钥(校验配对时 pin 的主机公钥)
 * → 此后所有消息为加密二进制帧(kind=1 控制 JSON,kind=2 终端流)。
 *
 * 设计约束:纯 TS、WebSocket 可注入(RN 全局 WebSocket / 测试 FakeWebSocket),
 * 定时器与随机源可注入,保证 vitest 全覆盖。
 */

import {
  E2eeSession,
  KIND_CTRL,
  KIND_TERMINAL,
  startHandshake,
  type PendingHandshake,
  type RandomBytes,
} from "./e2ee";

export const PROTOCOL_VERSION = 2;

export type ConnectionStatus =
  | "idle"
  | "connecting"
  | "authenticating"
  | "online"
  | "reconnecting"
  | "unauthorized"
  | "stopped";

export interface WebSocketLike {
  send(data: string | ArrayBufferLike | Uint8Array): void;
  close(code?: number, reason?: string): void;
  onopen: (() => void) | null;
  onmessage: ((event: { data: unknown }) => void) | null;
  onclose: (() => void) | null;
  onerror: (() => void) | null;
}

export type WebSocketFactory = (url: string) => WebSocketLike;

interface TimerApi {
  setTimeout(fn: () => void, ms: number): unknown;
  clearTimeout(handle: unknown): void;
}

export interface RemoteConnectionOptions {
  endpoints: string[];
  /** 配对时 pin 的桌面静态公钥(base64url);E2EE 信任根 */
  serverPublicKey: string;
  /** 每次(重)连成功后发送的 auth 参数(deviceToken 等) */
  authParams: () => Record<string, unknown>;
  wsFactory?: WebSocketFactory;
  timers?: TimerApi;
  randomBytes?: RandomBytes;
  requestTimeoutMs?: number;
  heartbeatIntervalMs?: number;
  maxBackoffMs?: number;
  /** 测试用:关掉抖动让退避时序可断言 */
  jitter?: (delay: number) => number;
}

interface PendingRequest {
  resolve: (value: unknown) => void;
  reject: (err: Error) => void;
  timer: unknown;
}

export interface AuthSuccess {
  deviceId: string;
  deviceToken?: string;
  host?: { name: string; version: string; platform: string };
}

/** RPC events.since 的响应(重连 watermark 补发)。 */
interface EventsSinceResult {
  events: Array<{ seq: number; event: string; data: unknown }>;
  latestSeq: number;
  reset: boolean;
}

/** 补发无法精确衔接(缓冲已滚动/服务端重启)时广播的合成推送名。 */
export const EVENTS_RESET_PUSH = "events.reset";

const DEFAULT_REQUEST_TIMEOUT = 15_000;
const DEFAULT_HEARTBEAT_INTERVAL = 25_000;
const DEFAULT_MAX_BACKOFF = 30_000;
const INITIAL_BACKOFF = 1_000;

const textEncoder = new TextEncoder();
const textDecoder = new TextDecoder();

/** 这些失败重试无意义:停止自动重连,提示用户重新配对/升级。 */
function isFatalAuthError(message: string): boolean {
  return (
    message.includes("Unknown device token") ||
    message.includes("主机身份验证失败") ||
    message.includes("Unsupported protocol")
  );
}

function toArrayBuffer(bytes: Uint8Array): ArrayBuffer {
  return bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength) as ArrayBuffer;
}

const defaultWsFactory: WebSocketFactory = (url) => {
  const ws = new WebSocket(url) as unknown as WebSocketLike & { binaryType?: string };
  // 加密帧走二进制;RN 的 WebSocket 需显式声明才回调 ArrayBuffer
  ws.binaryType = "arraybuffer";
  return ws;
};

const defaultTimers: TimerApi = {
  setTimeout: (fn, ms) => setTimeout(fn, ms),
  clearTimeout: (handle) => clearTimeout(handle as ReturnType<typeof setTimeout>),
};

export class RemoteConnection {
  private readonly endpoints: string[];
  private readonly serverPublicKey: string;
  private readonly authParams: () => Record<string, unknown>;
  private readonly wsFactory: WebSocketFactory;
  private readonly timers: TimerApi;
  private readonly randomBytes: RandomBytes | undefined;
  private readonly requestTimeoutMs: number;
  private readonly heartbeatIntervalMs: number;
  private readonly maxBackoffMs: number;
  private readonly jitter: (delay: number) => number;

  private ws: WebSocketLike | null = null;
  /** 竞速期间的全部候选 socket(teardown 时统一关闭) */
  private racers: WebSocketLike[] = [];
  private session: E2eeSession | null = null;
  private handshake: PendingHandshake | null = null;
  private statusValue: ConnectionStatus = "idle";
  private nextId = 1;
  private pending = new Map<number, PendingRequest>();
  private reconnectAttempts = 0;
  private reconnectTimer: unknown = null;
  private heartbeatTimer: unknown = null;
  /** 连接代数:旧 socket 的迟到回调用它自守卫 */
  private generation = 0;

  private statusListeners = new Set<(status: ConnectionStatus) => void>();
  private pushListeners = new Set<(push: string, data: unknown, seq?: number) => void>();
  private authListeners = new Set<(auth: AuthSuccess) => void>();
  private binaryListeners = new Set<(data: ArrayBuffer) => void>();
  private lastAuthError: string | null = null;
  /** 最后一次已分发的带 seq 推送(watermark);0 = 尚未收到任何可补发事件 */
  private lastPushSeq = 0;

  constructor(options: RemoteConnectionOptions) {
    if (options.endpoints.length === 0) {
      throw new Error("RemoteConnection requires at least one endpoint");
    }
    if (!options.serverPublicKey) {
      throw new Error("RemoteConnection requires the paired host public key");
    }
    this.endpoints = [...options.endpoints];
    this.serverPublicKey = options.serverPublicKey;
    this.authParams = options.authParams;
    this.wsFactory = options.wsFactory ?? defaultWsFactory;
    this.timers = options.timers ?? defaultTimers;
    this.randomBytes = options.randomBytes;
    this.requestTimeoutMs = options.requestTimeoutMs ?? DEFAULT_REQUEST_TIMEOUT;
    this.heartbeatIntervalMs = options.heartbeatIntervalMs ?? DEFAULT_HEARTBEAT_INTERVAL;
    this.maxBackoffMs = options.maxBackoffMs ?? DEFAULT_MAX_BACKOFF;
    this.jitter = options.jitter ?? ((delay) => delay * (0.85 + Math.random() * 0.3));
  }

  get status(): ConnectionStatus {
    return this.statusValue;
  }

  get authError(): string | null {
    return this.lastAuthError;
  }

  onStatusChange(listener: (status: ConnectionStatus) => void): () => void {
    this.statusListeners.add(listener);
    return () => this.statusListeners.delete(listener);
  }

  onPush(listener: (push: string, data: unknown, seq?: number) => void): () => void {
    this.pushListeners.add(listener);
    return () => this.pushListeners.delete(listener);
  }

  onAuthSuccess(listener: (auth: AuthSuccess) => void): () => void {
    this.authListeners.add(listener);
    return () => this.authListeners.delete(listener);
  }

  /** 终端流帧(解密后的 terminal_frames 帧)的入站分发。 */
  onBinary(listener: (data: ArrayBuffer) => void): () => void {
    this.binaryListeners.add(listener);
    return () => this.binaryListeners.delete(listener);
  }

  /** 发送终端流帧(自动加密);离线时静默丢弃(终端流由重订阅快照兜底)。 */
  sendBinary(data: Uint8Array): boolean {
    if (this.statusValue !== "online" || !this.ws || !this.session) return false;
    try {
      this.ws.send(this.session.encryptFrame(KIND_TERMINAL, data));
      return true;
    } catch {
      return false;
    }
  }

  start(): void {
    if (this.statusValue !== "idle" && this.statusValue !== "stopped") return;
    this.reconnectAttempts = 0;
    this.openSockets("connecting");
  }

  stop(): void {
    this.setStatus("stopped");
    this.teardownSocket();
    this.failAllPending(new Error("connection stopped"));
  }

  /** 发送白名单 RPC 请求;离线时直接拒绝(调用方决定重试策略)。 */
  request<T = unknown>(method: string, params?: Record<string, unknown>): Promise<T> {
    if (this.statusValue !== "online" || !this.ws) {
      return Promise.reject(new Error(`not online (${this.statusValue})`));
    }
    return this.sendRequest<T>(this.ws, method, params);
  }

  // ── 内部:socket 生命周期(并行竞速拨号) ────────────────────────────────

  private openSockets(nextStatus: ConnectionStatus): void {
    this.teardownSocket();
    this.setStatus(nextStatus);
    const generation = ++this.generation;
    const racers: WebSocketLike[] = [];
    let winner: WebSocketLike | null = null;
    let pendingDials = 0;
    const detach = (ws: WebSocketLike) => {
      ws.onopen = null;
      ws.onmessage = null;
      ws.onclose = null;
      ws.onerror = null;
      try {
        ws.close();
      } catch {
        // 竞速失败方可能尚未建立,close 抛错可忽略
      }
    };
    for (const endpoint of this.endpoints) {
      let ws: WebSocketLike;
      try {
        ws = this.wsFactory(endpoint);
      } catch {
        continue;
      }
      racers.push(ws);
      pendingDials += 1;
      ws.onopen = () => {
        if (generation !== this.generation) return;
        if (winner) {
          detach(ws);
          return;
        }
        winner = ws;
        this.ws = ws;
        for (const other of racers) {
          if (other !== ws) detach(other);
        }
        this.racers = [ws];
        this.beginHandshake(ws);
      };
      ws.onmessage = (event) => {
        if (generation !== this.generation || winner !== ws) return;
        this.handleMessage(event.data, generation);
      };
      ws.onclose = () => {
        if (generation !== this.generation) return;
        if (winner === ws) {
          this.handleDisconnect();
          return;
        }
        if (winner) return;
        pendingDials -= 1;
        if (pendingDials <= 0) {
          this.scheduleReconnect();
        }
      };
      ws.onerror = () => {
        // onerror 后通常伴随 onclose;这里不重复调度
      };
    }
    this.racers = racers;
    if (racers.length === 0) {
      this.scheduleReconnect();
    }
  }

  /** 胜出 socket 上开始 E2EE 握手:发明文 hello,等 hello_ack。 */
  private beginHandshake(ws: WebSocketLike): void {
    this.session = null;
    try {
      this.handshake = startHandshake(this.serverPublicKey, this.randomBytes);
      ws.send(this.handshake.helloJson);
    } catch (err) {
      // 公钥损坏 / 无随机源:重试无意义
      this.lastAuthError = err instanceof Error ? err.message : String(err);
      this.setStatus("unauthorized");
      this.teardownSocket();
    }
  }

  private async authenticate(ws: WebSocketLike, generation: number): Promise<void> {
    try {
      const result = (await this.sendRequest(ws, "auth", this.authParams())) as AuthSuccess;
      if (generation !== this.generation) return;
      this.lastAuthError = null;
      this.reconnectAttempts = 0;
      this.setStatus("online");
      this.authListeners.forEach((listener) => listener(result));
      this.startHeartbeat(generation);
      this.replayMissedEvents(ws, generation);
    } catch (err) {
      if (generation !== this.generation) return;
      const message = err instanceof Error ? err.message : String(err);
      this.lastAuthError = message;
      // 设备被撤销/令牌失效:停止自动重试,等用户重新配对
      if (isFatalAuthError(message)) {
        this.setStatus("unauthorized");
        this.teardownSocket();
        return;
      }
      this.handleDisconnect();
    }
  }

  private handleDisconnect(): void {
    if (this.statusValue === "stopped" || this.statusValue === "unauthorized") return;
    this.failAllPending(new Error("connection lost"));
    this.scheduleReconnect();
  }

  /**
   * 重连补发:用 watermark 请求断线期间错过的推送,按 seq 单调回放。
   * 已被更新的 live 推送覆盖(seq 更小)的回放事件被跳过,不会把状态打回旧值;
   * 缓冲滚动/服务端重启无法精确衔接时广播 events.reset,消费方全量刷新。
   */
  private replayMissedEvents(ws: WebSocketLike, generation: number): void {
    const after = this.lastPushSeq;
    if (after <= 0) return;
    this.sendRequest<EventsSinceResult>(ws, "events.since", { after })
      .then((result) => {
        if (generation !== this.generation || this.statusValue !== "online") return;
        if (result.reset || result.latestSeq < after) {
          this.lastPushSeq = result.latestSeq;
          this.pushListeners.forEach((listener) => listener(EVENTS_RESET_PUSH, null, undefined));
          return;
        }
        for (const event of result.events) {
          if (event.seq <= this.lastPushSeq) continue;
          this.lastPushSeq = event.seq;
          this.pushListeners.forEach((listener) => listener(event.event, event.data, event.seq));
        }
      })
      .catch(() => {
        // 补发失败不致命:列表消费方在 online 时本就会全量刷新
      });
  }

  private scheduleReconnect(): void {
    this.teardownSocket();
    this.setStatus("reconnecting");
    const backoff = Math.min(
      INITIAL_BACKOFF * 2 ** this.reconnectAttempts,
      this.maxBackoffMs,
    );
    this.reconnectAttempts += 1;
    this.reconnectTimer = this.timers.setTimeout(() => {
      this.reconnectTimer = null;
      if (this.statusValue !== "reconnecting") return;
      this.openSockets("reconnecting");
    }, Math.round(this.jitter(backoff)));
  }

  private teardownSocket(): void {
    if (this.reconnectTimer !== null) {
      this.timers.clearTimeout(this.reconnectTimer);
      this.reconnectTimer = null;
    }
    this.stopHeartbeat();
    this.session = null;
    this.handshake = null;
    this.ws = null;
    if (this.racers.length > 0) {
      const racers = this.racers;
      this.racers = [];
      this.generation += 1;
      for (const ws of racers) {
        ws.onopen = null;
        ws.onmessage = null;
        ws.onclose = null;
        ws.onerror = null;
        try {
          ws.close();
        } catch {
          // 已关闭的 socket close 再抛错可忽略
        }
      }
    }
  }

  // ── 内部:心跳 ────────────────────────────────────────────────────────────

  private startHeartbeat(generation: number): void {
    this.stopHeartbeat();
    const tick = () => {
      this.heartbeatTimer = this.timers.setTimeout(() => {
        if (generation !== this.generation || this.statusValue !== "online" || !this.ws) return;
        this.sendRequest(this.ws, "ping", undefined, 10_000)
          .then(() => {
            if (generation === this.generation && this.statusValue === "online") tick();
          })
          .catch(() => {
            if (generation === this.generation && this.statusValue === "online") {
              this.handleDisconnect();
            }
          });
      }, this.heartbeatIntervalMs);
    };
    tick();
  }

  private stopHeartbeat(): void {
    if (this.heartbeatTimer !== null) {
      this.timers.clearTimeout(this.heartbeatTimer);
      this.heartbeatTimer = null;
    }
  }

  // ── 内部:请求与消息 ──────────────────────────────────────────────────────

  private sendRequest<T = unknown>(
    ws: WebSocketLike,
    method: string,
    params?: Record<string, unknown>,
    timeoutMs?: number,
  ): Promise<T> {
    const id = this.nextId++;
    return new Promise<T>((resolve, reject) => {
      const session = this.session;
      if (!session) {
        reject(new Error("encrypted channel not established"));
        return;
      }
      const timer = this.timers.setTimeout(() => {
        this.pending.delete(id);
        reject(new Error(`request timeout: ${method}`));
      }, timeoutMs ?? this.requestTimeoutMs);
      this.pending.set(id, {
        resolve: (value) => resolve(value as T),
        reject,
        timer,
      });
      try {
        const payload = JSON.stringify({ v: PROTOCOL_VERSION, id, method, params: params ?? {} });
        ws.send(session.encryptFrame(KIND_CTRL, textEncoder.encode(payload)));
      } catch (err) {
        this.timers.clearTimeout(timer);
        this.pending.delete(id);
        reject(err instanceof Error ? err : new Error(String(err)));
      }
    });
  }

  private handleMessage(data: unknown, generation: number): void {
    // 明文 text 只存在于握手阶段(hello_ack / hello_error)
    if (typeof data === "string") {
      const handshake = this.handshake;
      if (!handshake || this.session) return;
      this.handshake = null;
      try {
        this.session = handshake.finish(data);
      } catch (err) {
        const message = err instanceof Error ? err.message : String(err);
        this.lastAuthError = message;
        if (isFatalAuthError(message)) {
          this.setStatus("unauthorized");
          this.teardownSocket();
        } else {
          this.handleDisconnect();
        }
        return;
      }
      this.setStatus("authenticating");
      if (this.ws) {
        void this.authenticate(this.ws, generation);
      }
      return;
    }
    if (!(data instanceof ArrayBuffer) || !this.session) return;
    let opened: { kind: number; plain: Uint8Array };
    try {
      opened = this.session.decryptFrame(new Uint8Array(data));
    } catch {
      // 篡改/乱序:立即重建连接(快照协议保证终端画面恢复)
      this.handleDisconnect();
      return;
    }
    if (opened.kind === KIND_TERMINAL) {
      const buffer = toArrayBuffer(opened.plain);
      this.binaryListeners.forEach((listener) => listener(buffer));
      return;
    }
    if (opened.kind !== KIND_CTRL) return;
    let parsed: unknown;
    try {
      parsed = JSON.parse(textDecoder.decode(opened.plain));
    } catch {
      return;
    }
    const frame = parsed as {
      id?: number;
      ok?: boolean;
      result?: unknown;
      error?: string;
      push?: string;
      seq?: number;
      data?: unknown;
    };
    if (typeof frame.push === "string") {
      // 带 seq 的推送参与 watermark:只单调分发,防重复/乱序
      if (typeof frame.seq === "number") {
        if (frame.seq <= this.lastPushSeq) return;
        this.lastPushSeq = frame.seq;
      }
      this.pushListeners.forEach((listener) =>
        listener(frame.push as string, frame.data, frame.seq),
      );
      return;
    }
    if (typeof frame.id === "number") {
      const pending = this.pending.get(frame.id);
      if (!pending) return;
      this.pending.delete(frame.id);
      this.timers.clearTimeout(pending.timer);
      if (frame.ok) {
        pending.resolve(frame.result);
      } else {
        pending.reject(new Error(frame.error ?? "request failed"));
      }
    }
  }

  private failAllPending(err: Error): void {
    for (const pending of this.pending.values()) {
      this.timers.clearTimeout(pending.timer);
      pending.reject(err);
    }
    this.pending.clear();
  }

  private setStatus(status: ConnectionStatus): void {
    if (this.statusValue === status) return;
    this.statusValue = status;
    this.statusListeners.forEach((listener) => listener(status));
  }
}

/**
 * 一次性配对:连上 endpoint,完成 E2EE 握手后用 invite 换长期 deviceToken。
 * 独立于 RemoteConnection 的重连机制(配对失败直接报错给用户)。
 */
export function pairWithInvite(options: {
  endpoint: string;
  invite: string;
  deviceName: string;
  /** 配对 offer 中的主机静态公钥(E2EE 必需) */
  serverPublicKey: string;
  wsFactory?: WebSocketFactory;
  timeoutMs?: number;
  timers?: TimerApi;
  randomBytes?: RandomBytes;
}): Promise<Required<Pick<AuthSuccess, "deviceId" | "deviceToken">> & AuthSuccess> {
  const wsFactory = options.wsFactory ?? defaultWsFactory;
  const timers = options.timers ?? defaultTimers;
  return new Promise((resolve, reject) => {
    let settled = false;
    let ws: WebSocketLike;
    let session: E2eeSession | null = null;
    let handshake: PendingHandshake | null = null;
    const finish = (fn: () => void) => {
      if (settled) return;
      settled = true;
      timers.clearTimeout(timer);
      try {
        ws.close();
      } catch {
        // ignore
      }
      fn();
    };
    const timer = timers.setTimeout(() => {
      finish(() => reject(new Error("配对超时,请确认手机与电脑在同一网络")));
    }, options.timeoutMs ?? 12_000);

    try {
      handshake = startHandshake(options.serverPublicKey, options.randomBytes);
      ws = wsFactory(options.endpoint);
    } catch (err) {
      timers.clearTimeout(timer);
      reject(err instanceof Error ? err : new Error(String(err)));
      return;
    }

    ws.onopen = () => {
      if (handshake) ws.send(handshake.helloJson);
    };
    ws.onmessage = (event) => {
      // 握手响应(明文 text)
      if (typeof event.data === "string") {
        if (!handshake || session) return;
        const pending = handshake;
        handshake = null;
        try {
          session = pending.finish(event.data);
        } catch (err) {
          finish(() => reject(err instanceof Error ? err : new Error(String(err))));
          return;
        }
        try {
          const payload = JSON.stringify({
            v: PROTOCOL_VERSION,
            id: 1,
            method: "auth",
            params: { invite: options.invite, deviceName: options.deviceName },
          });
          ws.send(session.encryptFrame(KIND_CTRL, textEncoder.encode(payload)));
        } catch (err) {
          finish(() => reject(err instanceof Error ? err : new Error(String(err))));
        }
        return;
      }
      // 加密响应
      if (!(event.data instanceof ArrayBuffer) || !session) return;
      let frame: { id?: number; ok?: boolean; result?: AuthSuccess; error?: string };
      try {
        const opened = session.decryptFrame(new Uint8Array(event.data));
        if (opened.kind !== KIND_CTRL) return;
        frame = JSON.parse(textDecoder.decode(opened.plain));
      } catch {
        finish(() => reject(new Error("连接被篡改或加密失败,配对未完成")));
        return;
      }
      if (frame.id !== 1) return;
      if (frame.ok && frame.result?.deviceId && frame.result.deviceToken) {
        const result = frame.result;
        finish(() =>
          resolve(result as Required<Pick<AuthSuccess, "deviceId" | "deviceToken">> & AuthSuccess),
        );
      } else {
        finish(() => reject(new Error(frame.error ?? "配对被拒绝")));
      }
    };
    ws.onclose = () => {
      finish(() => reject(new Error("连接已断开,配对未完成")));
    };
    ws.onerror = () => {
      // onclose 会随后触发并统一收尾
    };
  });
}
