/**
 * 远程连接核心:E2EE 握手、请求-响应关联、心跳、指数退避重连、
 * 多 endpoint 并行竞速拨号、推送分发。
 * 协议见桌面端 src-tauri/src/remote/protocol.rs(envelope v2)与 crypto.rs(E2EE)。
 *
 * 连接流程:对全部 endpoints 同时开 WS,依次让已连通候选完成 E2EE 握手与认证,
 * 首个认证成功者胜出(其余随后关闭)
 * → 此后所有消息为加密二进制帧(kind=1 控制 JSON,kind=2 终端流)。
 *
 * 设计约束:纯 TS、WebSocket 可注入(RN 全局 WebSocket / 测试 FakeWebSocket),
 * 定时器与随机源可注入,保证 vitest 全覆盖。
 */

import type { HostIdentity } from "../types";
import {
  E2eeSession,
  KIND_CTRL,
  KIND_TERMINAL,
  startHandshake,
  type PendingHandshake,
  type RandomBytes,
} from "./e2ee";
import {
  OrcaE2EESession,
  startOrcaE2EEHandshake,
  type OrcaE2EEContext,
  type OrcaE2EEPendingHandshake,
} from "./orca-e2ee-v2";
import {
  RPC_V2,
  authenticationParams,
  decodeAeroricEnvelope,
  encodeAeroricRequest,
  negotiatedRpcVersion,
  type RpcVersion,
} from "./rpc-codec";

export const PROTOCOL_VERSION = RPC_V2;

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
  onclose: ((event?: { code?: number; reason?: string }) => void) | null;
  onerror: ((event?: { message?: string }) => void) | null;
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
  /** Wire protocol. Defaults to Aeroric for existing pairings. */
  protocol?: "aeroric" | "orca";
  /** Orca v2 transport context; direct is used when omitted. */
  orcaContext?: OrcaE2EEContext;
  wsFactory?: WebSocketFactory;
  timers?: TimerApi;
  randomBytes?: RandomBytes;
  requestTimeoutMs?: number;
  dialTimeoutMs?: number;
  handshakeTimeoutMs?: number;
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

/** One liveness request shared by foreground recovery and the normal heartbeat. */
interface LivenessProbe {
  ws: WebSocketLike;
  generation: number;
  inboundSequence: number;
}

