import { describe, expect, it } from "vitest";
import {
  formatSftpFolderCounts,
  formatSftpPreviewModifiedTime,
  formatSftpPreviewSize,
} from "../components/sftp/SftpPreview";
import {
  defaultSftpPathForEndpoint,
  flattenSftpTreeEntries,
  pruneExpandedPathsForFolderSelection,
  sftpBreadcrumbSegments,
  sftpClickAction,
  sftpDropOperation,
  sftpEndpointKey,
  sftpFileIconKind,
  sftpKeyAction,
  shouldPromptForSftpConflict,
  shouldPromptForUnknownSftpConflict,
  type SftpEndpoint,
  type SftpEntry,
} from "../components/sftp/sftpTypes";

describe("sftp panel helpers", () => {
  it("uses stable keys for local and ssh endpoints", () => {
    expect(sftpEndpointKey({ kind: "local", path: "/Users/me" })).toBe("local:/Users/me");
    expect(
      sftpEndpointKey({
        kind: "ssh",
        connectionId: "conn-1",
        connectionName: "Prod",
        path: "/srv/app",
      }),
    ).toBe("ssh:conn-1:/srv/app");
  });

  it("uses move for mouse drag operations even across endpoints", () => {
    const left: SftpEndpoint = { kind: "local", path: "/Users/me" };
    const right: SftpEndpoint = {
      kind: "ssh",
      connectionId: "conn-1",
      connectionName: "Prod",
      path: "/srv/app",
    };

    expect(sftpDropOperation(left, { kind: "local", path: "/Users/me/Desktop" })).toBe("move");
    expect(sftpDropOperation(left, right)).toBe("move");
  });

  it("uses ssh connection remote path as default when available", () => {
    expect(defaultSftpPathForEndpoint("local", undefined, "/repo")).toBe("/repo");
    expect(
      defaultSftpPathForEndpoint(
        "ssh",
        {
          id: "conn-1",
          name: "Prod",
          host: "example.com",
          port: 22,
          username: "root",
          remotePath: "/srv/app",
          createdAt: 1,
        },
        "/repo",
      ),
    ).toBe("/srv/app");
  });

  it("selects folders on first click and toggles them on the second selected click", () => {
    expect(sftpClickAction({ isDir: true, isSelected: false })).toBe("select");
    expect(sftpClickAction({ isDir: true, isSelected: true })).toBe("toggle");
    expect(sftpClickAction({ isDir: false, isSelected: false })).toBe("select");
    expect(sftpClickAction({ isDir: false, isSelected: true })).toBe("select");
  });

  it("maps macOS keyboard shortcuts and space preview for selected items", () => {
    expect(sftpKeyAction({ metaKey: true, key: "c", code: "KeyC" })).toBe("copy");
    expect(sftpKeyAction({ metaKey: true, altKey: true, key: "c", code: "KeyC" })).toBe("copyPath");
    expect(sftpKeyAction({ metaKey: true, key: "v", code: "KeyV" })).toBe("paste");
    expect(sftpKeyAction({ metaKey: true, key: "Backspace", code: "Backspace" })).toBe("delete");
    expect(sftpKeyAction({ key: " ", code: "Space" })).toBe("preview");
    expect(sftpKeyAction({ metaKey: false, key: "Backspace", code: "Backspace" })).toBe(null);
  });

  it("detects existing destination names before copy and move operations", () => {
    const entries: SftpEntry[] = [
      { name: "same.txt", path: "/target/same.txt", isDir: false },
      { name: "docs", path: "/target/docs", isDir: true },
    ];

    expect(shouldPromptForSftpConflict(["/source/same.txt"], entries)).toBe(true);
    expect(shouldPromptForSftpConflict(["/source/docs/"], entries)).toBe(true);
    expect(shouldPromptForSftpConflict(["/source/new.txt"], entries)).toBe(false);
  });

  it("prompts conservatively when dropping into an unloaded directory", () => {
    expect(shouldPromptForUnknownSftpConflict(["/source/same.txt"], undefined)).toBe(true);
    expect(shouldPromptForUnknownSftpConflict(["/source/same.txt"], [])).toBe(false);
  });

  it("creates clickable breadcrumb segments for absolute paths", () => {
    expect(sftpBreadcrumbSegments("/Users/example/Documents")).toEqual([
      { label: "Users", path: "/Users" },
      { label: "example", path: "/Users/example" },
      { label: "Documents", path: "/Users/example/Documents" },
    ]);
    expect(sftpBreadcrumbSegments("/")).toEqual([{ label: "/", path: "/" }]);
  });

  it("flattens expanded nested directory entries with depth", () => {
    const entries: SftpEntry[] = [
      { name: "src", path: "/repo/src", isDir: true },
      { name: "README.md", path: "/repo/README.md", isDir: false },
    ];
    const children = new Map<string, SftpEntry[]>([
      [
        "/repo/src",
        [
          { name: "components", path: "/repo/src/components", isDir: true },
          { name: "main.ts", path: "/repo/src/main.ts", isDir: false },
        ],
      ],
      [
        "/repo/src/components",
        [{ name: "App.tsx", path: "/repo/src/components/App.tsx", isDir: false }],
      ],
    ]);
    const flattened = flattenSftpTreeEntries(entries, new Set(["/repo/src", "/repo/src/components"]), children);

    expect(flattened.map((item) => [item.entry.path, item.depth])).toEqual([
      ["/repo/src", 0],
      ["/repo/src/components", 1],
      ["/repo/src/components/App.tsx", 2],
      ["/repo/src/main.ts", 1],
      ["/repo/README.md", 0],
    ]);
  });

  it("collapses sibling expanded folders when selecting another folder but keeps the selected subtree", () => {
    const expanded = pruneExpandedPathsForFolderSelection(
      new Set([
        "/repo/src",
        "/repo/src/components",
        "/repo/docs",
        "/repo/docs/api",
      ]),
      "/repo/src/components",
    );

    expect([...expanded].sort()).toEqual(["/repo/src", "/repo/src/components"]);
  });

  it("keeps descendants of the selected folder expanded", () => {
    const expanded = pruneExpandedPathsForFolderSelection(
      new Set([
        "/repo/src",
        "/repo/src/components",
        "/repo/src/components/ui",
        "/repo/docs",
      ]),
      "/repo/src",
    );

    expect([...expanded].sort()).toEqual([
      "/repo/src",
      "/repo/src/components",
      "/repo/src/components/ui",
    ]);
  });

  it("keeps the full expanded tree when selecting the root folder", () => {
    const expanded = pruneExpandedPathsForFolderSelection(
      new Set(["/", "/repo", "/repo/src", "/tmp/cache"]),
      "/",
    );

    expect([...expanded].sort()).toEqual(["/", "/repo", "/repo/src", "/tmp/cache"]);
  });

  it("classifies file icon kinds by extension", () => {
    expect(sftpFileIconKind({ name: "src", path: "/repo/src", isDir: true })).toBe("folder");
    expect(sftpFileIconKind({ name: "photo.png", path: "/repo/photo.png", isDir: false })).toBe("image");
    expect(sftpFileIconKind({ name: "README.md", path: "/repo/README.md", isDir: false })).toBe("markdown");
    expect(sftpFileIconKind({ name: "package.json", path: "/repo/package.json", isDir: false })).toBe("json");
    expect(sftpFileIconKind({ name: "archive.zip", path: "/repo/archive.zip", isDir: false })).toBe("archive");
    expect(sftpFileIconKind({ name: "main.ts", path: "/repo/main.ts", isDir: false })).toBe("code");
    expect(sftpFileIconKind({ name: "notes.txt", path: "/repo/notes.txt", isDir: false })).toBe("text");
  });

  it("formats Finder-style folder preview metadata", () => {
    expect(formatSftpPreviewSize(1536)).toBe("1.5 KB");
    expect(formatSftpFolderCounts({ directoryCount: 2, fileCount: 3 })).toBe("2 个文件夹，3 个文件");
    const timestamp = 1_709_568_000_000;
    const expected = new Intl.DateTimeFormat("zh-CN", {
      year: "numeric",
      month: "numeric",
      day: "numeric",
      hour: "2-digit",
      minute: "2-digit",
      hour12: false,
    }).format(new Date(timestamp));
    expect(formatSftpPreviewModifiedTime(timestamp)).toBe(expected);
  });
});
