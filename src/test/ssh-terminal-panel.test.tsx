import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, afterEach, describe, expect, it, vi } from "vitest";
import { createRef, useState } from "react";
import { I18nProvider } from "../i18n";
import type { SshConnection } from "../types";
import type { SshTerminalPanelHandle } from "../components/ssh/SshTerminalPanel";
import { SSH_GROUPS_STORAGE_KEY, resetSshGroupNamesCache } from "../components/ssh/sshGroups";

/**
 * `SshTerminalPanel` 的会话编排层。这里只测面板自己的职责:
 * host key 闸门、孤儿 shell 的回收、自动连接的去重、ref 命令下发。
 *
 * 三个子组件(连接列表 / 连接对话框 / host key 对话框)各自已有测试文件,
 * 这里打桩成只暴露回调的壳 —— 否则子组件 UI 一动这个文件就跟着挂,
 * 而它想守的其实是"面板在什么时机调了什么"。
 * 真 xterm 同理跑不进 jsdom,在 `terminalRuntime` 这个缝上打桩。
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
    writeln: string[];
  }>,
}));

const channelState = vi.hoisted(() => ({
  instances: [] as Array<{ onmessage?: (data: string) => void }>,
}));

const invoke = vi.hoisted(() => vi.fn());

vi.mock("@tauri-apps/api/core", () => {
  class FakeChannel {
    onmessage?: (data: string) => void;
    constructor() {
      channelState.instances.push(this);
    }
  }
  return { invoke, Channel: FakeChannel };
});

vi.mock("../components/terminalRuntime", () => ({
  createTerminalRuntime: vi.fn((options: Record<string, unknown>) => {
    const writes: string[] = [];
    const writeln: string[] = [];
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
      writeln,
    };
    runtimeState.created.push(entry);
    return {
      term: { cols: 90, rows: 30, writeln: (line: string) => writeln.push(line) },
      fitAddon: {},
      writer: { write: (data: string) => writes.push(data) },
      ...entry,
    };
  }),
}));

vi.mock("../components/ssh/SshConnectionList", () => ({
  SshConnectionList: (props: Record<string, unknown>) => {
    const connections = props.connections as SshConnection[];
    return (
      <div data-testid="conn-list" data-selected={String(props.selectedId)}>
        {connections.map((c) => (
          <div key={c.id}>
            <button type="button" onClick={() => (props.onSelect as (c: SshConnection) => void)(c)}>
              {`select-${c.id}`}
            </button>
            <button type="button" onClick={() => (props.onDelete as (id: string) => void)(c.id)}>
              {`delete-${c.id}`}
            </button>
            <button
              type="button"
              onClick={() => (props.onConnect as (c: SshConnection, p: string) => void)(c, "ssh")}
            >
              {`menu-ssh-${c.id}`}
            </button>
            <button
              type="button"
              onClick={() => (props.onConnect as (c: SshConnection, p: string) => void)(c, "sftp")}
            >
              {`menu-sftp-${c.id}`}
            </button>
          </div>
        ))}
        <button type="button" onClick={() => (props.onCreate as () => void)()}>
          list-create
        </button>
        <button
          type="button"
          onClick={() => (props.onEdit as (c: SshConnection) => void)(connections[0])}
        >
          list-edit
        </button>
        <button
          type="button"
          onClick={() => (props.onCreateInGroup as (g: string) => void)("prod")}
        >
          list-create-in-prod
        </button>
      </div>
    );
  },
}));

vi.mock("../components/ssh/SshConnectionDialog", () => ({
  SshConnectionDialog: (props: Record<string, unknown>) => (
    <div data-testid="conn-dialog" data-initial-group={String(props.initialGroup)}>
      <span data-testid="dialog-editing">
        {props.connection ? (props.connection as SshConnection).id : "new"}
      </span>
      <button
        type="button"
        onClick={() =>
          (props.onSave as (c: SshConnection) => void)({
            id: "saved-1",
            name: "Saved",
            host: "h",
            port: 22,
            username: "u",
            group: "prod",
            createdAt: 1,
          })
        }
      >
        dialog-save
      </button>
      <button type="button" onClick={() => (props.onClose as () => void)()}>
        dialog-close
      </button>
    </div>
  ),
}));

vi.mock("../components/ssh/SshHostKeyDialog", () => ({
  SshHostKeyDialog: (props: Record<string, unknown>) => (
    <div data-testid="hostkey-dialog" data-target={String(props.target)}>
      <button type="button" onClick={() => (props.onTrusted as () => void)()}>
        hostkey-trust
      </button>
      <button type="button" onClick={() => (props.onCancel as () => void)()}>
        hostkey-cancel
      </button>
    </div>
  ),
}));

const { SshTerminalPanel } = await import("../components/ssh/SshTerminalPanel");

function conn(overrides: Partial<SshConnection> = {}): SshConnection {
  return {
    id: "c1",
    name: "Box One",
    host: "10.0.0.1",
    port: 22,
    username: "root",
    createdAt: 1,
    ...overrides,
  };
}

type PanelProps = Parameters<typeof SshTerminalPanel>[0];

function renderPanel(overrides: Partial<PanelProps> = {}) {
  const onConnectionsChange = vi.fn();
  const onReady = vi.fn();
  const onConnectSftp = vi.fn();
  const ref = createRef<SshTerminalPanelHandle>();
  const props: PanelProps = {
    connections: [conn()],
    onConnectionsChange,
    active: true,
    width: 800,
    themeVariant: "dark",
    terminalFontSize: 13,
    monoFontFamily: "mono",
    onReady,
    onConnectSftp,
    ...overrides,
  };
  const result = render(
    <I18nProvider>
      <SshTerminalPanel ref={ref} {...props} />
    </I18nProvider>,
  );
  const rerender = (next: Partial<PanelProps> = {}) =>
    result.rerender(
      <I18nProvider>
        <SshTerminalPanel ref={ref} {...props} {...next} />
      </I18nProvider>,
    );
  return { ...result, rerender, onConnectionsChange, onReady, onConnectSftp, ref };
}

/** 只回 `check_ssh_host_key`,其余命令一律成功。 */
function mockHostKey(status: { state: string; target?: string; keys?: unknown[] }) {
  invoke.mockImplementation((cmd: string) => {
    if (cmd === "check_ssh_host_key") return Promise.resolve(status);
    return Promise.resolve(undefined);
  });
}

