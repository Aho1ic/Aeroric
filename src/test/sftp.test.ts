import { describe, expect, it } from "vitest";
import {
  formatSftpFolderCounts,
  formatSftpPreviewModifiedTime,
  formatSftpPreviewSize,
} from "../components/sftp/SftpPreview";
import {
  defaultSftpPathForEndpoint,
  flattenSftpTreeEntries,
  filterSftpTreeEntriesByName,
  formatSftpTransferPercent,
  groupSftpSshConnections,
  normalizeSftpSortPreference,
  pruneExpandedPathsForFolderSelection,
  sftpBreadcrumbSegments,
  sftpClickAction,
  sftpDropOperation,
  sftpEndpointKey,
  sftpFileIconKind,
  sftpProgressRingBackground,
  sftpKeyAction,
  sortSftpEntries,
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

  it("groups SSH connections by their configured group with an explicit default group", () => {
    const grouped = groupSftpSshConnections(
      [
        {
          id: "prod-1",
          name: "Prod App",
          group: "Production",
          host: "prod.example.com",
          port: 22,
          username: "deploy",
          createdAt: 3,
        },
        {
          id: "ungrouped",
          name: "Scratch",
          group: "  ",
          host: "scratch.example.com",
          port: 22,
          username: "me",
          createdAt: 1,
        },
        {
          id: "stage-1",
          name: "Staging App",
          group: "Staging",
          host: "stage.example.com",
          port: 22,
          username: "deploy",
          createdAt: 2,
        },
        {
          id: "prod-2",
          name: "Prod Worker",
          group: "Production",
          host: "worker.example.com",
          port: 22,
          username: "deploy",
          createdAt: 4,
        },
      ],
      "Default",
    );

    expect(grouped.map((group) => [group.label, group.connections.map((item) => item.name)])).toEqual(
      [
        ["Production", ["Prod App", "Prod Worker"]],
        ["Default", ["Scratch"]],
        ["Staging", ["Staging App"]],
      ],
    );
  });

  it("formats transfer percentages and ring progress for known totals", () => {
    expect(formatSftpTransferPercent(0, 4)).toBe(0);
    expect(formatSftpTransferPercent(1, 4)).toBe(25);
    expect(formatSftpTransferPercent(3, 4)).toBe(75);
    expect(formatSftpTransferPercent(4, 4)).toBe(100);
    expect(formatSftpTransferPercent(4, 0)).toBe(100);

    expect(sftpProgressRingBackground(25)).toBe(
      "conic-gradient(var(--accent) 90deg, var(--border-dim) 90deg)",
    );
    expect(sftpProgressRingBackground(100, "var(--danger)")).toBe(
      "conic-gradient(var(--danger) 360deg, var(--border-dim) 360deg)",
    );
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
    const flattened = flattenSftpTreeEntries(
      entries,
      new Set(["/repo/src", "/repo/src/components"]),
      children,
    );

    expect(flattened.map((item) => [item.entry.path, item.depth])).toEqual([
      ["/repo/src", 0],
      ["/repo/src/components", 1],
      ["/repo/src/components/App.tsx", 2],
      ["/repo/src/main.ts", 1],
      ["/repo/README.md", 0],
    ]);
  });

  it("keeps folders above files when sorting SFTP entries by name or modification time", () => {
    const entries: SftpEntry[] = [
      { name: "z.py", path: "/repo/z.py", isDir: false, extension: "py", modifiedAtMs: 300 },
      { name: "src", path: "/repo/src", isDir: true, modifiedAtMs: 100 },
      {
        name: "README.md",
        path: "/repo/README.md",
        isDir: false,
        extension: "md",
        modifiedAtMs: 500,
      },
      { name: "docs", path: "/repo/docs", isDir: true, modifiedAtMs: 900 },
    ];

    expect(sortSftpEntries(entries, "name", "asc").map((entry) => entry.name)).toEqual([
      "docs",
      "src",
      "README.md",
      "z.py",
    ]);
    expect(sortSftpEntries(entries, "modified", "desc").map((entry) => entry.name)).toEqual([
      "docs",
      "src",
      "README.md",
      "z.py",
    ]);
  });

  it("filters SFTP entries and keeps matching descendants visible under folders", () => {
    const entries: SftpEntry[] = [
      { name: "src", path: "/repo/src", isDir: true },
      { name: "README.md", path: "/repo/README.md", isDir: false },
    ];
    const children = new Map<string, SftpEntry[]>([
      [
        "/repo/src",
        [
          { name: "components", path: "/repo/src/components", isDir: true },
          { name: "main.py", path: "/repo/src/main.py", isDir: false },
        ],
      ],
      [
        "/repo/src/components",
        [{ name: "Button.tsx", path: "/repo/src/components/Button.tsx", isDir: false }],
      ],
    ]);

    const filtered = filterSftpTreeEntriesByName(entries, children, "button");

    expect(filtered.entries.map((entry) => entry.path)).toEqual(["/repo/src"]);
    expect(filtered.childrenByPath.get("/repo/src")?.map((entry) => entry.path)).toEqual([
      "/repo/src/components",
    ]);
    expect(filtered.childrenByPath.get("/repo/src/components")?.map((entry) => entry.path)).toEqual(
      ["/repo/src/components/Button.tsx"],
    );
  });

  it("normalizes SFTP sort preferences with modification descending fallback", () => {
    expect(normalizeSftpSortPreference({ field: "name", direction: "asc" })).toEqual({
      field: "name",
      direction: "asc",
    });
    expect(normalizeSftpSortPreference({ field: "other", direction: "up" })).toEqual({
      field: "modified",
      direction: "desc",
    });
  });

  it("uses specific icon kinds for database, model, video, and wheel files", () => {
    expect(sftpFileIconKind({ name: "index.db", path: "/repo/index.db", isDir: false })).toBe(
      "database",
    );
    expect(sftpFileIconKind({ name: "model.pt", path: "/repo/model.pt", isDir: false })).toBe(
      "model",
    );
    expect(sftpFileIconKind({ name: "model.pth", path: "/repo/model.pth", isDir: false })).toBe(
      "model",
    );
    expect(
      sftpFileIconKind({ name: "detector.onnx", path: "/repo/detector.onnx", isDir: false }),
    ).toBe("model");
    expect(sftpFileIconKind({ name: "clip.mp4", path: "/repo/clip.mp4", isDir: false })).toBe(
      "video",
    );
    expect(sftpFileIconKind({ name: "pkg.whl", path: "/repo/pkg.whl", isDir: false })).toBe(
      "package",
    );
  });

  it("collapses sibling expanded folders when selecting another folder but keeps the selected subtree", () => {
    const expanded = pruneExpandedPathsForFolderSelection(
      new Set(["/repo/src", "/repo/src/components", "/repo/docs", "/repo/docs/api"]),
      "/repo/src/components",
    );

    expect([...expanded].sort()).toEqual(["/repo/src", "/repo/src/components"]);
  });

  it("keeps descendants of the selected folder expanded", () => {
    const expanded = pruneExpandedPathsForFolderSelection(
      new Set(["/repo/src", "/repo/src/components", "/repo/src/components/ui", "/repo/docs"]),
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
    expect(sftpFileIconKind({ name: "photo.png", path: "/repo/photo.png", isDir: false })).toBe(
      "image",
    );
    expect(sftpFileIconKind({ name: "README.md", path: "/repo/README.md", isDir: false })).toBe(
      "markdown",
    );
    expect(
      sftpFileIconKind({ name: "package.json", path: "/repo/package.json", isDir: false }),
    ).toBe("json");
    expect(sftpFileIconKind({ name: "archive.zip", path: "/repo/archive.zip", isDir: false })).toBe(
      "archive",
    );
    expect(sftpFileIconKind({ name: "main.ts", path: "/repo/main.ts", isDir: false })).toBe("code");
    expect(sftpFileIconKind({ name: "notes.txt", path: "/repo/notes.txt", isDir: false })).toBe(
      "text",
    );
  });

  it("formats Finder-style folder preview metadata", () => {
    expect(formatSftpPreviewSize(1536)).toBe("1.5 KB");
    expect(formatSftpFolderCounts({ directoryCount: 2, fileCount: 3 })).toBe(
      "2 个文件夹，3 个文件",
    );
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
