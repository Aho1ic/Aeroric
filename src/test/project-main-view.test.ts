import { describe, expect, it } from "vitest";
import type { ProjectLocation } from "../types";
import { shouldShowRemoteSshTerminal } from "../components/project-page/viewMode";

describe("project main view mode", () => {
  it("shows the SSH terminal in the center for connected SSH projects", () => {
    const location: ProjectLocation = {
      kind: "ssh",
      connectionId: "conn-1",
      remotePath: "/srv/app",
    };

    expect(shouldShowRemoteSshTerminal(location, true)).toBe(true);
  });

  it("does not show the SSH terminal for SSH projects without a resolved connection", () => {
    const location: ProjectLocation = {
      kind: "ssh",
      connectionId: "missing",
      remotePath: "/srv/app",
    };

    expect(shouldShowRemoteSshTerminal(location, false)).toBe(false);
  });

  it("does not replace the main view for local projects", () => {
    expect(shouldShowRemoteSshTerminal({ kind: "local", path: "/tmp/app" }, true)).toBe(false);
  });
});