function connectButton() {
  return screen.getByRole("button", { name: /Connect/i });
}

function disconnectButton() {
  return screen.getByRole("button", { name: /Disconnect/i });
}

function callsOf(cmd: string) {
  return invoke.mock.calls.filter(([name]) => name === cmd);
}

/**
 * 跑过 useEffect 里那个 50ms 的初始化延时。
 *
 * 必须先把 microtask 冲干净再推时钟:`startSession` 是在
 * `check_ssh_host_key` 的 `.then()` 里调的,会话要等这个 promise 落地才建,
 * 定时器也才注册。先推时钟会推到一个还不存在的定时器上,拿到 0 次调用。
 */
async function runInitTimer() {
  await act(async () => {
    await Promise.resolve();
    await Promise.resolve();
  });
  await act(async () => {
    vi.advanceTimersByTime(60);
    await Promise.resolve();
  });
}

beforeEach(() => {
  invoke.mockReset();
  invoke.mockResolvedValue(undefined);
  runtimeState.created.length = 0;
  channelState.instances.length = 0;
  localStorage.clear();
  // 分组名单有进程内快照缓存,不清会跨用例串(上一条建的组在下一条里还在)
  resetSshGroupNamesCache();
  vi.useFakeTimers({ shouldAdvanceTime: true });
});

afterEach(() => {
  vi.useRealTimers();
});

