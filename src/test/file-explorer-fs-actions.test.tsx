/**
 * FileExplorer 的**破坏性文件操作**契约:删除 / 重命名 / 新建 / 粘贴。
 *
 * 为什么另起一个文件:`file-explorer-ui.test.tsx` 是既有文件(守图标、排序、面包屑、
 * 远程超时),这里只补它没覆盖的那一半。这四条路径直接改真实文件系统且**没有撤销**,
 * 所以断言的重点不是「UI 变了没」,而是「到底对哪个路径调了哪个后端命令,
 * 以及什么情况下**绝对不能**调」。
 */
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { confirm } from "../lib/appDialog";
import { I18nProvider } from "../i18n";
import { FileExplorer } from "../components/FileExplorer";
import type { SshConnection } from "../types";

const { showToast } = vi.hoisted(() => ({ showToast: vi.fn() }));

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("../lib/appDialog", () => ({ confirm: vi.fn() }));

vi.mock("../components/Toast", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../components/Toast")>()),
  useToast: () => ({ showToast }),
}));

// 预览面板只需要证明「用什么参数打开了」和「关得掉」,它自己的渲染逻辑不在本文件范围内。
vi.mock("../components/sftp/SftpPreview", () => ({
  SftpPreview: ({
    filePath,
    isDirectory,
    onClose,
  }: {
    filePath: string;
    isDirectory: boolean;
    onClose: () => void;
  }) => (
    <div data-testid="sftp-preview" data-file={filePath} data-dir={String(isDirectory)}>
      <button type="button" onClick={onClose}>
        close-preview
      </button>
    </div>
  ),
}));

/** ResizeObserver 的回调:测试里要能手动触发,验证视口高度确实跟着变。 */
let resizeCallbacks: ResizeObserverCallback[] = [];

class ResizeObserverMock {
  constructor(callback: ResizeObserverCallback) {
    resizeCallbacks.push(callback);
  }
  observe = vi.fn();
  unobserve = vi.fn();
  disconnect = vi.fn();
}
vi.stubGlobal("ResizeObserver", ResizeObserverMock);

type Entry = {
  name: string;
  path: string;
  is_dir: boolean;
  extension?: string;
  modifiedAtMs: number;
  is_gitignored: boolean;
};

function dir(path: string, modifiedAtMs = 100): Entry {
  return {
    name: path.slice(path.lastIndexOf("/") + 1),
    path,
    is_dir: true,
    extension: undefined,
    modifiedAtMs,
    is_gitignored: false,
  };
}

function file(path: string, modifiedAtMs = 100): Entry {
  const name = path.slice(path.lastIndexOf("/") + 1);
  const dot = name.lastIndexOf(".");
  return {
    name,
    path,
    is_dir: false,
    extension: dot > 0 ? name.slice(dot + 1) : undefined,
    modifiedAtMs,
    is_gitignored: false,
  };
}

/**
 * 一棵可变的虚拟目录树:`目录路径 -> 该目录下的条目`。
 *
 * 用真实的读目录回放(而不是「断言调了 delete_path 就完」)是刻意的:删除/重命名之后
 * 组件会 `refresh()` 重读,只有让读结果真的变化,才能验证「删完树里确实没了」
 * 以及选中项回退这类**跨越一次刷新**的行为。
 */
let fs: Map<string, Entry[]>;

function resetFs() {
  fs = new Map<string, Entry[]>([
    ["/repo", [dir("/repo/src", 500), dir("/repo/src-extra", 400), file("/repo/README.md", 300)]],
    ["/repo/src", [file("/repo/src/a.ts", 200), file("/repo/src/b.ts", 100)]],
    ["/repo/src-extra", [file("/repo/src-extra/x.ts", 100)]],
    ["/", [dir("/repo", 900), dir("/other", 800)]],
  ]);
}

/** 覆盖默认路由的额外 handler:返回 undefined 表示「这条不管,交回默认」。 */
type Router = (command: string, args: Record<string, unknown>) => unknown | undefined;

let router: Router | null = null;

function setRouter(next: Router) {
  router = next;
}

function installInvoke() {
  vi.mocked(invoke).mockImplementation((command: string, rawArgs?: unknown) => {
    const args = (rawArgs ?? {}) as Record<string, unknown>;
    if (router) {
      const handled = router(command, args);
      if (handled !== undefined) return handled as Promise<unknown>;
    }
    // 三种项目位置各有一条读配置命令;都得回一个对象,回 undefined 会让组件
    // 在 `config.editor` 上抛 TypeError(只是被 catch 成一条 warn,但会掩盖真实失败)。
    if (
      command === "read_project_config" ||
      command === "remote_read_project_config" ||
      command === "read_wsl_project_config"
    ) {
      return Promise.resolve({});
    }
    if (
      command === "read_dir_entries" ||
      command === "remote_read_dir_entries" ||
      command === "wsl_read_dir_entries"
    ) {
      const path = String(args.path ?? args.remotePath ?? args.linuxPath ?? "");
      const entries = fs.get(path);
      if (!entries) return Promise.reject(new Error(`ENOENT: ${path}`));
      return Promise.resolve(entries);
    }
    return Promise.resolve(undefined);
  });
}

const connection: SshConnection = {
  id: "ssh-1",
  name: "prod",
  host: "example.com",
  port: 22,
  username: "deploy",
  createdAt: 1,
};

type Props = Partial<React.ComponentProps<typeof FileExplorer>>;

function renderExplorer(props: Props = {}) {
  const onFileSelect = vi.fn();
  const onPreviewRequest = vi.fn();
  const onOpenDatabaseFile = vi.fn();
  const view = render(
    <I18nProvider>
      <FileExplorer
        projectPath="/repo"
        projectName="repo"
        onFileSelect={onFileSelect}
        themeVariant="light"
        {...props}
      />
    </I18nProvider>,
  );
  return { ...view, onFileSelect, onPreviewRequest, onOpenDatabaseFile };
}

/** 虚拟滚动的可视高度。jsdom 里 `clientHeight` 恒为 0,不撑开就只渲染出前几行。 */
function stubViewportHeight(height = 600) {
  Object.defineProperty(HTMLElement.prototype, "clientHeight", {
    configurable: true,
    get: () => height,
  });
}

/** 造一个够长的目录,用来观察虚拟滚动窗口。 */
function longDirectory(count: number): Entry[] {
  return Array.from({ length: count }, (_, i) =>
    file(`/repo/f${String(i).padStart(3, "0")}.ts`, 1000 - i),
  );
}

/** 树容器:组件没给它 test id,靠 `tabIndex` 定位(它是唯一可聚焦的容器)。 */
function treeScroll(): HTMLElement {
  const el = document.querySelector<HTMLElement>("div[tabindex='0']");
  if (!el) throw new Error("tree scroll container not found");
  return el;
}

/**
 * 树行必须限定在树容器内查找:面包屑把同名目录也渲染成按钮文本
 * (选中 `/repo/src` 后 `getByText("src")` 会同时命中面包屑那一个)。
 */
function row(name: string): HTMLElement {
  const label = within(treeScroll()).getByText(name);
  const rowEl = label.parentElement;
  if (!rowEl) throw new Error(`row for ${name} not found`);
  return rowEl;
}

/** 选中态由行背景表达(`--bg-selected`),没有 aria 属性可依赖。 */
function isSelected(name: string): boolean {
  return row(name).style.background === "var(--bg-selected)";
}

/** 树内是否有这一行(同样要避开面包屑的同名按钮)。 */
function inTree(name: string): HTMLElement {
  return within(treeScroll()).getByText(name);
}

function notInTree(name: string) {
  expect(within(treeScroll()).queryByText(name)).not.toBeInTheDocument();
}

function openContextMenu(name: string) {
  fireEvent.contextMenu(row(name), { clientX: 20, clientY: 30 });
}

function menuItem(label: string): HTMLElement {
  return screen.getByRole("button", { name: label });
}

function queryMenuItem(label: string): HTMLElement | null {
  return screen.queryByRole("button", { name: label });
}

/** 造一个够用的 FileList 替身:`Array.from` 只要 `length` + 数字下标。 */
function fileList(paths: string[]): FileList {
  const bag: Record<string | number, unknown> = { length: paths.length };
  paths.forEach((path, i) => {
    bag[i] = { path, name: path };
  });
  return bag as unknown as FileList;
}

function firePasteEvent(target: HTMLElement, files: FileList) {
  fireEvent.paste(target, { clipboardData: { files } });
}

/**
 * 盯住事件处理器里抛出的异常。
 *
 * jsdom 会把 listener 抛的异常转成 window 的 `error` 事件,而不是让 fireEvent 抛出来 ——
 * 于是「守卫生效、什么都没做」和「守卫失效、读了 null 的属性崩掉」在
 * `expect(x).not.toHaveBeenCalled()` 上完全同构。凡是断言「什么都没发生」的用例,
 * 都得同时确认没崩。
 */
function watchErrors() {
  const errors: string[] = [];
  const onError = (e: ErrorEvent) => {
    errors.push(e.message || String(e.error));
  };
  window.addEventListener("error", onError);
  return { errors, stop: () => window.removeEventListener("error", onError) };
}

beforeEach(() => {
  resetFs();
  router = null;
  resizeCallbacks = [];
  vi.mocked(invoke).mockReset();
  vi.mocked(confirm).mockReset();
  showToast.mockReset();
  installInvoke();
  stubViewportHeight();
  Object.defineProperty(navigator, "clipboard", {
    configurable: true,
    value: { writeText: vi.fn().mockResolvedValue(undefined) },
  });
});

afterEach(() => {
  vi.useRealTimers();
});

