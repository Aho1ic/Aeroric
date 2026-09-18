import { describe, expect, it } from "vitest";
import { fileArgs, projectArgs, requireCommand, resolveCommand } from "../lib/invokeFacade";
import { GIT_MIRRORS } from "../lib/api/git";
import { FS_MIRRORS } from "../lib/api/fs";
import type { InvokeTarget } from "../lib/target";

const local: InvokeTarget = { kind: "local", path: "/repo" };
const sshConnection = { id: "c1", host: "h", port: 22, username: "u" };
const ssh: InvokeTarget = {
  kind: "ssh",
  connection: sshConnection,
  projectPath: "/srv/repo",
};
const wsl: InvokeTarget = {
  kind: "wsl",
  distribution: "Ubuntu",
  projectPath: "/home/u/repo",
};

describe("resolveCommand", () => {
  it("maps git_status across local/ssh/wsl", () => {
    const mirror = GIT_MIRRORS.status;
    expect(resolveCommand(mirror, local)).toBe("git_status");
    expect(resolveCommand(mirror, ssh)).toBe("remote_git_status");
    expect(resolveCommand(mirror, wsl)).toBe("wsl_git_status");
  });

  it("unifies local changes onto git_status while ssh/wsl use merged changes", () => {
    const mirror = GIT_MIRRORS.changes;
    expect(resolveCommand(mirror, local)).toBe("git_status");
    expect(resolveCommand(mirror, ssh)).toBe("remote_git_changes");
    expect(resolveCommand(mirror, wsl)).toBe("wsl_git_changes");
  });

  it("maps fs read_file_content across targets", () => {
    const mirror = FS_MIRRORS.readContent;
    expect(resolveCommand(mirror, local)).toBe("read_file_content");
    expect(resolveCommand(mirror, ssh)).toBe("remote_read_file_content");
    expect(resolveCommand(mirror, wsl)).toBe("wsl_read_file_content");
  });
});

describe("requireCommand", () => {
  it("throws when a target has no mirror", () => {
    expect(() =>
      requireCommand({ local: "only_local", ssh: undefined, wsl: undefined }, ssh),
    ).toThrow(/no ssh mirror/);
  });

  it("allows local always", () => {
    expect(requireCommand({ local: "only_local" }, local)).toBe("only_local");
  });
});

describe("projectArgs / fileArgs", () => {
  it("builds local project args", () => {
    expect(projectArgs(local)).toEqual({ projectPath: "/repo" });
  });

  it("builds ssh project args", () => {
    expect(projectArgs(ssh)).toEqual({
      connection: sshConnection,
      remoteProjectPath: "/srv/repo",
    });
  });

  it("builds wsl file args", () => {
    expect(fileArgs(wsl, "src/a.ts")).toEqual({
      distribution: "Ubuntu",
      linuxPath: "src/a.ts",
      linuxProjectPath: "/home/u/repo",
    });
  });
});
