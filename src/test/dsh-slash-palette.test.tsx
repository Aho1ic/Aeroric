import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createRef } from "react";
import { I18nProvider } from "../i18n";
import { DSH_SLASH_COMMANDS, type DshSlashCommand } from "../dshSlashCommands";

/**
 * dsh 斜杠命令面板与二级选择器。
 *
 * 这两块决定用户敲出去的到底是哪条命令 —— 插错名字就是把 `/permission` 发成别的,
 * 后端照着执行。所以重点在:远端目录拉取失败必须回落到静态目录(不能变成空面板)、
 * popup 类命令不能直接插入(要先选参数)、`editorInsert` 返回 false 时不能关面板
 * (否则用户输入原地消失)。
 */

const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));

const { DshSlashPalette, DshSlashPicker } = await import("../components/DshSlashPalette");

function renderPalette(
  overrides: { editorInsert?: (name: string) => boolean; sessionId?: string } = {},
) {
  const editorInsert = vi.fn(overrides.editorInsert ?? (() => true));
  const onDismiss = vi.fn();
  const result = render(
    <I18nProvider>
      <DshSlashPalette
        editorInsert={editorInsert}
        onDismiss={onDismiss}
        sessionId={overrides.sessionId}
      />
    </I18nProvider>,
  );
  return { ...result, editorInsert, onDismiss };
}

function listbox() {
  return screen.getByRole("listbox");
}

function options() {
  return screen.queryAllByRole("option");
}

function optionNames() {
  return options().map((o) => o.textContent?.replace(/\u00a0/g, " ").trim() ?? "");
}

function queryInput() {
  return screen.getByRole("textbox");
}

function selectedOption() {
  return options().find((o) => o.getAttribute("aria-selected") === "true");
}

function selectedIndex() {
  return options().findIndex((o) => o.getAttribute("aria-selected") === "true");
}

/**
 * 手工塞进 body 的节点(模拟 PromptEditor / 局外元素)。
 * Testing Library 的自动清理只管 `render` 挂的东西,这些得自己收 ——
 * 漏收会让局外那个 `<input>` 活到后面的用例里,把 getByRole("textbox") 撞成多个。
 */
const strayNodes: HTMLElement[] = [];

function appendStray(node: HTMLElement) {
  document.body.appendChild(node);
  strayNodes.push(node);
  return node;
}

beforeEach(() => {
  invoke.mockReset();
  invoke.mockResolvedValue([]);
});

afterEach(() => {
  for (const node of strayNodes.splice(0)) node.remove();
});

