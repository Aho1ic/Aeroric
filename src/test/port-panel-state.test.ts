import { describe, expect, it } from "vitest";
import {
  effectivePortFilterMode,
  extractRunPreviewCandidates,
  filterListeningPortsByProjectContext,
  findRunPreviewPort,
  formatListeningPortAddress,
  hasKnownProjectContext,
  listeningPortRank,
  resolvePreviewUrl,
  sortListeningPorts,
} from "../components/preview/portPanelState";
import type { ListeningPort } from "../types";

function port(overrides: Partial<ListeningPort>): ListeningPort {
  return {
    port: 9000,
    address: "127.0.0.1",
    protocol: "tcp",
    pid: 1,
    processName: "service",
    url: "http://127.0.0.1:9000",
    projectContext: "unknown",
    ...overrides,
  };
}

describe("port panel state", () => {
  it("prioritizes common dev server ports and runtime processes", () => {
    expect(listeningPortRank(port({ port: 5173, processName: "node" }))).toBe(0);
    expect(listeningPortRank(port({ port: 9999, processName: "node" }))).toBe(1);
    expect(listeningPortRank(port({ port: 9999, processName: "database" }))).toBe(2);
  });

  it("sorts listening ports by dev relevance then port number", () => {
    expect(
      sortListeningPorts([
        port({ port: 9000, processName: "service" }),
        port({ port: 5173, processName: "node" }),
        port({ port: 3000, processName: "node" }),
        port({ port: 7001, processName: "node" }),
      ]).map((item) => item.port),
    ).toEqual([3000, 5173, 7001, 9000]);
  });

  it("sorts project ports before unrelated system ports", () => {
    expect(
      sortListeningPorts([
        port({ port: 5173, processName: "node", projectContext: "other" }),
        port({ port: 9000, processName: "service", projectContext: "project" }),
      ]).map((item) => item.port),
    ).toEqual([9000, 5173]);
  });

  it("formats host and port compactly", () => {
    expect(formatListeningPortAddress(port({ address: "localhost", port: 1420 }))).toBe(
      "localhost:1420",
    );
  });

  it("keeps the selected preview URL while it is still available", () => {
    const ports = [
      port({ port: 5173, url: "http://localhost:5173" }),
      port({ port: 3000, url: "http://localhost:3000" }),
    ];

    expect(resolvePreviewUrl(ports, "http://localhost:3000")).toBe("http://localhost:3000");
  });

  it("falls back to the most relevant port when the preview URL disappears", () => {
    const ports = [
      port({ port: 9000, processName: "service", url: "http://localhost:9000" }),
      port({ port: 5173, processName: "node", url: "http://localhost:5173" }),
    ];

    expect(resolvePreviewUrl(ports, "http://localhost:3000")).toBe("http://localhost:5173");
    expect(resolvePreviewUrl([], "http://localhost:3000")).toBeNull();
  });

  it("filters to project-related ports when process context is known", () => {
    const ports = [
      port({ port: 5432, processName: "postgres", projectContext: "other" }),
      port({ port: 5173, processName: "node", projectContext: "project" }),
      port({ port: 9000, processName: "service", projectContext: "unknown" }),
    ];

    expect(hasKnownProjectContext(ports)).toBe(true);
    expect(filterListeningPortsByProjectContext(ports, "project").map((item) => item.port)).toEqual([
      5173,
    ]);
    expect(filterListeningPortsByProjectContext(ports, "all").map((item) => item.port)).toEqual([
      5173, 9000, 5432,
    ]);
  });

  it("falls back to all ports when project context is unavailable", () => {
    const ports = [
      port({ port: 9000, processName: "service", projectContext: "unknown" }),
      port({ port: 5173, processName: "node", projectContext: "unknown" }),
    ];

    expect(hasKnownProjectContext(ports)).toBe(false);
    expect(effectivePortFilterMode(ports, "project")).toBe("all");
    expect(filterListeningPortsByProjectContext(ports, "project").map((item) => item.port)).toEqual([
      5173, 9000,
    ]);
  });

  it("extracts preview ports from run output and command text", () => {
    expect(
      extractRunPreviewCandidates({
        command: "pnpm dev -- --port 3000",
        output: "Local: http://localhost:5173/\nNetwork: http://192.168.1.2:5173/",
      }),
    ).toEqual([5173, 3000]);

    expect(
      extractRunPreviewCandidates({
        command: "PORT=8787 npm run dev",
        output: "",
      }),
    ).toEqual([8787]);
  });

  it("finds the listening port that matches a run preview candidate", () => {
    const ports = [
      port({ port: 5173, processName: "node", projectContext: "other" }),
      port({ port: 5173, processName: "node", projectContext: "project", pid: 2 }),
      port({ port: 3000, processName: "node", projectContext: "project", pid: 3 }),
    ];

    expect(
      findRunPreviewPort(ports, {
        command: "pnpm dev",
        output: "ready on http://localhost:5173",
      }),
    ).toEqual(ports[1]);
  });
});