describe("删除", () => {
  it("用户点取消时一个后端调用都不发", async () => {
    vi.mocked(confirm).mockResolvedValue(false);
    renderExplorer();
    await screen.findByText("README.md");

    openContextMenu("README.md");
    fireEvent.click(menuItem("Delete"));

    await waitFor(() => expect(confirm).toHaveBeenCalledTimes(1));
    expect(vi.mocked(invoke).mock.calls.some(([c]) => c === "delete_path")).toBe(false);
    // 取消后文件必须还在树里。
    expect(screen.getByText("README.md")).toBeInTheDocument();
  });

  it("确认后按选中路径调 delete_path,并带上 projectPath 作为越界闸门", async () => {
    vi.mocked(confirm).mockResolvedValue(true);
    renderExplorer();
    await screen.findByText("README.md");

    openContextMenu("README.md");
    fireEvent.click(menuItem("Delete"));

    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("delete_path", {
        path: "/repo/README.md",
        projectPath: "/repo",
      }),
    );
  });

  it("文件与文件夹用不同的确认文案,标题里带名字", async () => {
    vi.mocked(confirm).mockResolvedValue(false);
    renderExplorer();
    await screen.findByText("README.md");

    openContextMenu("README.md");
    fireEvent.click(menuItem("Delete"));
    await waitFor(() => expect(confirm).toHaveBeenCalledTimes(1));
    expect(vi.mocked(confirm).mock.calls[0][0]).toMatch(/Move “README\.md” to the Trash/);
    expect(vi.mocked(confirm).mock.calls[0][1]).toMatchObject({
      title: "Delete README.md?",
      kind: "warning",
      okLabel: "Delete",
    });

    openContextMenu("src");
    fireEvent.click(menuItem("Delete"));
    await waitFor(() => expect(confirm).toHaveBeenCalledTimes(2));
    // 文件夹文案必须点明「连同全部内容」——这是不可逆操作的唯一提示。
    expect(vi.mocked(confirm).mock.calls[1][0]).toMatch(/folder “src” and all its contents/);
  });

  it("删除后**立刻**主动刷新,不靠 2500ms 的自动轮询兜", async () => {
    /*
     * 用假时钟是必须的:真时钟下 `waitFor` 的 3000ms 窗口里自动轮询(2500ms)会自己
     * 把树刷新掉 —— 于是「删完不调 refresh」这个变异也能全绿(变异测试实测)。
     * 冻住时钟后,树更新只可能来自删除分支里那一次显式 refresh。
     */
    vi.useFakeTimers();
    vi.mocked(confirm).mockResolvedValue(true);
    setRouter((command, args) => {
      if (command !== "delete_path") return undefined;
      const target = String(args.path);
      fs.set(
        "/repo",
        (fs.get("/repo") ?? []).filter((entry) => entry.path !== target),
      );
      return Promise.resolve(undefined);
    });
    renderExplorer();
    await act(async () => {
      await vi.advanceTimersByTimeAsync(0);
    });
    expect(inTree("README.md")).toBeInTheDocument();

    openContextMenu("README.md");
    fireEvent.click(menuItem("Delete"));
    // 只冲 microtask,一毫秒都不推进 —— 自动轮询没有机会开火。
    await act(async () => {
      await Promise.resolve();
      await Promise.resolve();
      await Promise.resolve();
    });

    notInTree("README.md");
  });

  it("删掉的正是选中项时清空 selectedPath(不留悬空路径)", async () => {
    /*
     * 断言**不能**用面包屑或行高亮:那两个都是从 `findNode(nodes, selectedPath)` 派生的,
     * 节点消失后自动回落到项目根 —— 于是「压根不清 selectedPath」这个变异也能全绿
     * (变异测试实测)。要看见差别,得找**直接读 selectedPath**的出口:
     * 复制路径(`if (!selectedPath) return`)就是一个。
     */
    vi.mocked(confirm).mockResolvedValue(true);
    setRouter((command, args) => {
      if (command !== "delete_path") return undefined;
      const target = String(args.path);
      fs.set(
        "/repo",
        (fs.get("/repo") ?? []).filter((entry) => entry.path !== target),
      );
      return Promise.resolve(undefined);
    });
    const { onFileSelect } = renderExplorer();
    await screen.findByText("README.md");

    fireEvent.click(row("README.md"));
    expect(onFileSelect).toHaveBeenCalledWith("/repo/README.md", "README.md");
    expect(isSelected("README.md")).toBe(true);

    openContextMenu("README.md");
    fireEvent.click(menuItem("Delete"));
    await waitFor(() => notInTree("README.md"));

    fireEvent.keyDown(treeScroll(), { key: "c", metaKey: true, altKey: true });
    await act(async () => {});

    // 留着悬空路径的话,用户会复制到一个已经不存在的文件路径(还会收到「已复制」提示)。
    expect(navigator.clipboard.writeText).not.toHaveBeenCalled();
    expect(showToast).not.toHaveBeenCalled();
  });

  it("删掉祖先目录时,选中的后代也一起清空", async () => {
    vi.mocked(confirm).mockResolvedValue(true);
    setRouter((command, args) => {
      if (command !== "delete_path") return undefined;
      const target = String(args.path);
      fs.set(
        "/repo",
        (fs.get("/repo") ?? []).filter((entry) => entry.path !== target),
      );
      fs.delete(target);
      return Promise.resolve(undefined);
    });
    renderExplorer();
    await screen.findByText("src");

    fireEvent.click(row("src"));
    const child = await screen.findByText("a.ts");
    fireEvent.click(child.parentElement!);
    expect(screen.getByRole("button", { name: "/repo/src" })).toHaveAttribute(
      "aria-current",
      "location",
    );

    openContextMenu("src");
    fireEvent.click(menuItem("Delete"));
    await waitFor(() => notInTree("a.ts"));

    // 同上:走复制路径这个直接读 selectedPath 的出口。
    fireEvent.keyDown(treeScroll(), { key: "c", metaKey: true, altKey: true });
    await act(async () => {});
    expect(navigator.clipboard.writeText).not.toHaveBeenCalled();
  });

  it("同前缀的兄弟目录不受影响(/repo/src 不能连坐 /repo/src-extra)", async () => {
    vi.mocked(confirm).mockResolvedValue(true);
    setRouter((command, args) => {
      if (command !== "delete_path") return undefined;
      const target = String(args.path);
      fs.set(
        "/repo",
        (fs.get("/repo") ?? []).filter((entry) => entry.path !== target),
      );
      fs.delete(target);
      return Promise.resolve(undefined);
    });
    renderExplorer();
    await screen.findByText("src-extra");

    // 选中 /repo/src-extra/x.ts,然后删掉 /repo/src。
    fireEvent.click(row("src-extra"));
    const child = await screen.findByText("x.ts");
    fireEvent.click(child.parentElement!);
    expect(screen.getByRole("button", { name: "/repo/src-extra" })).toHaveAttribute(
      "aria-current",
      "location",
    );

    openContextMenu("src");
    fireEvent.click(menuItem("Delete"));

    await waitFor(() => notInTree("src"));
    // 少了 `targetPath + sep` 里那个分隔符,startsWith 会把 src-extra 当成 src 的后代,
    // 于是删 src 会顺手清掉 src-extra 里的选中项。
    expect(screen.getByRole("button", { name: "/repo/src-extra" })).toHaveAttribute(
      "aria-current",
      "location",
    );
    expect(inTree("x.ts")).toBeInTheDocument();

    // 选中项确实还在(直接读 selectedPath 的出口)。
    fireEvent.keyDown(treeScroll(), { key: "c", metaKey: true, altKey: true });
    await waitFor(() =>
      expect(navigator.clipboard.writeText).toHaveBeenCalledWith("/repo/src-extra/x.ts"),
    );
  });

  it("后端报错时弹提示,且不动选中项", async () => {
    vi.mocked(confirm).mockResolvedValue(true);
    setRouter((command) => {
      if (command !== "delete_path") return undefined;
      return Promise.reject(new Error("Permission denied"));
    });
    renderExplorer();
    await screen.findByText("README.md");

    fireEvent.click(row("README.md"));
    openContextMenu("README.md");
    fireEvent.click(menuItem("Delete"));

    await waitFor(() =>
      expect(showToast).toHaveBeenCalledWith("Failed to delete: Permission denied"),
    );
    expect(screen.getByText("README.md")).toBeInTheDocument();
    expect(isSelected("README.md")).toBe(true);
  });

  it("根目录(空白处右键)没有删除项", async () => {
    renderExplorer();
    await screen.findByText("README.md");

    const scroll = treeScroll();
    fireEvent.contextMenu(scroll, { clientX: 5, clientY: 5, target: scroll });

    // 有菜单,但不含 Delete —— 否则一次误点就能把整个项目根目录删掉。
    expect(menuItem("New File")).toBeInTheDocument();
    expect(queryMenuItem("Delete")).not.toBeInTheDocument();
  });

  it("远程项目走 remote_delete_path 并带连接信息", async () => {
    vi.mocked(confirm).mockResolvedValue(true);
    // 目录必须在 render 之前就摆好:首读失败只会等到 2500ms 的自动刷新才重试。
    fs.set("/srv/app", [file("/srv/app/main.rs", 100)]);
    renderExplorer({
      projectPath: "/srv/app",
      remote: { kind: "ssh", connection, projectPath: "/srv/app" },
    });
    await screen.findByText("main.rs");

    openContextMenu("main.rs");
    fireEvent.click(menuItem("Delete"));

    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("remote_delete_path", {
        connection,
        remotePath: "/srv/app/main.rs",
        remoteProjectPath: "/srv/app",
      }),
    );
    expect(vi.mocked(invoke).mock.calls.some(([c]) => c === "delete_path")).toBe(false);
  });

  it("WSL 项目走 wsl_delete_path", async () => {
    vi.mocked(confirm).mockResolvedValue(true);
    fs.set("/home/dev/app", [file("/home/dev/app/main.go", 100)]);
    renderExplorer({
      projectPath: "/home/dev/app",
      remote: { kind: "wsl", distribution: "Ubuntu", projectPath: "/home/dev/app" },
    });
    await screen.findByText("main.go");

    openContextMenu("main.go");
    fireEvent.click(menuItem("Delete"));

    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("wsl_delete_path", {
        distribution: "Ubuntu",
        linuxPath: "/home/dev/app/main.go",
        linuxProjectPath: "/home/dev/app",
      }),
    );
  });

  it("上一次删除还没落地时,再次触发被 in-flight 闸门挡掉", async () => {
    let resolveDelete: (() => void) | undefined;
    vi.mocked(confirm).mockResolvedValue(true);
    setRouter((command) => {
      if (command !== "delete_path") return undefined;
      return new Promise<undefined>((resolve) => {
        resolveDelete = () => resolve(undefined);
      });
    });
    renderExplorer();
    await screen.findByText("README.md");

    fireEvent.click(row("README.md"));
    const scroll = treeScroll();
    fireEvent.keyDown(scroll, { key: "Backspace", metaKey: true });
    await waitFor(() =>
      expect(vi.mocked(invoke).mock.calls.filter(([c]) => c === "delete_path")).toHaveLength(1),
    );

    // 第一次的 delete_path 还挂着,这一次必须被挡在 confirm 之前。
    fireEvent.keyDown(scroll, { key: "Backspace", metaKey: true });
    await act(async () => {});
    expect(confirm).toHaveBeenCalledTimes(1);
    expect(vi.mocked(invoke).mock.calls.filter(([c]) => c === "delete_path")).toHaveLength(1);

    resolveDelete?.();
    await act(async () => {});
  });

  it("【现状固化】确认框还开着时重复触发,两次都会打到后端", async () => {
    /*
     * `deleteInFlightRef` 是在 `await confirm(...)` **之后**才置位的,所以在确认框
     * 未决的这段窗口里,第二次触发照样能进去。真实后果有限(同一路径删两次,第二次
     * 后端报 not found → 一条错误 toast),但它确实是一个可观测的重入窗口。
     *
     * 这条断言的是**现状**:谁把置位挪到 confirm 之前(那才是修法),它会失败并提醒
     * 同时更新这里的期望。没直接改是因为这属于行为变更,不在「不影响功能」的范围内。
     */
    let confirmCount = 0;
    const pending: Array<(ok: boolean) => void> = [];
    vi.mocked(confirm).mockImplementation(() => {
      confirmCount += 1;
      return new Promise<boolean>((resolve) => pending.push(resolve));
    });
    renderExplorer();
    await screen.findByText("README.md");

    fireEvent.click(row("README.md"));
    const scroll = treeScroll();
    fireEvent.keyDown(scroll, { key: "Backspace", metaKey: true });
    fireEvent.keyDown(scroll, { key: "Backspace", metaKey: true });
    await act(async () => {});

    expect(confirmCount).toBe(2);

    pending.forEach((resolve) => resolve(true));
    await act(async () => {});
    expect(vi.mocked(invoke).mock.calls.filter(([c]) => c === "delete_path")).toHaveLength(2);
  });

  it("没有选中项时 Cmd+Backspace 什么都不做(且不是因为崩了)", async () => {
    const watch = watchErrors();
    renderExplorer();
    await screen.findByText("README.md");

    fireEvent.keyDown(treeScroll(), { key: "Backspace", metaKey: true });
    await act(async () => {});

    expect(confirm).not.toHaveBeenCalled();
    expect(vi.mocked(invoke).mock.calls.some(([c]) => c === "delete_path")).toBe(false);
    /*
     * 必须同时盯 window 的 error:jsdom 把事件处理器里抛的异常转成 `error` 事件,
     * 所以「守卫生效、什么都没做」和「守卫失效、读 null.path 崩掉」在
     * `expect(confirm).not.toHaveBeenCalled()` 上完全同构(变异测试实测:
     * 把 `if (selectedNode)` 改成 `if (true)` 时全绿)。
     */
    expect(watch.errors).toEqual([]);
    watch.stop();
  });
});