describe("命令目录来源", () => {
  it("没有 sessionId 时用静态目录,不去拉远端", () => {
    renderPalette();
    expect(invoke).not.toHaveBeenCalled();
    expect(options()).toHaveLength(DSH_SLASH_COMMANDS.length);
  });

  it("远端返回数组:换成远端目录", async () => {
    invoke.mockResolvedValue([{ name: "deploy" }, { name: "rollback" }]);
    renderPalette({ sessionId: "s1" });
    await waitFor(() => expect(options()).toHaveLength(2));
    expect(optionNames().join(" ")).toContain("deploy");
    expect(invoke).toHaveBeenCalledWith("list_dsh_commands", { sessionId: "s1" });
  });

  it("远端返回 { commands } 包装:同样认", async () => {
    invoke.mockResolvedValue({ commands: [{ name: "deploy" }] });
    renderPalette({ sessionId: "s1" });
    await waitFor(() => expect(options()).toHaveLength(1));
    expect(optionNames()[0]).toContain("deploy");
  });

  it("远端里已知的命令沿用静态描述,未知的用兜底描述", async () => {
    invoke.mockResolvedValue([{ name: "compact" }, { name: "deploy" }]);
    renderPalette({ sessionId: "s1" });
    await waitFor(() => expect(options()).toHaveLength(2));
    // compact 命中静态目录,用的是自己的描述文案
    expect(options()[0].textContent).toContain("Compress conversation history");
    // deploy 不在静态目录里,回落到通用标题 dsh.slash.title
    expect(options()[1].textContent).toContain("Slash commands");
  });

  it("name 不是字符串的行被丢掉", async () => {
    invoke.mockResolvedValue([{ name: "ok" }, { name: 42 }, {}]);
    renderPalette({ sessionId: "s1" });
    await waitFor(() => expect(options()).toHaveLength(1));
    expect(optionNames()[0]).toContain("ok");
  });

  it("远端返回空列表:保留静态目录,不变成空面板", async () => {
    invoke.mockResolvedValue([]);
    renderPalette({ sessionId: "s1" });
    await waitFor(() => expect(invoke).toHaveBeenCalled());
    expect(options()).toHaveLength(DSH_SLASH_COMMANDS.length);
  });

  it("远端全被过滤掉后也保留静态目录", async () => {
    invoke.mockResolvedValue([{ name: 1 }, { name: null }]);
    renderPalette({ sessionId: "s1" });
    await waitFor(() => expect(invoke).toHaveBeenCalled());
    expect(options()).toHaveLength(DSH_SLASH_COMMANDS.length);
  });

  it("远端调用失败:静默回落到静态目录(老版本 DSH 没这个 RPC)", async () => {
    invoke.mockRejectedValue(new Error("unknown command"));
    renderPalette({ sessionId: "s1" });
    await waitFor(() => expect(invoke).toHaveBeenCalled());
    expect(options()).toHaveLength(DSH_SLASH_COMMANDS.length);
    // 不能把 RPC 错误当成用户可见错误
    expect(screen.queryByText(/unknown command/)).not.toBeInTheDocument();
  });

  it("响应在卸载后才回来:不写已卸载组件的 state", async () => {
    let resolve!: (v: unknown) => void;
    invoke.mockImplementation(() => new Promise((r) => (resolve = r)));
    const { unmount } = renderPalette({ sessionId: "s1" });
    unmount();
    const errors: unknown[] = [];
    const onError = (e: ErrorEvent) => errors.push(e.error);
    window.addEventListener("error", onError);
    resolve([{ name: "late" }]);
    await Promise.resolve();
    window.removeEventListener("error", onError);
    expect(errors).toHaveLength(0);
  });

  it("hasArg 由远端的 input.hint 决定", async () => {
    invoke.mockResolvedValue([{ name: "withhint", input: { hint: "x" } }, { name: "nohint" }]);
    renderPalette({ sessionId: "s1" });
    await waitFor(() => expect(options()).toHaveLength(2));
    // hasArg 不上屏,通过"点了之后插入的是纯名字"间接确认两者都能插
    fireEvent.click(options()[0]);
    expect(options().length).toBeGreaterThan(0);
  });
});

describe("过滤", () => {
  it("按前缀过滤,不是任意位置包含", () => {
    renderPalette();
    fireEvent.change(queryInput(), { target: { value: "comp" } });
    expect(optionNames().every((n) => n.includes("compact"))).toBe(true);
    // "ode" 是 model 的中段,前缀匹配不该命中
    fireEvent.change(queryInput(), { target: { value: "ode" } });
    expect(options()).toHaveLength(0);
  });

  it("大小写与首尾空白都不影响", () => {
    renderPalette();
    fireEvent.change(queryInput(), { target: { value: "  MOD " } });
    expect(optionNames()).toHaveLength(1);
    expect(optionNames()[0]).toContain("model");
  });

  it("空查询显示全部", () => {
    renderPalette();
    fireEvent.change(queryInput(), { target: { value: "mod" } });
    fireEvent.change(queryInput(), { target: { value: "" } });
    expect(options()).toHaveLength(DSH_SLASH_COMMANDS.length);
  });

  it("一个都不匹配时显示占位横杠,不是空白", () => {
    renderPalette();
    fireEvent.change(queryInput(), { target: { value: "zzzz" } });
    expect(options()).toHaveLength(0);
    expect(screen.getByText("—")).toBeInTheDocument();
  });

  it("改查询会把高亮拉回第一条", () => {
    renderPalette();
    fireEvent.keyDown(listbox(), { key: "ArrowDown" });
    fireEvent.keyDown(listbox(), { key: "ArrowDown" });
    expect(selectedIndex()).toBe(2);
    fireEvent.change(queryInput(), { target: { value: "p" } });
    expect(selectedIndex()).toBe(0);
  });
});

