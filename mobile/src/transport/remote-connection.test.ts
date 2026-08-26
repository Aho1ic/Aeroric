import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  pairWithInvite,
  PairingError,
  preferredPairingError,
  RemoteConnection,
  type ConnectionStatus,
  type WebSocketLike,
} from "./remote-connection";
import {
  KIND_CTRL,
  KIND_TERMINAL,
  testGenerateServerKeys,
  testRespondHandshake,
  type E2eeSession,
  type TestServerKeys,
} from "./e2ee";

function utf8(text: string): Uint8Array {
  return new TextEncoder().encode(text);
}

function toArrayBuffer(bytes: Uint8Array): ArrayBuffer {
  return bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength) as ArrayBuffer;
}

/** 模拟桌面端:WS 对端 + E2EE 服务端会话(严格按序解密客户端帧)。 */
class FakeWebSocket implements WebSocketLike {
  sent: Array<string | ArrayBufferLike | Uint8Array> = [];
  closed = false;
  serverSession: E2eeSession | null = null;
  onopen: (() => void) | null = null;
  onmessage: ((event: { data: unknown }) => void) | null = null;
  onclose: ((event?: { code?: number; reason?: string }) => void) | null = null;
  onerror: ((event?: { message?: string }) => void) | null = null;

  private consumed = 0;
  private ctrlFrames: Array<{
    v: number;
    id: number;
    method: string;
    params: Record<string, unknown>;
  }> = [];
  terminalFrames: Uint8Array[] = [];

  send(data: string | ArrayBufferLike | Uint8Array): void {
    this.sent.push(data);
  }

  close(): void {
    this.closed = true;
  }

  // ── 测试驱动辅助 ──
  open(): void {
    this.onopen?.();
  }

  /** 服务端视角:消费客户端 hello,回 hello_ack 建立会话。 */
  acceptHandshake(keys: TestServerKeys): void {
    const hello = this.sent.find((d): d is string => typeof d === "string");
    if (!hello) throw new Error("client never sent hello");
    const { ackJson, session } = testRespondHandshake(keys, hello);
    this.serverSession = session;
    this.onmessage?.({ data: ackJson });
  }

  /** 按序解密到目前为止的全部客户端加密帧,返回控制面 JSON 列表。 */
  clientFrames(): Array<{
    v: number;
    id: number;
    method: string;
    params: Record<string, unknown>;
  }> {
    for (; this.consumed < this.sent.length; this.consumed += 1) {
      const item = this.sent[this.consumed];
      if (typeof item === "string") continue;
      const bytes = item instanceof Uint8Array ? item : new Uint8Array(item as ArrayBufferLike);
      const opened = this.serverSession!.decryptFrame(bytes);
      if (opened.kind === KIND_CTRL) {
        this.ctrlFrames.push(JSON.parse(new TextDecoder().decode(opened.plain)));
      } else {
        this.terminalFrames.push(opened.plain);
      }
    }
    return this.ctrlFrames;
  }

  lastFrame(): { v: number; id: number; method: string; params: Record<string, unknown> } {
    const frames = this.clientFrames();
    return frames[frames.length - 1];
  }

  /** 认证后会并发发出 events.since 与 hello,按方法名定位比 lastFrame() 稳。 */
  frameFor(method: string): {
    v: number;
    id: number;
    method: string;
    params: Record<string, unknown>;
  } {
    const frames = this.clientFrames().filter((frame) => frame.method === method);
    return frames[frames.length - 1];
  }

  receiveCtrl(frame: unknown): void {
    const sealed = this.serverSession!.encryptFrame(KIND_CTRL, utf8(JSON.stringify(frame)));
    this.onmessage?.({ data: toArrayBuffer(sealed) });
  }

  receiveTerminal(plain: Uint8Array): void {
    const sealed = this.serverSession!.encryptFrame(KIND_TERMINAL, plain);
    this.onmessage?.({ data: toArrayBuffer(sealed) });
  }

  replyOk(result: unknown): void {
    const { id } = this.lastFrame();
    this.receiveCtrl({ v: 2, id, ok: true, result });
  }

  replyError(error: string): void {
    const { id } = this.lastFrame();
    this.receiveCtrl({ v: 2, id, ok: false, error });
  }

  dropped(): void {
    this.onclose?.();
  }
}