describe("重命名", () => {
  /** 选中一行后按 Enter 进入重命名,返回那个 input。 */
  async function startRename(name: string): Promise<HTMLInputElement> {
    fireEvent.click(row(name));
    fireEvent.keyDown(treeScroll(), { key: "Enter" });
    const inputs = await waitFor(() => {
      const found = treeScroll().querySelectorAll("input");
      if (found.length === 0) throw new Error("rename input not rendered");
      return found;
    });
    return inputs[0] as HTMLInputElement;
  }

  it("改名后调 rename_path,选中项移到新路径并回调新名字", async () => {
    setRouter((command, args) => {
      if (command !== "rename_path") return undefined;
      const oldPath = String(args.path);
      const newName = String(args.newName);
      fs.set(
        "/repo",
        (fs.get("/repo") ?? []).map((entry) =>
          entry.path === oldPath ? file(`/repo/${newName}`, entry.modifiedAtMs) : entry,
        ),
      );
      return Promise.resolve(undefined);
    });
    const { onFileSelect } = renderExplorer();
    await screen.findByText("README.md");

    const input = await startRename("README.md");
    expect(input).toHaveValue("README.md");
    fireEvent.change(input, { target: { value: "READ.md" } });
    fireEvent.keyDown(input, { key: "Enter" });

    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("rename_path", {
        path: "/repo/README.md",
        newName: "READ.md",
        projectPath: "/repo",
      }),
    );
    await waitFor(() => expect(screen.getByText("READ.md")).toBeInTheDocument());
    expect(onFileSelect).toHaveBeenLastCalledWith("/repo/READ.md", "READ.md");
    expect(isSelected("READ.md")).toBe(true);
  });

  it("目录改名不触发 onFileSelect(不该把目录当文件打开)", async () => {
    setRouter((command, args) => {
      if (command !== "rename_path") return undefined;
      const newName = String(args.newName);
      fs.set("/repo", [dir(`/repo/${newName}`, 500), file("/repo/README.md", 300)]);
      fs.set(`/repo/${newName}`, []);
      return Promise.resolve(undefined);
    });
    const { onFileSelect } = renderExplorer();
    await screen.findByText("src");

    const input = await startRename("src");
    fireEvent.change(input, { target: { value: "source" } });
    fireEvent.keyDown(input, { key: "Enter" });

    await waitFor(() => expect(inTree("source")).toBeInTheDocument());
    expect(onFileSelect).not.toHaveBeenCalled();
  });

  it("名字没变时直接收起,不调后端", async () => {
    renderExplorer();
    await screen.findByText("README.md");

    const input = await startRename("README.md");
    fireEvent.keyDown(input, { key: "Enter" });

    await act(async () => {});
    expect(vi.mocked(invoke).mock.calls.some(([c]) => c === "rename_path")).toBe(false);
    expect(treeScroll().querySelectorAll("input")).toHaveLength(0);
  });

  it("只输空白等于取消,不调后端", async () => {
    renderExplorer();
    await screen.findByText("README.md");

    const input = await startRename("README.md");
    fireEvent.change(input, { target: { value: "   " } });
    fireEvent.keyDown(input, { key: "Enter" });

    await act(async () => {});
    expect(vi.mocked(invoke).mock.calls.some(([c]) => c === "rename_path")).toBe(false);
    expect(treeScroll().querySelectorAll("input")).toHaveLength(0);
  });

  it.each([
    ["斜杠", "a/b.md"],
    ["反斜杠", "a\\b.md"],
  ])("名字里带%s时报错且不调后端,输入行留着让用户改", async (_label, value) => {
    renderExplorer();
    await screen.findByText("README.md");

    const input = await startRename("README.md");
    fireEvent.change(input, { target: { value } });
    fireEvent.keyDown(input, { key: "Enter" });

    await act(async () => {});
    // 放过去的话后端会按相对路径解释,文件会跑到别的目录里。
    expect(vi.mocked(invoke).mock.calls.some(([c]) => c === "rename_path")).toBe(false);
    expect(showToast).toHaveBeenCalledWith("Failed to rename: Invalid file name");
    expect(treeScroll().querySelectorAll("input")).toHaveLength(1);
  });

  it("Escape 取消重命名", async () => {
    renderExplorer();
    await screen.findByText("README.md");

    const input = await startRename("README.md");
    fireEvent.change(input, { target: { value: "x.md" } });
    fireEvent.keyDown(input, { key: "Escape" });

    await act(async () => {});
    expect(vi.mocked(invoke).mock.calls.some(([c]) => c === "rename_path")).toBe(false);
    expect(treeScroll().querySelectorAll("input")).toHaveLength(0);
    expect(screen.getByText("README.md")).toBeInTheDocument();
  });

  it("失焦丢弃输入,不会静默改名", async () => {
    renderExplorer();
    await screen.findByText("README.md");

    const input = await startRename("README.md");
    fireEvent.change(input, { target: { value: "x.md" } });
    fireEvent.blur(input);

    await act(async () => {});
    expect(vi.mocked(invoke).mock.calls.some(([c]) => c === "rename_path")).toBe(false);
    expect(screen.getByText("README.md")).toBeInTheDocument();
  });

  it("后端报错时提示,原名保留", async () => {
    setRouter((command) => {
      if (command !== "rename_path") return undefined;
      return Promise.reject(new Error("EEXIST"));
    });
    renderExplorer();
    await screen.findByText("README.md");

    const input = await startRename("README.md");
    fireEvent.change(input, { target: { value: "b.ts" } });
    fireEvent.keyDown(input, { key: "Enter" });

    await waitFor(() => expect(showToast).toHaveBeenCalledWith("Failed to rename: EEXIST"));
    // 失败时输入行**不收起**(`cancelRename` 只在成功分支调),用户可以就地改名重试;
    // 也正因为还在重命名态,这一行的普通树行被渲染成 null,所以断言输入框而不是行文本。
    const stillEditing = treeScroll().querySelectorAll("input");
    expect(stillEditing).toHaveLength(1);
    expect(stillEditing[0]).toHaveValue("b.ts");
    // 磁盘上没动:重命名只发了一次,没有第二次「补偿」调用。
    expect(vi.mocked(invoke).mock.calls.filter(([c]) => c === "rename_path")).toHaveLength(1);
  });

  it("连按两次 Enter 只发一次请求", async () => {
    let resolveRename: (() => void) | undefined;
    setRouter((command) => {
      if (command !== "rename_path") return undefined;
      return new Promise<undefined>((resolve) => {
        resolveRename = () => resolve(undefined);
      });
    });
    renderExplorer();
    await screen.findByText("README.md");

    const input = await startRename("README.md");
    fireEvent.change(input, { target: { value: "READ.md" } });
    fireEvent.keyDown(input, { key: "Enter" });
    fireEvent.keyDown(input, { key: "Enter" });

    await act(async () => {});
    expect(vi.mocked(invoke).mock.calls.filter(([c]) => c === "rename_path")).toHaveLength(1);
    resolveRename?.();
    await act(async () => {});
  });

  it("远程项目走 remote_rename_path", async () => {
    fs.set("/srv/app", [file("/srv/app/main.rs", 100)]);
    renderExplorer({
      projectPath: "/srv/app",
      remote: { kind: "ssh", connection, projectPath: "/srv/app" },
    });
    await screen.findByText("main.rs");

    const input = await startRename("main.rs");
    fireEvent.change(input, { target: { value: "lib.rs" } });
    fireEvent.keyDown(input, { key: "Enter" });

    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("remote_rename_path", {
        connection,
        remotePath: "/srv/app/main.rs",
        remoteProjectPath: "/srv/app",
        newName: "lib.rs",
      }),
    );
  });

  it("没有选中项时按 Enter 不会开出重命名输入框", async () => {
    renderExplorer();
    await screen.findByText("README.md");

    fireEvent.keyDown(treeScroll(), { key: "Enter" });
    await act(async () => {});

    expect(treeScroll().querySelectorAll("input")).toHaveLength(0);
  });
});

describe("新建", () => {
  /** 从右键菜单开一个新建输入行;`target` 省略表示在空白处(根目录)。 */
  async function startCreate(kind: "New File" | "New Folder", target?: string) {
    if (target) {
      openContextMenu(target);
    } else {
      const scroll = treeScroll();
      fireEvent.contextMenu(scroll, { clientX: 5, clientY: 5, target: scroll });
    }
    fireEvent.click(menuItem(kind));
    const inputs = await waitFor(() => {
      const found = treeScroll().querySelectorAll("input");
      if (found.length === 0) throw new Error("create input not rendered");
      return found;
    });
    return inputs[0] as HTMLInputElement;
  }

  it("在根目录新建文件:调 create_file,然后选中并打开它", async () => {
    setRouter((command, args) => {
      if (command !== "create_file") return undefined;
      const path = String(args.path);
      fs.set("/repo", [...(fs.get("/repo") ?? []), file(path, 999)]);
      return Promise.resolve(undefined);
    });
    const { onFileSelect } = renderExplorer();
    await screen.findByText("README.md");

    const input = await startCreate("New File");
    fireEvent.change(input, { target: { value: "new.ts" } });
    fireEvent.keyDown(input, { key: "Enter" });

    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("create_file", {
        path: "/repo/new.ts",
        projectPath: "/repo",
      }),
    );
    await waitFor(() => expect(inTree("new.ts")).toBeInTheDocument());
    expect(onFileSelect).toHaveBeenCalledWith("/repo/new.ts", "new.ts");

    // 新文件同时成为选中项 —— 用直接读 selectedPath 的出口验证(行高亮是派生值,
    // 单靠它「压根不 setSelectedPath」的变异也能全绿)。
    fireEvent.keyDown(treeScroll(), { key: "c", metaKey: true, altKey: true });
    await waitFor(() => expect(navigator.clipboard.writeText).toHaveBeenCalledWith("/repo/new.ts"));
  });

  it("新建文件夹调 create_directory,且不当成文件打开", async () => {
    setRouter((command, args) => {
      if (command !== "create_directory") return undefined;
      const path = String(args.path);
      fs.set("/repo", [...(fs.get("/repo") ?? []), dir(path, 999)]);
      fs.set(path, []);
      return Promise.resolve(undefined);
    });
    const { onFileSelect } = renderExplorer();
    await screen.findByText("README.md");

    const input = await startCreate("New Folder");
    fireEvent.change(input, { target: { value: "assets" } });
    fireEvent.keyDown(input, { key: "Enter" });

    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("create_directory", {
        path: "/repo/assets",
        projectPath: "/repo",
      }),
    );
    expect(onFileSelect).not.toHaveBeenCalled();
  });

  it("对着目录右键时新建到该目录内", async () => {
    setRouter((command, args) => {
      if (command !== "create_file") return undefined;
      const path = String(args.path);
      fs.set("/repo/src", [...(fs.get("/repo/src") ?? []), file(path, 999)]);
      return Promise.resolve(undefined);
    });
    renderExplorer();
    await screen.findByText("src");

    const input = await startCreate("New File", "src");
    fireEvent.change(input, { target: { value: "c.ts" } });
    fireEvent.keyDown(input, { key: "Enter" });

    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("create_file", {
        path: "/repo/src/c.ts",
        projectPath: "/repo",
      }),
    );
  });

  it("对着文件右键时新建到该文件的父目录,而不是文件里面", async () => {
    setRouter((command, args) => {
      if (command !== "create_file") return undefined;
      fs.set("/repo/src", [...(fs.get("/repo/src") ?? []), file(String(args.path), 999)]);
      return Promise.resolve(undefined);
    });
    renderExplorer();
    await screen.findByText("src");

    fireEvent.click(row("src"));
    await screen.findByText("a.ts");

    const input = await startCreate("New File", "a.ts");
    fireEvent.change(input, { target: { value: "c.ts" } });
    fireEvent.keyDown(input, { key: "Enter" });

    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("create_file", {
        path: "/repo/src/c.ts",
        projectPath: "/repo",
      }),
    );
  });

  it.each([
    ["空", ""],
    ["纯空白", "   "],
  ])("%s名字等于取消,不调后端", async (_label, value) => {
    renderExplorer();
    await screen.findByText("README.md");

    const input = await startCreate("New File");
    if (value) fireEvent.change(input, { target: { value } });
    fireEvent.keyDown(input, { key: "Enter" });

    await act(async () => {});
    expect(vi.mocked(invoke).mock.calls.some(([c]) => c === "create_file")).toBe(false);
    expect(treeScroll().querySelectorAll("input")).toHaveLength(0);
  });

  it("名字两端的空白会被剪掉,不会造出带空格的文件名", async () => {
    setRouter((command, args) => {
      if (command !== "create_file") return undefined;
      fs.set("/repo", [...(fs.get("/repo") ?? []), file(String(args.path), 999)]);
      return Promise.resolve(undefined);
    });
    renderExplorer();
    await screen.findByText("README.md");

    const input = await startCreate("New File");
    fireEvent.change(input, { target: { value: "  new.ts  " } });
    fireEvent.keyDown(input, { key: "Enter" });

    // 不 trim 的话磁盘上会出现 `"  new.ts  "` 这种名字,肉眼看不出来还很难删。
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("create_file", {
        path: "/repo/new.ts",
        projectPath: "/repo",
      }),
    );
  });

  it.each([
    ["斜杠", "a/b.ts"],
    ["反斜杠", "a\\b.ts"],
  ])("名字里带%s时报错且不调后端", async (_label, value) => {
    renderExplorer();
    await screen.findByText("README.md");

    const input = await startCreate("New File");
    fireEvent.change(input, { target: { value } });
    fireEvent.keyDown(input, { key: "Enter" });

    await act(async () => {});
    expect(vi.mocked(invoke).mock.calls.some(([c]) => c === "create_file")).toBe(false);
    expect(showToast).toHaveBeenCalledWith("Failed to create: Invalid file name");
    expect(treeScroll().querySelectorAll("input")).toHaveLength(1);
  });

  it("Escape 与失焦都丢弃新建", async () => {
    renderExplorer();
    await screen.findByText("README.md");

    const escInput = await startCreate("New File");
    fireEvent.change(escInput, { target: { value: "x.ts" } });
    fireEvent.keyDown(escInput, { key: "Escape" });
    await act(async () => {});
    expect(treeScroll().querySelectorAll("input")).toHaveLength(0);

    const blurInput = await startCreate("New File");
    fireEvent.change(blurInput, { target: { value: "y.ts" } });
    fireEvent.blur(blurInput);
    await act(async () => {});
    expect(treeScroll().querySelectorAll("input")).toHaveLength(0);
    expect(vi.mocked(invoke).mock.calls.some(([c]) => c === "create_file")).toBe(false);
  });

  it("后端报错时提示,输入行留着", async () => {
    setRouter((command) => {
      if (command !== "create_file") return undefined;
      return Promise.reject(new Error("EACCES"));
    });
    renderExplorer();
    await screen.findByText("README.md");

    const input = await startCreate("New File");
    fireEvent.change(input, { target: { value: "new.ts" } });
    fireEvent.keyDown(input, { key: "Enter" });

    await waitFor(() => expect(showToast).toHaveBeenCalledWith("Failed to create: EACCES"));
    expect(treeScroll().querySelectorAll("input")).toHaveLength(1);
  });

  it("连按两次 Enter 只建一次", async () => {
    let resolveCreate: (() => void) | undefined;
    setRouter((command) => {
      if (command !== "create_file") return undefined;
      return new Promise<undefined>((resolve) => {
        resolveCreate = () => resolve(undefined);
      });
    });
    renderExplorer();
    await screen.findByText("README.md");

    const input = await startCreate("New File");
    fireEvent.change(input, { target: { value: "new.ts" } });
    fireEvent.keyDown(input, { key: "Enter" });
    fireEvent.keyDown(input, { key: "Enter" });

    await act(async () => {});
    expect(vi.mocked(invoke).mock.calls.filter(([c]) => c === "create_file")).toHaveLength(1);
    resolveCreate?.();
    await act(async () => {});
  });

  it("远程项目走 remote_create_file / remote_create_directory", async () => {
    fs.set("/srv/app", [file("/srv/app/main.rs", 100)]);
    renderExplorer({
      projectPath: "/srv/app",
      remote: { kind: "ssh", connection, projectPath: "/srv/app" },
    });
    await screen.findByText("main.rs");

    const fileInput = await startCreate("New File");
    fireEvent.change(fileInput, { target: { value: "lib.rs" } });
    fireEvent.keyDown(fileInput, { key: "Enter" });
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("remote_create_file", {
        connection,
        remotePath: "/srv/app/lib.rs",
        remoteProjectPath: "/srv/app",
      }),
    );

    const dirInput = await startCreate("New Folder");
    fireEvent.change(dirInput, { target: { value: "tests" } });
    fireEvent.keyDown(dirInput, { key: "Enter" });
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("remote_create_directory", {
        connection,
        remotePath: "/srv/app/tests",
        remoteProjectPath: "/srv/app",
      }),
    );
  });
});

