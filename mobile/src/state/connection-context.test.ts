// @vitest-environment jsdom
/**
 * ConnectionProvider 对主机 typed 关闭原因的透出与退订。
 * 用 jsdom + react-dom/client 是因为要真正跑 effect:订阅/退订只发生在 effect 里,
 * renderToStaticMarkup 不执行 effect,证明不了「卸载不泄漏监听器」。
 */

import { act, createElement, type ReactNode } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ConnectionProvider, useConnection } from "./connection-context";

/** 测试实际断言到的假连接表面(监听表用 Set:要断言 .size 与运行时增删)。 */
interface FakeConn {
  readonly statusListeners: Set<(status: string) => void>;
  readonly reasonListeners: Set<(reason: "signed-out") => void>;
  stopped: boolean;
  /** 主机在 close frame 上给出原因:transport 立刻分发,状态随后落到 reconnecting。 */
  dropWithReason(reason: "signed-out"): void;
  /**
   * 竞速中落败的候选带着原因关闭:transport 记下并分发原因,但 failCandidate
   * 只在候选是 active 时才改状态,所以这一路完全没有状态跳变。
   */
  noteReasonOnly(reason: "signed-out"): void;
  emitStatus(next: string): void;
}

const reconcileHostIdentity = vi.fn(async () => {});

const hoisted = vi.hoisted(() => {
  const instances: FakeConn[] = [];

  /** 只实现 provider 真正调用到的表面。 */
  class FakeConnection implements FakeConn {
    readonly statusListeners = new Set<(status: string) => void>();
    readonly reasonListeners = new Set<(reason: "signed-out") => void>();
    readonly authListeners = new Set<(auth: unknown) => void>();
    readonly pushListeners = new Set<(push: string, data: unknown, seq?: number) => void>();
    readonly binaryListeners = new Set<(data: ArrayBuffer) => void>();
    readonly identityListeners = new Set<(identity: unknown, endpoint: string | null) => void>();
    status = "connecting";
    authError: string | null = null;
    hostCloseReason: "signed-out" | null = null;
    negotiatedRpcVersion = 2;
    negotiatedCapabilities: readonly string[] = [];
    stopped = false;

    constructor() {
      instances.push(this);
    }

    onStatusChange(listener: (status: string) => void) {
      this.statusListeners.add(listener);
      return () => this.statusListeners.delete(listener);
    }
    onHostCloseReason(listener: (reason: "signed-out") => void) {
      this.reasonListeners.add(listener);
      return () => this.reasonListeners.delete(listener);
    }
    onAuthSuccess(listener: (auth: unknown) => void) {
      this.authListeners.add(listener);
      return () => this.authListeners.delete(listener);
    }
    onPush(listener: (push: string, data: unknown, seq?: number) => void) {
      this.pushListeners.add(listener);
      return () => this.pushListeners.delete(listener);
    }
    onBinary(listener: (data: ArrayBuffer) => void) {
      this.binaryListeners.add(listener);
      return () => this.binaryListeners.delete(listener);
    }
    onHostIdentity(listener: (identity: unknown, endpoint: string | null) => void) {
      this.identityListeners.add(listener);
      return () => this.identityListeners.delete(listener);
    }
    start() {}
    stop() {
      this.stopped = true;
    }
    updateEndpoints() {}
    notifyForeground() {}

    dropWithReason(reason: "signed-out") {
      this.hostCloseReason = reason;
      this.reasonListeners.forEach((listener) => listener(reason));
      this.emitStatus("reconnecting");
    }

    noteReasonOnly(reason: "signed-out") {
      this.hostCloseReason = reason;
      this.reasonListeners.forEach((listener) => listener(reason));
    }

    emitStatus(next: string) {
      this.status = next;
      if (next === "online") this.hostCloseReason = null;
      this.statusListeners.forEach((listener) => listener(next));
    }
  }

  return { instances, FakeConnection };
});

vi.mock("react-native", () => ({
  AppState: {
    currentState: "active",
    addEventListener: () => ({ remove: () => {} }),
  },
}));

vi.mock("./hosts-context", () => ({
  useHosts: () => ({
    activeHost: {
      id: "host-1",
      name: "Desk",
      endpoints: ["ws://127.0.0.1:8787"],
      publicKey: "pk",
      deviceToken: "token",
      protocol: "aeroric",
    },
    reconcileHostIdentity,
  }),
}));

vi.mock("../transport/remote-connection", () => ({
  RemoteConnection: hoisted.FakeConnection,
}));

let container: HTMLDivElement;
let root: Root;
let seen: { hostCloseReason: string | null; status: string } | null = null;

function Probe(): ReactNode {
  const { hostCloseReason, status } = useConnection();
  seen = { hostCloseReason, status };
  return null;
}

beforeEach(async () => {
  hoisted.instances.length = 0;
  seen = null;
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
  await act(async () => {
    root.render(createElement(ConnectionProvider, null, createElement(Probe)));
  });
});

afterEach(() => {
  container.remove();
});

describe("ConnectionProvider host close reason", () => {
  it("透出主机给出的 signed-out 原因", async () => {
    const conn = hoisted.instances[0]!;
    expect(seen?.hostCloseReason).toBeNull();

    await act(async () => conn.dropWithReason("signed-out"));

    expect(seen).toEqual({ hostCloseReason: "signed-out", status: "reconnecting" });
  });

  it("没有状态跳变时也必须透出原因(竞速落败候选带原因关闭)", async () => {
    const conn = hoisted.instances[0]!;
    const statusBefore = seen?.status;

    await act(async () => conn.noteReasonOnly("signed-out"));

    // 只订阅 onStatusChange 的实现在这一路会漏掉原因:状态一动不动
    expect(seen).toEqual({ hostCloseReason: "signed-out", status: statusBefore });
  });

  it("重连成功后原因过期,不再挂在界面上", async () => {
    const conn = hoisted.instances[0]!;
    await act(async () => conn.dropWithReason("signed-out"));
    expect(seen?.hostCloseReason).toBe("signed-out");

    await act(async () => conn.emitStatus("online"));

    expect(seen).toEqual({ hostCloseReason: null, status: "online" });
  });

  it("卸载时退订原因监听,不泄漏监听器", async () => {
    const conn = hoisted.instances[0]!;
    expect(conn.reasonListeners.size).toBe(1);

    await act(async () => root.unmount());

    expect(conn.reasonListeners.size).toBe(0);
    expect(conn.statusListeners.size).toBe(0);
    expect(conn.stopped).toBe(true);
  });
});
