import type { ProjectLocation } from "../../types";

export function shouldShowRemoteSshTerminal(
  projectLocation: ProjectLocation,
  hasRemoteConnection: boolean,
): boolean {
  return projectLocation.kind === "ssh" && hasRemoteConnection;
}