describe("粘贴", () => {
  it("没选中时粘到浏览根", async () => {
    renderExplorer();
    await screen.findByText("README.md");

    firePasteEvent(treeScroll(), fileList(["/outside/a.txt"]));

    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("copy_paths_to_directory", {
        sourcePaths: ["/outside/a.txt"],
        targetDirectory: "/repo",
        projectPath: "/repo",
      }),
    );
  });

  it("选中目录时粘到该目录内", async () => {
    renderExplorer();
    await screen.findByText("src");

    fireEvent.click(row("src"));
    firePasteEvent(treeScroll(), fileList(["/outside/a.txt"]));

    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith(
        "copy_paths_to_directory",
        expect.objectContaining({ targetDirectory: "/repo/src" }),
      ),
    );
  });

  it("选中文件时粘到它的父目录,而不是覆盖那个文件", async () => {
    renderExplorer();
    await screen.findByText("src");

    fireEvent.click(row("src"));
    const child = await screen.findByText("a.ts");
    fireEvent.click(child.parentElement!);

    firePasteEvent(treeScroll(), fileList(["/outside/a.txt"]));

    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith(
        "copy_paths_to_directory",
        expect.objectContaining({ targetDirectory: "/repo/src" }),
      ),
    );
  });

  it("多个来源一次性传下去", async () => {
    renderExplorer();
    await screen.findByText("README.md");

    firePasteEvent(treeScroll(), fileList(["/outside/a.txt", "/outside/b.txt"]));

    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith(
        "copy_paths_to_directory",
        expect.objectContaining({ sourcePaths: ["/outside/a.txt", "/outside/b.txt"] }),
      ),
    );
  });

  it("剪贴板里没有文件时不进入粘贴流程", async () => {
    renderExplorer();
    await screen.findByText("README.md");

    firePasteEvent(treeScroll(), fileList([]));

    await act(async () => {});
    expect(vi.mocked(invoke).mock.calls.some(([c]) => c === "copy_paths_to_directory")).toBe(false);
    // `handleTreePaste` 在 files.length 为 0 时直接 return,连 pasteNoFiles 都不该弹。
    expect(showToast).not.toHaveBeenCalled();
  });

  it("焦点在输入框里时不劫持粘贴", async () => {
    renderExplorer();
    await screen.findByText("README.md");

    fireEvent.click(row("README.md"));
    fireEvent.keyDown(treeScroll(), { key: "Enter" });
    const input = await waitFor(() => {
      const found = treeScroll().querySelector("input");
      if (!found) throw new Error("rename input not rendered");
      return found;
    });

    firePasteEvent(input as HTMLElement, fileList(["/outside/a.txt"]));

    await act(async () => {});
    // 否则用户在重命名框里粘一段文本会变成往目录里拷文件。
    expect(vi.mocked(invoke).mock.calls.some(([c]) => c === "copy_paths_to_directory")).toBe(false);
  });

  it("Cmd+V 从系统剪贴板读路径再粘", async () => {
    setRouter((command) => {
      if (command !== "read_clipboard_file_paths") return undefined;
      return Promise.resolve(["/outside/a.txt"]);
    });
    renderExplorer();
    await screen.findByText("README.md");

    fireEvent.keyDown(treeScroll(), { key: "v", metaKey: true });

    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith(
        "copy_paths_to_directory",
        expect.objectContaining({ sourcePaths: ["/outside/a.txt"] }),
      ),
    );
  });

  it("系统剪贴板里没有路径时给出提示", async () => {
    setRouter((command) => {
      if (command !== "read_clipboard_file_paths") return undefined;
      return Promise.resolve([]);
    });
    renderExplorer();
    await screen.findByText("README.md");

    fireEvent.keyDown(treeScroll(), { key: "v", metaKey: true });

    await waitFor(() =>
      expect(showToast).toHaveBeenCalledWith("No files found in clipboard", "warning"),
    );
    expect(vi.mocked(invoke).mock.calls.some(([c]) => c === "copy_paths_to_directory")).toBe(false);
  });

  it("读剪贴板本身失败时报错而不是静默", async () => {
    setRouter((command) => {
      if (command !== "read_clipboard_file_paths") return undefined;
      return Promise.reject(new Error("clipboard unavailable"));
    });
    renderExplorer();
    await screen.findByText("README.md");

    fireEvent.keyDown(treeScroll(), { key: "v", metaKey: true });

    await waitFor(() =>
      expect(showToast).toHaveBeenCalledWith("Failed to paste: clipboard unavailable"),
    );
  });

  it("拷贝失败时提示", async () => {
    setRouter((command) => {
      if (command !== "copy_paths_to_directory") return undefined;
      return Promise.reject(new Error("ENOSPC"));
    });
    renderExplorer();
    await screen.findByText("README.md");

    firePasteEvent(treeScroll(), fileList(["/outside/a.txt"]));

    await waitFor(() => expect(showToast).toHaveBeenCalledWith("Failed to paste: ENOSPC"));
  });

  it("上一次粘贴没结束时再粘一次会被挡掉", async () => {
    let resolveCopy: (() => void) | undefined;
    setRouter((command) => {
      if (command !== "copy_paths_to_directory") return undefined;
      return new Promise<undefined>((resolve) => {
        resolveCopy = () => resolve(undefined);
      });
    });
    renderExplorer();
    await screen.findByText("README.md");

    firePasteEvent(treeScroll(), fileList(["/outside/a.txt"]));
    await waitFor(() =>
      expect(
        vi.mocked(invoke).mock.calls.filter(([c]) => c === "copy_paths_to_directory"),
      ).toHaveLength(1),
    );
    firePasteEvent(treeScroll(), fileList(["/outside/b.txt"]));
    await act(async () => {});

    expect(
      vi.mocked(invoke).mock.calls.filter(([c]) => c === "copy_paths_to_directory"),
    ).toHaveLength(1);
    resolveCopy?.();
    await act(async () => {});
  });

  it("SSH 项目走上传命令,并给 300s 超时(远程拷贝比本地慢得多)", async () => {
    vi.useFakeTimers();
    fs.set("/srv/app", [file("/srv/app/main.rs", 100)]);
    setRouter((command) => {
      if (command !== "remote_upload_local_paths_to_directory") return undefined;
      return new Promise(() => {});
    });
    renderExplorer({
      projectPath: "/srv/app",
      remote: { kind: "ssh", connection, projectPath: "/srv/app" },
    });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(0);
    });

    firePasteEvent(treeScroll(), fileList(["C:/local/a.txt"]));
    await act(async () => {
      await vi.advanceTimersByTimeAsync(0);
    });

    expect(invoke).toHaveBeenCalledWith("remote_upload_local_paths_to_directory", {
      connection,
      localSourcePaths: ["C:/local/a.txt"],
      targetDirectory: "/srv/app",
      remoteProjectPath: "/srv/app",
    });

    // 60s(默认远程超时)时还不能报超时,否则大文件上传会被误杀。
    await act(async () => {
      await vi.advanceTimersByTimeAsync(299_000);
    });
    expect(showToast).not.toHaveBeenCalled();

    await act(async () => {
      await vi.advanceTimersByTimeAsync(1_500);
    });
    expect(showToast).toHaveBeenCalledWith(expect.stringMatching(/timed out after 300s/));
  });

  it("WSL 项目明确报「暂不支持」,而不是悄悄什么都不做", async () => {
    fs.set("/home/dev/app", [file("/home/dev/app/main.go", 100)]);
    renderExplorer({
      projectPath: "/home/dev/app",
      remote: { kind: "wsl", distribution: "Ubuntu", projectPath: "/home/dev/app" },
    });
    await screen.findByText("main.go");

    firePasteEvent(treeScroll(), fileList(["C:/local/a.txt"]));

    await waitFor(() =>
      expect(showToast).toHaveBeenCalledWith(
        "Failed to paste: Copying Windows clipboard files into WSL is not supported yet.",
      ),
    );
    expect(vi.mocked(invoke).mock.calls.some(([c]) => c === "copy_paths_to_directory")).toBe(false);
  });

  it("剪贴板项没有 path 字段时当作空来源处理", async () => {
    renderExplorer();
    await screen.findByText("README.md");

    // 浏览器里从网页复制的图片就是这种:有 File 但没有本地路径。
    const noPath = { length: 1, 0: { name: "image.png" } } as unknown as FileList;
    firePasteEvent(treeScroll(), noPath);

    await waitFor(() =>
      expect(showToast).toHaveBeenCalledWith("No files found in clipboard", "warning"),
    );
    expect(vi.mocked(invoke).mock.calls.some(([c]) => c === "copy_paths_to_directory")).toBe(false);

    /*
     * 这里钉不住 `"path" in file` 那个判断本身 —— 摘掉它(直接 `String(file.path ?? "")`)
     * 测试全绿,而且**任何输入都绿**:`"path" in file` 为假意味着自有和继承的属性里
     * 都没有 path,那么 `file.path` 必然是 undefined,`?? ""` 之后同样被 `filter(Boolean)`
     * 滤掉。两种写法没有可观测差异,记为等价变异。
     */
  });

  it("粘贴成功后**立刻**刷新,不靠自动轮询", async () => {
    // 与删除那条同理:真时钟下 2500ms 的轮询会替它把树刷出来,
    // 「粘完不 refresh」的变异就看不出差别(实测过)。
    vi.useFakeTimers();
    setRouter((command, args) => {
      if (command !== "copy_paths_to_directory") return undefined;
      const target = String(args.targetDirectory);
      fs.set(target, [...(fs.get(target) ?? []), file(`${target}/pasted.txt`, 999)]);
      return Promise.resolve(undefined);
    });
    renderExplorer();
    await act(async () => {
      await vi.advanceTimersByTimeAsync(0);
    });

    firePasteEvent(treeScroll(), fileList(["/outside/pasted.txt"]));
    await act(async () => {
      for (let i = 0; i < 6; i += 1) await Promise.resolve();
    });

    expect(inTree("pasted.txt")).toBeInTheDocument();
  });
});

