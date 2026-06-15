import { describe, expect, it } from "vitest";
import type { SshConnection } from "../types";
import {
  deriveRemoteProjectName,
  sshProjectInputForConnection,
} from "../components/ssh/SshProjectDialog";

function connection(remotePath?: string): SshConnection {
  return {
    id: "conn-1",
    name: "Prod",
    host: "example.com",
    port: 22,
    username: "deploy",
    remotePath,
    createdAt: 1,
  };
}

describe("SSH project opening", () => {
  it("derives a remote project name from the final path segment", () => {
    expect(deriveRemoteProjectName("/srv/apps/aeroric/", "Prod")).toBe("aeroric");
    expect(deriveRemoteProjectName("   ", "Prod")).toBe("Prod");
  });

  it("builds the open-project input used by the SSH card double click", () => {
    expect(sshProjectInputForConnection(connection("/srv/apps/aeroric"))).toEqual({
      connectionId: "conn-1",
      remotePath: "/srv/apps/aeroric",
      name: "aeroric",
    });
    expect(sshProjectInputForConnection(connection())).toBeNull();
  });
});