function createHarness(overrides?: {
  endpoints?: string[];
  serverKeys?: TestServerKeys;
  dialTimeoutMs?: number;
  handshakeTimeoutMs?: number;
}) {
  const serverKeys = overrides?.serverKeys ?? testGenerateServerKeys();
  const sockets: FakeWebSocket[] = [];
  const urls: string[] = [];
  const conn = new RemoteConnection({
    endpoints: overrides?.endpoints ?? ["ws://host-a:1", "ws://host-b:2"],
    serverPublicKey: serverKeys.publicB64,
    authParams: () => ({ deviceToken: "tok" }),
    wsFactory: (url) => {
      urls.push(url);
      const ws = new FakeWebSocket();
      sockets.push(ws);
      return ws;
    },
    dialTimeoutMs: overrides?.dialTimeoutMs,
    handshakeTimeoutMs: overrides?.handshakeTimeoutMs,
    jitter: (delay) => delay,
  });
  const statuses: ConnectionStatus[] = [];
  conn.onStatusChange((status) => statuses.push(status));
  return {
    conn,
    serverKeys,
    sockets,
    urls,
    statuses,
    latest: () => sockets[sockets.length - 1],
  };
}

async function flush(): Promise<void> {
  for (let i = 0; i < 8; i++) {
    await Promise.resolve();
  }
}

/** 让第一个 endpoint 胜出并完成 握手 → auth,返回胜出 socket。 */
async function goOnline(h: ReturnType<typeof createHarness>): Promise<FakeWebSocket> {
  h.conn.start();
  const winner = h.sockets[0];
  winner.open();
  await flush();
  winner.acceptHandshake(h.serverKeys);
  await flush();
  winner.replyOk({ deviceId: "d1" });
  await flush();
  expect(h.conn.status).toBe("online");
  return winner;
}

beforeEach(() => {
  vi.useFakeTimers();
});

afterEach(() => {
  vi.useRealTimers();
});

