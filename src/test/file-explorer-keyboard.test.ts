import { describe, expect, it } from "vitest";
import {
  fileExplorerClickAction,
  fileExplorerKeyAction,
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
    expect(pasteTargetDirectory({ selectedPath: "/repo/src", selectedIsDir: true, rootPath: "/repo" })).toBe(
      "/repo/src",
    );
    expect(
      pasteTargetDirectory({
        selectedPath: "/repo/src/App.tsx",
        selectedIsDir: false,
        rootPath: "/repo",
      }),
    ).toBe("/repo/src");
    expect(pasteTargetDirectory({ selectedPath: null, selectedIsDir: false, rootPath: "/repo" })).toBe(
      "/repo",
    );
  });

  it("selects a folder on first click and toggles it when already selected", () => {
    expect(fileExplorerClickAction({ isDir: true, isSelected: false })).toBe("select");
    expect(fileExplorerClickAction({ isDir: true, isSelected: true })).toBe("toggle");
    expect(fileExplorerClickAction({ isDir: false, isSelected: true })).toBe("select");
  });
});
