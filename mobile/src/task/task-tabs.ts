import type { RpcCapability } from "../transport/rpc-codec";

export type TaskTabKey = "session" | "terminal" | "files" | "changes";

const OPTIONAL_TABS: readonly [Exclude<TaskTabKey, "session">, RpcCapability][] = [
  ["terminal", "terminal.stream"],
  ["files", "files.read"],
  ["changes", "git.read"],
];

export function availableTaskTabKeys(
  capabilitiesReady: boolean,
  hasCapability: (capability: RpcCapability) => boolean,
): readonly TaskTabKey[] {
  if (!capabilitiesReady) return ["session", "terminal", "files", "changes"];
  return [
    "session",
    ...OPTIONAL_TABS.filter(([, capability]) => hasCapability(capability)).map(([tab]) => tab),
  ];
}