describe("RemoteConnection", () => {
  it("handshakes, authenticates and reaches online (E2EE end-to-end)", async () => {
    const h = createHarness();
    const authResults: unknown[] = [];
    h.conn.onAuthSuccess((auth) => authResults.push(auth));

    const winner = await goOnline(h);
    // 竞速:两个 endpoint 都被拨号
    expect(h.urls).toEqual(["ws://host-a:1", "ws://host-b:2"]);
    // 落败方被关闭
    expect(h.sockets[1].closed).toBe(true);
    // 第一条明文帧是 hello,随后 auth 走加密通道
    expect(typeof winner.sent[0]).toBe("string");
    expect(JSON.parse(winner.sent[0] as string).type).toBe("hello");
    const authFrame = winner.clientFrames()[0];
    expect(authFrame.v).toBe(2);
    expect(authFrame.method).toBe("auth");
    expect(authFrame.params).toMatchObject({
      deviceToken: "tok",
      supportedRpcVersions: [3, 2],
      capabilities: expect.arrayContaining([
        "typed-envelope",
        "structured-error",
        "tasks.lifecycle",
      ]),
    });
    expect(authResults).toEqual([{ deviceId: "d1" }]);
    expect(h.statuses).toEqual(["connecting", "authenticating", "online"]);
  });

  it("keeps auth on v2 then uses negotiated v3 envelopes", async () => {
    const h = createHarness();
    h.conn.start();
    const winner = h.sockets[0];
    winner.open();
    await flush();
    winner.acceptHandshake(h.serverKeys);
    await flush();
    winner.receiveCtrl({
      v: 2,
      id: winner.lastFrame().id,
      ok: true,
      result: { deviceId: "d1", rpcVersion: 3 },
    });
    await flush();

    const resultPromise = h.conn.request<string[]>("projects.list");
    await flush();
    const request = winner.clientFrames().at(-1) as unknown as {
      v: number;
      type: string;
      id: string;
      method: string;
    };
    expect(request).toMatchObject({
      v: 3,
      type: "request",
      method: "projects.list",
    });
    winner.receiveCtrl({
      v: 3,
      type: "response",
      id: request.id,
      ok: true,
      result: ["project-1"],
    });
    await expect(resultPromise).resolves.toEqual(["project-1"]);
  });

  it("preserves live identity fields while normalizing the host snapshot", async () => {
    const h = createHarness();
    const identities: unknown[] = [];
    h.conn.onHostIdentity((identity) => identities.push(identity));
    h.conn.start();
    const winner = h.sockets[0];
    winner.open();
    await flush();
    winner.acceptHandshake(h.serverKeys);
    await flush();
    winner.receiveCtrl({
      v: 2,
      id: winner.lastFrame().id,
      ok: true,
      result: {
        deviceId: "d1",
        rpcVersion: 3,
        capabilities: ["tasks.lifecycle", 7],
      },
    });
    await flush();

    const hello = winner.frameFor("hello");
    winner.receiveCtrl({
      v: 3,
      type: "response",
      id: hello.id,
      ok: true,
      result: {
        name: "aeroric",
        hostId: "host-1",
        hostName: "Studio Mac",
        version: "1.4.4",
        platform: "darwin",
        rpcVersions: [3, 2, 99],
        capabilities: ["tasks.lifecycle", "files.read", null],
        endpoints: ["ws://192.168.1.20:38473"],
        lanEndpoints: ["ws://192.168.1.20:38473"],
      },
    });
    await flush();

    expect(h.conn.negotiatedRpcVersion).toBe(3);
    expect(h.conn.negotiatedCapabilities).toEqual(["tasks.lifecycle"]);
    expect(identities).toEqual([
      expect.objectContaining({
        hostId: "host-1",
        hostName: "Studio Mac",
        rpcVersions: [3, 2],
        capabilities: ["tasks.lifecycle", "files.read"],
        endpoints: ["ws://192.168.1.20:38473"],
        lanEndpoints: ["ws://192.168.1.20:38473"],
      }),
    ]);
  });

  it("aborts as unauthorized when the host key does not match the pinned key", async () => {
    const h = createHarness();
    h.conn.start();
    const imposterKeys = testGenerateServerKeys();
    for (const candidate of h.sockets) {
      candidate.open();
    }
    await flush();
    // 所有候选都是冒名主机时才停止重试；单个坏 endpoint 不应淘汰健康候选。
    h.sockets[0].acceptHandshake(imposterKeys);
    await flush();
    h.sockets[1].acceptHandshake(imposterKeys);
    await flush();
    expect(h.conn.status).toBe("unauthorized");
    expect(h.conn.authError).toContain("主机身份验证失败");
    await vi.advanceTimersByTimeAsync(60_000);
    expect(h.sockets.length).toBe(2); // 不再重试
  });

  it("keeps other endpoints alive until one completes authenticated E2EE", async () => {
    const h = createHarness();
    h.conn.start();
    const stale = h.sockets[0];
    const healthy = h.sockets[1];
    stale.open();
    healthy.open();
    await flush();

    expect(stale.sent.some((frame) => typeof frame === "string")).toBe(true);
    expect(healthy.sent).toHaveLength(0);
    expect(healthy.closed).toBe(false);

    stale.acceptHandshake(testGenerateServerKeys());
    await flush();
    expect(healthy.sent.some((frame) => typeof frame === "string")).toBe(true);

    healthy.acceptHandshake(h.serverKeys);
    await flush();
    healthy.replyOk({ deviceId: "d1" });
    await flush();

    expect(h.conn.status).toBe("online");
    expect(stale.closed).toBe(true);
    expect(healthy.closed).toBe(false);
  });

  it("times out a silent handshake and falls back to an already-open endpoint", async () => {
    const h = createHarness({ handshakeTimeoutMs: 500 });
    h.conn.start();
    const silent = h.sockets[0];
    const healthy = h.sockets[1];
    silent.open();
    healthy.open();
    await flush();

    await vi.advanceTimersByTimeAsync(500);
    expect(silent.closed).toBe(true);
    expect(healthy.sent.some((frame) => typeof frame === "string")).toBe(true);

    healthy.acceptHandshake(h.serverKeys);
    await flush();
    healthy.replyOk({ deviceId: "d1" });
    await flush();
    expect(h.conn.status).toBe("online");
  });

  it("correlates requests and pushes independently", async () => {
    const h = createHarness();
    const winner = await goOnline(h);

    const pushes: Array<[string, unknown]> = [];
    h.conn.onPush((push, data) => pushes.push([push, data]));

    const promise = h.conn.request("projects.list");
    winner.receiveCtrl({ v: 2, push: "task-status", data: { task_id: "t1", status: "done" } });
    winner.replyOk([{ id: "p1" }]);
    await expect(promise).resolves.toEqual([{ id: "p1" }]);
    expect(pushes).toEqual([["task-status", { task_id: "t1", status: "done" }]]);
  });

  it("rejects request on error reply and when offline", async () => {
    const h = createHarness();
    await expect(h.conn.request("ping")).rejects.toThrow(/not online/);

    const winner = await goOnline(h);
    const promise = h.conn.request("tasks.list", { projectId: "p1" });
    winner.replyError("Missing param: projectId");
    await expect(promise).rejects.toThrow("Missing param: projectId");
  });

  it("times out requests", async () => {
    const h = createHarness();
    await goOnline(h);
    const promise = h.conn.request("ping");
    const assertion = expect(promise).rejects.toThrow(/request timeout/);
    await vi.advanceTimersByTimeAsync(15_000);
    await assertion;
  });

  it("reconnects with exponential backoff, racing all endpoints each round", async () => {
    const h = createHarness();
    h.conn.start();
    expect(h.urls).toEqual(["ws://host-a:1", "ws://host-b:2"]);

    // 双双失败 → 1s 后整轮重试
    h.sockets[0].dropped();
    h.sockets[1].dropped();
    expect(h.conn.status).toBe("reconnecting");
    await vi.advanceTimersByTimeAsync(1_000);
    expect(h.urls.length).toBe(4);

    // 再失败 → 2s 后第三轮
    h.sockets[2].dropped();
    h.sockets[3].dropped();
    await vi.advanceTimersByTimeAsync(2_000);
    expect(h.urls.length).toBe(6);

    // 第三轮胜出并认证成功 → 退避计数重置
    const winner = h.sockets[4];
    winner.open();
    await flush();
    winner.acceptHandshake(h.serverKeys);
    await flush();
    winner.replyOk({ deviceId: "d1" });
    await flush();
    expect(h.conn.status).toBe("online");
    winner.dropped();
    await vi.advanceTimersByTimeAsync(1_000);
    expect(h.urls.length).toBe(8);
  });

  it("redials a stale in-flight round when the app returns to the foreground", async () => {
    const h = createHarness();
    h.conn.start();
    const staleSockets = [...h.sockets];

    // A suspended RN app can retain these sockets in CONNECTING even after the
    // OS has dropped their network path and paused their timeout callbacks.
    await vi.advanceTimersByTimeAsync(2_000);
    h.conn.notifyForeground();

    expect(h.conn.status).toBe("connecting");
    expect(h.urls).toEqual(["ws://host-a:1", "ws://host-b:2", "ws://host-a:1", "ws://host-b:2"]);
    expect(staleSockets.every((socket) => socket.closed)).toBe(true);
  });

  it("keeps a fresh in-flight round when duplicate foreground signals arrive", async () => {
    const h = createHarness();
    h.conn.start();
    const freshSockets = [...h.sockets];

    await vi.advanceTimersByTimeAsync(1_999);
    h.conn.notifyForeground();
    h.conn.notifyForeground();

    expect(h.urls).toEqual(["ws://host-a:1", "ws://host-b:2"]);
    expect(freshSockets.some((socket) => socket.closed)).toBe(false);
  });

  it("skips a queued reconnect delay when the app returns to the foreground", async () => {
    const h = createHarness();
    h.conn.start();
    h.sockets[0].dropped();
    h.sockets[1].dropped();
    expect(h.conn.status).toBe("reconnecting");

    h.conn.notifyForeground();

    expect(h.conn.status).toBe("connecting");
    expect(h.urls).toEqual(["ws://host-a:1", "ws://host-b:2", "ws://host-a:1", "ws://host-b:2"]);
    await vi.advanceTimersByTimeAsync(1_000);
    expect(h.urls).toHaveLength(4);
  });

  it("keeps escalating after stale foreground rounds are abandoned", async () => {
    const h = createHarness();
    h.conn.start();

    // Each foreground recovery replaces a stale in-flight round. Those rounds
    // still count as failed attempts, rather than resetting to a 1s retry on
    // every app resume during a real outage.
    for (let attempt = 0; attempt < 2; attempt += 1) {
      await vi.advanceTimersByTimeAsync(2_000);
      h.conn.notifyForeground();
    }

    const currentRound = h.sockets.slice(-2);
    currentRound[0].dropped();
    currentRound[1].dropped();
    expect(h.conn.status).toBe("reconnecting");

    // Two abandoned rounds have already booked two failures, so this next
    // retry waits 4s (not a reset 1s backoff).
    const socketsBeforeRetry = h.sockets.length;
    await vi.advanceTimersByTimeAsync(3_999);
    expect(h.sockets).toHaveLength(socketsBeforeRetry);
    await vi.advanceTimersByTimeAsync(1);
    expect(h.sockets).toHaveLength(socketsBeforeRetry + 2);
  });

  it("redials a stale active retry round without resetting its backoff", async () => {
    const h = createHarness();
    h.conn.start();
    h.sockets[0].dropped();
    h.sockets[1].dropped();
    await vi.advanceTimersByTimeAsync(1_000);

    // `openSockets("reconnecting")` leaves the retry's sockets in flight while
    // preserving the reconnecting status. A suspension here must take the same
    // stale-dial path as an initial round.
    expect(h.conn.status).toBe("reconnecting");
    const staleRetrySockets = h.sockets.slice(-2);
    await vi.advanceTimersByTimeAsync(2_000);
    h.conn.notifyForeground();

    expect(h.conn.status).toBe("connecting");
    expect(staleRetrySockets.every((socket) => socket.closed)).toBe(true);

    // The initial failed round plus this abandoned retry book two failures, so
    // a further failure waits 4s instead of being reset to the 1s first retry.
    const replacementRound = h.sockets.slice(-2);
    replacementRound[0].dropped();
    replacementRound[1].dropped();
    const socketsBeforeRetry = h.sockets.length;
    await vi.advanceTimersByTimeAsync(3_999);
    expect(h.sockets).toHaveLength(socketsBeforeRetry);
    await vi.advanceTimersByTimeAsync(1);
    expect(h.sockets).toHaveLength(socketsBeforeRetry + 2);
  });

  it("stops retrying after token revocation", async () => {
    const h = createHarness();
    h.conn.start();
    const winner = h.sockets[0];
    winner.open();
    await flush();
    winner.acceptHandshake(h.serverKeys);
    await flush();
    winner.replyError("Unknown device token");
    await flush();
    expect(h.conn.status).toBe("unauthorized");
    expect(h.conn.authError).toContain("Unknown device token");
    await vi.advanceTimersByTimeAsync(60_000);
    expect(h.sockets.length).toBe(2);
  });

  it("stop() prevents any further reconnects", async () => {
    const h = createHarness();
    const winner = await goOnline(h);
    h.conn.stop();
    expect(h.conn.status).toBe("stopped");
    await vi.advanceTimersByTimeAsync(120_000);
    expect(h.sockets.length).toBe(2);
    expect(winner.closed).toBe(true);
  });

  it("sends heartbeat pings and reconnects when they time out", async () => {
    const h = createHarness();
    const winner = await goOnline(h);

    // 25s 后发出 ping,pong 回来则继续在线
    await vi.advanceTimersByTimeAsync(25_000);
    expect(winner.lastFrame().method).toBe("ping");
    winner.replyOk("pong");
    await flush();
    expect(h.conn.status).toBe("online");

    // 下一轮 ping 无响应 → 10s 超时 → 重连
    await vi.advanceTimersByTimeAsync(25_000);
    expect(winner.lastFrame().method).toBe("ping");
    await vi.advanceTimersByTimeAsync(10_000);
    expect(h.conn.status).toBe("reconnecting");
  });

  it("probes an online socket immediately after returning to the foreground", async () => {
    const h = createHarness();
    const winner = await goOnline(h);

    h.conn.notifyForeground();
    expect(winner.frameFor("ping").method).toBe("ping");

    // The foreground probe uses the existing ping timeout rather than waiting
    // for the next 25-second heartbeat to discover a silently dropped socket.
    await vi.advanceTimersByTimeAsync(10_000);
    expect(h.conn.status).toBe("reconnecting");
  });

  it("keeps the socket when another valid response arrives during a foreground probe", async () => {
    const h = createHarness();
    const winner = await goOnline(h);

    const request = h.conn.request("projects.list");
    h.conn.notifyForeground();
    expect(winner.frameFor("ping").method).toBe("ping");

    const projects = winner.frameFor("projects.list");
    winner.receiveCtrl({ v: 2, id: projects.id, ok: true, result: [] });
    await expect(request).resolves.toEqual([]);

    // The foreground ping itself is deliberately left unanswered. The valid
    // response above proves this same authenticated socket is still alive.
    await vi.advanceTimersByTimeAsync(10_000);
    expect(h.conn.status).toBe("online");
    expect(winner.closed).toBe(false);
  });

  it("reuses an in-flight heartbeat for foreground liveness and accepts its response", async () => {
    const h = createHarness();
    const winner = await goOnline(h);

    await vi.advanceTimersByTimeAsync(25_000);
    const heartbeat = winner.frameFor("ping");
    h.conn.notifyForeground();

    // A foreground edge while a heartbeat is pending must not create a second
    // independent timeout that can race to tear down a healthy connection.
    expect(winner.clientFrames().filter((frame) => frame.method === "ping")).toHaveLength(1);
    winner.receiveCtrl({ v: 2, id: heartbeat.id, ok: true, result: "pong" });
    await flush();

    expect(h.conn.status).toBe("online");
    await vi.advanceTimersByTimeAsync(25_000);
    expect(winner.clientFrames().filter((frame) => frame.method === "ping")).toHaveLength(2);
  });

  it("encrypts terminal frames in both directions", async () => {
    const h = createHarness();
    const received: ArrayBuffer[] = [];
    h.conn.onBinary((data) => received.push(data));

    // 离线时发送被丢弃
    expect(h.conn.sendBinary(new Uint8Array([1, 2, 3]))).toBe(false);

    const winner = await goOnline(h);
    expect(h.conn.sendBinary(new Uint8Array([9, 9]))).toBe(true);
    winner.clientFrames();
    expect(winner.terminalFrames.map((f) => Array.from(f))).toEqual([[9, 9]]);

    winner.receiveTerminal(new Uint8Array([0x74, 1, 1, 0]));
    await flush();
    expect(received.length).toBe(1);
    expect(Array.from(new Uint8Array(received[0]))).toEqual([0x74, 1, 1, 0]);
  });

  it("drops the connection when an inbound frame fails to decrypt", async () => {
    const h = createHarness();
    const winner = await goOnline(h);
    const sealed = winner.serverSession!.encryptFrame(KIND_CTRL, utf8("{}"));
    sealed[sealed.length - 1] ^= 1;
    winner.onmessage?.({ data: toArrayBuffer(sealed) });
    await flush();
    expect(h.conn.status).toBe("reconnecting");
  });

  it("replays missed pushes via events.since after reconnect", async () => {
    const h = createHarness();
    const pushes: Array<[string, unknown, number | undefined]> = [];
    h.conn.onPush((push, data, seq) => pushes.push([push, data, seq]));

    const winner = await goOnline(h);
    // 在线期间收到 seq=1 的推送(建立 watermark)
    winner.receiveCtrl({
      v: 2,
      push: "task-status",
      seq: 1,
      data: { task_id: "t1", status: "running" },
    });
    await flush();
    expect(pushes).toHaveLength(1);

    // 断线 → 重连(重新竞速 + 握手 + 认证)
    winner.dropped();
    await vi.advanceTimersByTimeAsync(1_000);
    const second = h.sockets[2];
    second.open();
    await flush();
    second.acceptHandshake(h.serverKeys);
    await flush();
    // 第一帧是 auth
    expect(second.lastFrame().method).toBe("auth");
    second.replyOk({ deviceId: "d1" });
    await flush();
    // 认证成功后自动请求补发
    const replay = second.frameFor("events.since");
    expect(replay.method).toBe("events.since");
    expect(replay.params).toEqual({ after: 1 });
    second.receiveCtrl({
      v: 2,
      id: replay.id,
      ok: true,
      result: {
        events: [
          { seq: 2, event: "task-status", data: { task_id: "t1", status: "input_required" } },
        ],
        latestSeq: 2,
        reset: false,
      },
    });
    await flush();
    expect(pushes).toHaveLength(2);
    expect(pushes[1]).toEqual(["task-status", { task_id: "t1", status: "input_required" }, 2]);

    // 重复 seq 的推送被单调丢弃
    second.receiveCtrl({
      v: 2,
      push: "task-status",
      seq: 2,
      data: { task_id: "t1", status: "input_required" },
    });
    expect(pushes).toHaveLength(2);
  });

  it("broadcasts events.reset when the backfill window is gone", async () => {
    const h = createHarness();
    const pushes: string[] = [];
    h.conn.onPush((push) => pushes.push(push));

    const winner = await goOnline(h);
    winner.receiveCtrl({
      v: 2,
      push: "task-status",
      seq: 7,
      data: { task_id: "t", status: "done" },
    });
    await flush();
    winner.dropped();
    await vi.advanceTimersByTimeAsync(1_000);
    const second = h.sockets[2];
    second.open();
    await flush();
    second.acceptHandshake(h.serverKeys);
    await flush();
    second.replyOk({ deviceId: "d1" });
    await flush();
    const replay = second.frameFor("events.since");
    expect(replay.method).toBe("events.since");
    second.receiveCtrl({
      v: 2,
      id: replay.id,
      ok: true,
      result: { events: [], latestSeq: 900, reset: true },
    });
    await flush();
    expect(pushes[pushes.length - 1]).toBe("events.reset");
  });
});