describe("复制路径与预览", () => {
  it("Cmd+Alt+C 复制选中路径并提示", async () => {
    renderExplorer();
    await screen.findByText("README.md");

    fireEvent.click(row("README.md"));
    fireEvent.keyDown(treeScroll(), { key: "c", metaKey: true, altKey: true });

    await waitFor(() => expect(showToast).toHaveBeenCalledWith("Path copied"));
    expect(navigator.clipboard.writeText).toHaveBeenCalledWith("/repo/README.md");
  });

  it("复制失败时提示错误", async () => {
    vi.mocked(navigator.clipboard.writeText).mockRejectedValue(new Error("denied"));
    // 走 execCommand 兜底路径,jsdom 里没有实现,给一个返回 false 的替身让它抛。
    Object.defineProperty(document, "execCommand", {
      configurable: true,
      value: vi.fn().mockReturnValue(false),
    });
    renderExplorer();
    await screen.findByText("README.md");

    fireEvent.click(row("README.md"));
    fireEvent.keyDown(treeScroll(), { key: "c", metaKey: true, altKey: true });

    await waitFor(() =>
      expect(showToast).toHaveBeenCalledWith("Failed to copy path: Copy command was rejected"),
    );
  });

  it("没有选中项时 Cmd+Alt+C 不写剪贴板", async () => {
    renderExplorer();
    await screen.findByText("README.md");

    fireEvent.keyDown(treeScroll(), { key: "c", metaKey: true, altKey: true });
    await act(async () => {});

    expect(navigator.clipboard.writeText).not.toHaveBeenCalled();
    expect(showToast).not.toHaveBeenCalled();
  });

  it("右键菜单的两条复制项分别给裸路径与 @ 路径", async () => {
    renderExplorer();
    await screen.findByText("README.md");

    openContextMenu("README.md");
    fireEvent.click(menuItem("Copy full path"));
    await waitFor(() =>
      expect(navigator.clipboard.writeText).toHaveBeenCalledWith("/repo/README.md"),
    );

    openContextMenu("README.md");
    fireEvent.click(menuItem("Copy @full path"));
    await waitFor(() =>
      expect(navigator.clipboard.writeText).toHaveBeenCalledWith("@/repo/README.md"),
    );
  });

  it("空格键预览:有回调时交给宿主,没有时开内置弹窗", async () => {
    const onPreviewRequest = vi.fn();
    const { unmount } = renderExplorer({ onPreviewRequest });
    await screen.findByText("README.md");

    fireEvent.click(row("README.md"));
    fireEvent.keyDown(treeScroll(), { key: " " });
    await act(async () => {});

    expect(onPreviewRequest).toHaveBeenCalledWith({
      endpoint: { kind: "local", path: "/repo" },
      filePath: "/repo/README.md",
      isDirectory: false,
      connections: [],
    });
    expect(screen.queryByTestId("sftp-preview")).not.toBeInTheDocument();
    unmount();

    renderExplorer();
    await screen.findByText("README.md");
    fireEvent.click(row("README.md"));
    fireEvent.keyDown(treeScroll(), { key: " " });

    const preview = await screen.findByTestId("sftp-preview");
    expect(preview).toHaveAttribute("data-file", "/repo/README.md");
    expect(preview).toHaveAttribute("data-dir", "false");
  });

  it("远程项目预览带上 SSH 端点与连接", async () => {
    fs.set("/srv/app", [file("/srv/app/main.rs", 100)]);
    const onPreviewRequest = vi.fn();
    renderExplorer({
      projectPath: "/srv/app",
      remote: { kind: "ssh", connection, projectPath: "/srv/app" },
      onPreviewRequest,
    });
    await screen.findByText("main.rs");

    fireEvent.click(row("main.rs"));
    fireEvent.keyDown(treeScroll(), { key: " " });
    await act(async () => {});

    expect(onPreviewRequest).toHaveBeenCalledWith({
      endpoint: {
        kind: "ssh",
        connectionId: "ssh-1",
        connectionName: "prod",
        path: "/srv/app",
      },
      filePath: "/srv/app/main.rs",
      isDirectory: false,
      connections: [connection],
    });
  });

  it("焦点在输入框里时空格不触发预览", async () => {
    const onPreviewRequest = vi.fn();
    renderExplorer({ onPreviewRequest });
    await screen.findByText("README.md");

    fireEvent.click(row("README.md"));
    fireEvent.keyDown(treeScroll(), { key: "Enter" });
    const input = await waitFor(() => {
      const found = treeScroll().querySelector("input");
      if (!found) throw new Error("rename input not rendered");
      return found;
    });

    fireEvent.keyDown(input as HTMLElement, { key: " " });
    await act(async () => {});

    // 否则重命名时打一个空格就会弹预览。
    expect(onPreviewRequest).not.toHaveBeenCalled();
  });

  it("远程项目的右键菜单里没有「在系统文件夹中打开」", async () => {
    fs.set("/srv/app", [file("/srv/app/main.rs", 100)]);
    renderExplorer({
      projectPath: "/srv/app",
      remote: { kind: "ssh", connection, projectPath: "/srv/app" },
    });
    await screen.findByText("main.rs");

    openContextMenu("main.rs");
    // 菜单确实开着(有 New File),只是不该出现远程环境下没意义的这一项。
    expect(menuItem("New File")).toBeInTheDocument();
    expect(queryMenuItem("Open in System Folder")).not.toBeInTheDocument();
  });

  it("本地项目可以在系统文件夹里打开,失败时提示", async () => {
    setRouter((command) => {
      if (command !== "open_in_system_file_manager") return undefined;
      return Promise.reject(new Error("no handler"));
    });
    renderExplorer();
    await screen.findByText("README.md");

    openContextMenu("README.md");
    fireEvent.click(menuItem("Open in System Folder"));

    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("open_in_system_file_manager", {
        path: "/repo/README.md",
        projectPath: "/repo",
      }),
    );
    await waitFor(() =>
      expect(showToast).toHaveBeenCalledWith("Failed to open in system folder: no handler"),
    );
  });
});

describe("生命周期与刷新", () => {
  it("切换项目时清空旧树,不残留上一个项目的文件", async () => {
    fs.set("/other-project", [file("/other-project/only-here.ts", 100)]);
    const { rerender } = renderExplorer();
    await screen.findByText("README.md");

    rerender(
      <I18nProvider>
        <FileExplorer
          projectPath="/other-project"
          projectName="other"
          onFileSelect={vi.fn()}
          themeVariant="light"
        />
      </I18nProvider>,
    );

    await waitFor(() => expect(inTree("only-here.ts")).toBeInTheDocument());
    notInTree("README.md");
  });

  it("切换项目时旧项目的选中项也一起清掉,不留跨项目的悬空路径", async () => {
    /*
     * 与上一条互补:上一条断言的树内容,换项目后的 `refresh()` 自己就能刷对,所以
     * 「切项目不清状态」这个变异照样全绿(实测)。选中项不一样 —— 它会一路留到新项目里,
     * 而复制路径直接读它,于是这里能拿到旧项目的文件路径。
     */
    fs.set("/other-project", [file("/other-project/only-here.ts", 100)]);
    const { rerender } = renderExplorer();
    await screen.findByText("README.md");
    fireEvent.click(row("README.md"));

    rerender(
      <I18nProvider>
        <FileExplorer
          projectPath="/other-project"
          projectName="other"
          onFileSelect={vi.fn()}
          themeVariant="light"
        />
      </I18nProvider>,
    );
    await waitFor(() => expect(inTree("only-here.ts")).toBeInTheDocument());

    fireEvent.keyDown(treeScroll(), { key: "c", metaKey: true, altKey: true });
    await act(async () => {});
    expect(navigator.clipboard.writeText).not.toHaveBeenCalled();
  });

  it("active=false 时不读目录", async () => {
    renderExplorer({ active: false });
    await act(async () => {});

    expect(vi.mocked(invoke).mock.calls.some(([c]) => c === "read_dir_entries")).toBe(false);
    // 变成 active 后才开始读。
    expect(screen.getByText("Loading...")).toBeInTheDocument();
  });

  it("active=false 时连轮询定时器都不装", async () => {
    /*
     * 上一条只看了「首读没发生」,那是另一个 effect 的 `if (!active) return`。
     * 轮询是**独立的一段守卫**:摘掉它,面板在后台也会每 2500ms 打一次后端
     * (变异测试实测,不推时钟看不出来)。所以这里必须真的把时钟推过去。
     */
    vi.useFakeTimers();
    renderExplorer({ active: false });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(2_500 * 3);
    });

    expect(vi.mocked(invoke).mock.calls.some(([c]) => c === "read_dir_entries")).toBe(false);
  });

  it("从 inactive 变 active 会补上一次读取", async () => {
    const { rerender } = renderExplorer({ active: false });
    await act(async () => {});
    expect(vi.mocked(invoke).mock.calls.some(([c]) => c === "read_dir_entries")).toBe(false);

    rerender(
      <I18nProvider>
        <FileExplorer
          projectPath="/repo"
          projectName="repo"
          onFileSelect={vi.fn()}
          themeVariant="light"
          active
        />
      </I18nProvider>,
    );

    await waitFor(() => expect(inTree("README.md")).toBeInTheDocument());
  });

  it("页面可见时按 2500ms 自动刷新,隐藏时停下", async () => {
    vi.useFakeTimers();
    renderExplorer();
    await act(async () => {
      await vi.advanceTimersByTimeAsync(0);
    });
    const baseline = vi.mocked(invoke).mock.calls.filter(([c]) => c === "read_dir_entries").length;

    await act(async () => {
      await vi.advanceTimersByTimeAsync(2_500);
    });
    const afterVisible = vi
      .mocked(invoke)
      .mock.calls.filter(([c]) => c === "read_dir_entries").length;
    // 先证明这个时钟确实能推动轮询,否则下面的「没增加」是空断言。
    expect(afterVisible).toBeGreaterThan(baseline);

    const visibility = vi.spyOn(document, "visibilityState", "get").mockReturnValue("hidden");
    await act(async () => {
      await vi.advanceTimersByTimeAsync(2_500 * 3);
    });
    expect(vi.mocked(invoke).mock.calls.filter(([c]) => c === "read_dir_entries")).toHaveLength(
      afterVisible,
    );
    visibility.mockRestore();
  });

  it("卸载后定时器不再打后端", async () => {
    vi.useFakeTimers();
    const { unmount } = renderExplorer();
    await act(async () => {
      await vi.advanceTimersByTimeAsync(0);
    });
    unmount();
    const afterUnmount = vi
      .mocked(invoke)
      .mock.calls.filter(([c]) => c === "read_dir_entries").length;

    await act(async () => {
      await vi.advanceTimersByTimeAsync(2_500 * 4);
    });

    expect(vi.mocked(invoke).mock.calls.filter(([c]) => c === "read_dir_entries")).toHaveLength(
      afterUnmount,
    );
  });

  it("窗口重新获得焦点时立刻刷新一次", async () => {
    renderExplorer();
    await screen.findByText("README.md");
    const baseline = vi.mocked(invoke).mock.calls.filter(([c]) => c === "read_dir_entries").length;

    await act(async () => {
      window.dispatchEvent(new Event("focus"));
      await Promise.resolve();
    });

    await waitFor(() =>
      expect(
        vi.mocked(invoke).mock.calls.filter(([c]) => c === "read_dir_entries").length,
      ).toBeGreaterThan(baseline),
    );
  });

  it("手动刷新按钮会重读目录", async () => {
    renderExplorer();
    await screen.findByText("README.md");
    const baseline = vi.mocked(invoke).mock.calls.filter(([c]) => c === "read_dir_entries").length;

    fireEvent.click(screen.getByTitle("Refresh"));

    await waitFor(() =>
      expect(
        vi.mocked(invoke).mock.calls.filter(([c]) => c === "read_dir_entries").length,
      ).toBeGreaterThan(baseline),
    );
  });

  it("读目录失败时显示错误条", async () => {
    fs.delete("/repo");
    renderExplorer();

    const error = await screen.findByTestId("file-explorer-error");
    expect(error).toHaveTextContent("ENOENT: /repo");
  });

  it("读目录恢复后错误条消失", async () => {
    fs.delete("/repo");
    renderExplorer();
    await screen.findByTestId("file-explorer-error");

    resetFs();
    fireEvent.click(screen.getByTitle("Refresh"));

    await waitFor(() =>
      expect(screen.queryByTestId("file-explorer-error")).not.toBeInTheDocument(),
    );
    expect(inTree("README.md")).toBeInTheDocument();
  });
});

describe("搜索", () => {
  /**
   * 打开搜索框并返回那个 input。
   *
   * 搜索按钮与搜索框的 aria-label 都是 "Search files",开着的时候 `getByLabelText`
   * 会同时命中两个 —— 所以按钮用 title 定位,输入框用 role 定位。
   */
  function openSearch(): HTMLElement {
    fireEvent.click(screen.getByTitle("Search files"));
    return screen.getByRole("textbox");
  }

  it("查询词带大写也能匹配(大小写不敏感)", async () => {
    /*
     * 用**带大写**的查询词是必须的:`node.name.toLowerCase().includes(query)` 里
     * 名字已经小写了,所以拿小写词去搜,少一次 `query.toLowerCase()` 照样命中 ——
     * 那是个空断言(变异测试实测:摘掉 query 的 toLowerCase 全绿)。
     */
    renderExplorer();
    await screen.findByText("README.md");

    fireEvent.change(openSearch(), { target: { value: "READ" } });

    await waitFor(() => notInTree("src-extra"));
    expect(inTree("README.md")).toBeInTheDocument();
  });

  it("按名字过滤,清空后恢复全部", async () => {
    renderExplorer();
    await screen.findByText("README.md");

    const search = openSearch();
    fireEvent.change(search, { target: { value: "readme" } });

    await waitFor(() => notInTree("src-extra"));
    expect(inTree("README.md")).toBeInTheDocument();

    fireEvent.change(search, { target: { value: "" } });
    await waitFor(() => expect(inTree("src-extra")).toBeInTheDocument());
  });

  it("没有命中时显示空结果文案,而不是「目录为空」", async () => {
    renderExplorer();
    await screen.findByText("README.md");

    fireEvent.change(openSearch(), { target: { value: "zzz-nothing" } });

    expect(await screen.findByText("No matching files")).toBeInTheDocument();
    // 「目录为空」和「搜不到」是两回事,混用会让人以为文件被删了。
    expect(screen.queryByText("Empty directory")).not.toBeInTheDocument();
  });

  it("关闭搜索会清掉查询词", async () => {
    renderExplorer();
    await screen.findByText("README.md");

    fireEvent.change(openSearch(), { target: { value: "readme" } });
    await waitFor(() => notInTree("src-extra"));

    fireEvent.click(screen.getByRole("button", { name: "Close" }));

    await waitFor(() => expect(inTree("src-extra")).toBeInTheDocument());
    expect(screen.queryByRole("textbox")).not.toBeInTheDocument();
  });

  it("空目录显示空态", async () => {
    fs.set("/repo", []);
    renderExplorer();

    expect(await screen.findByText("Empty directory")).toBeInTheDocument();
  });
});