describe("键盘", () => {
  it("上下键在两端夹住,不越界", () => {
    renderPalette();
    fireEvent.change(queryInput(), { target: { value: "p" } });
    const total = options().length;
    expect(total).toBeGreaterThan(1);
    for (let i = 0; i < total + 3; i += 1) {
      fireEvent.keyDown(listbox(), { key: "ArrowDown" });
    }
    expect(selectedIndex()).toBe(total - 1);
    for (let i = 0; i < total + 3; i += 1) {
      fireEvent.keyDown(listbox(), { key: "ArrowUp" });
    }
    expect(selectedIndex()).toBe(0);
  });

  it("Enter 提交当前高亮项,插入的是它的名字", () => {
    const { editorInsert, onDismiss } = renderPalette();
    fireEvent.keyDown(listbox(), { key: "ArrowDown" });
    const name = selectedOption()!.textContent!.replace(/[/\s ]/g, "");
    fireEvent.keyDown(listbox(), { key: "Enter" });
    expect(editorInsert).toHaveBeenCalledTimes(1);
    expect(name.startsWith(editorInsert.mock.calls[0][0] as string)).toBe(true);
    expect(onDismiss).toHaveBeenCalledTimes(1);
  });

  it("过滤到空时按 Enter 什么都不做", () => {
    const { editorInsert, onDismiss } = renderPalette();
    fireEvent.change(queryInput(), { target: { value: "zzzz" } });
    fireEvent.keyDown(listbox(), { key: "Enter" });
    expect(editorInsert).not.toHaveBeenCalled();
    expect(onDismiss).not.toHaveBeenCalled();
  });

  it("Escape 关面板,不插入任何东西", () => {
    const { editorInsert, onDismiss } = renderPalette();
    fireEvent.keyDown(listbox(), { key: "Escape" });
    expect(onDismiss).toHaveBeenCalledTimes(1);
    expect(editorInsert).not.toHaveBeenCalled();
  });

  it("其他按键不拦(要能正常打字)", () => {
    const { onDismiss } = renderPalette();
    const event = fireEvent.keyDown(listbox(), { key: "a" });
    expect(event).toBe(true);
    expect(onDismiss).not.toHaveBeenCalled();
  });

  it("鼠标移过去也会改高亮", () => {
    renderPalette();
    fireEvent.mouseEnter(options()[3]);
    expect(selectedIndex()).toBe(3);
  });
});

describe("提交", () => {
  function clickByName(name: string) {
    const opt = options().find((o) => o.textContent?.includes(name));
    expect(opt, `找不到 ${name}`).toBeDefined();
    fireEvent.click(opt!);
  }

  it("普通命令:直接插入名字并关面板", () => {
    const { editorInsert, onDismiss } = renderPalette();
    clickByName("compact");
    expect(editorInsert).toHaveBeenCalledWith("compact");
    expect(onDismiss).toHaveBeenCalledTimes(1);
  });

  it("editorInsert 返回 false 时不关面板(光标不在斜杠上,关了用户就白敲了)", () => {
    const { editorInsert, onDismiss } = renderPalette({ editorInsert: () => false });
    clickByName("compact");
    expect(editorInsert).toHaveBeenCalledWith("compact");
    expect(onDismiss).not.toHaveBeenCalled();
    // 面板还在
    expect(listbox()).toBeInTheDocument();
  });

  it("popup 类命令不直接插入,先开二级选择器", () => {
    const { editorInsert, onDismiss } = renderPalette();
    clickByName("permission");
    expect(editorInsert).not.toHaveBeenCalled();
    expect(onDismiss).not.toHaveBeenCalled();
    // 换成了 picker:标题行带命令名,且有返回项
    expect(screen.getByText(/permission/)).toBeInTheDocument();
  });

  it("二级选择器选中参数后插入「名字 参数」", async () => {
    const { editorInsert, onDismiss } = renderPalette();
    clickByName("permission");
    await waitFor(() => expect(screen.getByText("read-only")).toBeInTheDocument());
    fireEvent.mouseDown(screen.getByText("workspace-write"));
    expect(editorInsert).toHaveBeenCalledWith("permission workspace-write");
    expect(onDismiss).toHaveBeenCalledTimes(1);
  });

  it("二级选择器里 editorInsert 失败也不关", async () => {
    const { onDismiss } = renderPalette({ editorInsert: () => false });
    clickByName("permission");
    await waitFor(() => expect(screen.getByText("read-only")).toBeInTheDocument());
    fireEvent.mouseDown(screen.getByText("read-only"));
    expect(onDismiss).not.toHaveBeenCalled();
  });

  it("从二级选择器返回能回到命令列表", async () => {
    renderPalette();
    clickByName("permission");
    await waitFor(() => expect(screen.getByText("read-only")).toBeInTheDocument());
    fireEvent.mouseDown(screen.getByText(/‹/));
    expect(queryInput()).toBeInTheDocument();
    expect(options()).toHaveLength(DSH_SLASH_COMMANDS.length);
  });
});

