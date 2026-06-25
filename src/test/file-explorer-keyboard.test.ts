import { describe, expect, it } from "vitest";
import {
  fileExplorerClickAction,
  fileExplorerKeyAction,
  fileExplorerPreviewEndpoint,
  pasteTargetDirectory,
} from "../components/file-explorer/keyboard";

describe("file explorer keyboard helpers", () => {
  it("maps Command+Option+C to copy path", () => {
    expect(fileExplorerKeyAction({ key: "c", metaKey: true, altKey: true })).toBe("copyPath");
  });

  it("maps macOS Command+Option+C even when key is a special character", () => {
    expect(fileExplorerKeyAction({ key: "ç", code: "KeyC", metaKey: true, altKey: true })).toBe(
      "copyPath",
    );
  });

  it("maps Enter to rename", () => {
    expect(fileExplorerKeyAction({ key: "Enter" })).toBe("rename");
  });

  it("maps Command+V to paste", () => {
    expect(fileExplorerKeyAction({ key: "v", metaKey: true })).toBe("paste");
  });

  it("maps Command+Delete to delete", () => {
    expect(fileExplorerKeyAction({ key: "Backspace", metaKey: true })).toBe("delete");
    expect(fileExplorerKeyAction({ key: "Delete", metaKey: true })).toBe("delete");
  });

  it("pastes into selected directory or selected file parent", () => {
    expect(
      pasteTargetDirectory({ selectedPath: "/repo/src", selectedIsDir: true, rootPath: "/repo" }),
    ).toBe("/repo/src");
    expect(
      pasteTargetDirectory({
        selectedPath: "/repo/src/App.tsx",
        selectedIsDir: false,
        rootPath: "/repo",
      }),
    ).toBe("/repo/src");
    expect(
      pasteTargetDirectory({ selectedPath: null, selectedIsDir: false, rootPath: "/repo" }),
    ).toBe("/repo");
  });

  it("selects and toggles a folder on first click", () => {
    expect(fileExplorerClickAction({ isDir: true, isSelected: false })).toBe("selectAndToggle");
    expect(fileExplorerClickAction({ isDir: true, isSelected: true })).toBe("toggle");
    expect(fileExplorerClickAction({ isDir: false, isSelected: true })).toBe("select");
  });

  it("maps Space to preview without stealing modifier copy shortcuts", () => {
    expect(fileExplorerKeyAction({ key: " ", code: "Space" })).toBe("preview");
    expect(fileExplorerKeyAction({ key: "c", metaKey: true, altKey: true })).toBe("copyPath");
  });

  it("builds local and SSH preview endpoints for the selected path", () => {
    expect(fileExplorerPreviewEndpoint({ selectedPath: "/repo/src", remote: undefined })).toEqual({
      kind: "local",
      path: "/repo/src",
    });
    expect(
      fileExplorerPreviewEndpoint({
        selectedPath: "/srv/app/src",
        remote: {
          connection: {
            id: "prod",
            name: "prod",
            host: "example.com",
            port: 22,
            username: "root",
            createdAt: 1,
          },
          projectPath: "/srv/app",
        },
      }),
    ).toEqual({
      kind: "ssh",
      connection: {
        id: "prod",
        name: "prod",
        host: "example.com",
        port: 22,
        username: "root",
        createdAt: 1,
      },
      path: "/srv/app/src",
    });
  });
});