describe("host key 闸门", () => {
  it("未登记的主机:先弹确认框,这一步不开 shell", async () => {
    mockHostKey({ state: "unknown", target: "10.0.0.1:22", keys: [{ keyType: "ssh-ed25519" }] });
    renderPanel();
    fireEvent.click(connectButton());
    await waitFor(() => expect(screen.getByTestId("hostkey-dialog")).toBeInTheDocument());
    expect(screen.getByTestId("hostkey-dialog")).toHaveAttribute("data-target", "10.0.0.1:22");
    // 关键:确认之前一个 shell 都不能开
    await runInitTimer();
    expect(callsOf("open_ssh_shell")).toHaveLength(0);
  });

  it("确认指纹之后才真的开 shell,弹窗随之消失", async () => {
    mockHostKey({ state: "unknown", target: "10.0.0.1:22", keys: [] });
    renderPanel();
    fireEvent.click(connectButton());
    await waitFor(() => expect(screen.getByTestId("hostkey-dialog")).toBeInTheDocument());
    fireEvent.click(screen.getByRole("button", { name: "hostkey-trust" }));
    expect(screen.queryByTestId("hostkey-dialog")).not.toBeInTheDocument();
    await runInitTimer();
    expect(callsOf("open_ssh_shell")).toHaveLength(1);
  });

  it("取消确认:不开 shell,也不留下会话", async () => {
    mockHostKey({ state: "unknown", target: "10.0.0.1:22", keys: [] });
    renderPanel();
    fireEvent.click(connectButton());
    await waitFor(() => expect(screen.getByTestId("hostkey-dialog")).toBeInTheDocument());
    fireEvent.click(screen.getByRole("button", { name: "hostkey-cancel" }));
    expect(screen.queryByTestId("hostkey-dialog")).not.toBeInTheDocument();
    await runInitTimer();
    expect(callsOf("open_ssh_shell")).toHaveLength(0);
    // 还停在未连接状态:显示的是 Connect 而不是 Disconnect
    expect(connectButton()).toBeInTheDocument();
  });

  it("已登记的主机直接连,不弹框", async () => {
    mockHostKey({ state: "known" });
    renderPanel();
    fireEvent.click(connectButton());
    await runInitTimer();
    expect(screen.queryByTestId("hostkey-dialog")).not.toBeInTheDocument();
    expect(callsOf("open_ssh_shell")).toHaveLength(1);
  });

  it("host key 查不出来时照常连 —— 这一步只改措辞,不该变成新的失败点", async () => {
    invoke.mockImplementation((cmd: string) => {
      if (cmd === "check_ssh_host_key") return Promise.reject(new Error("no ssh binary"));
      return Promise.resolve(undefined);
    });
    renderPanel();
    fireEvent.click(connectButton());
    await runInitTimer();
    expect(screen.queryByTestId("hostkey-dialog")).not.toBeInTheDocument();
    expect(callsOf("open_ssh_shell")).toHaveLength(1);
  });
});

