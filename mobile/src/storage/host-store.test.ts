import { describe, expect, it, vi } from "vitest";

vi.mock("expo-secure-store", () => ({
  getItemAsync: vi.fn(),
  setItemAsync: vi.fn(),
}));

import type { HostIdentity, PairedHost } from "../types";
import {
  addOrReplaceHost,
  isSameHost,
  mergeHostIdentity,
  type HostStoreState,
} from "./host-store";

function host(overrides: Partial<PairedHost>): PairedHost {
  return {
    id: "local-id",
    name: "Desktop",
    endpoints: ["ws://192.168.1.10:6790"],
    deviceId: "device-id",
    deviceToken: "token",
    pairedAt: 1,
    ...overrides,
  };
}

describe("host identity", () => {
  it("keeps same-name hosts with different identities", () => {
    const first = host({ id: "a", hostId: "host-a", name: "Mac" });
    const second = host({
      id: "b",
      hostId: "host-b",
      name: "Mac",
      endpoints: ["ws://192.168.1.11:6790"],
    });
    const state: HostStoreState = { hosts: [first], activeHostId: first.id };

    const next = addOrReplaceHost(state, second);
    expect(next.hosts).toEqual([first, second]);
    expect(next.activeHostId).toBe(second.id);
  });

  it("replaces a re-paired host by stable host id", () => {
    const oldHost = host({ id: "host-a", hostId: "host-a", deviceToken: "old" });
    const newHost = host({
      id: "host-a",
      hostId: "host-a",
      deviceId: "new-device",
      deviceToken: "new",
      endpoints: ["wss://relay.example/host-a"],
    });

    const next = addOrReplaceHost(
      { hosts: [oldHost], activeHostId: oldHost.id },
      newHost,
    );
    expect(next.hosts).toEqual([newHost]);
  });

  it("uses endpoint matching for legacy records without a host id", () => {
    const legacy = host({ id: "legacy", hostId: undefined });
    const paired = host({
      id: "stable",
      hostId: "stable",
      endpoints: ["ws://192.168.1.10:6790/"],
    });
    expect(isSameHost(legacy, paired)).toBe(true);
  });
});

describe("mergeHostIdentity", () => {
  function state(hosts: PairedHost[], activeHostId: string | null = null): HostStoreState {
    return { hosts, activeHostId };
  }

  it("回填缺失的 hostId 并同步主机名", () => {
    const before = state([host({ id: "a", name: "旧名字", hostId: undefined })]);
    const identity: HostIdentity = { hostId: "H1", hostName: "MacBook" };

    const after = mergeHostIdentity(before, "a", identity);

    expect(after.hosts[0].hostId).toBe("H1");
    expect(after.hosts[0].name).toBe("MacBook");
  });

  it("身份无变化时返回同一引用,避免多余写入", () => {
    const before = state([host({ id: "a", hostId: "H1", name: "MacBook" })]);
    const identity: HostIdentity = {
      hostId: "H1",
      hostName: "MacBook",
      lanEndpoints: ["ws://192.168.1.10:6790"],
    };

    expect(mergeHostIdentity(before, "a", identity, "ws://192.168.1.10:6790")).toBe(before);
  });

  it("按 hostId 合并跨网段产生的重复记录,端点取并集且保留当前记录的凭据", () => {
    const before = state(
      [
        host({
          id: "a",
          hostId: "H1",
          endpoints: ["ws://10.10.20.2:6790", "wss://tunnel.example.com/connect"],
          deviceToken: "token-current",
        }),
        host({ id: "b", hostId: "H1", endpoints: ["ws://192.168.1.10:6790"] }),
      ],
      "b",
    );
    const identity: HostIdentity = { hostId: "H1", lanEndpoints: ["ws://10.10.20.2:6790"] };

    const after = mergeHostIdentity(before, "a", identity, "ws://10.10.20.2:6790");

    expect(after.hosts).toHaveLength(1);
    expect(after.hosts[0].id).toBe("a");
    expect(after.hosts[0].deviceToken).toBe("token-current");
    expect(after.hosts[0].endpoints).toContain("wss://tunnel.example.com/connect");
    // 被合并掉的记录正是当前活跃主机 → 活跃指针改指向保留下来的那条
    expect(after.activeHostId).toBe("a");
  });

  it("换网段后用新 LAN 地址替换过期私网地址,但保留隧道/中继地址", () => {
    const before = state([
      host({
        id: "a",
        hostId: "H1",
        endpoints: ["ws://192.168.1.10:6790", "wss://relay.example.com/connect/x"],
      }),
    ]);
    const identity: HostIdentity = { hostId: "H1", lanEndpoints: ["ws://10.10.20.2:6790"] };

    const after = mergeHostIdentity(before, "a", identity, "ws://10.10.20.2:6790");

    expect(after.hosts[0].endpoints).not.toContain("ws://192.168.1.10:6790");
    expect(after.hosts[0].endpoints).toContain("wss://relay.example.com/connect/x");
  });

  it("换网段后同时淘汰过期的 IPv6 ULA 与 link-local 地址", () => {
    const before = state([
      host({
        id: "a",
        hostId: "H1",
        endpoints: [
          "ws://[fd12:3456::10]:6790",
          "ws://[fe80::10%en0]:6790",
          "wss://relay.example.com/connect/x",
        ],
      }),
    ]);
    const identity: HostIdentity = {
      hostId: "H1",
      lanEndpoints: ["ws://[fd12:9999::20]:6790"],
    };

    const after = mergeHostIdentity(before, "a", identity, "ws://[fd12:9999::20]:6790");

    expect(after.hosts[0].endpoints).not.toContain("ws://[fd12:3456::10]:6790");
    expect(after.hosts[0].endpoints).not.toContain("ws://[fe80::10%en0]:6790");
    expect(after.hosts[0].endpoints).toContain("wss://relay.example.com/connect/x");
  });

  it("把本轮实际连上的地址排到候选列表最前", () => {
    const before = state([
      host({ id: "a", hostId: "H1", endpoints: ["ws://192.168.1.10:6790"] }),
    ]);
    const identity: HostIdentity = {
      hostId: "H1",
      lanEndpoints: ["ws://10.10.20.5:6790", "ws://10.10.20.2:6790"],
    };

    const after = mergeHostIdentity(before, "a", identity, "ws://10.10.20.2:6790");

    expect(after.hosts[0].endpoints[0]).toBe("ws://10.10.20.2:6790");
  });

  it("桌面未返回 LAN 地址时保留已保存端点,不清空", () => {
    const before = state([
      host({ id: "a", hostId: "H1", endpoints: ["ws://192.168.1.10:6790"] }),
    ]);

    const after = mergeHostIdentity(before, "a", { hostId: "H1" });

    expect(after.hosts[0].endpoints).toEqual(["ws://192.168.1.10:6790"]);
  });

  it("找不到目标记录时原样返回", () => {
    const before = state([host({ id: "a" })]);
    expect(mergeHostIdentity(before, "missing", { hostId: "H1" })).toBe(before);
  });
});