export interface AuthSuccess {
  deviceId: string;
  deviceToken?: string;
  host?: { name: string; version: string; platform: string };
  rpcVersion?: RpcVersion;
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
const DEFAULT_DIAL_TIMEOUT = 10_000;
const DEFAULT_HANDSHAKE_TIMEOUT = 10_000;
const DEFAULT_HEARTBEAT_INTERVAL = 25_000;
const DEFAULT_MAX_BACKOFF = 30_000;
const INITIAL_BACKOFF = 1_000;
// React Native can preserve a dead WebSocket while the app is suspended. A
// foreground event more than a moment after this round started should replace
// an in-flight dial instead of waiting out its remaining timeout/backoff.
const STALE_FOREGROUND_DIAL_MS = 2_000;

const textEncoder = new TextEncoder();
const textDecoder = new TextDecoder();

/** 这些失败重试无意义:停止自动重连,提示用户重新配对/升级。 */
function isFatalAuthError(message: string): boolean {
  return (
    message.includes("Unknown device token") ||
    message.includes("主机身份验证失败") ||
    message.includes("unauthorized") ||
    message.includes("bad_auth") ||
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
  private endpoints: string[];
  private readonly serverPublicKey: string;
  private readonly authParams: () => Record<string, unknown>;
  private readonly protocol: "aeroric" | "orca";
  private readonly orcaContext: OrcaE2EEContext;
  private readonly wsFactory: WebSocketFactory;
  private readonly timers: TimerApi;
  private readonly randomBytes: RandomBytes | undefined;
  private readonly requestTimeoutMs: number;
  private readonly dialTimeoutMs: number;
  private readonly handshakeTimeoutMs: number;
  private readonly heartbeatIntervalMs: number;
  private readonly maxBackoffMs: number;
  private readonly jitter: (delay: number) => number;

  private ws: WebSocketLike | null = null;
  /** 竞速期间的全部候选 socket(teardown 时统一关闭) */
  private racers: WebSocketLike[] = [];
  private session: E2eeSession | OrcaE2EESession | null = null;
  private handshake: PendingHandshake | OrcaE2EEPendingHandshake | null = null;
  private orcaAuthenticated = false;
  private rpcVersion: RpcVersion = RPC_V2;
  private statusValue: ConnectionStatus = "idle";
  private nextId = 1;
  private pending = new Map<string, PendingRequest>();
  private reconnectAttempts = 0;
  private reconnectTimer: unknown = null;
  private heartbeatTimer: unknown = null;
  private attemptTimers = new Set<unknown>();
  /** 前台恢复与常规心跳共用一个在途探针，避免并发 ping 互相误判。 */
  private livenessProbe: LivenessProbe | null = null;
  /** 连接代数:旧 socket 的迟到回调用它自守卫 */
  private generation = 0;
  /** 当前竞速拨号轮次开始时间;用于前台恢复时识别被系统挂起的半开连接。 */
  private dialStartedAt: number | null = null;

  private statusListeners = new Set<(status: ConnectionStatus) => void>();
  private pushListeners = new Set<(push: string, data: unknown, seq?: number) => void>();
  private authListeners = new Set<(auth: AuthSuccess) => void>();
  private binaryListeners = new Set<(data: ArrayBuffer) => void>();
  private identityListeners = new Set<
    (identity: HostIdentity, connectedEndpoint: string | null) => void
  >();
  /** 竞速胜出的地址(用于把有效地址排到候选列表最前) */
  private activeEndpoint: string | null = null;
  private lastAuthError: string | null = null;
  /** 最后一次已分发的带 seq 推送(watermark);0 = 尚未收到任何可补发事件 */
  private lastPushSeq = 0;
  /** 仅在认证/解析成功的入站帧上递增，不能被畸形数据伪造为“存活”。 */
  private inboundSequence = 0;

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
    this.protocol = options.protocol ?? "aeroric";
    this.orcaContext = options.orcaContext ?? {
      protocol: "orca-mobile-e2ee",
      initiator: "mobile",
      responder: "desktop",
      transport: "direct",
    };
    this.wsFactory = options.wsFactory ?? defaultWsFactory;
    this.timers = options.timers ?? defaultTimers;
    this.randomBytes = options.randomBytes;
    this.requestTimeoutMs = options.requestTimeoutMs ?? DEFAULT_REQUEST_TIMEOUT;
    this.dialTimeoutMs = options.dialTimeoutMs ?? DEFAULT_DIAL_TIMEOUT;
    this.handshakeTimeoutMs = options.handshakeTimeoutMs ?? DEFAULT_HANDSHAKE_TIMEOUT;
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

  /**
   * 每次认证成功后由 `hello` 带回的实时主机身份(hostId / hostName / 当前候选地址),
   * 附带本轮竞速实际连上的地址。消费方据此刷新已保存主机,解决换网段后地址过期。
   */
  onHostIdentity(
    listener: (identity: HostIdentity, connectedEndpoint: string | null) => void,
  ): () => void {
    this.identityListeners.add(listener);
    return () => this.identityListeners.delete(listener);
  }

  /**
   * 就地替换候选地址(主机记录被身份合并刷新后调用),避免重建整个连接。
   * 已在线时只记下,留给下次重连;未在线时立刻用新地址重拨,
   * 免得用户刚手填完正确地址还要等最长 30s 的退避。
   */
  updateEndpoints(next: string[]): void {
    const cleaned = next.filter((endpoint) => endpoint.length > 0);
    if (cleaned.length === 0) return;
    if (
      cleaned.length === this.endpoints.length &&
      cleaned.every((endpoint, index) => endpoint === this.endpoints[index])
    ) {
      return;
    }
    this.endpoints = [...cleaned];
    if (
      this.statusValue === "online" ||
      this.statusValue === "stopped" ||
      this.statusValue === "unauthorized"
    ) {
      return;
    }
    this.reconnectAttempts = 0;
    this.openSockets("connecting");
  }

  /** 发送终端流帧(自动加密);离线时静默丢弃(终端流由重订阅快照兜底)。 */
  sendBinary(data: Uint8Array): boolean {
    if (this.statusValue !== "online" || !this.ws || !this.session) return false;
    try {
      if (this.session instanceof OrcaE2EESession) {
        this.ws.send(this.session.encryptBinary(data));
      } else {
        this.ws.send(this.session.encryptFrame(KIND_TERMINAL, data));
      }
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

  /**
   * 应用回到前台时调用。iOS/Android 可能在后台回收网络路径却不向 JS
   * 派发 close，导致连接永远卡在 dialing 或 E2EE 握手中。只有早于本次
   * 前台恢复 2 秒以上的轮次才会被替换，避免连续 active 信号反复中断新拨号。
   */
  notifyForeground(): void {
    if (this.statusValue === "online") {
      this.probeForegroundConnection();
      return;
    }

    let abandonedStaleRound = false;
    const hasActiveRetryRound = this.statusValue === "reconnecting" && this.racers.length > 0;
    if (
      (this.statusValue === "connecting" ||
        this.statusValue === "authenticating" ||
        hasActiveRetryRound) &&
      this.dialStartedAt !== null &&
      Date.now() - this.dialStartedAt >= STALE_FOREGROUND_DIAL_MS
    ) {
      // authenticating 时可能有尚未完成的内部 auth 请求；先拒绝它，防止旧
      // generation 的 promise 留到超时后才清理。
      this.failAllPending(new Error("connection restarted after app resumed"));
      // 这个半开轮次本身已经代表一次失败；沿用其退避计数，不能因用户
      // 多次切回前台而持续把不可达主机显示成新的首次拨号。
      this.scheduleReconnect();
      abandonedStaleRound = true;
    }

    if (this.statusValue === "reconnecting" && this.racers.length === 0) {
      // 用户已经回到前台，没必要继续等后台期间保留下来的指数退避。
      // 只有原本就是等待中的退避才视为一次全新的前台尝试；若刚废弃了
      // 半开连接，则必须保留其刚记下的失败次数。
      if (!abandonedStaleRound) {
        this.reconnectAttempts = 0;
      }
      this.openSockets("connecting");
    }
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
    this.dialStartedAt = Date.now();
    const generation = ++this.generation;
    type Candidate = {
      ws: WebSocketLike;
      endpoint: string;
      phase: "dialing" | "ready" | "active" | "online" | "failed";
      dialTimer: unknown | null;
      handshakeTimer: unknown | null;
      error: string | null;
    };
    const candidates: Candidate[] = [];
    let active: Candidate | null = null;

    const detach = (candidate: Candidate) => {
      this.clearAttemptTimeout(candidate.dialTimer);
      this.clearAttemptTimeout(candidate.handshakeTimer);
      candidate.dialTimer = null;
      candidate.handshakeTimer = null;
      candidate.ws.onopen = null;
      candidate.ws.onmessage = null;
      candidate.ws.onclose = null;
      candidate.ws.onerror = null;
      try {
        candidate.ws.close();
      } catch {
        // 竞速失败方可能尚未建立,close 抛错可忽略
      }
    };

    const finishRound = () => {
      if (generation !== this.generation || active) return;
      const ready = candidates.find((candidate) => candidate.phase === "ready");
      if (ready) {
        activate(ready);
        return;
      }
      if (candidates.some((candidate) => candidate.phase === "dialing")) return;

      const errors = candidates
        .map((candidate) => candidate.error)
        .filter((error): error is string => Boolean(error));
      if (
        candidates.length > 0 &&
        errors.length === candidates.length &&
        errors.every(isFatalAuthError)
      ) {
        this.lastAuthError = errors[errors.length - 1];
        this.setStatus("unauthorized");
        this.teardownSocket();
        return;
      }
      this.scheduleReconnect();
    };

    const failCandidate = (candidate: Candidate, error?: string) => {
      if (candidate.phase === "failed") return;
      if (error) {
        candidate.error = error;
        this.lastAuthError = error;
      }
      const wasActive = active === candidate;
      candidate.phase = "failed";
      detach(candidate);
      if (wasActive) {
        active = null;
        this.ws = null;
        this.session = null;
        this.handshake = null;
        this.failAllPending(new Error(error ?? "connection lost"));
        this.setStatus(nextStatus);
      }
      finishRound();
    };

    const markOnline = (candidate: Candidate) => {
      if (generation !== this.generation || active !== candidate) return;
      candidate.phase = "online";
      this.clearAttemptTimeout(candidate.handshakeTimer);
      candidate.handshakeTimer = null;
      for (const other of candidates) {
        if (other === candidate || other.phase === "failed") continue;
        other.phase = "failed";
        detach(other);
      }
      this.racers = [candidate.ws];
      this.activeEndpoint = candidate.endpoint;
    };

    const activate = (candidate: Candidate) => {
      if (generation !== this.generation || active || candidate.phase !== "ready") return;
      active = candidate;
      candidate.phase = "active";
      this.ws = candidate.ws;
      this.setStatus(nextStatus);
      if (!this.beginHandshake(candidate.ws)) return;
      candidate.handshakeTimer = this.setAttemptTimeout(() => {
        failCandidate(candidate, "E2EE handshake timeout");
      }, this.handshakeTimeoutMs);
    };

    for (const endpoint of this.endpoints) {
      let ws: WebSocketLike;
      try {
        ws = this.wsFactory(endpoint);
      } catch {
        continue;
      }
      const candidate: Candidate = {
        ws,
        endpoint,
        phase: "dialing",
        dialTimer: null,
        handshakeTimer: null,
        error: null,
      };
      candidates.push(candidate);
      candidate.dialTimer = this.setAttemptTimeout(() => {
        failCandidate(candidate, "WebSocket dial timeout");
      }, this.dialTimeoutMs);
      ws.onopen = () => {
        if (generation !== this.generation || candidate.phase !== "dialing") return;
        this.clearAttemptTimeout(candidate.dialTimer);
        candidate.dialTimer = null;
        candidate.phase = "ready";
        finishRound();
      };
      ws.onmessage = (event) => {
        if (
          generation !== this.generation ||
          active !== candidate ||
          (candidate.phase !== "active" && candidate.phase !== "online")
        )
          return;
        if (typeof event.data === "string") {
          this.clearAttemptTimeout(candidate.handshakeTimer);
          candidate.handshakeTimer = null;
        }
        this.handleMessage(
          event.data,
          generation,
          ws,
          (error) => failCandidate(candidate, error),
          () => markOnline(candidate),
        );
      };
      ws.onclose = () => {
        if (generation !== this.generation) return;
        if (candidate.phase === "online") {
          this.handleDisconnect();
          return;
        }
        failCandidate(candidate, "connection closed before authentication");
      };
      ws.onerror = () => {
        // onerror 后通常伴随 onclose;这里不重复调度
      };
    }
    this.racers = candidates.map((candidate) => candidate.ws);
    if (candidates.length === 0) {
      this.scheduleReconnect();
    }
  }

  /** 候选 socket 上开始 E2EE 握手:发明文 hello,等 hello_ack。 */
  private beginHandshake(ws: WebSocketLike): boolean {
    this.session = null;
    this.orcaAuthenticated = false;
    try {
      this.handshake =
        this.protocol === "orca"
          ? startOrcaE2EEHandshake(this.serverPublicKey, this.orcaContext, this.randomBytes)
          : startHandshake(this.serverPublicKey, this.randomBytes);
      ws.send(this.handshake.helloJson);
      return true;
    } catch (err) {
      // 公钥损坏 / 无随机源:重试无意义
      this.lastAuthError = err instanceof Error ? err.message : String(err);
      this.setStatus("unauthorized");
      this.teardownSocket();
      return false;
    }
  }

  private async authenticate(
    ws: WebSocketLike,
    generation: number,
    onCandidateFailure: (error: string) => void,
    onAuthenticated: () => void,
  ): Promise<void> {
    try {
      const result = (await this.sendRequest(
        ws,
        "auth",
        authenticationParams(this.authParams()),
        undefined,
        RPC_V2,
      )) as AuthSuccess;
      if (generation !== this.generation || this.ws !== ws) return;
      this.rpcVersion = negotiatedRpcVersion(result.rpcVersion);
      onAuthenticated();
      this.lastAuthError = null;
      this.reconnectAttempts = 0;
      this.setStatus("online");
      this.authListeners.forEach((listener) => listener(result));
      this.startHeartbeat(generation);
      this.replayMissedEvents(ws, generation);
      this.fetchHostIdentity(ws, generation);
    } catch (err) {
      if (generation !== this.generation || this.ws !== ws) return;
      const message = err instanceof Error ? err.message : String(err);
      this.lastAuthError = message;
      // 设备被撤销/令牌失效:停止自动重试,等用户重新配对
      if (isFatalAuthError(message)) {
        this.setStatus("unauthorized");
        this.teardownSocket();
        return;
      }
      onCandidateFailure(message);
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
    if (this.protocol === "orca") return;
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

  /**
   * 认证成功后取一次实时主机身份(桌面 rpc.rs::hello)。
   * 纯附加信息:失败不影响连接可用性,静默吞掉即可。
   */
  private fetchHostIdentity(ws: WebSocketLike, generation: number): void {
    if (this.protocol === "orca") return;
    const connectedEndpoint = this.activeEndpoint;
    this.sendRequest<HostIdentity>(ws, "hello")
      .then((identity) => {
        if (generation !== this.generation || this.statusValue !== "online") return;
        if (!identity || typeof identity !== "object") return;
        this.identityListeners.forEach((listener) => listener(identity, connectedEndpoint));
      })
      .catch(() => {
        // 旧桌面版本没有身份字段 / 请求超时:保持已保存地址不变
      });
  }

  private scheduleReconnect(): void {
    this.teardownSocket();
    this.setStatus("reconnecting");
    const backoff = Math.min(INITIAL_BACKOFF * 2 ** this.reconnectAttempts, this.maxBackoffMs);
    this.reconnectAttempts += 1;
    this.reconnectTimer = this.timers.setTimeout(
      () => {
        this.reconnectTimer = null;
        if (this.statusValue !== "reconnecting") return;
        this.openSockets("reconnecting");
      },
      Math.round(this.jitter(backoff)),
    );
  }

  private setAttemptTimeout(callback: () => void, delayMs: number): unknown {
    const handle = this.timers.setTimeout(() => {
      this.attemptTimers.delete(handle);
      callback();
    }, delayMs);
    this.attemptTimers.add(handle);
    return handle;
  }

  private clearAttemptTimeout(handle: unknown | null): void {
    if (handle === null || !this.attemptTimers.delete(handle)) return;
    this.timers.clearTimeout(handle);
  }

  private teardownSocket(): void {
    if (this.reconnectTimer !== null) {
      this.timers.clearTimeout(this.reconnectTimer);
      this.reconnectTimer = null;
    }
    for (const timer of this.attemptTimers) {
      this.timers.clearTimeout(timer);
    }
    this.attemptTimers.clear();
    this.stopHeartbeat();
    this.livenessProbe = null;
    this.session = null;
    this.handshake = null;
    this.orcaAuthenticated = false;
    this.rpcVersion = RPC_V2;
    this.ws = null;
    this.activeEndpoint = null;
    this.dialStartedAt = null;
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
    this.scheduleHeartbeat(generation);
  }

  private scheduleHeartbeat(generation: number): void {
    if (generation !== this.generation || this.statusValue !== "online" || !this.ws) return;
    this.stopHeartbeat();
    this.heartbeatTimer = this.timers.setTimeout(() => {
      this.heartbeatTimer = null;
      if (generation !== this.generation || this.statusValue !== "online" || !this.ws) return;
      this.runLivenessProbe(generation);
    }, this.heartbeatIntervalMs);
  }

  /**
   * 恢复前台时立即验证在线 socket。移动系统可能静默回收后台连接，若只等
   * 常规 25 秒心跳，用户会额外看到一段“在线”但不可用的假状态。
   */
  private probeForegroundConnection(): void {
    // 让前台探测取代下一个定时心跳；若已有心跳在途，复用它而不是并发发 ping。
    this.stopHeartbeat();
    this.runLivenessProbe(this.generation);
  }

  /**
   * 发起一次 socket 存活探测。请求本身的成功当然证明连接可用；如果请求超时，
   * 同一 socket 上任何已认证、已解析的入站帧也足以证明它仍活着，因此不能误断开。
   */
  private runLivenessProbe(generation: number): void {
    const ws = this.ws;
    if (
      generation !== this.generation ||
      this.statusValue !== "online" ||
      !ws ||
      !this.session ||
      this.livenessProbe
    ) {
      return;
    }

    const probe: LivenessProbe = {
      ws,
      generation,
      inboundSequence: this.inboundSequence,
    };
    this.livenessProbe = probe;
    this.sendRequest(ws, this.protocol === "orca" ? "status.get" : "ping", undefined, 10_000).then(
      () => this.finishLivenessProbe(probe),
      () => this.failLivenessProbe(probe),
    );
  }

  private finishLivenessProbe(probe: LivenessProbe): void {
    if (!this.isCurrentLivenessProbe(probe)) return;
    this.livenessProbe = null;
    this.scheduleHeartbeat(probe.generation);
  }

  private failLivenessProbe(probe: LivenessProbe): void {
    if (!this.isCurrentLivenessProbe(probe)) return;
    this.livenessProbe = null;
    if (this.inboundSequence > probe.inboundSequence) {
      // 例如另一个 RPC 响应、推送或终端帧在 ping 丢失时到达；同一经过认证的
      // socket 已经证明存活，恢复常规定时器即可。
      this.scheduleHeartbeat(probe.generation);
      return;
    }
    this.handleDisconnect();
  }

  private isCurrentLivenessProbe(probe: LivenessProbe): boolean {
    return (
      this.livenessProbe === probe &&
      probe.generation === this.generation &&
      this.statusValue === "online" &&
      this.ws === probe.ws
    );
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
    versionOverride?: RpcVersion,
  ): Promise<T> {
    const id = this.protocol === "orca" ? `rpc-${this.nextId++}` : String(this.nextId++);
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
        const payload =
          this.protocol === "orca"
            ? JSON.stringify({ id, method, params: params ?? {} })
            : encodeAeroricRequest(versionOverride ?? this.rpcVersion, id, method, params ?? {});
        if (session instanceof OrcaE2EESession) {
          ws.send(session.encryptText(payload));
        } else {
          ws.send(session.encryptFrame(KIND_CTRL, textEncoder.encode(payload)));
        }
      } catch (err) {
        this.timers.clearTimeout(timer);
        this.pending.delete(id);
        reject(err instanceof Error ? err : new Error(String(err)));
      }
    });
  }

  private handleMessage(
    data: unknown,
    generation: number,
    ws: WebSocketLike,
    onCandidateFailure: (error: string) => void,
    onAuthenticated: () => void,
  ): void {
    // 明文 text 只存在于握手阶段(hello_ack / e2ee_ready)。
    if (typeof data === "string" && this.handshake && !this.session) {
      const handshake = this.handshake;
      this.handshake = null;
      try {
        this.session = handshake.finish(data);
      } catch (err) {
        const message = err instanceof Error ? err.message : String(err);
        this.lastAuthError = message;
        onCandidateFailure(message);
        return;
      }
      this.setStatus("authenticating");
      if (this.session instanceof OrcaE2EESession) {
        const deviceToken = this.authParams().deviceToken;
        if (typeof deviceToken !== "string" || deviceToken.length === 0) {
          onCandidateFailure("bad_auth");
          return;
        }
        try {
          ws.send(
            this.session.encryptText(
              JSON.stringify({
                type: "e2ee_auth",
                v: 2,
                transcriptHashB64: this.session.transcriptHashB64,
                deviceToken,
              }),
            ),
          );
        } catch (err) {
          onCandidateFailure(err instanceof Error ? err.message : String(err));
        }
      } else {
        void this.authenticate(ws, generation, onCandidateFailure, onAuthenticated);
      }
      return;
    }

    if (!this.session) return;
    if (this.session instanceof OrcaE2EESession) {
      let plaintext: string | Uint8Array;
      try {
        if (typeof data === "string") {
          plaintext = this.session.decryptText(data);
        } else if (data instanceof ArrayBuffer) {
          plaintext = this.session.decryptBinary(new Uint8Array(data));
        } else if (data instanceof Uint8Array) {
          plaintext = this.session.decryptBinary(data);
        } else {
          return;
        }
      } catch {
        if (this.statusValue === "online") this.handleDisconnect();
        else onCandidateFailure("encrypted handshake response is invalid");
        return;
      }
      this.noteValidatedInboundTraffic();
      if (typeof plaintext !== "string") {
        const buffer = toArrayBuffer(plaintext);
        this.binaryListeners.forEach((listener) => listener(buffer));
        return;
      }
      let frame: {
        id?: string;
        ok?: boolean;
        result?: unknown;
        error?: { code?: string; message?: string };
        type?: string;
        v?: number;
        transcriptHashB64?: string;
        push?: string;
        data?: unknown;
      };
      try {
        frame = JSON.parse(plaintext) as typeof frame;
      } catch {
        return;
      }
      if (!this.orcaAuthenticated) {
        if (
          frame.type !== "e2ee_authenticated" ||
          frame.v !== 2 ||
          frame.transcriptHashB64 !== this.session.transcriptHashB64
        ) {
          const message = frame.error?.message ?? "bad_auth";
          onCandidateFailure(message);
          return;
        }
        this.orcaAuthenticated = true;
        this.lastAuthError = null;
        this.reconnectAttempts = 0;
        this.setStatus("online");
        onAuthenticated();
        const token = this.authParams().deviceToken;
        this.authListeners.forEach((listener) =>
          listener({
            deviceId: typeof token === "string" ? token : "orca",
            ...(typeof token === "string" ? { deviceToken: token } : {}),
          }),
        );
        this.startHeartbeat(generation);
        return;
      }
      if (typeof frame.push === "string") {
        this.pushListeners.forEach((listener) => listener(frame.push!, frame.data));
        return;
      }
      if (typeof frame.id !== "string" || typeof frame.ok !== "boolean") return;
      const pending = this.pending.get(frame.id);
      if (!pending) return;
      this.pending.delete(frame.id);
      this.timers.clearTimeout(pending.timer);
      if (frame.ok) {
        pending.resolve(frame.result);
      } else {
        pending.reject(new Error(frame.error?.message ?? frame.error?.code ?? "request failed"));
      }
      return;
    }

    const bytes =
      data instanceof ArrayBuffer ? new Uint8Array(data) : data instanceof Uint8Array ? data : null;
    if (!bytes) return;
    let opened: { kind: number; plain: Uint8Array };
    try {
      opened = this.session.decryptFrame(bytes);
    } catch {
      // 篡改/乱序:立即重建连接(快照协议保证终端画面恢复)
      if (this.statusValue === "online") {
        this.handleDisconnect();
      } else {
        onCandidateFailure("encrypted handshake response is invalid");
      }
      return;
    }
    if (opened.kind === KIND_TERMINAL) {
      this.noteValidatedInboundTraffic();
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
    const envelope = decodeAeroricEnvelope(parsed);
    if (!envelope) return;
    this.noteValidatedInboundTraffic();
    if (envelope.kind === "push") {
      // 带 seq 的推送参与 watermark:只单调分发,防重复/乱序
      if (typeof envelope.seq === "number") {
        if (envelope.seq <= this.lastPushSeq) return;
        this.lastPushSeq = envelope.seq;
      }
      this.pushListeners.forEach((listener) =>
        listener(envelope.event, envelope.data, envelope.seq),
      );
      return;
    }
    const pending = this.pending.get(envelope.id);
    if (!pending) return;
    this.pending.delete(envelope.id);
    this.timers.clearTimeout(pending.timer);
    if (envelope.ok) {
      pending.resolve(envelope.result);
    } else {
      pending.reject(new Error(envelope.error));
    }
  }

  private failAllPending(err: Error): void {
    for (const pending of this.pending.values()) {
      this.timers.clearTimeout(pending.timer);
      pending.reject(err);
    }
    this.pending.clear();
  }

  private noteValidatedInboundTraffic(): void {
    this.inboundSequence += 1;
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
    let phase: "connecting" | "handshake" | "auth" = "connecting";
    let socketError: string | null = null;
    const disconnectedMessage = (event?: { code?: number; reason?: string }): string => {
      const closeDetails = [
        typeof event?.code === "number" ? `code=${event.code}` : "",
        event?.reason?.trim() ?? "",
        socketError ?? "",
      ]
        .filter(Boolean)
        .join(", ");
      const suffix = closeDetails ? ` (${closeDetails})` : "";
      if (phase === "connecting") {
        return `无法连接到电脑,请检查手机与电脑在同一网络、电脑防火墙和远程服务端口${suffix}`;
      }
      if (phase === "handshake") {
        return `电脑在加密握手阶段断开连接,请确认二维码来自当前电脑${suffix}`;
      }
      return `电脑在验证配对码时断开连接,请重新生成二维码${suffix}`;
    };
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
      if (handshake) {
        phase = "handshake";
        ws.send(handshake.helloJson);
      }
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
        phase = "auth";
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
    ws.onclose = (event) => {
      finish(() => reject(new Error(disconnectedMessage(event))));
    };
    ws.onerror = (event) => {
      const message = event?.message?.trim();
      if (message) socketError = message;
    };
  });
}