describe("孤儿 shell 的回收", () => {
  /** 连上一条,返回它的 shellId。 */
  async function connectFirst() {
    mockHostKey({ state: "known" });
    const api = renderPanel();
    fireEvent.click(connectButton());
    await runInitTimer();
    const call = callsOf("open_ssh_shell")[0];
    return { ...api, shellId: (call[1] as { shellId: string }).shellId };
  }

  it("已连接时再连:先杀旧 shell 再开新的", async () => {
    const { shellId } = await connectFirst();
    invoke.mockClear();
    mockHostKey({ state: "known" });
    // 已连接时头部按钮是 Disconnect,所以走列表右键菜单那条路径再连一次
    fireEvent.click(screen.getByText("menu-ssh-c1"));
    await runInitTimer();
    const kills = callsOf("kill_ssh_shell");
    expect(kills).toHaveLength(1);
    expect(kills[0][1]).toMatchObject({ shellId });
    // 新 shellId 必须与旧的不同,否则后端会认成同一个会话
    const opened = callsOf("open_ssh_shell");
    expect(opened).toHaveLength(1);
    expect((opened[0][1] as { shellId: string }).shellId).not.toBe(shellId);
  });

  it("Disconnect 杀掉 shell 并回到未连接状态", async () => {
    const { shellId } = await connectFirst();
    invoke.mockClear();
    fireEvent.click(disconnectButton());
    const kills = callsOf("kill_ssh_shell");
    expect(kills).toHaveLength(1);
    expect(kills[0][1]).toMatchObject({ shellId });
    expect(connectButton()).toBeInTheDocument();
  });

  it("删掉正在连接的那条:同时杀 shell", async () => {
    const { shellId } = await connectFirst();
    invoke.mockClear();
    fireEvent.click(screen.getByText("delete-c1"));
    const kills = callsOf("kill_ssh_shell");
    expect(kills).toHaveLength(1);
    expect(kills[0][1]).toMatchObject({ shellId });
  });

  it("删掉没在连接的那条:不碰现有 shell", async () => {
    mockHostKey({ state: "known" });
    renderPanel({ connections: [conn(), conn({ id: "c2", name: "Box Two" })] });
    fireEvent.click(connectButton());
    await runInitTimer();
    invoke.mockClear();
    fireEvent.click(screen.getByText("delete-c2"));
    expect(callsOf("kill_ssh_shell")).toHaveLength(0);
  });

  it("卸载时销毁 runtime(否则 xterm 和 PTY 都留着)", async () => {
    const { unmount } = await connectFirst();
    const runtime = runtimeState.created.at(-1)!;
    expect(runtime.dispose).not.toHaveBeenCalled();
    unmount();
    expect(runtime.dispose).toHaveBeenCalledTimes(1);
  });

  it("50ms 内就卸载:根本不开 shell", async () => {
    // 变异测试结论:这里也是两道闸门 —— cleanup 里的 `clearTimeout`,和回调开头的
    // `if (cleaned) return`。单摘任一道全绿,两道一起摘才被这条抓到。
    mockHostKey({ state: "known" });
    const { unmount } = renderPanel();
    fireEvent.click(connectButton());
    await act(async () => {
      await Promise.resolve();
      await Promise.resolve();
    });
    await act(async () => {
      vi.advanceTimersByTime(20);
    });
    unmount();
    await act(async () => {
      vi.advanceTimersByTime(60);
    });
    expect(callsOf("open_ssh_shell")).toHaveLength(0);
  });
});

describe("终端接线", () => {
  async function connected() {
    mockHostKey({ state: "known" });
    const api = renderPanel();
    fireEvent.click(connectButton());
    await runInitTimer();
    const shellId = (callsOf("open_ssh_shell")[0][1] as { shellId: string }).shellId;
    return { ...api, shellId, runtime: runtimeState.created.at(-1)! };
  }

  it("键入转成 send_input,带上本会话的 shellId", async () => {
    const { runtime, shellId } = await connected();
    invoke.mockClear();
    runtime.onInput("ls -la\r");
    expect(invoke).toHaveBeenCalledWith("send_input", { taskId: shellId, data: "ls -la\r" });
  });

  it("尺寸变化转成 resize_pty", async () => {
    const { runtime, shellId } = await connected();
    invoke.mockClear();
    runtime.onResize?.({ cols: 120, rows: 40 });
    expect(invoke).toHaveBeenCalledWith("resize_pty", { taskId: shellId, cols: 120, rows: 40 });
  });

  it("open_ssh_shell 带上 runtime 实际的行列数,不是写死的 80x24", async () => {
    await connected();
    expect(callsOf("open_ssh_shell")[0][1]).toMatchObject({ cols: 90, rows: 30 });
  });

  it("Channel 收到的输出写进终端", async () => {
    const { runtime } = await connected();
    const channel = channelState.instances.at(-1)!;
    channel.onmessage?.("hello from remote");
    expect(runtime.writes).toContain("hello from remote");
  });

  it("卸载后再来的输出不再写(cleaned 闸门)", async () => {
    const { runtime, unmount } = await connected();
    const channel = channelState.instances.at(-1)!;
    unmount();
    channel.onmessage?.("late output");
    expect(runtime.writes).not.toContain("late output");
  });

  it("open_ssh_shell 成功后回调 onReady 并聚焦", async () => {
    mockHostKey({ state: "known" });
    const { onReady } = renderPanel();
    fireEvent.click(connectButton());
    await runInitTimer();
    expect(onReady).toHaveBeenCalledTimes(1);
    expect(runtimeState.created.at(-1)!.focus).toHaveBeenCalled();
  });

  it("面板不 active 时不抢焦点", async () => {
    mockHostKey({ state: "known" });
    renderPanel({ active: false, autoConnect: false });
    // active=false 时头部按钮仍可点
    fireEvent.click(connectButton());
    await runInitTimer();
    expect(runtimeState.created.at(-1)!.focus).not.toHaveBeenCalled();
  });

  it("open_ssh_shell 失败:错误横幅 + 终端里也写一行", async () => {
    invoke.mockImplementation((cmd: string) => {
      if (cmd === "check_ssh_host_key") return Promise.resolve({ state: "known" });
      if (cmd === "open_ssh_shell") return Promise.reject(new Error("permission denied"));
      return Promise.resolve(undefined);
    });
    renderPanel();
    fireEvent.click(connectButton());
    await runInitTimer();
    expect(screen.getByText(/permission denied/)).toBeInTheDocument();
    expect(runtimeState.created.at(-1)!.writeln.join("\n")).toContain("permission denied");
  });

  it("ref.sendCommand 发到当前会话", async () => {
    const { ref, shellId } = await connected();
    invoke.mockClear();
    ref.current!.sendCommand("whoami\r");
    expect(invoke).toHaveBeenCalledWith("send_input", { taskId: shellId, data: "whoami\r" });
  });

  it("没有会话时 ref.sendCommand 是空操作,不是抛错", async () => {
    mockHostKey({ state: "known" });
    const { ref } = renderPanel();
    invoke.mockClear();
    expect(() => ref.current!.sendCommand("whoami\r")).not.toThrow();
    expect(callsOf("send_input")).toHaveLength(0);
  });
});

