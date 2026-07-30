import { describe, expect, it, vi } from "vitest";

vi.mock("expo-secure-store", () => ({
  getItemAsync: vi.fn(),
  setItemAsync: vi.fn(),
}));

import type { PairedHost } from "../types";
import { addOrReplaceHost, isSameHost, type HostStoreState } from "./host-store";

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
