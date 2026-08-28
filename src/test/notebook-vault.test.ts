import { beforeEach, describe, expect, it, vi } from "vitest";

/* 这一层的职责是把「磁盘上的 .md」翻译成面板认识的笔记,所以测试盯的是
 * 翻译的正确性:标题从哪来、editor 模式怎么传、冲突怎么上报、保存写回的
 * 字节对不对。后端命令全部 mock —— 后端行为由 Rust 侧的测试覆盖。 */

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invoke(...args),
}));

const { createNote, flattenTree, listNotes, loadNote, persistNote, persistOrder, removeNote } =
  await import("../components/notebook/notebookVault");
type VaultNote = Awaited<ReturnType<typeof loadNote>>;

function sig(mtimeMs: number, hash = "h") {
  return { mtimeMs, hash };
}

function entry(overrides: Record<string, unknown> = {}) {
  return {
    name: "note.md",
    path: "/v/note.md",
    isDir: false,
    size: 10,
    modifiedMs: 1000,
    children: null,
    truncated: false,
    ...overrides,
  };
}

beforeEach(() => {
  invoke.mockReset();
});

describe("flattenTree", () => {
  it("collects notes from nested directories and drops the directories", () => {
    const tree = [
      entry({ name: "a.md", path: "/v/a.md" }),
      entry({
        name: "sub",
        path: "/v/sub",
        isDir: true,
        children: [
          entry({ name: "b.md", path: "/v/sub/b.md" }),
          entry({
            name: "deep",
            path: "/v/sub/deep",
            isDir: true,
            children: [entry({ name: "c.md", path: "/v/sub/deep/c.md" })],
          }),
        ],
      }),
    ];

    expect(flattenTree(tree).map((note) => note.path)).toEqual([
      "/v/a.md",
      "/v/sub/b.md",
      "/v/sub/deep/c.md",
    ]);
  });

  it("tolerates a directory with null children", () => {
    const tree = [entry({ name: "empty", path: "/v/empty", isDir: true, children: null })];
    expect(flattenTree(tree)).toEqual([]);
  });
});

/** listNotes 会并发读树和排序,按调用的命令名分派返回值。 */
function mockList(entries: unknown[], order: string[] = []) {
  invoke.mockImplementation((command: string) => {
    if (command === "notebook_read_tree") return Promise.resolve(entries);
    if (command === "notebook_read_order") return Promise.resolve(order);
    throw new Error(`unexpected command: ${command}`);
  });
}

describe("listNotes", () => {
  it("sorts newest first and defers reading bodies", async () => {
    mockList([
      entry({ name: "old.md", path: "/v/old.md", modifiedMs: 100 }),
      entry({ name: "new.md", path: "/v/new.md", modifiedMs: 900 }),
    ]);

    const notes = await listNotes("/v");

    // 「最近写的在最上面」—— 与原 localStorage 版把新笔记 unshift 到队首一致。
    expect(notes.map((note) => note.path)).toEqual(["/v/new.md", "/v/old.md"]);
    // 列表阶段只拿元数据,正文按需读。
    expect(notes.every((note) => !note.loaded && note.body === "")).toBe(true);
    // 标题暂用文件名(去扩展名),读入后才换成 frontmatter 里的。
    expect(notes[0]?.title).toBe("new");
  });

  it("honours the manual order over modification time", async () => {
    mockList(
      [
        entry({ name: "a.md", path: "/v/a.md", modifiedMs: 900 }),
        entry({ name: "b.md", path: "/v/b.md", modifiedMs: 100 }),
      ],
      // 用户把 b 拖到了 a 前面 —— 手工排序要压过 mtime。
      ["b.md", "a.md"],
    );

    const notes = await listNotes("/v");

    expect(notes.map((note) => note.path)).toEqual(["/v/b.md", "/v/a.md"]);
  });

  it("puts unordered notes first so new ones stay visible", async () => {
    mockList(
      [
        entry({ name: "known.md", path: "/v/known.md", modifiedMs: 900 }),
        entry({ name: "fresh.md", path: "/v/fresh.md", modifiedMs: 100 }),
      ],
      ["known.md"],
    );

    const notes = await listNotes("/v");

    // fresh 没排过(刚建的 / 外部拖进来的),要排在最前面让用户看见。
    expect(notes.map((note) => note.path)).toEqual(["/v/fresh.md", "/v/known.md"]);
  });

  it("still lists notes when the order file cannot be read", async () => {
    // 排序丢了只是回落到按 mtime 排,不该让面板打不开。
    invoke.mockImplementation((command: string) => {
      if (command === "notebook_read_tree") {
        return Promise.resolve([entry({ name: "a.md", path: "/v/a.md" })]);
      }
      return Promise.reject(new Error("order.json is corrupt"));
    });

    const notes = await listNotes("/v");

    expect(notes.map((note) => note.path)).toEqual(["/v/a.md"]);
  });
});

