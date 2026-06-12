import { describe, expect, it } from "vitest";
import type { Project } from "../types";
import { isRemoteProject, resolveProjectLocation, sshProjectPath } from "../types";

describe("resolveProjectLocation", () => {
  it("treats legacy projects without location as local paths", () => {
    const project: Project = {
      id: "p1",
      name: "legacy",
      path: "/Users/me/work/legacy",
      lastOpenedAt: 1700000000000,
    };

    expect(resolveProjectLocation(project)).toEqual({
      kind: "local",
      path: "/Users/me/work/legacy",
    });
  });

  it("returns explicit SSH project locations unchanged", () => {
    const project: Project = {
      id: "p2",
      name: "remote",
      path: "ssh://conn-1/srv/app",
      location: { kind: "ssh", connectionId: "conn-1", remotePath: "/srv/app" },
      lastOpenedAt: 1700000000000,
    };

    expect(resolveProjectLocation(project)).toEqual({
      kind: "ssh",
      connectionId: "conn-1",
      remotePath: "/srv/app",
    });
  });

  it("formats SSH project paths for stable persistence and display", () => {
    expect(sshProjectPath("conn-1", "/srv/app")).toBe("ssh://conn-1/srv/app");
    expect(sshProjectPath("conn-1", "srv/app")).toBe("ssh://conn-1/srv/app");
  });

  it("detects remote projects without treating legacy local paths as remote", () => {
    const remoteProject: Project = {
      id: "p2",
      name: "remote",
      path: "ssh://conn-1/srv/app",
      location: { kind: "ssh", connectionId: "conn-1", remotePath: "/srv/app" },
      lastOpenedAt: 1700000000000,
    };
    const legacyLocal: Project = {
      id: "p1",
      name: "legacy",
      path: "/Users/me/work/legacy",
      lastOpenedAt: 1700000000000,
    };

    expect(isRemoteProject(remoteProject)).toBe(true);
    expect(isRemoteProject(legacyLocal)).toBe(false);
  });
});