describe("自动连接", () => {
  it("autoConnect + active 时自己连上", async () => {
    mockHostKey({ state: "known" });
    renderPanel({ autoConnect: true });
    await runInitTimer();
    expect(callsOf("open_ssh_shell")).toHaveLength(1);
  });

  it("同一条连接只自动连一次(重渲染不会连第二次)", async () => {
    mockHostKey({ state: "known" });
    const { rerender } = renderPanel({ autoConnect: true });
    await runInitTimer();
    expect(callsOf("open_ssh_shell")).toHaveLength(1);
    rerender({ terminalFontSize: 15 });
    await runInitTimer();
    expect(callsOf("open_ssh_shell")).toHaveLength(1);
  });

  it("不 active 时不自动连", async () => {
    mockHostKey({ state: "known" });
    renderPanel({ autoConnect: true, active: false });
    await runInitTimer();
    expect(callsOf("open_ssh_shell")).toHaveLength(0);
  });

  it("autoConnect 关掉就不自动连", async () => {
    mockHostKey({ state: "known" });
    renderPanel({ autoConnect: false });
    await runInitTimer();
    expect(callsOf("open_ssh_shell")).toHaveLength(0);
  });

  it("离开再回来会重置去重记录,断线后能再自动连上", async () => {
    // active 转 false 时 autoConnectStartedRef 清空 —— 这是"切走再切回来
    // 应该重新连"的依据。少了这一步,断线后切回来会永远停在未连接。
    mockHostKey({ state: "known" });
    const { rerender } = renderPanel({ autoConnect: true });
    await runInitTimer();
    fireEvent.click(disconnectButton());
    rerender({ autoConnect: true, active: false });
    await runInitTimer();
    invoke.mockClear();
    mockHostKey({ state: "known" });
    rerender({ autoConnect: true, active: true });
    await runInitTimer();
    expect(callsOf("open_ssh_shell")).toHaveLength(1);
  });
});