describe("loadNote", () => {
  it("splits frontmatter, takes the title from it, and records the baseline", async () => {
    invoke.mockResolvedValueOnce({
      content: '---\ntitle: "Real title"\ncustom: keep\n---\n\n# Body\n',
      sig: sig(500, "abc"),
    });

    const loaded = await loadNote(stubNote());

    expect(loaded.title).toBe("Real title");
    expect(loaded.body).toBe("# Body\n");
    expect(loaded.sig).toEqual(sig(500, "abc"));
    expect(loaded.loaded).toBe(true);
    // 不认识的字段要留住,保存时原样写回。
    expect(loaded.frontmatter.extra).toContain("custom: keep");
  });

  it("falls back to the file name for a bare markdown file", async () => {
    // 用户从别的工具拖进来的裸 md,没有 frontmatter 也没有标题行。
    invoke.mockResolvedValueOnce({ content: "just text\n", sig: sig(1) });

    const loaded = await loadNote(stubNote("/v/Dragged In.md"));

    expect(loaded.title).toBe("Dragged In");
  });
});

describe("persistNote", () => {
  it("writes the title back into frontmatter and passes the baseline along", async () => {
    invoke.mockResolvedValueOnce({ status: "saved", sig: sig(700, "new") });

    const note = {
      ...stubNote(),
      title: "Edited",
      body: "content\n",
      sig: sig(600, "old"),
    };
    const result = await persistNote(note);

    expect(result.status).toBe("saved");
    const [command, args] = invoke.mock.calls[0] as [string, Record<string, unknown>];
    expect(command).toBe("notebook_save_note");
    expect(args.content).toContain('title: "Edited"');
    expect(args.content).toContain("content");
    // 基线必须原样传给后端 —— 少了它后端只能一律报冲突。
    expect(args.expected).toEqual(sig(600, "old"));
    expect(args.force).toBe(false);
    if (result.status === "saved") {
      expect(result.note.sig).toEqual(sig(700, "new"));
    }
  });

  it("reports a conflict without writing", async () => {
    invoke.mockResolvedValueOnce({ status: "conflict", disk: sig(999, "disk") });

    const result = await persistNote(stubNote());

    // 冲突要原样上报,由调用方决定弹窗 —— 这一层不能替用户选择覆盖。
    expect(result).toEqual({ status: "conflict", diskSig: sig(999, "disk") });
  });

  it("forwards force after the user chooses to overwrite", async () => {
    invoke.mockResolvedValueOnce({ status: "saved", sig: sig(3) });

    await persistNote(stubNote(), true);

    const [, args] = invoke.mock.calls[0] as [string, Record<string, unknown>];
    expect(args.force).toBe(true);
  });
});

describe("createNote", () => {
  it("lets the backend pick the file name and seeds frontmatter", async () => {
    invoke.mockResolvedValueOnce({ path: "/v/My-note.md", sig: sig(10) });

    const created = await createNote("/v", "My note");

    const [command, args] = invoke.mock.calls[0] as [string, Record<string, unknown>];
    expect(command).toBe("notebook_create_note_in_vault");
    // 命名归后端:slug 规则有 Windows 保留名等平台细节,两份实现会漂。
    expect(args.vault).toBe("/v");
    expect(args.title).toBe("My note");
    expect(args.content).toContain('title: "My note"');
    expect(created.path).toBe("/v/My-note.md");
    expect(created.loaded).toBe(true);
  });

  it("substitutes a placeholder name for an empty title", async () => {
    invoke.mockResolvedValueOnce({ path: "/v/untitled.md", sig: sig(10) });

    const created = await createNote("/v", "   ");

    const [, args] = invoke.mock.calls[0] as [string, Record<string, unknown>];
    expect(args.title).toBe("untitled");
    // 标题本身留空,让面板显示 i18n 的「未命名」而不是写死的 untitled。
    expect(created.title).toBe("");
    expect(args.content).not.toContain("title:");
  });
});

describe("persistOrder", () => {
  it("stores file names rather than absolute paths", async () => {
    invoke.mockResolvedValueOnce(undefined);

    await persistOrder("/v", ["/v/b.md", "/v/sub/a.md"]);

    const [command, args] = invoke.mock.calls[0] as [string, Record<string, unknown>];
    expect(command).toBe("notebook_write_order");
    // 存文件名而不是绝对路径:vault 目录整个搬走后排序还能用。
    expect(args.names).toEqual(["b.md", "a.md"]);
  });
});

describe("removeNote", () => {
  it("delegates to the trash-backed delete command", async () => {
    invoke.mockResolvedValueOnce(undefined);

    await removeNote(stubNote());

    const [command, args] = invoke.mock.calls[0] as [string, Record<string, unknown>];
    // 删除必须可恢复 —— 走系统回收站,不是 unlink。
    expect(command).toBe("notebook_delete_note");
    expect(args.path).toBe("/v/note.md");
  });
});

/** 造一条「已列出但未读入」的笔记,等价于 listNotes 的产物。
 *
 * 直接构造而不走 listNotes:后者要消耗一次 mock 返回值,会和调用方自己排的
 * 队打乱顺序。 */
function stubNote(path = "/v/note.md"): VaultNote {
  const name = path.split("/").pop() ?? path;
  const dot = name.lastIndexOf(".");
  return {
    path,
    title: dot > 0 ? name.slice(0, dot) : name,
    body: "",
    frontmatter: { title: null, extra: [] },
    sig: null,
    modifiedMs: 1000,
    loaded: false,
  };
}