describe("二级选择器的候选来源", () => {
  function renderPicker(
    command: DshSlashCommand,
    extra: { projectPath?: string; keyboardTargetRef?: React.RefObject<HTMLElement | null> } = {},
  ) {
    const onPick = vi.fn();
    const onBack = vi.fn();
    const onDismiss = vi.fn();
    const result = render(
      <I18nProvider>
        <DshSlashPicker
          command={command}
          onPick={onPick}
          onBack={onBack}
          onDismiss={onDismiss}
          {...extra}
        />
      </I18nProvider>,
    );
    return { ...result, onPick, onBack, onDismiss };
  }

  const cmd = (popup: DshSlashCommand["popup"]): DshSlashCommand => ({
    name: String(popup),
    descriptionKey: "dsh.slash.title",
    hasArg: true,
    popup,
  });

  it("model:把所有分组的模型拉平成一个列表", async () => {
    invoke.mockResolvedValue({
      groups: [
        { models: [{ id: "a", name: "Alpha" }] },
        { models: [{ id: "b" }, { id: "c", name: "Gamma" }] },
      ],
    });
    renderPicker(cmd("model"));
    await waitFor(() => expect(screen.getAllByRole("option")).toHaveLength(3));
    // 没有 name 的用 id 兜底
    expect(screen.getByText("Alpha")).toBeInTheDocument();
    expect(screen.getByText("b")).toBeInTheDocument();
    expect(screen.getByText("Gamma")).toBeInTheDocument();
  });

  it("model:RPC 失败时显示空占位而不是报错", async () => {
    invoke.mockRejectedValue(new Error("no models"));
    renderPicker(cmd("model"));
    await waitFor(() => expect(screen.getByText("—")).toBeInTheDocument());
    expect(screen.queryByText(/no models/)).not.toBeInTheDocument();
  });

  it("model:响应里没有 groups 字段也不炸", async () => {
    invoke.mockResolvedValue({});
    renderPicker(cmd("model"));
    await waitFor(() => expect(screen.getByText("—")).toBeInTheDocument());
  });

  it("skill:用 skill 名字做候选,带上 projectPath", async () => {
    invoke.mockResolvedValue([{ name: "review" }, { name: "deploy" }]);
    renderPicker(cmd("skill"), { projectPath: "/work/app" });
    await waitFor(() => expect(screen.getAllByRole("option")).toHaveLength(2));
    expect(invoke).toHaveBeenCalledWith("list_project_skills", {
      projectPath: "/work/app",
      agent: "dsh",
    });
  });

  it("skill:没有 projectPath 时不带这个字段(而不是传 undefined)", async () => {
    invoke.mockResolvedValue([]);
    renderPicker(cmd("skill"));
    await waitFor(() => expect(invoke).toHaveBeenCalled());
    expect(invoke.mock.calls[0][1]).toEqual({ agent: "dsh" });
  });

  it("skill:RPC 失败回落成空列表", async () => {
    invoke.mockRejectedValue(new Error("nope"));
    renderPicker(cmd("skill"));
    await waitFor(() => expect(screen.getByText("—")).toBeInTheDocument());
  });

  it("skill:返回 null 也当空列表", async () => {
    invoke.mockResolvedValue(null);
    renderPicker(cmd("skill"));
    await waitFor(() => expect(screen.getByText("—")).toBeInTheDocument());
  });

  it("permission:三个固定档位,不走 RPC", async () => {
    renderPicker(cmd("permission"));
    await waitFor(() => expect(screen.getAllByRole("option")).toHaveLength(3));
    expect(screen.getByText("read-only")).toBeInTheDocument();
    expect(screen.getByText("workspace-write")).toBeInTheDocument();
    expect(screen.getByText("danger-full-access")).toBeInTheDocument();
    expect(invoke).not.toHaveBeenCalled();
  });

  it("subagent:没有候选(id 是会话内的),显示占位", async () => {
    renderPicker(cmd("subagent"));
    await waitFor(() => expect(screen.getByText("—")).toBeInTheDocument());
    expect(invoke).not.toHaveBeenCalled();
  });

  it("加载中先显示省略号", () => {
    invoke.mockImplementation(() => new Promise(() => {}));
    renderPicker(cmd("model"));
    expect(screen.getByText("…")).toBeInTheDocument();
  });

  it("标题行带命令名和描述", async () => {
    renderPicker(cmd("permission"));
    await waitFor(() => expect(screen.getAllByRole("option")).toHaveLength(3));
    expect(screen.getByText(/permission/)).toBeInTheDocument();
  });
});