describe("展开 / 选中 / 打开", () => {
  it("第一次点目录是「选中并展开」,再点一次才收起", async () => {
    renderExplorer();
    await screen.findByText("src");

    fireEvent.click(row("src"));
    expect(await screen.findByText("a.ts")).toBeInTheDocument();
    expect(isSelected("src")).toBe(true);

    // 已选中状态下再点 = 只收起,不取消选中。
    fireEvent.click(row("src"));
    await waitFor(() => notInTree("a.ts"));
    expect(isSelected("src")).toBe(true);
  });

  it("点箭头只展开,不改选中项", async () => {
    renderExplorer();
    await screen.findByText("src");

    fireEvent.click(row("README.md"));
    expect(isSelected("README.md")).toBe(true);

    const chevron = row("src").firstElementChild as HTMLElement;
    fireEvent.click(chevron);

    expect(await screen.findByText("a.ts")).toBeInTheDocument();
    expect(isSelected("README.md")).toBe(true);
    expect(isSelected("src")).toBe(false);
  });

  it("文件行的箭头位是死区,点它等于点整行", async () => {
    const { onFileSelect } = renderExplorer();
    await screen.findByText("README.md");

    const chevron = row("README.md").firstElementChild as HTMLElement;
    fireEvent.click(chevron);
    await act(async () => {});

    // 文件行上的 chevron handler 是 `if (!node.is_dir) return;` —— **没有** stopPropagation,
    // 所以点击继续冒泡到行本身,行为等同点整行(选中 + 打开)。
    // 这是有意的:那一格对文件来说只是占位缩进,不该是个点不动的死角。
    expect(isSelected("README.md")).toBe(true);
    expect(onFileSelect).toHaveBeenCalledWith("/repo/README.md", "README.md");
  });

  it("双击文件打开它,双击目录只切展开", async () => {
    const { onFileSelect } = renderExplorer();
    await screen.findByText("README.md");

    fireEvent.doubleClick(row("README.md"));
    expect(onFileSelect).toHaveBeenCalledWith("/repo/README.md", "README.md");

    onFileSelect.mockClear();
    fireEvent.doubleClick(row("src"));
    expect(await screen.findByText("a.ts")).toBeInTheDocument();
    expect(onFileSelect).not.toHaveBeenCalled();
  });

  it("双击 sqlite 文件交给数据库工作区,不走文本打开", async () => {
    fs.set("/repo", [file("/repo/app.db", 100)]);
    const onOpenDatabaseFile = vi.fn();
    const { onFileSelect } = renderExplorer({ onOpenDatabaseFile });
    await screen.findByText("app.db");

    // 单击那条路径已由 file-explorer-ui.test.tsx 覆盖,这里补双击(handleOpen)那一支。
    fireEvent.doubleClick(row("app.db"));

    expect(onOpenDatabaseFile).toHaveBeenCalledWith("/repo/app.db", "app.db");
    // 用文本编辑器打开 sqlite 会显示二进制乱码,这条 return 是必须的。
    expect(onFileSelect).not.toHaveBeenCalled();
  });

  it("没有数据库回调时 sqlite 文件退回文本打开", async () => {
    fs.set("/repo", [file("/repo/app.db", 100)]);
    const { onFileSelect } = renderExplorer();
    await screen.findByText("app.db");

    fireEvent.doubleClick(row("app.db"));

    expect(onFileSelect).toHaveBeenCalledWith("/repo/app.db", "app.db");
  });

  it("目录读取失败时展开会显示错误条", async () => {
    renderExplorer();
    await screen.findByText("src");

    fs.delete("/repo/src");
    fireEvent.click(row("src"));

    const error = await screen.findByTestId("file-explorer-error");
    expect(error).toHaveTextContent("ENOENT: /repo/src");
  });

  it("排序偏好从项目配置里读,并按它排列", async () => {
    setRouter((command) => {
      if (command !== "read_project_config") return undefined;
      return Promise.resolve({
        editor: { file_browser_sort: { field: "name", direction: "asc" } },
      });
    });
    renderExplorer();
    await screen.findByText("README.md");

    await waitFor(() =>
      expect(screen.getByRole("button", { name: /Name ascending/ })).toBeInTheDocument(),
    );
  });

  it("配置里的排序偏好非法时回落到「修改时间倒序」", async () => {
    setRouter((command) => {
      if (command !== "read_project_config") return undefined;
      return Promise.resolve({ editor: { file_browser_sort: { field: "bogus" } } });
    });
    renderExplorer();
    await screen.findByText("README.md");

    expect(screen.getByRole("button", { name: /Modified descending/ })).toBeInTheDocument();
  });

  it("展开的目录在切换排序时连子层一起重排", async () => {
    renderExplorer();
    await screen.findByText("src");

    fireEvent.click(row("src"));
    await screen.findByText("a.ts");

    const labelsBefore = Array.from(treeScroll().querySelectorAll("span"))
      .map((el) => el.textContent)
      .filter((text) => text === "a.ts" || text === "b.ts");
    expect(labelsBefore).toEqual(["a.ts", "b.ts"]);

    // 排序按钮的 aria-label 只在该字段激活时才带方向词:先点一次切到「名字升序」,
    // 再点同一个按钮翻成降序。
    fireEvent.click(screen.getByRole("button", { name: "Name" }));
    fireEvent.click(screen.getByRole("button", { name: "Name ascending" }));

    await waitFor(() => {
      const labelsAfter = Array.from(treeScroll().querySelectorAll("span"))
        .map((el) => el.textContent)
        .filter((text) => text === "a.ts" || text === "b.ts");
      expect(labelsAfter).toEqual(["b.ts", "a.ts"]);
    });
  });

  it("读目录失败时切换排序,已加载的树仍然就地重排(含展开的子层)", async () => {
    /*
     * 上一条断言不到 `sortTreeNodes` 本身:换排序同时会触发 `refresh()`,而 refresh 重读时
     * 每一层都会自己 `sortFileEntries` 一遍 —— 顺序对不对全归它,「递归那行删掉」的变异
     * 照样全绿(变异测试实测)。
     *
     * 读失败是唯一让 `sortTreeNodes` 独自负责的窗口:refresh 抛错后只写 `loadError`,
     * 一个节点都不动(FileExplorer.tsx:308-312),已加载的树原样留在屏幕上。这时候
     * 顺序只可能来自那个排序 effect,连子层也一样。
     */
    renderExplorer();
    await screen.findByText("src");
    fireEvent.click(row("src"));
    await screen.findByText("a.ts");

    // 之后所有读目录都失败:refresh 再也无法代替排序 effect 把顺序摆对。
    setRouter((command) => {
      if (command === "read_dir_entries") throw new Error("EIO");
      return undefined;
    });

    fireEvent.click(screen.getByRole("button", { name: "Name" }));
    fireEvent.click(screen.getByRole("button", { name: "Name ascending" }));

    await waitFor(() => {
      const labels = Array.from(treeScroll().querySelectorAll("span"))
        .map((el) => el.textContent)
        .filter((text) => text && ["src", "src-extra", "a.ts", "b.ts"].includes(text));
      expect(labels).toEqual(["src-extra", "src", "b.ts", "a.ts"]);
    });
  });

  it("面包屑点当前根之内的祖先目录时原地展开,不换根", async () => {
    renderExplorer();
    await screen.findByText("src");

    fireEvent.click(row("src"));
    const child = await screen.findByText("a.ts");
    fireEvent.click(child.parentElement!);
    expect(screen.getByRole("button", { name: "/repo/src" })).toHaveAttribute(
      "aria-current",
      "location",
    );

    const readsBefore = vi
      .mocked(invoke)
      .mock.calls.filter(
        ([c, a]) => c === "read_dir_entries" && (a as { path?: string }).path === "/",
      ).length;

    fireEvent.click(screen.getByRole("button", { name: "/repo/src" }));
    await act(async () => {});

    // 根之内的跳转不该触发换根(否则已加载的整棵树会被丢掉重读)。
    const readsAfter = vi
      .mocked(invoke)
      .mock.calls.filter(
        ([c, a]) => c === "read_dir_entries" && (a as { path?: string }).path === "/",
      ).length;
    expect(readsAfter).toBe(readsBefore);
    expect(inTree("a.ts")).toBeInTheDocument();
  });

  it("跳到根之外时丢掉旧树与旧选中项,不带着上一层的状态换根", async () => {
    /*
     * 换根前那三行(清 `nodesRef` / `setNodes([])` / `setSelectedPath(null)`)不能只靠
     * 「新的兄弟目录出现了」来断言:换根后的 `refresh()` 会整棵重建,所以**不清**也一样
     * 能刷出正确的树(变异测试实测,整段删掉全绿)。真正留下痕迹的是选中项 ——
     * 它不清就成了指向旧根内部的悬空路径,而复制路径这个出口直接读它。
     */
    renderExplorer();
    await screen.findByText("README.md");
    fireEvent.click(row("README.md"));

    fireEvent.click(screen.getByRole("button", { name: "/" }));
    await waitFor(() => expect(inTree("other")).toBeInTheDocument());

    fireEvent.keyDown(treeScroll(), { key: "c", metaKey: true, altKey: true });
    await act(async () => {});
    expect(navigator.clipboard.writeText).not.toHaveBeenCalled();
  });

  it("远程项目不允许跳到 projectPath 之外", async () => {
    fs.set("/srv/app", [file("/srv/app/main.rs", 100)]);
    renderExplorer({
      projectPath: "/srv/app",
      remote: { kind: "ssh", connection, projectPath: "/srv/app" },
    });
    await screen.findByText("main.rs");

    // "/" 与 "/srv" 都在项目根之外,必须是 disabled。
    expect(screen.getByRole("button", { name: "/" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "/srv" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "/srv/app" })).toBeEnabled();
  });

  it("点面包屑回到根会清掉选中项", async () => {
    renderExplorer();
    await screen.findByText("src");

    fireEvent.click(row("README.md"));
    expect(isSelected("README.md")).toBe(true);

    fireEvent.click(screen.getByRole("button", { name: "/repo" }));
    await act(async () => {});

    expect(isSelected("README.md")).toBe(false);
  });

  it("内置预览弹窗点遮罩关闭", async () => {
    renderExplorer();
    await screen.findByText("README.md");

    fireEvent.click(row("README.md"));
    fireEvent.keyDown(treeScroll(), { key: " " });
    await screen.findByTestId("sftp-preview");

    const overlay = document.querySelector(".sftp-preview-overlay") as HTMLElement;
    fireEvent.mouseDown(overlay, { target: overlay });

    await waitFor(() => expect(screen.queryByTestId("sftp-preview")).not.toBeInTheDocument());
  });

  it("右键菜单点遮罩关闭,右键树行外的空白不误开根菜单", async () => {
    renderExplorer();
    await screen.findByText("README.md");

    openContextMenu("README.md");
    expect(menuItem("Delete")).toBeInTheDocument();

    // 菜单背板是第一个绝对定位覆盖层。
    const backdrop = document.querySelector("div[style*='position: fixed']") as HTMLElement;
    fireEvent.pointerDown(backdrop);
    await waitFor(() => expect(queryMenuItem("Delete")).not.toBeInTheDocument());

    // 在树行上右键时 target !== currentTarget,`handleEmptyContextMenu` 必须早退,
    // 否则行菜单会被根菜单顶掉(根菜单没有 Delete)。
    openContextMenu("README.md");
    expect(menuItem("Delete")).toBeInTheDocument();
  });

  it("未识别的按键不阻止默认行为", async () => {
    renderExplorer();
    await screen.findByText("README.md");

    const event = new KeyboardEvent("keydown", { key: "a", bubbles: true, cancelable: true });
    treeScroll().dispatchEvent(event);

    expect(event.defaultPrevented).toBe(false);
  });

  it("卸载后落地的删除结果不再写状态", async () => {
    let resolveDelete: (() => void) | undefined;
    vi.mocked(confirm).mockResolvedValue(true);
    setRouter((command) => {
      if (command !== "delete_path") return undefined;
      return new Promise<undefined>((resolve) => {
        resolveDelete = () => resolve(undefined);
      });
    });
    const { unmount } = renderExplorer();
    await screen.findByText("README.md");

    openContextMenu("README.md");
    fireEvent.click(menuItem("Delete"));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("delete_path", expect.anything()));

    unmount();
    const readsAfterUnmount = vi
      .mocked(invoke)
      .mock.calls.filter(([c]) => c === "read_dir_entries").length;

    resolveDelete?.();
    await act(async () => {});

    // `isCancelled()` 之后不该再走 refresh —— 否则就是往已卸载组件里 setState。
    expect(vi.mocked(invoke).mock.calls.filter(([c]) => c === "read_dir_entries")).toHaveLength(
      readsAfterUnmount,
    );
  });

  it("卸载后落地的失败结果不再弹提示", async () => {
    let rejectDelete: (() => void) | undefined;
    vi.mocked(confirm).mockResolvedValue(true);
    setRouter((command) => {
      if (command !== "delete_path") return undefined;
      return new Promise<undefined>((_, reject) => {
        rejectDelete = () => reject(new Error("EPERM"));
      });
    });
    const { unmount } = renderExplorer();
    await screen.findByText("README.md");

    openContextMenu("README.md");
    fireEvent.click(menuItem("Delete"));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("delete_path", expect.anything()));

    unmount();
    rejectDelete?.();
    await act(async () => {});

    expect(showToast).not.toHaveBeenCalled();
  });
});