describe("选中与连接列表", () => {
  it("initialConnectionId 决定初始选中项", () => {
    renderPanel({
      connections: [conn(), conn({ id: "c2", name: "Box Two" })],
      initialConnectionId: "c2",
    });
    expect(screen.getByTestId("conn-list")).toHaveAttribute("data-selected", "c2");
    expect(screen.getByText("Box Two")).toBeInTheDocument();
  });

  it("initialConnectionId 变化会跟着切换", () => {
    const { rerender } = renderPanel({
      connections: [conn(), conn({ id: "c2", name: "Box Two" })],
      initialConnectionId: "c1",
    });
    rerender({ initialConnectionId: "c2" });
    expect(screen.getByTestId("conn-list")).toHaveAttribute("data-selected", "c2");
  });

  it("选中的连接被删掉后回落到第一条", () => {
    const { rerender } = renderPanel({
      connections: [conn(), conn({ id: "c2", name: "Box Two" })],
      initialConnectionId: "c2",
    });
    rerender({ connections: [conn()], initialConnectionId: undefined });
    expect(screen.getByTestId("conn-list")).toHaveAttribute("data-selected", "c1");
  });

  it("一条连接都没有时:标题回落到默认文案,Connect 禁用", () => {
    renderPanel({ connections: [] });
    expect(connectButton()).toBeDisabled();
    // 标题位显示 ssh.title 而不是空白
    expect(screen.getByText("SSH")).toBeInTheDocument();
    expect(screen.getByText(/Select a saved connection/i)).toBeInTheDocument();
  });

  it("右键菜单选 sftp 走 onConnectSftp,不开 ssh shell", async () => {
    mockHostKey({ state: "known" });
    const { onConnectSftp } = renderPanel();
    fireEvent.click(screen.getByText("menu-sftp-c1"));
    await runInitTimer();
    expect(onConnectSftp).toHaveBeenCalledTimes(1);
    expect(callsOf("open_ssh_shell")).toHaveLength(0);
  });

  it("hideConnectionList 时不渲染列表,也不渲染连接对话框", () => {
    renderPanel({ hideConnectionList: true });
    expect(screen.queryByTestId("conn-list")).not.toBeInTheDocument();
  });
});

describe("连接对话框", () => {
  it("新建:对话框以空白态打开", () => {
    renderPanel();
    fireEvent.click(screen.getByText("list-create"));
    expect(screen.getByTestId("dialog-editing")).toHaveTextContent("new");
    expect(screen.getByTestId("conn-dialog")).toHaveAttribute("data-initial-group", "");
  });

  it("编辑:带着被编辑的连接打开", () => {
    renderPanel();
    fireEvent.click(screen.getByText("list-edit"));
    expect(screen.getByTestId("dialog-editing")).toHaveTextContent("c1");
  });

  it("在分组标题上新建:预填该分组", () => {
    renderPanel();
    fireEvent.click(screen.getByText("list-create-in-prod"));
    expect(screen.getByTestId("conn-dialog")).toHaveAttribute("data-initial-group", "prod");
  });

  it("保存新连接:插到最前面并选中它", () => {
    const { onConnectionsChange } = renderPanel();
    fireEvent.click(screen.getByText("list-create"));
    fireEvent.click(screen.getByText("dialog-save"));
    const next = onConnectionsChange.mock.calls.at(-1)![0] as SshConnection[];
    expect(next.map((c) => c.id)).toEqual(["saved-1", "c1"]);
    expect(screen.queryByTestId("conn-dialog")).not.toBeInTheDocument();
  });

  it("保存已存在的连接:就地替换,不新增一条", () => {
    const { onConnectionsChange } = renderPanel({
      connections: [conn({ id: "saved-1", name: "Old" }), conn({ id: "c9" })],
    });
    fireEvent.click(screen.getByText("list-create"));
    fireEvent.click(screen.getByText("dialog-save"));
    const next = onConnectionsChange.mock.calls.at(-1)![0] as SshConnection[];
    expect(next).toHaveLength(2);
    expect(next.find((c) => c.id === "saved-1")!.name).toBe("Saved");
  });

  it("保存时把手输的分组也登记进名单(否则移走最后一条连接分组就消失)", () => {
    renderPanel();
    fireEvent.click(screen.getByText("list-create"));
    fireEvent.click(screen.getByText("dialog-save"));
    expect(localStorage.getItem(SSH_GROUPS_STORAGE_KEY) ?? "").toContain("prod");
  });

  it("关闭对话框会清掉编辑态和预填分组", () => {
    renderPanel();
    fireEvent.click(screen.getByText("list-edit"));
    fireEvent.click(screen.getByText("dialog-close"));
    expect(screen.queryByTestId("conn-dialog")).not.toBeInTheDocument();
    fireEvent.click(screen.getByText("list-create"));
    expect(screen.getByTestId("dialog-editing")).toHaveTextContent("new");
  });

  it("onDeleteConnection 给了就交给它,不自己改名单", () => {
    const onDeleteConnection = vi.fn();
    const { onConnectionsChange } = renderPanel({ onDeleteConnection });
    onConnectionsChange.mockClear();
    fireEvent.click(screen.getByText("delete-c1"));
    expect(onDeleteConnection).toHaveBeenCalledWith("c1");
    expect(onConnectionsChange).not.toHaveBeenCalled();
  });

  it("没给 onDeleteConnection 时自己把它从名单里摘掉", () => {
    const { onConnectionsChange } = renderPanel({
      connections: [conn(), conn({ id: "c2" })],
    });
    onConnectionsChange.mockClear();
    fireEvent.click(screen.getByText("delete-c1"));
    const next = onConnectionsChange.mock.calls.at(-1)![0] as SshConnection[];
    expect(next.map((c) => c.id)).toEqual(["c2"]);
  });
});

