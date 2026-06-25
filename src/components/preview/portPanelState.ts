import type { ListeningPort } from "../../types";

const DEV_PORTS = new Set([1420, 3000, 4173, 5173, 5174, 8000, 8080, 8787]);

export type PortFilterMode = "project" | "all";

export interface RunPreviewSource {
  command: string;
  output: string;
}

function listeningPortContextRank(port: ListeningPort): number {
  if (port.projectContext === "project") return 0;
  if (port.projectContext === "unknown") return 1;
  return 2;
}

export function listeningPortRank(port: ListeningPort): number {
  const process = port.processName.toLowerCase();
  if (DEV_PORTS.has(port.port)) return 0;
  if (process.includes("node") || process.includes("vite") || process.includes("bun")) return 1;
  return 2;
}

export function sortListeningPorts(ports: ListeningPort[]): ListeningPort[] {
  return [...ports].sort((a, b) => {
    const contextRank = listeningPortContextRank(a) - listeningPortContextRank(b);
    if (contextRank !== 0) return contextRank;
    const rank = listeningPortRank(a) - listeningPortRank(b);
    if (rank !== 0) return rank;
    if (a.port !== b.port) return a.port - b.port;
    return a.processName.localeCompare(b.processName) || a.pid - b.pid;
  });
}

export function resolvePreviewUrl(
  ports: ListeningPort[],
  currentUrl: string | null,
): string | null {
  if (currentUrl && ports.some((port) => port.url === currentUrl)) {
    return currentUrl;
  }
  return sortListeningPorts(ports)[0]?.url ?? null;
}

export function hasKnownProjectContext(ports: ListeningPort[]): boolean {
  return ports.some((port) => port.projectContext !== "unknown");
}

export function effectivePortFilterMode(
  ports: ListeningPort[],
  requestedMode: PortFilterMode,
): PortFilterMode {
  if (requestedMode === "project" && !hasKnownProjectContext(ports)) {
    return "all";
  }
  return requestedMode;
}

export function filterListeningPortsByProjectContext(
  ports: ListeningPort[],
  requestedMode: PortFilterMode,
): ListeningPort[] {
  const sorted = sortListeningPorts(ports);
  if (effectivePortFilterMode(sorted, requestedMode) === "all") {
    return sorted;
  }
  return sorted.filter((port) => port.projectContext === "project");
}

function normalizeCandidatePort(value: string): number | null {
  const port = Number.parseInt(value, 10);
  if (!Number.isInteger(port) || port < 1 || port > 65535) return null;
  return port;
}

function appendCandidate(candidates: number[], seen: Set<number>, value: string) {
  const port = normalizeCandidatePort(value);
  if (port === null || seen.has(port)) return;
  seen.add(port);
  candidates.push(port);
}

export function extractRunPreviewCandidates(source: RunPreviewSource | null | undefined): number[] {
  if (!source) return [];
  const candidates: number[] = [];
  const seen = new Set<number>();

  const output = source.output ?? "";
  const command = source.command ?? "";
  const localUrlPattern =
    /\bhttps?:\/\/(?:localhost|127\.0\.0\.1|0\.0\.0\.0|\[::1\])(?::(\d{1,5}))?\b/gi;
  let match: RegExpExecArray | null;
  while ((match = localUrlPattern.exec(output)) !== null) {
    if (match[1]) appendCandidate(candidates, seen, match[1]);
  }

  const bareLocalPattern = /\b(?:localhost|127\.0\.0\.1|0\.0\.0\.0|\[::1\]):(\d{1,5})\b/gi;
  while ((match = bareLocalPattern.exec(output)) !== null) {
    appendCandidate(candidates, seen, match[1]);
  }

  const commandPortPattern = /(?:^|\s)--port(?:=|\s+)(\d{1,5})(?=\s|$)/gi;
  while ((match = commandPortPattern.exec(command)) !== null) {
    appendCandidate(candidates, seen, match[1]);
  }

  const envPortPattern = /(?:^|\s)(?:PORT|VITE_PORT|NEXT_PORT)=(\d{1,5})(?=\s|$)/g;
  while ((match = envPortPattern.exec(command)) !== null) {
    appendCandidate(candidates, seen, match[1]);
  }

  return candidates;
}

export function findRunPreviewPort(
  ports: ListeningPort[],
  source: RunPreviewSource | null | undefined,
): ListeningPort | null {
  for (const candidate of extractRunPreviewCandidates(source)) {
    const matches = sortListeningPorts(ports.filter((port) => port.port === candidate));
    if (matches.length > 0) return matches[0];
  }
  return null;
}

export function formatListeningPortAddress(port: ListeningPort): string {
  return `${port.address}:${port.port}`;
}
