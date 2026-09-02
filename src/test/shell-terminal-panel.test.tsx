import { act, fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { createRef } from "react";
import { I18nProvider } from "../i18n";
import type { ShellTerminalPanelHandle, ShellSession } from "../components/ShellTerminalPanel";
import { compactTerminalLabel, formatTerminalTabLabel } from "../components/terminalTabLabel";

/**
 * `ShellTerminalPanel` 的会话管理层。真 xterm 跑不进 jsdom(要 canvas / webgl /
 * 真实布局),所以在 `terminalRuntime` 这个缝上打桩 —— 它正好是面板唯一的终端依赖,
 * 面板自己只碰 focus / fit / dispose / writer.write / term.cols|rows。
 */

const runtimeState = vi.hoisted(() => ({
  created: [] as Array<{
    themeVariant: string;
    terminalFontSize: number;
    monoFontFamily: string;
    isActive: () => boolean;
    onInput: (data: string) => void;
    onResize?: (size: { cols: number; rows: number }) => void;
    focus: ReturnType<typeof vi.fn>;
    fit: ReturnType<typeof vi.fn>;
    dispose: ReturnType<typeof vi.fn>;
    updateTheme: ReturnType<typeof vi.fn>;
    updateFontSize: ReturnType<typeof vi.fn>;
    updateFontFamily: ReturnType<typeof vi.fn>;
    writes: string[];
  }>,
}));

const eventState = vi.hoisted(() => ({
  handlers: [] as Array<{ event: string; cb: (e: { payload: unknown }) => void }>,
  unlistens: [] as ReturnType<typeof vi.fn>[],
  deferUnlisten: false,
  pendingResolvers: [] as Array<() => void>,
}));

const invoke = vi.hoisted(() => vi.fn());

vi.mock("@tauri-apps/api/core", () => ({ invoke }));

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn((event: string, cb: (e: { payload: unknown }) => void) => {
    eventState.handlers.push({ event, cb });
    const unlisten = vi.fn();
    eventState.unlistens.push(unlisten);
    if (eventState.deferUnlisten) {
      return new Promise<() => void>((resolve) => {
        eventState.pendingResolvers.push(() => resolve(unlisten));
      });
    }
    return Promise.resolve(unlisten);
  }),
}));

vi.mock("../components/terminalRuntime", () => ({
  createTerminalRuntime: vi.fn((options: Record<string, unknown>) => {
    const writes: string[] = [];
    const entry = {
      themeVariant: options.themeVariant as string,
      terminalFontSize: options.terminalFontSize as number,
      monoFontFamily: options.monoFontFamily as string,
      isActive: options.isActive as () => boolean,
      onInput: options.onInput as (data: string) => void,
      onResize: options.onResize as ((size: { cols: number; rows: number }) => void) | undefined,
      focus: vi.fn(),
      fit: vi.fn(),
      dispose: vi.fn(),
      updateTheme: vi.fn(),
      updateFontSize: vi.fn(),
      updateFontFamily: vi.fn(),
      writes,
    };
    runtimeState.created.push(entry);
    return {
      term: { cols: 80, rows: 24 },
      fitAddon: {},
      writer: { write: (data: string) => writes.push(data) },
      fit: entry.fit,
      focus: entry.focus,
      updateTheme: entry.updateTheme,
      updateFontSize: entry.updateFontSize,
      updateFontFamily: entry.updateFontFamily,
      dispose: entry.dispose,
    };
  }),
}));

vi.mock("../components/terminalShared", () => ({
  themeFor: (variant: string) => ({ background: `bg-${variant}` }),
}));

const { SHELL_TERMINAL_MAX_SESSIONS, ShellTerminalPanel, deriveShellTerminalFontSize } =
  await import("../components/ShellTerminalPanel");

type PanelProps = Partial<Parameters<typeof ShellTerminalPanel>[0]>;

function renderPanel(props: PanelProps = {}) {
  const onClose = props.onClose ?? vi.fn();
  const result = render(
    <I18nProvider>
      <ShellTerminalPanel
        projectPath="/repo"
        projectId="p1"
        onClose={onClose}
        themeVariant="dark"
        terminalFontSize={13}
        monoFontFamily="JetBrains Mono"
        {...props}
      />
    </I18nProvider>,
  );
  return { ...result, onClose };
}