describe("连接时刷新 lastConnectedAt", () => {
  it("连接会写回 lastConnectedAt,只动这一条", async () => {
    mockHostKey({ state: "known" });
    const { onConnectionsChange } = renderPanel({
      connections: [conn(), conn({ id: "c2", lastConnectedAt: 111 })],
    });
    onConnectionsChange.mockClear();
    fireEvent.click(connectButton());
    await runInitTimer();
    const next = onConnectionsChange.mock.calls.at(-1)![0] as SshConnection[];
    expect(next.find((c) => c.id === "c1")!.lastConnectedAt).toBeGreaterThan(0);
    expect(next.find((c) => c.id === "c2")!.lastConnectedAt).toBe(111);
  });
});

describe("实时设置", () => {
  async function connected() {
    mockHostKey({ state: "known" });
    const api = renderPanel();
    fireEvent.click(connectButton());
    await runInitTimer();
    return { ...api, runtime: runtimeState.created.at(-1)! };
  }

  it("建 runtime 时用的是当前主题/字号/字体", async () => {
    const { runtime } = await connected();
    expect(runtime.themeVariant).toBe("dark");
    expect(runtime.terminalFontSize).toBe(13);
    expect(runtime.monoFontFamily).toBe("mono");
  });

  it("改主题会推给已有 runtime,不重建终端", async () => {
    const { runtime, rerender } = await connected();
    const createdBefore = runtimeState.created.length;
    rerender({ themeVariant: "light" });
    expect(runtime.updateTheme).toHaveBeenCalledWith("light");
    expect(runtimeState.created).toHaveLength(createdBefore);
  });

  it("改字号推给已有 runtime", async () => {
    const { runtime, rerender } = await connected();
    rerender({ terminalFontSize: 16 });
    expect(runtime.updateFontSize).toHaveBeenCalledWith(16);
  });

  it("改字体推给已有 runtime", async () => {
    const { runtime, rerender } = await connected();
    rerender({ monoFontFamily: "JetBrains Mono" });
    expect(runtime.updateFontFamily).toHaveBeenCalledWith("JetBrains Mono");
  });

  it("没有会话时改字号不会炸(runtimeRef 是空的)", () => {
    const { rerender } = renderPanel();
    expect(() => rerender({ terminalFontSize: 16 })).not.toThrow();
  });
});