describe("二级选择器的键盘接管", () => {
  /**
   * 这个选择器挂的是 document 级 capture 监听 —— 因为焦点还在 PromptEditor 里,
   * 键盘事件根本不会经过选择器自己的 DOM。所以它必须自己判断"焦点是不是在
   * 那个编辑器上",判错了就会去抢别处的按键。
   */
  function renderWithEditor(command: DshSlashCommand, opts: { useRef?: boolean } = {}) {
    const editor = document.createElement("div");
    editor.className = "prompt-editor";
    editor.tabIndex = 0;
    appendStray(editor);
    const ref = createRef<HTMLElement | null>();
    (ref as { current: HTMLElement | null }).current = opts.useRef ? editor : null;
    const onPick = vi.fn();
    const onBack = vi.fn();
    const onDismiss = vi.fn();
    const result = render(
      <I18nProvider>
        <DshSlashPicker
          command={command}
          onPick={onPick}
          onBack={onBack}
          onDismiss={onDismiss}
          {...(opts.useRef ? { keyboardTargetRef: ref } : {})}
        />
      </I18nProvider>,
    );
    return { ...result, editor, onPick, onBack, onDismiss };
  }

  const permissionCmd: DshSlashCommand = {
    name: "permission",
    descriptionKey: "dsh.slash.title",
    hasArg: true,
    popup: "permission",
  };

  function press(target: HTMLElement, key: string, init: KeyboardEventInit = {}) {
    target.focus();
    fireEvent.keyDown(target, { key, ...init });
  }

  it("焦点在 prompt-editor 上时接管方向键与 Enter", async () => {
    const { editor, onPick } = renderWithEditor(permissionCmd);
    await waitFor(() => expect(screen.getAllByRole("option")).toHaveLength(3));
    press(editor, "ArrowDown");
    press(editor, "Enter");
    expect(onPick).toHaveBeenCalledWith("workspace-write");
  });

  it("Tab 也提交", async () => {
    const { editor, onPick } = renderWithEditor(permissionCmd);
    await waitFor(() => expect(screen.getAllByRole("option")).toHaveLength(3));
    press(editor, "Tab");
    expect(onPick).toHaveBeenCalledWith("read-only");
  });

  it("Escape 关掉整个面板(不是只退回上一层)", async () => {
    const { editor, onDismiss, onBack } = renderWithEditor(permissionCmd);
    await waitFor(() => expect(screen.getAllByRole("option")).toHaveLength(3));
    press(editor, "Escape");
    expect(onDismiss).toHaveBeenCalledTimes(1);
    expect(onBack).not.toHaveBeenCalled();
  });

  it("方向键在两端夹住", async () => {
    const { editor, onPick } = renderWithEditor(permissionCmd);
    await waitFor(() => expect(screen.getAllByRole("option")).toHaveLength(3));
    for (let i = 0; i < 6; i += 1) press(editor, "ArrowDown");
    press(editor, "Enter");
    expect(onPick).toHaveBeenLastCalledWith("danger-full-access");
    for (let i = 0; i < 6; i += 1) press(editor, "ArrowUp");
    press(editor, "Enter");
    expect(onPick).toHaveBeenLastCalledWith("read-only");
  });

  it("焦点不在编辑器上时完全不接管", async () => {
    const { onPick, onDismiss } = renderWithEditor(permissionCmd);
    await waitFor(() => expect(screen.getAllByRole("option")).toHaveLength(3));
    const outsider = appendStray(document.createElement("input"));
    press(outsider, "Escape");
    press(outsider, "Enter");
    expect(onDismiss).not.toHaveBeenCalled();
    expect(onPick).not.toHaveBeenCalled();
  });

  it("给了 keyboardTargetRef 就只认那个元素,不再看 class", async () => {
    const { editor, onPick } = renderWithEditor(permissionCmd, { useRef: true });
    await waitFor(() => expect(screen.getAllByRole("option")).toHaveLength(3));
    press(editor, "Enter");
    expect(onPick).toHaveBeenCalledWith("read-only");

    // 另一个同样带 prompt-editor class 的元素不该被接管
    const other = document.createElement("div");
    other.className = "prompt-editor";
    other.tabIndex = 0;
    appendStray(other);
    onPick.mockClear();
    press(other, "Enter");
    expect(onPick).not.toHaveBeenCalled();
  });

  it("输入法组合中不接管(否则中文选字会被吃掉)", async () => {
    const { editor, onPick, onDismiss } = renderWithEditor(permissionCmd);
    await waitFor(() => expect(screen.getAllByRole("option")).toHaveLength(3));
    press(editor, "Enter", { isComposing: true });
    press(editor, "Escape", { isComposing: true });
    expect(onPick).not.toHaveBeenCalled();
    expect(onDismiss).not.toHaveBeenCalled();
  });

  it("候选还没到时方向键不接管,Enter 也不提交", async () => {
    invoke.mockImplementation(() => new Promise(() => {}));
    const { editor, onPick } = renderWithEditor({ ...permissionCmd, popup: "model" });
    expect(screen.getByText("…")).toBeInTheDocument();
    press(editor, "ArrowDown");
    press(editor, "Enter");
    expect(onPick).not.toHaveBeenCalled();
  });

  it("候选为空时方向键不接管", async () => {
    const { editor, onPick } = renderWithEditor({ ...permissionCmd, popup: "subagent" });
    await waitFor(() => expect(screen.getByText("—")).toBeInTheDocument());
    press(editor, "ArrowDown");
    press(editor, "Enter");
    expect(onPick).not.toHaveBeenCalled();
  });

  it("卸载后不再接管 document 上的按键", async () => {
    const { editor, unmount, onDismiss } = renderWithEditor(permissionCmd);
    await waitFor(() => expect(screen.getAllByRole("option")).toHaveLength(3));
    unmount();
    press(editor, "Escape");
    expect(onDismiss).not.toHaveBeenCalled();
  });
});

