import { describe, expect, it } from "vitest";

import {
  candidateLabel,
  isBenchmarkAddress,
  isPrivateAddress,
  isVirtualInterface,
  rankCandidates,
  shouldPromptForLanAddress,
} from "./lan-address.mjs";

const interfaces = {
  lo0: [{ address: "127.0.0.1", family: "IPv4", internal: true }],
  en0: [
    { address: "192.168.0.121", family: "IPv4", internal: false },
    { address: "fe80::1", family: "IPv6", internal: false },
  ],
  utun4: [{ address: "100.125.106.127", family: "IPv4", internal: false }],
  utun5: [{ address: "10.0.0.2", family: "IPv4", internal: false }],
  utun1024: [{ address: "198.18.0.1", family: "IPv4", internal: false }],
};

describe("address classification", () => {
  it("detects Clash/Surge fake-IP range", () => {
    expect(isBenchmarkAddress("198.18.0.1")).toBe(true);
    expect(isBenchmarkAddress("198.19.255.1")).toBe(true);
    expect(isBenchmarkAddress("198.20.0.1")).toBe(false);
    expect(isBenchmarkAddress("192.168.0.121")).toBe(false);
  });

  it("detects private and CGNAT ranges", () => {
    expect(isPrivateAddress("192.168.0.121")).toBe(true);
    expect(isPrivateAddress("10.0.0.2")).toBe(true);
    expect(isPrivateAddress("172.16.5.4")).toBe(true);
    expect(isPrivateAddress("172.32.5.4")).toBe(false);
    expect(isPrivateAddress("100.125.106.127")).toBe(true);
    expect(isPrivateAddress("8.8.8.8")).toBe(false);
  });

  it("detects virtual interfaces", () => {
    expect(isVirtualInterface("utun1024")).toBe(true);
    expect(isVirtualInterface("bridge100")).toBe(true);
    expect(isVirtualInterface("en0")).toBe(false);
    expect(isVirtualInterface("eth0")).toBe(false);
  });
});

describe("rankCandidates", () => {
  it("prefers the physical LAN address and never the fake-IP one", () => {
    const ranked = rankCandidates(interfaces);
    expect(ranked[0]).toEqual({ name: "en0", address: "192.168.0.121" });
    expect(ranked.at(-1)).toEqual({ name: "utun1024", address: "198.18.0.1" });
  });

  it("skips internal and IPv6 entries and de-duplicates addresses", () => {
    const ranked = rankCandidates({
      ...interfaces,
      en1: [{ address: "192.168.0.121", family: 4, internal: false }],
    });
    expect(ranked.map((candidate) => candidate.address)).toEqual([
      "192.168.0.121",
      "100.125.106.127",
      "10.0.0.2",
      "198.18.0.1",
    ]);
  });

  it("returns an empty list when nothing is usable", () => {
    expect(
      rankCandidates({ lo0: [{ address: "127.0.0.1", family: "IPv4", internal: true }] }),
    ).toEqual([]);
    expect(rankCandidates(undefined)).toEqual([]);
  });
});

describe("candidateLabel", () => {
  it("warns about fake-IP addresses and marks the recommendation", () => {
    expect(candidateLabel({ name: "en0", address: "192.168.0.121" }, true)).toBe(" [推荐，局域网]");
    expect(candidateLabel({ name: "utun1024", address: "198.18.0.1" }, false)).toBe(
      " [代理 fake-IP，手机无法访问]",
    );
    expect(candidateLabel({ name: "utun4", address: "100.125.106.127" }, false)).toBe(
      " [VPN/虚拟接口]",
    );
  });
});

describe("shouldPromptForLanAddress", () => {
  it("prompts for LAN-mode start commands", () => {
    expect(shouldPromptForLanAddress(["start", "--lan"])).toBe(true);
    expect(shouldPromptForLanAddress(["start"])).toBe(true);
    expect(shouldPromptForLanAddress(["start", "--host", "lan"])).toBe(true);
    expect(shouldPromptForLanAddress(["start", "--host=lan"])).toBe(true);
    expect(shouldPromptForLanAddress(["start", "--offline"])).toBe(true);
    expect(shouldPromptForLanAddress(["start", "--android", "--clear"])).toBe(true);
  });

  it("skips modes that do not depend on a LAN address", () => {
    expect(shouldPromptForLanAddress(["start", "--tunnel"])).toBe(false);
    expect(shouldPromptForLanAddress(["start", "--localhost"])).toBe(false);
    expect(shouldPromptForLanAddress(["start", "--host", "tunnel"])).toBe(false);
    expect(shouldPromptForLanAddress(["start", "-m", "localhost"])).toBe(false);
    expect(shouldPromptForLanAddress(["start", "--web"])).toBe(false);
    expect(shouldPromptForLanAddress(["start", "-w"])).toBe(false);
    expect(shouldPromptForLanAddress(["start", "--lan", "--help"])).toBe(false);
  });

  it("skips non-start commands", () => {
    expect(shouldPromptForLanAddress(["install", "expo-camera"])).toBe(false);
    expect(shouldPromptForLanAddress(["run:ios"])).toBe(false);
    expect(shouldPromptForLanAddress([])).toBe(false);
  });
});