describe("剩下的三条边界", () => {
  it("连接列表被清空:选中项归零而不是挂着一个不存在的 id", () => {
    const { rerender } = renderPanel();
    expect(screen.getByTestId("conn-list")).toHaveAttribute("data-selected", "c1");
    rerender({ connections: [] });
    // 列表没了,按标题位判断:回落到默认 ssh.title
    expect(screen.getByText("SSH")).toBeInTheDocument();
    expect(connectButton()).toBeDisabled();
  });

  it("点列表里的连接会改选中项", () => {
    renderPanel({ connections: [conn(), conn({ id: "c2", name: "Box Two" })] });
    fireEvent.click(screen.getByText("select-c2"));
    expect(screen.getByTestId("conn-list")).toHaveAttribute("data-selected", "c2");
    expect(screen.getByText("Box Two")).toBeInTheDocument();
  });

  it("runtime 的 isActive 读的是最新的 active,不是建终端那一刻的值", async () => {
    // 这个回调是 xterm 决定要不要吞按键用的。它读 ref 而非闭包变量,
    // 所以父组件把 active 翻过来之后必须立刻反映 —— 否则切走的面板
    // 还在抢键盘。
    mockHostKey({ state: "known" });
    const { rerender } = renderPanel({ active: true });
    fireEvent.click(connectButton());
    await runInitTimer();
    const runtime = runtimeState.created.at(-1)!;
    expect(runtime.isActive()).toBe(true);
    rerender({ active: false });
    expect(runtime.isActive()).toBe(false);
  });
});

describe("删除后的选中项回落(父组件真的会更新名单)", () => {
  /**
   * 前面几条删除用例的父组件是 vi.fn(),props 不会真的变 —— 那测不到
   * "名单少了一条之后选中项该落在哪"。这里用一个真的持状态的父组件,
   * 让 onConnectionsChange 真正把新名单写回去。
   */
  function StatefulHost({ initial }: { initial: SshConnection[] }) {
    const [list, setList] = useState(initial);
    return (
      <SshTerminalPanel
        connections={list}
        onConnectionsChange={setList}
        active
        width={800}
        themeVariant="dark"
        terminalFontSize={13}
        monoFontFamily="mono"
      />
    );
  }

  function renderHost(initial: SshConnection[]) {
    return render(
      <I18nProvider>
        <StatefulHost initial={initial} />
      </I18nProvider>,
    );
  }

  /**
   * 变异测试的结论记在这里:选中项回落有**三套**机制,互相兜底 ——
   *   1. `selectedConnection` 自己的 `?? connections[0] ?? null`(第 120 行)
   *   2. 那个同步 effect 里的 `setSelectedId(selectedConnection.id)`
   *   3. `handleDeleteConnection` 里的 `if (selectedId === connectionId) …`
   * 渲染只读 `selectedConnection`,从不直接读 `selectedId`,所以 2 和 3 单独摘掉、
   * 甚至两个一起摘掉,本文件 53 条全绿 —— 真正决定显示的是 1。
   * 2 和 3 是无可观测效果的状态卫生,属于「多道闸门互相兜底」,记为收敛候选
   * (见 HANDOFF §4),这一轮不动实现,所以不为它们单独补测试。
   * 下面三条守的是行为本身(不管由哪套机制实现),收敛之后应该照样绿。
   */
  it("删掉当前选中的那条,选中项落到剩下的第一条", () => {
    renderHost([conn({ id: "c1" }), conn({ id: "c2", name: "Box Two" })]);
    expect(screen.getByTestId("conn-list")).toHaveAttribute("data-selected", "c1");
    fireEvent.click(screen.getByText("delete-c1"));
    expect(screen.getByTestId("conn-list")).toHaveAttribute("data-selected", "c2");
  });

  it("删掉最后一条:选中项归零,Connect 禁用", () => {
    renderHost([conn({ id: "c1" })]);
    fireEvent.click(screen.getByText("delete-c1"));
    expect(connectButton()).toBeDisabled();
  });

  it("删掉没选中的那条:选中项不动", () => {
    renderHost([conn({ id: "c1" }), conn({ id: "c2" })]);
    fireEvent.click(screen.getByText("delete-c2"));
    expect(screen.getByTestId("conn-list")).toHaveAttribute("data-selected", "c1");
  });
});