describe("二级选择器自身的 keyDown(焦点落在选择器里时)", () => {
  async function open() {
    const api = renderPalette();
    const opt = options().find((o) => o.textContent?.includes("permission"))!;
    fireEvent.click(opt);
    await waitFor(() => expect(screen.getByText("read-only")).toBeInTheDocument());
    return api;
  }

  it("Escape 退回命令列表(与 document 级那条 Escape 关整个面板不同)", async () => {
    const { onDismiss } = await open();
    fireEvent.keyDown(listbox(), { key: "Escape" });
    expect(queryInput()).toBeInTheDocument();
    expect(onDismiss).not.toHaveBeenCalled();
  });

  it("方向键 + Enter 选参数", async () => {
    const { editorInsert } = await open();
    fireEvent.keyDown(listbox(), { key: "ArrowDown" });
    fireEvent.keyDown(listbox(), { key: "Enter" });
    expect(editorInsert).toHaveBeenCalledWith("permission workspace-write");
  });

  it("方向键在两端夹住", async () => {
    const { editorInsert } = await open();
    for (let i = 0; i < 5; i += 1) fireEvent.keyDown(listbox(), { key: "ArrowDown" });
    fireEvent.keyDown(listbox(), { key: "Enter" });
    expect(editorInsert).toHaveBeenCalledWith("permission danger-full-access");
  });

  it("鼠标移过候选也改高亮", async () => {
    const { editorInsert } = await open();
    fireEvent.mouseEnter(screen.getByText("danger-full-access"));
    fireEvent.keyDown(listbox(), { key: "Enter" });
    expect(editorInsert).toHaveBeenCalledWith("permission danger-full-access");
  });
});