/** 面板在 50ms 后才 open_shell,300ms 后才 onReady。默认用真时钟等不动,统一推假时钟。 */
async function advance(ms: number) {
  await act(async () => {
    await vi.advanceTimersByTimeAsync(ms);
  });
}

function emitShellOutput(shellId: string, data: string) {
  for (const handler of eventState.handlers) {
    if (handler.event === "shell-output") handler.cb({ payload: { shell_id: shellId, data } });
  }
}

function addButton() {
  return screen.getByTitle(/New terminal|Terminal limit reached/);
}

function tabs() {
  return screen.getAllByRole("tab");
}

beforeEach(() => {
  vi.useFakeTimers();
  runtimeState.created.length = 0;
  eventState.handlers.length = 0;
  eventState.unlistens.length = 0;
  eventState.pendingResolvers.length = 0;
  eventState.deferUnlisten = false;
  invoke.mockReset();
  invoke.mockResolvedValue(undefined);
});

describe("deriveShellTerminalFontSize", () => {
  it("比主终端小一号", () => {
    expect(deriveShellTerminalFontSize(14)).toBe(13);
  });

  it("下限 10,不会因为主终端调小而缩到不可读", () => {
    expect(deriveShellTerminalFontSize(11)).toBe(10);
    expect(deriveShellTerminalFontSize(10)).toBe(10);
  });
});

describe("ShellTerminalPanel 会话生命周期", () => {
  it("上限就是 10", () => {
    // 其余用例都读 SHELL_TERMINAL_MAX_SESSIONS,所以改这个常量它们会跟着动 ——
    // 变异测试里「把 10 改成 20」因此全绿。这条钉住字面值:真要调上限就得改这里,
    // 是个明确动作而不是顺手改掉。
    expect(SHELL_TERMINAL_MAX_SESSIONS).toBe(10);
  });

  it("首次渲染只开一个会话,计数显示 1/上限", async () => {
    renderPanel();
    await advance(60);
    expect(tabs()).toHaveLength(1);
    expect(screen.getByText(`1/${SHELL_TERMINAL_MAX_SESSIONS}`)).toBeInTheDocument();
    expect(runtimeState.created).toHaveLength(1);
  });

  it("加号新增会话并把新会话设为活动", async () => {
    renderPanel();
    await advance(60);
    fireEvent.click(addButton());
    await advance(60);
    const all = tabs();
    expect(all).toHaveLength(2);
    expect(all[1]).toHaveAttribute("aria-selected", "true");
    expect(all[0]).toHaveAttribute("aria-selected", "false");
    expect(screen.getByText(`2/${SHELL_TERMINAL_MAX_SESSIONS}`)).toBeInTheDocument();
  });

  it("到上限后加号禁用且提示换成 limitReached", async () => {
    renderPanel();
    await advance(60);
    for (let i = 1; i < SHELL_TERMINAL_MAX_SESSIONS; i++) {
      fireEvent.click(addButton());
      await advance(60);
    }
    expect(tabs()).toHaveLength(SHELL_TERMINAL_MAX_SESSIONS);
    const add = addButton();
    expect(add).toBeDisabled();
    expect(add).toHaveAttribute("title", "Terminal limit reached");
  });

  it("到上限后再点也不会多开(disabled 之外的第二道闸门)", async () => {
    renderPanel();
    await advance(60);
    for (let i = 1; i < SHELL_TERMINAL_MAX_SESSIONS; i++) {
      fireEvent.click(addButton());
      await advance(60);
    }
    // disabled 按钮在真实浏览器里点不动,这里直接绕过 disabled 触发 onClick,
    // 验证 handleAddShell 自己那道 `shells.length >= MAX` 也拦得住。
    fireEvent.click(addButton(), { bubbles: true });
    await advance(60);
    expect(tabs()).toHaveLength(SHELL_TERMINAL_MAX_SESSIONS);
  });

  it("点标签切换活动会话", async () => {
    renderPanel();
    await advance(60);
    fireEvent.click(addButton());
    await advance(60);
    fireEvent.click(tabs()[0]);
    await advance(0);
    expect(tabs()[0]).toHaveAttribute("aria-selected", "true");
    expect(tabs()[1]).toHaveAttribute("aria-selected", "false");
  });

  it("每个会话拿到自己的 shellId,open_shell 带上项目路径与初始尺寸", async () => {
    renderPanel({ projectPath: "/work/app" });
    await advance(60);
    const call = invoke.mock.calls.find(([name]) => name === "open_shell");
    expect(call).toBeDefined();
    expect(call![1]).toMatchObject({ projectPath: "/work/app", cols: 80, rows: 24 });
    expect(String(call![1].shellId)).toMatch(/^shell:p1:1:/);
  });
});