describe("虚拟滚动与视口", () => {
  it("只渲染可视窗口内的行,滚动后窗口跟着移动", async () => {
    fs.set("/repo", longDirectory(80));
    renderExplorer();
    await screen.findByText("f000.ts");

    // 600px / 22px ≈ 27 行,加 5 行 overscan —— 第 60 行必须还没渲染。
    notInTree("f060.ts");

    const scroll = treeScroll();
    Object.defineProperty(scroll, "scrollTop", { configurable: true, value: 1_100 });
    fireEvent.scroll(scroll);

    await waitFor(() => expect(inTree("f060.ts")).toBeInTheDocument());
    // 窗口移走之后开头那些行应该被卸载,否则虚拟化等于没做。
    notInTree("f000.ts");
  });

  it("容器尺寸变化时重算可视高度", async () => {
    fs.set("/repo", longDirectory(80));
    renderExplorer();
    await screen.findByText("f000.ts");
    expect(resizeCallbacks).toHaveLength(1);

    notInTree("f060.ts");
    // 把视口撑到能装下全部 80 行(80 * 22 = 1760),再让 ResizeObserver 通知一次。
    stubViewportHeight(2_000);
    await act(async () => {
      resizeCallbacks[0]([], {} as ResizeObserver);
    });

    expect(inTree("f060.ts")).toBeInTheDocument();
  });

  it("新建行在视口外时把它滚进来", async () => {
    fs.set("/repo", longDirectory(80));
    renderExplorer();
    await screen.findByText("f000.ts");

    const scroll = treeScroll();
    const scrollTo = vi.fn();
    Object.defineProperty(scroll, "scrollTo", { configurable: true, value: scrollTo });
    Object.defineProperty(scroll, "scrollTop", { configurable: true, value: 1_400 });
    fireEvent.scroll(scroll);
    await act(async () => {});

    // 根目录的新建行永远在第 0 行,此刻远在视口上方。
    fireEvent.contextMenu(scroll, { clientX: 5, clientY: 5, target: scroll });
    fireEvent.click(menuItem("New File"));

    await waitFor(() => expect(scrollTo).toHaveBeenCalledWith({ top: 0, behavior: "auto" }));
  });

  it("重命名行在视口外时把它滚进来", async () => {
    fs.set("/repo", longDirectory(80));
    renderExplorer();
    await screen.findByText("f000.ts");

    const scroll = treeScroll();
    fireEvent.click(row("f000.ts"));

    const scrollTo = vi.fn();
    Object.defineProperty(scroll, "scrollTo", { configurable: true, value: scrollTo });
    Object.defineProperty(scroll, "scrollTop", { configurable: true, value: 1_400 });
    fireEvent.scroll(scroll);
    await act(async () => {});

    fireEvent.keyDown(scroll, { key: "Enter" });

    await waitFor(() => expect(scrollTo).toHaveBeenCalledWith({ top: 0, behavior: "auto" }));
  });

  it("面包屑跳转会把视口外的目标行滚进来", async () => {
    /*
     * 目标行必须落在**列表中段**:`sortFileEntries` 永远把目录排在文件之前,
     * 所以只放一个目录时它总是第 0 行 —— 那条路径会走 `revealDirectoryPath === browseRoot`
     * 的 scrollTo(0) 分支,盖不到「按行号居中」那一段。这里铺 40 个目录,取第 35 个。
     */
    // 默认排序是「修改时间倒序」,所以 mtime 要递减,d000 才落在第 0 行、d035 落在第 35 行。
    const dirs = Array.from({ length: 40 }, (_, i) =>
      dir(`/repo/d${String(i).padStart(3, "0")}`, 1_000 - i),
    );
    fs.set("/repo", dirs);
    for (const entry of dirs) fs.set(entry.path, [file(`${entry.path}/leaf.ts`, 1)]);
    renderExplorer();
    await screen.findByText("d000");

    const scroll = treeScroll();
    // 先滚到能看见 d035 的位置,展开它并选中里面的文件(这样面包屑里才有 /repo/d035)。
    Object.defineProperty(scroll, "scrollTop", { configurable: true, value: 600 });
    fireEvent.scroll(scroll);
    await waitFor(() => expect(inTree("d035")).toBeInTheDocument());
    fireEvent.click(row("d035"));
    const leaf = await screen.findByText("leaf.ts");
    fireEvent.click(leaf.parentElement!);
    expect(screen.getByRole("button", { name: "/repo/d035" })).toBeInTheDocument();

    // 回到顶部,此时第 35 行(top = 770)在视口 600 之外。
    const scrollTo = vi.fn();
    Object.defineProperty(scroll, "scrollTo", { configurable: true, value: scrollTo });
    Object.defineProperty(scroll, "scrollTop", { configurable: true, value: 0 });
    fireEvent.scroll(scroll);
    fireEvent.click(screen.getByRole("button", { name: "/repo/d035" }));

    await waitFor(() => expect(scrollTo).toHaveBeenCalled());
    // 居中:rowTop(770) - clientHeight/2(300) + ROW_HEIGHT(22) = 492。
    expect(scrollTo).toHaveBeenCalledWith({ top: 492, behavior: "smooth" });
  });
});

describe("预览弹窗", () => {
  async function openPreview(name: string) {
    fireEvent.click(row(name));
    fireEvent.keyDown(treeScroll(), { key: " " });
    return screen.findByTestId("sftp-preview");
  }

  it("预览面板自己的关闭按钮能关掉", async () => {
    renderExplorer();
    await screen.findByText("README.md");
    await openPreview("README.md");

    fireEvent.click(screen.getByRole("button", { name: "close-preview" }));

    await waitFor(() => expect(screen.queryByTestId("sftp-preview")).not.toBeInTheDocument());
  });

  it("点弹窗内部不会关掉它", async () => {
    renderExplorer();
    await screen.findByText("README.md");
    const preview = await openPreview("README.md");

    const overlay = document.querySelector(".sftp-preview-overlay") as HTMLElement;
    fireEvent.mouseDown(preview);

    await act(async () => {});
    // 遮罩的 onMouseDown 只在 target === currentTarget 时关闭;少了这个判断,
    // 在预览里选文字都会把窗口关掉。
    expect(overlay).toBeInTheDocument();
    expect(screen.getByTestId("sftp-preview")).toBeInTheDocument();
  });

  it("目录预览用 compact 布局", async () => {
    renderExplorer();
    await screen.findByText("src");
    const preview = await openPreview("src");

    expect(preview).toHaveAttribute("data-dir", "true");
    expect(document.querySelector(".sftp-preview-dialog")?.className).toContain("compact");
  });
});