describe("pairWithInvite", () => {
  function pairHarness(
    serverKeys: TestServerKeys,
    confirmation?: {
      persistProvisionalCredentials: NonNullable<
        Parameters<typeof pairWithInvite>[0]["persistProvisionalCredentials"]
      >;
      discardProvisionalCredentials?: () => Promise<void>;
    },
  ) {
    let ws: FakeWebSocket | null = null;
    const promise = pairWithInvite({
      endpoint: "ws://host:1",
      invite: "inv",
      deviceName: "iPhone",
      serverPublicKey: serverKeys.publicB64,
      pairingConfirmationVersion: confirmation ? 1 : undefined,
      persistProvisionalCredentials: confirmation?.persistProvisionalCredentials,
      discardProvisionalCredentials: confirmation?.discardProvisionalCredentials,
      wsFactory: () => {
        ws = new FakeWebSocket();
        return ws;
      },
    });
    return { promise, socket: () => ws! };
  }

  it("handshakes then exchanges invite for device credentials", async () => {
    const keys = testGenerateServerKeys();
    const { promise, socket } = pairHarness(keys);
    socket().open();
    socket().acceptHandshake(keys);
    const frame = socket().lastFrame();
    expect(frame.method).toBe("auth");
    expect(frame.params).toEqual({ invite: "inv", deviceName: "iPhone" });
    socket().replyOk({
      deviceId: "d1",
      deviceToken: "tok",
      host: { name: "Mac", version: "1", platform: "macos" },
    });
    await expect(promise).resolves.toMatchObject({ deviceId: "d1", deviceToken: "tok" });
  });

  it.each(["uint8array", "blob"] as const)(
    "accepts an encrypted auth response delivered as %s",
    async (kind) => {
      const keys = testGenerateServerKeys();
      const { promise, socket } = pairHarness(keys);
      socket().open();
      socket().acceptHandshake(keys);
      const request = socket().lastFrame();
      const sealed = socket().serverSession!.encryptFrame(
        KIND_CTRL,
        utf8(
          JSON.stringify({
            v: 2,
            id: request.id,
            ok: true,
            result: { deviceId: "d1", deviceToken: "tok" },
          }),
        ),
      );
      const data =
        kind === "uint8array" ? sealed : { arrayBuffer: async () => toArrayBuffer(sealed) };

      socket().onmessage?.({ data });

      await expect(promise).resolves.toMatchObject({ deviceId: "d1", deviceToken: "tok" });
    },
  );

  it("persists provisional credentials before sending confirmation", async () => {
    const keys = testGenerateServerKeys();
    let socketRef: FakeWebSocket | null = null;
    const persistProvisionalCredentials = vi.fn(async () => {
      expect(socketRef?.frameFor("pairing.confirm")).toBeUndefined();
    });
    const harness = pairHarness(keys, { persistProvisionalCredentials });
    socketRef = harness.socket();
    harness.socket().open();
    harness.socket().acceptHandshake(keys);
    expect(harness.socket().lastFrame().params).toEqual({
      invite: "inv",
      deviceName: "iPhone",
      pairingConfirmationVersion: 1,
    });

    harness.socket().replyOk({
      deviceId: "d1",
      deviceToken: "tok",
      pairingId: "pair-1",
      pairingConfirmationVersion: 1,
    });
    await flush();

    expect(persistProvisionalCredentials).toHaveBeenCalledWith(
      expect.objectContaining({ deviceId: "d1", deviceToken: "tok", pairingId: "pair-1" }),
    );
    const confirm = harness.socket().frameFor("pairing.confirm");
    expect(confirm.params).toEqual({ pairingId: "pair-1", pairingConfirmationVersion: 1 });
    harness.socket().receiveCtrl({
      v: 2,
      id: confirm.id,
      ok: true,
      result: { confirmed: true },
    });

    await expect(harness.promise).resolves.toMatchObject({
      deviceId: "d1",
      deviceToken: "tok",
    });
  });

  it("does not confirm when provisional credential storage fails", async () => {
    const keys = testGenerateServerKeys();
    const failure = new Error("SecureStore unavailable");
    const { promise, socket } = pairHarness(keys, {
      persistProvisionalCredentials: async () => Promise.reject(failure),
    });
    socket().open();
    socket().acceptHandshake(keys);
    socket().replyOk({
      deviceId: "d1",
      deviceToken: "tok",
      pairingId: "pair-1",
      pairingConfirmationVersion: 1,
    });

    await expect(promise).rejects.toMatchObject({
      kind: "credential_storage",
      message: "SecureStore unavailable",
    });
    expect(socket().frameFor("pairing.confirm")).toBeUndefined();
  });

  it("discards staged credentials after an explicit negative confirmation", async () => {
    const keys = testGenerateServerKeys();
    const persistProvisionalCredentials = vi.fn(async () => undefined);
    const discardProvisionalCredentials = vi.fn(async () => undefined);
    const { promise, socket } = pairHarness(keys, {
      persistProvisionalCredentials,
      discardProvisionalCredentials,
    });
    socket().open();
    socket().acceptHandshake(keys);
    socket().replyOk({
      deviceId: "d1",
      deviceToken: "tok",
      pairingId: "pair-1",
      pairingConfirmationVersion: 1,
    });
    await flush();
    const confirm = socket().frameFor("pairing.confirm");
    socket().receiveCtrl({ v: 2, id: confirm.id, ok: false, error: "disk full" });

    await expect(promise).rejects.toMatchObject({ kind: "confirmation", message: "disk full" });
    expect(persistProvisionalCredentials).toHaveBeenCalledTimes(1);
    expect(discardProvisionalCredentials).toHaveBeenCalledTimes(1);
  });

  it("keeps staged credentials when the confirmation ACK is network-uncertain", async () => {
    const keys = testGenerateServerKeys();
    const discardProvisionalCredentials = vi.fn(async () => undefined);
    const { promise, socket } = pairHarness(keys, {
      persistProvisionalCredentials: async () => undefined,
      discardProvisionalCredentials,
    });
    socket().open();
    socket().acceptHandshake(keys);
    socket().replyOk({
      deviceId: "d1",
      deviceToken: "tok",
      pairingId: "pair-1",
      pairingConfirmationVersion: 1,
    });
    await flush();
    expect(socket().frameFor("pairing.confirm")).toBeDefined();
    socket().dropped();

    await expect(promise).rejects.toMatchObject({ kind: "network" });
    expect(discardProvisionalCredentials).not.toHaveBeenCalled();
  });

  it("rejects on auth failure with server message", async () => {
    const keys = testGenerateServerKeys();
    const { promise, socket } = pairHarness(keys);
    socket().open();
    socket().acceptHandshake(keys);
    socket().replyError("Invalid or expired invite");
    await expect(promise).rejects.toMatchObject({
      kind: "invite",
      message: "Invalid or expired invite",
    });
  });

  it("rejects when the host identity does not match the QR code", async () => {
    const keys = testGenerateServerKeys();
    const { promise, socket } = pairHarness(keys);
    socket().open();
    socket().acceptHandshake(testGenerateServerKeys());
    await expect(promise).rejects.toMatchObject({
      kind: "host_identity",
      message: expect.stringMatching(/主机身份验证失败/),
    });
  });

  it("reports an unsupported encrypted auth payload separately from a network failure", async () => {
    const keys = testGenerateServerKeys();
    const { promise, socket } = pairHarness(keys);
    socket().open();
    socket().acceptHandshake(keys);
    socket().onmessage?.({ data: { unsupportedBinary: true } });

    await expect(promise).rejects.toMatchObject({
      kind: "auth_response",
      message: expect.stringMatching(/无法解析.*加密认证响应/),
    });
  });

  it("rejects on timeout", async () => {
    const keys = testGenerateServerKeys();
    const { promise } = pairHarness(keys);
    const assertion = expect(promise).rejects.toThrow(/配对超时/);
    await vi.advanceTimersByTimeAsync(12_000);
    await assertion;
  });

  it("reports a pre-open disconnect as a network or firewall failure", async () => {
    const keys = testGenerateServerKeys();
    const { promise, socket } = pairHarness(keys);
    socket().onclose?.({ code: 1006 });
    await expect(promise).rejects.toMatchObject({
      kind: "network",
      message: expect.stringMatching(/防火墙和远程服务端口.*code=1006/),
    });
  });

  it("fails fast when the hello send throws instead of waiting out the timeout", async () => {
    // send 抛出时 socket 仍是 open,onclose 不会触发。没有兜底的话配对要挂满 12 秒
    // 才超时;这里不推进定时器,拿到 rejection 才说明走的是即时失败路径。
    const keys = testGenerateServerKeys();
    let socket: FakeWebSocket | null = null;
    const promise = pairWithInvite({
      endpoint: "ws://host:1",
      invite: "inv",
      deviceName: "iPhone",
      serverPublicKey: keys.publicB64,
      wsFactory: () => {
        socket = new FakeWebSocket();
        socket.send = () => {
          throw new Error("socket write failed");
        };
        return socket;
      },
    });
    socket!.open();
    await expect(promise).rejects.toMatchObject({ kind: "network" });
    expect(socket!.closed).toBe(true);
  });

  it("prefers an invite or identity diagnosis over a later endpoint disconnect", () => {
    const selected = preferredPairingError([
      new PairingError("host_identity", "wrong host"),
      new PairingError("network", "connection closed"),
      new PairingError("invite", "Invalid or expired invite"),
    ]);

    expect(selected).toMatchObject({ kind: "invite", message: "Invalid or expired invite" });
  });
});