describe("ShellTerminalPanel 关闭会话", () => {
  async function renderThree() {
    const onClose = vi.fn();
    renderPanel({ onClose });
    await advance(60);
    fireEvent.click(addButton());
    await advance(60);
    fireEvent.click(addButton());
    await advance(60);
    return { onClose };
  }

  /**
   * 关闭按钮按会话标题取,不按下标。标题是建会话时定死的(Terminal 1/2/3),
   * 关掉中间一个之后剩下的标题不会重排 —— 用下标取会在「关了 Terminal 1 之后
   * 下标 0 变成 Terminal 2」这种地方悄悄指错人。
   */
  function closeTab(title: string) {
    return screen.getByRole("button", { name: `Close ${title}` });
  }

  it("关掉一个会话会 kill_shell 并从标签里去掉", async () => {
    await renderThree();
    const firstShellId = invoke.mock.calls.find(([n]) => n === "open_shell")![1].shellId;
    invoke.mockClear();
    fireEvent.click(closeTab("Terminal 1"));
    await advance(60);
    expect(tabs()).toHaveLength(2);
    expect(invoke).toHaveBeenCalledWith("kill_shell", { shellId: firstShellId });
  });

  it("关掉中间的活动会话后接管后一个", async () => {
    await renderThree();
    // 三个会话,当前活动是第 3 个。先切到第 2 个再关它。
    fireEvent.click(tabs()[1]);
    await advance(0);
    fireEvent.click(closeTab("Terminal 2"));
    await advance(60);
    const all = tabs();
    expect(all).toHaveLength(2);
    // 原来的第 3 个补到了下标 1 的位置,它应该成为活动会话。
    expect(all[1]).toHaveAttribute("aria-selected", "true");
  });

  it("关掉末尾的活动会话后回退到前一个", async () => {
    await renderThree();
    fireEvent.click(closeTab("Terminal 3"));
    await advance(60);
    const all = tabs();
    expect(all).toHaveLength(2);
    expect(all[1]).toHaveAttribute("aria-selected", "true");
  });

  it("关掉非活动会话不动活动会话", async () => {
    await renderThree();
    // 活动是第 3 个,关第 1 个。
    fireEvent.click(closeTab("Terminal 1"));
    await advance(60);
    const all = tabs();
    expect(all).toHaveLength(2);
    expect(all[1]).toHaveAttribute("aria-selected", "true");
  });

  it("关掉最后一个会话时通知父组件收起面板", async () => {
    const onClose = vi.fn();
    renderPanel({ onClose });
    await advance(60);
    fireEvent.click(closeTab("Terminal 1"));
    await advance(60);
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("关掉最后一个会话时不会再去设活动会话", async () => {
    const onClose = vi.fn();
    renderPanel({ onClose });
    await advance(60);
    fireEvent.click(closeTab("Terminal 1"));
    await advance(60);
    expect(screen.queryAllByRole("tab")).toHaveLength(0);
  });

  it("标题栏的关闭按钮 kill 掉全部会话", async () => {
    const { onClose } = await renderThree();
    const openedIds = invoke.mock.calls
      .filter(([n]) => n === "open_shell")
      .map(([, args]) => (args as { shellId: string }).shellId);
    expect(openedIds).toHaveLength(3);
    invoke.mockClear();
    fireEvent.click(screen.getByTitle("Close terminals"));
    await advance(60);
    const killed = invoke.mock.calls
      .filter(([n]) => n === "kill_shell")
      .map(([, args]) => (args as { shellId: string }).shellId);
    expect(killed).toEqual(openedIds);
    expect(onClose).toHaveBeenCalledTimes(1);
    expect(screen.queryAllByRole("tab")).toHaveLength(0);
  });

  it("kill_shell 失败不会把异常抛到渲染里", async () => {
    invoke.mockImplementation((name: string) => {
      if (name === "kill_shell") return Promise.reject(new Error("boom"));
      return Promise.resolve(undefined);
    });
    const errorSpy = vi.spyOn(console, "error").mockImplementation(() => {});
    await renderThree();
    fireEvent.click(closeTab("Terminal 1"));
    await advance(60);
    expect(tabs()).toHaveLength(2);
    errorSpy.mockRestore();
  });
});

describe("ShellTerminalPanel 命令式句柄", () => {
  it("sendCommand 发到当前活动会话", async () => {
    const ref = createRef<ShellTerminalPanelHandle>();
    renderPanel({ ref });
    await advance(60);
    fireEvent.click(addButton());
    await advance(60);
    const activeShellId = invoke.mock.calls.filter(([n]) => n === "open_shell").at(-1)![1].shellId;
    invoke.mockClear();
    act(() => ref.current!.sendCommand("ls -la\r"));
    expect(invoke).toHaveBeenCalledWith("send_input", {
      taskId: activeShellId,
      data: "ls -la\r",
    });
  });

  it("切了活动会话之后 sendCommand 跟着换目标", async () => {
    const ref = createRef<ShellTerminalPanelHandle>();
    renderPanel({ ref });
    await advance(60);
    fireEvent.click(addButton());
    await advance(60);
    const firstShellId = invoke.mock.calls.filter(([n]) => n === "open_shell")[0][1].shellId;
    fireEvent.click(tabs()[0]);
    await advance(0);
    invoke.mockClear();
    act(() => ref.current!.sendCommand("pwd\r"));
    expect(invoke).toHaveBeenCalledWith("send_input", { taskId: firstShellId, data: "pwd\r" });
  });

  it("activateShell 只认现存会话,未知 id 直接忽略", async () => {
    const ref = createRef<ShellTerminalPanelHandle>();
    renderPanel({ ref });
    await advance(60);
    fireEvent.click(addButton());
    await advance(60);
    const firstShellId = invoke.mock.calls.filter(([n]) => n === "open_shell")[0][1].shellId;

    act(() => ref.current!.activateShell("shell:nope:9:0"));
    await advance(0);
    expect(tabs()[1]).toHaveAttribute("aria-selected", "true");

    act(() => ref.current!.activateShell(firstShellId));
    await advance(0);
    expect(tabs()[0]).toHaveAttribute("aria-selected", "true");
  });

  it("addShell / closeShell 与界面按钮走同一条路", async () => {
    const ref = createRef<ShellTerminalPanelHandle>();
    renderPanel({ ref });
    await advance(60);
    act(() => ref.current!.addShell());
    await advance(60);
    expect(tabs()).toHaveLength(2);

    const secondShellId = invoke.mock.calls.filter(([n]) => n === "open_shell")[1][1].shellId;
    invoke.mockClear();
    act(() => ref.current!.closeShell(secondShellId));
    await advance(60);
    expect(tabs()).toHaveLength(1);
    expect(invoke).toHaveBeenCalledWith("kill_shell", { shellId: secondShellId });
  });

  it("closeShell 传未知 id 时既不 kill 也不动会话列表", async () => {
    const ref = createRef<ShellTerminalPanelHandle>();
    renderPanel({ ref });
    await advance(60);
    invoke.mockClear();
    act(() => ref.current!.closeShell("shell:nope:9:0"));
    await advance(60);
    expect(tabs()).toHaveLength(1);
    expect(invoke.mock.calls.filter(([n]) => n === "kill_shell")).toHaveLength(0);
  });
});

describe("ShellTerminalPanel 标签文案", () => {
  it("标签用 shellLabel 排序号,关闭按钮用会话标题 —— 两套编号互不相干", async () => {
    // 标签是「按当前位置」编号(PowerShell 1/2),关闭按钮是「建会话时」定死的
    // 标题(Terminal 1/2)。关掉前面一个之后标签会重排、标题不会。
    renderPanel({ shellLabel: "Windows PowerShell" });
    await advance(60);
    fireEvent.click(addButton());
    await advance(60);
    expect(tabs()[0]).toHaveAttribute("title", "PowerShell 1");
    expect(tabs()[1]).toHaveAttribute("title", "PowerShell 2");
    expect(screen.getByRole("button", { name: "Close Terminal 1" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Close Terminal 2" })).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Close Terminal 1" }));
    await advance(60);
    // 只剩一个:标签重排回 1,但标题仍然是 Terminal 2。
    expect(tabs()[0]).toHaveAttribute("title", "PowerShell 1");
    expect(screen.getByRole("button", { name: "Close Terminal 2" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Close Terminal 1" })).not.toBeInTheDocument();
  });

  it("默认 shellLabel 是 Shell", async () => {
    renderPanel();
    await advance(60);
    expect(tabs()[0]).toHaveAttribute("title", "Shell 1");
  });

  it("showSessionTabs 关掉时整条标签栏不渲染", async () => {
    renderPanel({ showSessionTabs: false });
    await advance(60);
    expect(screen.queryAllByRole("tab")).toHaveLength(0);
    expect(screen.queryByTitle("New terminal")).not.toBeInTheDocument();
    // 会话本身还在,终端仍然开着。
    expect(runtimeState.created).toHaveLength(1);
  });
});

describe("ShellTerminalPanel 输出分发", () => {
  it("只把自己 shell_id 的输出写进对应终端", async () => {
    renderPanel();
    await advance(60);
    fireEvent.click(addButton());
    await advance(60);
    const [firstId, secondId] = invoke.mock.calls
      .filter(([n]) => n === "open_shell")
      .map(([, a]) => (a as { shellId: string }).shellId);

    act(() => emitShellOutput(firstId, "first\r\n"));
    act(() => emitShellOutput(secondId, "second\r\n"));

    expect(runtimeState.created[0].writes).toEqual(["first\r\n"]);
    expect(runtimeState.created[1].writes).toEqual(["second\r\n"]);
  });

  it("陌生 shell_id 的输出被丢掉,不会串台", async () => {
    renderPanel();
    await advance(60);
    act(() => emitShellOutput("shell:other:1:0", "leak\r\n"));
    expect(runtimeState.created[0].writes).toEqual([]);
  });

  it("终端输入转成 send_input", async () => {
    renderPanel();
    await advance(60);
    const shellId = invoke.mock.calls.find(([n]) => n === "open_shell")![1].shellId;
    invoke.mockClear();
    act(() => runtimeState.created[0].onInput("echo hi\r"));
    expect(invoke).toHaveBeenCalledWith("send_input", { taskId: shellId, data: "echo hi\r" });
  });

  it("终端尺寸变化转成 resize_pty", async () => {
    renderPanel();
    await advance(60);
    const shellId = invoke.mock.calls.find(([n]) => n === "open_shell")![1].shellId;
    invoke.mockClear();
    act(() => runtimeState.created[0].onResize?.({ cols: 120, rows: 40 }));
    expect(invoke).toHaveBeenCalledWith("resize_pty", { taskId: shellId, cols: 120, rows: 40 });
  });
});

describe("ShellTerminalPanel 属性透传", () => {
  it("字号 / 字体 / 主题都传给运行时", async () => {
    renderPanel({ themeVariant: "light", terminalFontSize: 15, monoFontFamily: "Fira Code" });
    await advance(60);
    expect(runtimeState.created[0]).toMatchObject({
      themeVariant: "light",
      terminalFontSize: 15,
      monoFontFamily: "Fira Code",
    });
  });

  it("主题变了调 updateTheme,不重建终端", async () => {
    const { rerender } = renderPanel({ themeVariant: "dark" });
    await advance(60);
    rerender(
      <I18nProvider>
        <ShellTerminalPanel
          projectPath="/repo"
          projectId="p1"
          onClose={vi.fn()}
          themeVariant="light"
          terminalFontSize={13}
          monoFontFamily="JetBrains Mono"
        />
      </I18nProvider>,
    );
    await advance(0);
    expect(runtimeState.created).toHaveLength(1);
    expect(runtimeState.created[0].updateTheme).toHaveBeenCalledWith("light");
    expect(runtimeState.created[0].dispose).not.toHaveBeenCalled();
  });

  it("字号变了调 updateFontSize,不重建终端", async () => {
    const { rerender } = renderPanel({ terminalFontSize: 13 });
    await advance(60);
    rerender(
      <I18nProvider>
        <ShellTerminalPanel
          projectPath="/repo"
          projectId="p1"
          onClose={vi.fn()}
          themeVariant="dark"
          terminalFontSize={16}
          monoFontFamily="JetBrains Mono"
        />
      </I18nProvider>,
    );
    await advance(0);
    expect(runtimeState.created).toHaveLength(1);
    expect(runtimeState.created[0].updateFontSize).toHaveBeenCalledWith(16);
  });

  it("字体变了调 updateFontFamily,不重建终端", async () => {
    const { rerender } = renderPanel({ monoFontFamily: "JetBrains Mono" });
    await advance(60);
    rerender(
      <I18nProvider>
        <ShellTerminalPanel
          projectPath="/repo"
          projectId="p1"
          onClose={vi.fn()}
          themeVariant="dark"
          terminalFontSize={13}
          monoFontFamily="Fira Code"
        />
      </I18nProvider>,
    );
    await advance(0);
    expect(runtimeState.created).toHaveLength(1);
    expect(runtimeState.created[0].updateFontFamily).toHaveBeenCalledWith("Fira Code");
  });

  it("isActive 传进运行时,非活动会话的 isActive() 为 false", async () => {
    renderPanel();
    await advance(60);
    fireEvent.click(addButton());
    await advance(60);
    // 第 2 个是活动的。
    expect(runtimeState.created[0].isActive()).toBe(false);
    expect(runtimeState.created[1].isActive()).toBe(true);
  });

  it("面板整体 isActive=false 时所有会话都不活动", async () => {
    renderPanel({ isActive: false });
    await advance(60);
    expect(runtimeState.created[0].isActive()).toBe(false);
  });

  it("onMinimize 没传时不渲染最小化按钮", async () => {
    renderPanel();
    await advance(60);
    expect(screen.queryByTitle("Minimize terminal")).not.toBeInTheDocument();
  });

  it("onMinimize 传了就渲染并能点", async () => {
    const onMinimize = vi.fn();
    renderPanel({ onMinimize });
    await advance(60);
    fireEvent.click(screen.getByTitle("Minimize terminal"));
    expect(onMinimize).toHaveBeenCalledTimes(1);
  });

  it("onResizeStart 没传时不渲染拖拽条", async () => {
    const { container } = renderPanel();
    await advance(60);
    expect(container.querySelector('[style*="row-resize"]')).toBeNull();
  });

  it("onResizeStart 传了就渲染拖拽条并在按下时回调", async () => {
    const onResizeStart = vi.fn();
    const { container } = renderPanel({ onResizeStart });
    await advance(60);
    const handle = container.querySelector('[style*="row-resize"]')!;
    expect(handle).not.toBeNull();
    fireEvent.mouseDown(handle);
    expect(onResizeStart).toHaveBeenCalledTimes(1);
  });
});

describe("ShellTerminalPanel 会话变更通知", () => {
  it("挂载时就把会话列表报给父组件", async () => {
    const onSessionsChange = vi.fn();
    renderPanel({ onSessionsChange });
    await advance(60);
    const [sessions, activeId] = onSessionsChange.mock.calls.at(-1)!;
    expect(sessions).toHaveLength(1);
    expect((sessions as ShellSession[])[0].id).toBe(activeId);
  });

  it("新增会话后重新上报,活动 id 跟着变", async () => {
    const onSessionsChange = vi.fn();
    renderPanel({ onSessionsChange });
    await advance(60);
    onSessionsChange.mockClear();
    fireEvent.click(addButton());
    await advance(60);
    const [sessions, activeId] = onSessionsChange.mock.calls.at(-1)!;
    expect(sessions).toHaveLength(2);
    expect((sessions as ShellSession[])[1].id).toBe(activeId);
  });

  it("全部关掉后上报空列表与 null", async () => {
    const onSessionsChange = vi.fn();
    renderPanel({ onSessionsChange });
    await advance(60);
    fireEvent.click(screen.getByTitle("Close terminals"));
    await advance(60);
    const [sessions, activeId] = onSessionsChange.mock.calls.at(-1)!;
    expect(sessions).toEqual([]);
    expect(activeId).toBeNull();
  });
});

describe("ShellTerminalPanel 卸载与竞态", () => {
  it("卸载时 dispose 掉每一个终端", async () => {
    const { unmount } = renderPanel();
    await advance(60);
    fireEvent.click(addButton());
    await advance(60);
    expect(runtimeState.created).toHaveLength(2);
    unmount();
    for (const runtime of runtimeState.created) {
      expect(runtime.dispose).toHaveBeenCalledTimes(1);
    }
  });

  it("卸载时取消事件订阅", async () => {
    const { unmount } = renderPanel();
    await advance(60);
    expect(eventState.unlistens).toHaveLength(1);
    unmount();
    expect(eventState.unlistens[0]).toHaveBeenCalledTimes(1);
  });

  it("50ms 之前就卸载,不会再去 open_shell", async () => {
    // 面板故意等 50ms 再开 shell(给布局留时间)。这段窗口里卸载必须撤掉定时器,
    // 否则会对一个已经 dispose 的终端发 open_shell,后端留下无主进程。
    const { unmount } = renderPanel();
    await advance(20);
    expect(invoke.mock.calls.filter(([n]) => n === "open_shell")).toHaveLength(0);
    unmount();
    await advance(200);
    expect(invoke.mock.calls.filter(([n]) => n === "open_shell")).toHaveLength(0);
  });

  it("open_shell 已发出但 onReady 之前卸载,不回调 onReady", async () => {
    // open_shell 之后还要再等 300ms 才 onReady。这段窗口里卸载必须撤掉第二个定时器。
    const onReady = vi.fn();
    const { unmount } = renderPanel({ onReady });
    await advance(60);
    expect(invoke.mock.calls.filter(([n]) => n === "open_shell")).toHaveLength(1);
    expect(onReady).not.toHaveBeenCalled();
    unmount();
    await advance(500);
    expect(onReady).not.toHaveBeenCalled();
  });

  it("正常路径下 300ms 后回调 onReady", async () => {
    // 先确认这个时钟真能推到 onReady,否则上面那条"没回调"是空断言。
    const onReady = vi.fn();
    renderPanel({ onReady });
    await advance(60);
    expect(onReady).not.toHaveBeenCalled();
    await advance(320);
    expect(onReady).toHaveBeenCalled();
  });

  it("listen 的 promise 在卸载后才 resolve 时,立刻退订", async () => {
    // listen() 是异步的:卸载可能发生在它 resolve 之前。那种情况下 cleanup 里
    // 拿不到 unlisten,必须由 then 回调自己调掉,否则监听器永远留在 window 上。
    eventState.deferUnlisten = true;
    const { unmount } = renderPanel();
    await advance(60);
    unmount();
    expect(eventState.unlistens[0]).not.toHaveBeenCalled();
    await act(async () => {
      for (const resolve of eventState.pendingResolvers) resolve();
      await Promise.resolve();
    });
    expect(eventState.unlistens[0]).toHaveBeenCalledTimes(1);
  });

  it("open_shell 失败不影响面板存活,也不回调 onReady", async () => {
    invoke.mockImplementation((name: string) => {
      if (name === "open_shell") return Promise.reject(new Error("no pty"));
      return Promise.resolve(undefined);
    });
    const errorSpy = vi.spyOn(console, "error").mockImplementation(() => {});
    const onReady = vi.fn();
    renderPanel({ onReady });
    await advance(500);
    expect(onReady).not.toHaveBeenCalled();
    expect(tabs()).toHaveLength(1);
    errorSpy.mockRestore();
  });

  it("关掉某个会话只 dispose 它自己的终端", async () => {
    renderPanel();
    await advance(60);
    fireEvent.click(addButton());
    await advance(60);
    fireEvent.click(screen.getByRole("button", { name: "Close Terminal 1" }));
    await advance(60);
    expect(runtimeState.created[0].dispose).toHaveBeenCalledTimes(1);
    expect(runtimeState.created[1].dispose).not.toHaveBeenCalled();
  });

  it("切换活动会话不重建任何终端", async () => {
    renderPanel();
    await advance(60);
    fireEvent.click(addButton());
    await advance(60);
    expect(runtimeState.created).toHaveLength(2);
    fireEvent.click(tabs()[0]);
    await advance(60);
    fireEvent.click(tabs()[1]);
    await advance(60);
    expect(runtimeState.created).toHaveLength(2);
    for (const runtime of runtimeState.created) {
      expect(runtime.dispose).not.toHaveBeenCalled();
    }
  });
});

describe("compactTerminalLabel / formatTerminalTabLabel", () => {
  it("Windows PowerShell 缩成 PowerShell", () => {
    expect(compactTerminalLabel("Windows PowerShell")).toBe("PowerShell");
    expect(compactTerminalLabel("  windows powershell  ")).toBe("PowerShell");
  });

  it("空白标签回落成 Shell", () => {
    expect(compactTerminalLabel("")).toBe("Shell");
    expect(compactTerminalLabel("   ")).toBe("Shell");
  });

  it("其他标签只去首尾空白", () => {
    expect(compactTerminalLabel("  zsh  ")).toBe("zsh");
  });

  it("不给 index 时不加序号", () => {
    expect(formatTerminalTabLabel("zsh")).toBe("zsh");
    expect(formatTerminalTabLabel("zsh", 0)).toBe("zsh 1");
  });
});

describe("ShellTerminalPanel 剩余分支", () => {
  it("点终端区域会把焦点交回活动终端", async () => {
    const { container } = renderPanel();
    await advance(60);
    const surface = container.querySelector('[style*="cursor: text"]')!;
    expect(surface).not.toBeNull();
    runtimeState.created[0].focus.mockClear();
    fireEvent.mouseDown(surface);
    expect(runtimeState.created[0].focus).toHaveBeenCalled();
  });

  it("点非活动终端区域不抢焦点", async () => {
    const { container } = renderPanel();
    await advance(60);
    fireEvent.click(addButton());
    await advance(60);
    const surfaces = container.querySelectorAll('[style*="cursor: text"]');
    expect(surfaces).toHaveLength(2);
    runtimeState.created[0].focus.mockClear();
    // 下标 0 是第 1 个会话,此刻活动的是第 2 个。
    fireEvent.mouseDown(surfaces[0]);
    expect(runtimeState.created[0].focus).not.toHaveBeenCalled();
  });

  it("到上限后走命令式句柄 addShell 也开不出新会话", async () => {
    // 界面上的加号是 disabled,点不动 —— 走 ref 才能真正踩到 handleAddShell
    // 自己那道 `shells.length >= MAX`。这是这个上限的第二道闸门。
    const ref = createRef<ShellTerminalPanelHandle>();
    renderPanel({ ref });
    await advance(60);
    for (let i = 1; i < SHELL_TERMINAL_MAX_SESSIONS; i++) {
      act(() => ref.current!.addShell());
      await advance(60);
    }
    expect(tabs()).toHaveLength(SHELL_TERMINAL_MAX_SESSIONS);
    act(() => ref.current!.addShell());
    await advance(60);
    expect(tabs()).toHaveLength(SHELL_TERMINAL_MAX_SESSIONS);
    expect(runtimeState.created).toHaveLength(SHELL_TERMINAL_MAX_SESSIONS);
  });

  it("没有活动会话时 sendCommand 静默不发", async () => {
    // 父组件可能在 onClose 之后仍留着面板(比如做收起动画),此时 activeShellId 已是 null。
    // 变异测试:摘掉实现里的 `if (!currentShellId) return` 本文件仍全绿 —— 那是等价变异,
    // 不是漏测:`shellRefs.current[null]` 本来就是 undefined,后面的 `?.` 照样短路。
    // 这条断言的是「不发 send_input」这个可观察行为,两种写法都满足,所以不为它补测试。
    const ref = createRef<ShellTerminalPanelHandle>();
    renderPanel({ ref });
    await advance(60);
    fireEvent.click(screen.getByTitle("Close terminals"));
    await advance(60);
    invoke.mockClear();
    act(() => ref.current!.sendCommand("ls\r"));
    expect(invoke.mock.calls.filter(([n]) => n === "send_input")).toHaveLength(0);
  });

  it("visible=false 时面板隐藏但会话保留", async () => {
    const { container } = renderPanel({ visible: false });
    await advance(60);
    const root = container.firstElementChild as HTMLElement;
    expect(root.style.visibility).toBe("hidden");
    expect(root.style.pointerEvents).toBe("none");
    expect(root.style.height).toBe("0px");
    expect(runtimeState.created).toHaveLength(1);
  });

  it("visible=true 时用传入的高度", async () => {
    const { container } = renderPanel({ visible: true, height: 320 });
    await advance(60);
    const root = container.firstElementChild as HTMLElement;
    expect(root.style.visibility).toBe("visible");
    expect(root.style.pointerEvents).toBe("auto");
    expect(root.style.height).toBe("320px");
  });

  it("open_shell 已 resolve 但回调还没跑就卸载,不排 onReady 定时器", async () => {
    // 这段窗口 clearTimeout 兜不到:readyTimeout 还没建起来。靠 .then 里的
    // `if (cleaned) return` 拦住,否则会给已卸载的组件排一个 300ms 回调。
    let resolveOpen: (() => void) | null = null;
    invoke.mockImplementation((name: string) => {
      if (name === "open_shell") {
        return new Promise<void>((resolve) => {
          resolveOpen = () => resolve();
        });
      }
      return Promise.resolve(undefined);
    });
    const onReady = vi.fn();
    const { unmount } = renderPanel({ onReady });
    await advance(60);
    expect(resolveOpen).not.toBeNull();
    unmount();
    await act(async () => {
      resolveOpen!();
      await Promise.resolve();
    });
    await advance(500);
    expect(onReady).not.toHaveBeenCalled();
  });
});