describe("二级选择器的错误分支", () => {
  function renderPicker(command: DshSlashCommand) {
    const onPick = vi.fn();
    const onBack = vi.fn();
    const onDismiss = vi.fn();
    render(
      <I18nProvider>
        <DshSlashPicker command={command} onPick={onPick} onBack={onBack} onDismiss={onDismiss} />
      </I18nProvider>,
    );
    return { onPick, onBack, onDismiss };
  }

  it("响应形状不对时把错误显示出来,而不是卡在加载中", async () => {
    // 内层 `.catch()` 只挡 reject;成功但形状不对(groups 不可迭代)会在
    // `for...of` 处抛出,落到外层 catch。这一条区分「RPC 失败」与「RPC 成功
    // 但回了垃圾」——后者必须让用户看见,否则永远停在 "…"。
    invoke.mockResolvedValue({ groups: 42 });
    renderPicker({
      name: "model",
      descriptionKey: "dsh.slash.title",
      hasArg: true,
      popup: "model",
    });
    await waitFor(() => expect(screen.getByText(/TypeError|not iterable/)).toBeInTheDocument());
    expect(screen.queryByText("…")).not.toBeInTheDocument();
  });

  it("skill 回了非数组:同样报错而不是卡住", async () => {
    invoke.mockResolvedValue({ nope: true });
    renderPicker({
      name: "skill",
      descriptionKey: "dsh.slash.title",
      hasArg: true,
      popup: "skill",
    });
    await waitFor(() => expect(screen.getByText(/TypeError|not a function/)).toBeInTheDocument());
  });

  it("选择器自身的 ArrowUp 也夹在 0", async () => {
    renderPicker({
      name: "permission",
      descriptionKey: "dsh.slash.title",
      hasArg: true,
      popup: "permission",
    });
    await waitFor(() => expect(screen.getAllByRole("option")).toHaveLength(3));
    const box = screen.getByRole("listbox");
    fireEvent.keyDown(box, { key: "ArrowDown" });
    fireEvent.keyDown(box, { key: "ArrowUp" });
    fireEvent.keyDown(box, { key: "ArrowUp" });
    expect(screen.getAllByRole("option")[0]).toHaveAttribute("aria-selected", "true");
  });
});

describe("在途响应的归属", () => {
  it("换了 sessionId 之后,旧 session 的迟到响应不能覆盖新的", async () => {
    // 这条才是 `disposed` 闸门真正守的东西。
    // 原先那条「卸载后不写 state」测不到它:React 19 对卸载后 setState 不再告警,
    // 所以「没抛错」是空断言。只有"旧响应比新响应晚到"才有可观测差异 ——
    // 缺了闸门就会把上一个会话的命令目录显示成当前会话的。
    const resolvers: Array<(v: unknown) => void> = [];
    invoke.mockImplementation(() => new Promise((r) => resolvers.push(r)));
    const { rerender } = render(
      <I18nProvider>
        <DshSlashPalette editorInsert={() => true} onDismiss={() => {}} sessionId="old" />
      </I18nProvider>,
    );
    rerender(
      <I18nProvider>
        <DshSlashPalette editorInsert={() => true} onDismiss={() => {}} sessionId="new" />
      </I18nProvider>,
    );
    expect(resolvers).toHaveLength(2);
    resolvers[1]({ commands: [{ name: "from-new" }] });
    await act(async () => {
      await Promise.resolve();
    });
    expect(optionNames()[0]).toContain("from-new");
    resolvers[0]({ commands: [{ name: "from-old" }] });
    await act(async () => {
      await Promise.resolve();
      await Promise.resolve();
    });
    expect(optionNames()[0]).toContain("from-new");
    expect(optionNames()[0]).not.toContain("from-old");
  });

  it("候选值为空串时插入的是纯命令名,不带尾随空格", async () => {
    // `.trim()` 只在参数为空时才有区别。远端 skill 目录回一条空名字就会走到这里,
    // 不 trim 会把 "skill " 发出去,dsh 那边按带空参数解析。
    // 注意必须从面板点进二级选择器,走面板自己的 onPick —— 在测试里自己拼一遍
    // `.trim()` 就变成测试测自己了。
    invoke.mockResolvedValue([{ name: "" }]);
    const { editorInsert } = renderPalette();
    fireEvent.click(options().find((o) => o.textContent?.includes("skill"))!);
    await waitFor(() => expect(screen.getAllByRole("option")).toHaveLength(1));
    fireEvent.mouseDown(screen.getAllByRole("option")[0]);
    expect(editorInsert).toHaveBeenCalledWith("skill");
  });
});