describe("零散分支", () => {
  it("右键菜单里复制路径失败时只记日志,不弹提示", async () => {
    const consoleError = vi.spyOn(console, "error").mockImplementation(() => {});
    vi.mocked(navigator.clipboard.writeText).mockRejectedValue(new Error("denied"));
    Object.defineProperty(document, "execCommand", {
      configurable: true,
      value: vi.fn().mockReturnValue(false),
    });
    renderExplorer();
    await screen.findByText("README.md");

    openContextMenu("README.md");
    fireEvent.click(menuItem("Copy full path"));

    // 菜单照样关掉(finally 里做的),失败只写 console。
    await waitFor(() => expect(queryMenuItem("Copy full path")).not.toBeInTheDocument());
    expect(consoleError).toHaveBeenCalled();
    expect(showToast).not.toHaveBeenCalled();
    consoleError.mockRestore();
  });

  it("标签页切回前台(visibilitychange)会刷新", async () => {
    renderExplorer();
    await screen.findByText("README.md");
    const baseline = vi.mocked(invoke).mock.calls.filter(([c]) => c === "read_dir_entries").length;

    await act(async () => {
      document.dispatchEvent(new Event("visibilitychange"));
      await Promise.resolve();
    });

    await waitFor(() =>
      expect(
        vi.mocked(invoke).mock.calls.filter(([c]) => c === "read_dir_entries").length,
      ).toBeGreaterThan(baseline),
    );
  });

  it("隐藏状态下的 visibilitychange 不刷新", async () => {
    renderExplorer();
    await screen.findByText("README.md");
    const baseline = vi.mocked(invoke).mock.calls.filter(([c]) => c === "read_dir_entries").length;

    const visibility = vi.spyOn(document, "visibilityState", "get").mockReturnValue("hidden");
    await act(async () => {
      document.dispatchEvent(new Event("visibilitychange"));
      await Promise.resolve();
    });

    expect(vi.mocked(invoke).mock.calls.filter(([c]) => c === "read_dir_entries")).toHaveLength(
      baseline,
    );
    visibility.mockRestore();
  });

  it("配置还没回来就卸载时不再 setState", async () => {
    let resolveConfig: ((value: unknown) => void) | undefined;
    setRouter((command) => {
      if (command !== "read_project_config") return undefined;
      return new Promise((resolve) => {
        resolveConfig = resolve;
      });
    });
    const { unmount } = renderExplorer();
    await screen.findByText("README.md");

    unmount();
    // effect 的 cancelled 闸门:晚到的配置不能再改排序状态。
    await act(async () => {
      resolveConfig?.({ editor: { file_browser_sort: { field: "name", direction: "asc" } } });
    });
  });

  it("选中项在外部被删掉后,面包屑自动退回项目根", async () => {
    renderExplorer();
    await screen.findByText("src");
    fireEvent.click(row("src"));
    const child = await screen.findByText("a.ts");
    fireEvent.click(child.parentElement!);
    expect(screen.getByRole("button", { name: "/repo/src" })).toHaveAttribute(
      "aria-current",
      "location",
    );

    // 别的进程把 src 删了(选中项本身没被组件清掉,它只在自己发起的删除里清)。
    fs.set("/repo", [file("/repo/README.md", 300)]);
    fs.delete("/repo/src");
    fireEvent.click(screen.getByTitle("Refresh"));
    await waitFor(() => notInTree("src"));

    /*
     * `currentDirectoryPath` 是从 `findNode(nodes, selectedPath)` 派生的,节点没了就
     * 回落到 browseRoot —— 所以面包屑不会留下一个指向已删目录的死按钮。
     * 这也说明 `handleBreadcrumbNavigate` 里 `if (!node?.is_dir) return` 那条守卫
     * 走不到:面包屑上能点到的段,必然还在树里(理由见文件末尾的不可达清单)。
     */
    expect(screen.queryByRole("button", { name: "/repo/src" })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "/repo" })).toHaveAttribute(
      "aria-current",
      "location",
    );
  });

  it("切回「修改时间」排序时默认倒序,切到「名字」时默认升序", async () => {
    renderExplorer();
    await screen.findByText("README.md");

    fireEvent.click(screen.getByRole("button", { name: "Name" }));
    expect(screen.getByRole("button", { name: "Name ascending" })).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Modified" }));
    // 文件列表默认「最近改的在上面」,所以换回时间字段必须是 desc 而不是沿用 asc。
    expect(screen.getByRole("button", { name: "Modified descending" })).toBeInTheDocument();

    // 每次换排序都会带出一次 refresh。不排掉它,结果会落在用例结束之后 → act 警告。
    await act(async () => {});
  });

  it("搜索命中已加载的深层文件时自动展开它的父目录", async () => {
    renderExplorer();
    await screen.findByText("src");

    // 先展开再收起,让 src 的 children 留在内存里 ——
    // `filterTreeNodesByName` 只过滤**已加载**的子树,没展开过的目录它看不见,
    // 所以搜索不是全局索引,而是「在已经读过的范围内筛」。
    fireEvent.click(row("src"));
    await screen.findByText("a.ts");
    fireEvent.click(row("src"));
    await waitFor(() => notInTree("a.ts"));

    fireEvent.click(screen.getByTitle("Search files"));
    fireEvent.change(screen.getByRole("textbox"), { target: { value: "a.ts" } });

    // 命中在子层时父目录得自动张开,否则用户看到一个空面板。
    await waitFor(() => expect(inTree("a.ts")).toBeInTheDocument());
    expect(inTree("src")).toBeInTheDocument();
    notInTree("README.md");
  });

  it("没有分隔符的条目路径也能取出名字", async () => {
    // 后端理论上可以回相对名(比如根就是当前目录时),`lastIndexOf` 两个都返回 -1。
    fs.set("/repo", [
      { name: "weird", path: "weird", is_dir: false, modifiedAtMs: 1, is_gitignored: false },
    ]);
    vi.mocked(confirm).mockResolvedValue(false);
    renderExplorer();
    await screen.findByText("weird");

    openContextMenu("weird");
    fireEvent.click(menuItem("Delete"));

    await waitFor(() => expect(confirm).toHaveBeenCalledTimes(1));
    // 取不到分隔符时整个路径就是名字,不能变成空串(确认框会变成 "Delete ?")。
    expect(vi.mocked(confirm).mock.calls[0][1]).toMatchObject({ title: "Delete weird?" });
  });

  it("没有选中项时空格不弹预览", async () => {
    const onPreviewRequest = vi.fn();
    renderExplorer({ onPreviewRequest });
    await screen.findByText("README.md");

    fireEvent.keyDown(treeScroll(), { key: " " });
    await act(async () => {});

    expect(onPreviewRequest).not.toHaveBeenCalled();
    expect(screen.queryByTestId("sftp-preview")).not.toBeInTheDocument();
  });

  it("远程项目用内置预览时端点是 ssh", async () => {
    fs.set("/srv/app", [file("/srv/app/main.rs", 100)]);
    renderExplorer({
      projectPath: "/srv/app",
      remote: { kind: "ssh", connection, projectPath: "/srv/app" },
    });
    await screen.findByText("main.rs");

    fireEvent.click(row("main.rs"));
    fireEvent.keyDown(treeScroll(), { key: " " });

    const preview = await screen.findByTestId("sftp-preview");
    expect(preview).toHaveAttribute("data-file", "/srv/app/main.rs");
  });

  it("对着还没加载过的目录新建时,输入行立刻就在,不用等目录读完", async () => {
    let resolveChildren: ((entries: Entry[]) => void) | undefined;
    setRouter((command, args) => {
      if (command !== "read_dir_entries" || args.path !== "/repo/src") return undefined;
      return new Promise<Entry[]>((resolve) => {
        resolveChildren = resolve;
      });
    });
    renderExplorer();
    await screen.findByText("src");

    openContextMenu("src");
    fireEvent.click(menuItem("New File"));
    await act(async () => {});

    /*
     * `handleToggle` 展开时把 children 从 null 兜成 `[]`,而 `flattenVisible` 判的是
     * `n.children` 真值 —— 空数组为真,所以输入行不等读完就渲染出来了。
     * 这正是想要的:点「新建」立刻能打字,不必等远程目录的往返。
     */
    expect(treeScroll().querySelectorAll("input")).toHaveLength(1);

    // 目录读完之后输入行还在(不会被刷新顶掉)。
    await act(async () => {
      resolveChildren?.([file("/repo/src/a.ts", 200)]);
    });
    expect(treeScroll().querySelectorAll("input")).toHaveLength(1);
  });

  it("展开过程中的旧刷新结果不会把刚展开的目录收回去", async () => {
    /*
     * 组件注释里的 issue #194:自动刷新拿的是展开**之前**的快照,落地晚于展开时
     * 会把整棵树覆盖回去。`handleToggle` 先给 `refreshIdRef` +1 让那次刷新作废。
     */
    let resolveSlowRead: ((entries: Entry[]) => void) | undefined;
    let rootReads = 0;
    setRouter((command, args) => {
      if (command !== "read_dir_entries" || args.path !== "/repo") return undefined;
      rootReads += 1;
      if (rootReads === 2) {
        return new Promise<Entry[]>((resolve) => {
          resolveSlowRead = resolve;
        });
      }
      return Promise.resolve(fs.get("/repo") ?? []);
    });
    renderExplorer();
    await screen.findByText("src");

    // 触发第二次读(会挂住),再在它落地之前展开 src。
    fireEvent.click(screen.getByTitle("Refresh"));
    await waitFor(() => expect(resolveSlowRead).toBeTruthy());
    fireEvent.click(row("src"));
    await screen.findByText("a.ts");

    // 慢读现在落地,带着「src 未展开」的旧快照。
    await act(async () => {
      resolveSlowRead?.(fs.get("/repo") ?? []);
    });

    expect(inTree("a.ts")).toBeInTheDocument();
  });

  it.each([
    ["新建", "create_file"],
    ["重命名", "rename_path"],
    ["粘贴", "copy_paths_to_directory"],
  ])("卸载后落地的%s结果既不刷新也不弹提示", async (_label, command) => {
    let settle: ((ok: boolean) => void) | undefined;
    setRouter((cmd) => {
      if (cmd !== command) return undefined;
      return new Promise<undefined>((resolve, reject) => {
        settle = (ok: boolean) => (ok ? resolve(undefined) : reject(new Error("late failure")));
      });
    });
    const { unmount } = renderExplorer();
    await screen.findByText("README.md");

    if (command === "create_file") {
      const scroll = treeScroll();
      fireEvent.contextMenu(scroll, { clientX: 5, clientY: 5, target: scroll });
      fireEvent.click(menuItem("New File"));
      const input = await waitFor(() => {
        const found = treeScroll().querySelector("input");
        if (!found) throw new Error("input not rendered");
        return found;
      });
      fireEvent.change(input, { target: { value: "new.ts" } });
      fireEvent.keyDown(input, { key: "Enter" });
    } else if (command === "rename_path") {
      fireEvent.click(row("README.md"));
      fireEvent.keyDown(treeScroll(), { key: "Enter" });
      const input = await waitFor(() => {
        const found = treeScroll().querySelector("input");
        if (!found) throw new Error("input not rendered");
        return found;
      });
      fireEvent.change(input, { target: { value: "READ.md" } });
      fireEvent.keyDown(input, { key: "Enter" });
    } else {
      firePasteEvent(treeScroll(), fileList(["/outside/a.txt"]));
    }

    await waitFor(() => expect(invoke).toHaveBeenCalledWith(command, expect.anything()));
    unmount();
    const readsAfterUnmount = vi
      .mocked(invoke)
      .mock.calls.filter(([c]) => c === "read_dir_entries").length;

    await act(async () => {
      settle?.(true);
    });
    expect(vi.mocked(invoke).mock.calls.filter(([c]) => c === "read_dir_entries")).toHaveLength(
      readsAfterUnmount,
    );
    expect(showToast).not.toHaveBeenCalled();
  });

  it("卸载后落地的失败结果也不弹提示(新建 / 重命名 / 粘贴各一次)", async () => {
    for (const command of ["create_file", "rename_path", "copy_paths_to_directory"] as const) {
      showToast.mockClear();
      resetFs();
      let fail: (() => void) | undefined;
      setRouter((cmd) => {
        if (cmd !== command) return undefined;
        return new Promise<undefined>((_, reject) => {
          fail = () => reject(new Error("late failure"));
        });
      });
      const { unmount } = renderExplorer();
      await screen.findByText("README.md");

      if (command === "copy_paths_to_directory") {
        firePasteEvent(treeScroll(), fileList(["/outside/a.txt"]));
      } else {
        if (command === "rename_path") {
          fireEvent.click(row("README.md"));
          fireEvent.keyDown(treeScroll(), { key: "Enter" });
        } else {
          const scroll = treeScroll();
          fireEvent.contextMenu(scroll, { clientX: 5, clientY: 5, target: scroll });
          fireEvent.click(menuItem("New File"));
        }
        const input = await waitFor(() => {
          const found = treeScroll().querySelector("input");
          if (!found) throw new Error("input not rendered");
          return found;
        });
        fireEvent.change(input, { target: { value: "other.ts" } });
        fireEvent.keyDown(input, { key: "Enter" });
      }

      await waitFor(() => expect(invoke).toHaveBeenCalledWith(command, expect.anything()));
      unmount();
      await act(async () => {
        fail?.();
      });
      expect(showToast).not.toHaveBeenCalled();
    }
  });

  it("展开的目录在卸载后返回结果时不再写状态", async () => {
    let resolveChildren: ((entries: Entry[]) => void) | undefined;
    setRouter((command, args) => {
      if (command !== "read_dir_entries" || args.path !== "/repo/src") return undefined;
      return new Promise<Entry[]>((resolve) => {
        resolveChildren = resolve;
      });
    });
    const { unmount } = renderExplorer();
    await screen.findByText("src");

    fireEvent.click(row("src"));
    await waitFor(() => expect(resolveChildren).toBeTruthy());
    unmount();

    await act(async () => {
      resolveChildren?.([file("/repo/src/a.ts", 200)]);
    });
    // 只要没抛「setState on unmounted」之类的错误就算过;这里同时确认没弹错误条。
    expect(screen.queryByTestId("file-explorer-error")).not.toBeInTheDocument();
  });

  it("展开时读目录失败但组件已卸载,不写错误状态", async () => {
    let rejectChildren: (() => void) | undefined;
    setRouter((command, args) => {
      if (command !== "read_dir_entries" || args.path !== "/repo/src") return undefined;
      return new Promise<Entry[]>((_, reject) => {
        rejectChildren = () => reject(new Error("EIO"));
      });
    });
    const { unmount } = renderExplorer();
    await screen.findByText("src");

    fireEvent.click(row("src"));
    await waitFor(() => expect(rejectChildren).toBeTruthy());
    unmount();

    await act(async () => {
      rejectChildren?.();
    });
    expect(screen.queryByTestId("file-explorer-error")).not.toBeInTheDocument();
  });

  it("刷新按钮的 hover 样式会改颜色再还原", async () => {
    renderExplorer();
    await screen.findByText("README.md");
    const button = screen.getByTitle("Refresh");

    fireEvent.mouseEnter(button);
    expect(button.style.color).toBe("var(--text-primary)");
    expect(button.style.background).toBe("var(--bg-hover)");

    fireEvent.mouseLeave(button);
    expect(button.style.color).toBe("var(--text-hint)");
    expect(button.style.background).toBe("none");
  });
});

/*
 * 以下分支**够不着**,不为它们编造状态(逐条给了不可达的理由):
 *
 * - `handleEmptyContextMenu` 的 `e.target !== e.currentTarget` 早退:树容器内每一个
 *   能收到 contextMenu 的后代(TreeItem、新建行、重命名行、loading/空态 div)都自己
 *   调了 stopPropagation,所以事件到达外层容器时 target 必然就是它自己。
 * - `handleToggle` 里 `node.expanded === shouldExpand && node.children === nextChildren`
 *   的「无变化」分支:`shouldExpand = !current?.expanded` 永远与当前值相反。
 * - `handleSelect` 里 `if (!node.is_dir)` 的 else:走到那一行时 action 已经是 "select",
 *   而 `fileExplorerClickAction` 只在 `!isDir` 时返回 "select"。
 * - `handleBreadcrumbNavigate` 的 `if (remote) return`:远程模式下根之外的面包屑按钮
 *   是 `disabled`,点不到;根之内的路径走上面那条分支。
 * - 同一函数里的 `if (!node?.is_dir) return`:面包屑段是从 `currentDirectoryPath` 推的,
 *   而它又是从 `findNode(nodes, selectedPath)` 推的 —— 节点从树里消失时面包屑同时
 *   退回项目根,不会留下一个能点的死段(上面有用例证明这一点)。
 * - `handleDelete` 的 `!ctxMenu || ctxMenu.isRoot`:根菜单不渲染删除项(已有用例证明)。
 * - 各处 `if (!scrollRef.current) return` / `if (!breadcrumbRef.current) return`:
 *   两个 ref 都挂在无条件渲染的节点上,组件挂载期间不可能为 null。
 * - `commitCreate` / `commitRename` / `startCreate` 开头的 `if (!creating)` /
 *   `if (!renamingPath)` / `if (!ctxMenu)`:这些回调只在对应状态非空时才渲染出触发点。
 * - 预览里的 `if (!endpoint) return` 与 `remote?.projectPath ?? endpoint.path` 的兜底:
 *   `selectedNode` 存在即 path 非空;`kind === "ssh"` 的端点只在 `remote.kind === "ssh"`
 *   时产生,而那个对象必然带 projectPath。
 * - `pasteFiles` 的 `files ?? []`:`handleTreePaste` 先读 `event.clipboardData.files.length`,
 *   传 null 会在那一步就抛,到不了这里。
 *
 * 这两条面包屑守卫在变异测试里**如期存活**(把它们摘掉,132 条全绿):
 * `if (remote) return` 有 `disabled` 兜在前面(单独把 `navigable` 改成恒真会被杀掉,
 * 说明真正拦住用户的是那个属性),`if (!node?.is_dir) return` 则纯粹是防御性写法。
 * 两条都保留 —— 它们的成本是零,而删掉之后一旦上游的推导变了就直接越界。
 */
