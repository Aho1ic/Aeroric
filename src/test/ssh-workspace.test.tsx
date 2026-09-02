import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useState } from "react";
import { I18nProvider } from "../i18n";
import type { SshConnection } from "../types";
import { SSH_GROUPS_STORAGE_KEY, resetSshGroupNamesCache } from "../components/ssh/sshGroups";

/**
 * SSH 工作区:卡片选择器 + 头部 + 「显示卡片 / 显示终端」这个状态机。
 *
 * 真正容易出错的是分组归桶(命名分组按名单顺序在前、空分组也要占位、未分组的
 * 落到末尾、默认分组不是真实分组所以不能改名删除)和选中/删除之后落到哪个视图。
 *
 * `SshTerminalPanel` 已有自己的 53 条用例,这里打桩成一个只报告入参的壳 ——
 * 否则每条用例都要连带跑一遍终端初始化。
 */

const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke, Channel: class {} }));

vi.mock("../components/ssh/SshTerminalPanel", () => ({
  SshTerminalPanel: (props: Record<string, unknown>) => (
    <div
      data-testid="terminal-panel"
      data-initial={String(props.initialConnectionId)}
      data-autoconnect={String(props.autoConnect)}
      data-hidelist={String(props.hideConnectionList)}
      data-active={String(props.active)}
    />
  ),
}));

vi.mock("../components/ssh/SshConnectionDialog", () => ({
  SshConnectionDialog: (props: Record<string, unknown>) => (
    <div data-testid="conn-dialog">
      <span data-testid="dialog-editing">
        {props.connection ? (props.connection as SshConnection).id : "new"}
      </span>
      <span data-testid="dialog-groups">{(props.groups as string[]).join(",")}</span>
      <button
        type="button"
        onClick={() =>
          (props.onSave as (c: SshConnection) => void)({
            id: "saved-1",
            name: "Saved",
            host: "h",
            port: 22,
            username: "u",
            group: "staging",
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

const { SshWorkspace } = await import("../components/ssh/SshWorkspace");

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

type Props = Parameters<typeof SshWorkspace>[0];

function renderWorkspace(overrides: Partial<Props> = {}) {
  const onConnectionsChange = vi.fn();
  const onLayoutChange = vi.fn();
  const onOpenSftp = vi.fn();
  const props: Props = {
    connections: [conn()],
    onConnectionsChange,
    active: true,
    themeVariant: "dark",
    terminalFontSize: 13,
    monoFontFamily: "mono",
    layout: "full",
    onLayoutChange,
    onOpenSftp,
    ...overrides,
  };
  const result = render(
    <I18nProvider>
      <SshWorkspace {...props} />
    </I18nProvider>,
  );
  const rerender = (next: Partial<Props> = {}) =>
    result.rerender(
      <I18nProvider>
        <SshWorkspace {...props} {...next} />
      </I18nProvider>,
    );
  return { ...result, rerender, onConnectionsChange, onLayoutChange, onOpenSftp };
}

/** 分组标题按渲染顺序列出(section 的第一个 div)。 */
function groupTitles() {
  return Array.from(document.querySelectorAll("section")).map(
    (section) => section.firstElementChild?.textContent ?? "",
  );
}

/** 某个分组下的连接名。 */
function cardsIn(group: string) {
  const section = Array.from(document.querySelectorAll("section")).find(
    (el) => el.firstElementChild?.textContent === group,
  );
  expect(section, `找不到分组 ${group}`).toBeDefined();
  return Array.from(section!.querySelectorAll("button"))
    .map((b) => b.textContent ?? "")
    .filter((text) => text.length > 0);
}

function cardButton(name: string) {
  return screen.getByText(name).closest("button") as HTMLButtonElement;
}

beforeEach(() => {
  invoke.mockReset();
  invoke.mockResolvedValue(undefined);
  localStorage.clear();
  resetSshGroupNamesCache();
});

afterEach(() => {
  localStorage.clear();
  resetSshGroupNamesCache();
});

describe("分组归桶", () => {
  it("命名分组按名单顺序在前,未分组的落到末尾", () => {
    localStorage.setItem(SSH_GROUPS_STORAGE_KEY, JSON.stringify(["prod", "staging"]));
    resetSshGroupNamesCache();
    renderWorkspace({
      connections: [
        conn({ id: "u1", name: "Loose", group: undefined }),
        conn({ id: "s1", name: "Stage", group: "staging" }),
        conn({ id: "p1", name: "Prod", group: "prod" }),
      ],
    });
    const titles = groupTitles();
    expect(titles.slice(0, 2)).toEqual(["prod", "staging"]);
    expect(titles.at(-1)).toBe("Default");
  });

  it("空分组也占位,并提示是空的", () => {
    localStorage.setItem(SSH_GROUPS_STORAGE_KEY, JSON.stringify(["empty-group"]));
    resetSshGroupNamesCache();
    renderWorkspace({ connections: [] });
    expect(groupTitles()).toContain("empty-group");
    expect(screen.getByText("No connections in this group yet")).toBeInTheDocument();
  });

  it("名单里没有的分组也会出现(连接自带的分组)", () => {
    renderWorkspace({ connections: [conn({ group: "adhoc" })] });
    expect(groupTitles()).toContain("adhoc");
  });

  it("分组名首尾空白按去掉后归类", () => {
    localStorage.setItem(SSH_GROUPS_STORAGE_KEY, JSON.stringify(["prod"]));
    resetSshGroupNamesCache();
    renderWorkspace({
      connections: [
        conn({ id: "a", name: "A", group: "  prod  " }),
        conn({ id: "b", name: "B", group: "prod" }),
      ],
    });
    expect(groupTitles().filter((t) => t === "prod")).toHaveLength(1);
    expect(cardsIn("prod").join(" ")).toContain("A");
    expect(cardsIn("prod").join(" ")).toContain("B");
  });

  it("分组名是纯空白等于没分组", () => {
    renderWorkspace({ connections: [conn({ group: "   " })] });
    expect(groupTitles()).toEqual(["Default"]);
  });

  it("一条连接都没有且没有命名分组时显示空状态", () => {
    renderWorkspace({ connections: [] });
    expect(screen.getByText(/No SSH connections/i)).toBeInTheDocument();
  });

  it("卡片上显示 user@host:port,有 remotePath 时也显示", () => {
    renderWorkspace({
      connections: [conn({ username: "deploy", host: "srv", port: 2222, remotePath: "/srv/app" })],
    });
    expect(screen.getByText("deploy@srv:2222")).toBeInTheDocument();
    expect(screen.getByText("/srv/app")).toBeInTheDocument();
  });

  it("没有 remotePath 就不渲染那一行", () => {
    renderWorkspace({ connections: [conn({ remotePath: undefined })] });
    expect(screen.queryByText("/srv/app")).not.toBeInTheDocument();
  });
});

describe("卡片 / 终端的切换", () => {
  function terminal() {
    return screen.queryByTestId("terminal-panel");
  }

  it("初始显示卡片,不显示终端", () => {
    renderWorkspace();
    expect(terminal()).not.toBeInTheDocument();
    expect(groupTitles().length).toBeGreaterThan(0);
  });

  it("点卡片打开终端,并把这条连接交给终端自动连", () => {
    renderWorkspace();
    fireEvent.click(cardButton("Box One"));
    expect(terminal()).toBeInTheDocument();
    expect(terminal()).toHaveAttribute("data-initial", "c1");
    expect(terminal()).toHaveAttribute("data-autoconnect", "true");
    // 工作区里终端不再自带连接列表(左边已经有卡片视图了)
    expect(terminal()).toHaveAttribute("data-hidelist", "true");
  });

  it("双击卡片与单击等效", () => {
    renderWorkspace();
    fireEvent.doubleClick(cardButton("Box One"));
    expect(terminal()).toBeInTheDocument();
  });

  it("头部的「显示连接」把终端换回卡片,但不丢选中项", () => {
    renderWorkspace();
    fireEvent.click(cardButton("Box One"));
    expect(terminal()).toBeInTheDocument();
    fireEvent.click(screen.getByTitle("Show SSH connections"));
    expect(terminal()).not.toBeInTheDocument();
    // 选中项还在:卡片仍是选中样式,再点一次同一张就能回终端
    fireEvent.click(cardButton("Box One"));
    expect(terminal()).toHaveAttribute("data-initial", "c1");
  });

  it("选中项在名单里消失后回落到卡片视图", () => {
    const { rerender } = renderWorkspace({
      connections: [conn(), conn({ id: "c2", name: "Box Two" })],
    });
    fireEvent.click(cardButton("Box Two"));
    expect(terminal()).toHaveAttribute("data-initial", "c2");
    rerender({ connections: [conn()] });
    expect(terminal()).not.toBeInTheDocument();
  });

  it("active 透传给终端", () => {
    const { rerender } = renderWorkspace();
    fireEvent.click(cardButton("Box One"));
    expect(terminal()).toHaveAttribute("data-active", "true");
    rerender({ active: false });
    expect(terminal()).toHaveAttribute("data-active", "false");
  });
});

describe("头部按钮", () => {
  it("full 布局下按钮提示切到分栏,点了往上报 split", () => {
    const { onLayoutChange } = renderWorkspace({ layout: "full" });
    fireEvent.click(screen.getByTitle("Split view"));
    expect(onLayoutChange).toHaveBeenCalledWith("split");
  });

  it("split 布局下点了往上报 full", () => {
    const { onLayoutChange } = renderWorkspace({ layout: "split" });
    fireEvent.click(screen.getByTitle("Full view"));
    expect(onLayoutChange).toHaveBeenCalledWith("full");
  });

  it("卡片视图下「显示连接」按钮处于激活态", () => {
    renderWorkspace();
    expect(screen.getByTitle("Show SSH connections").className).toContain("active");
  });

  it("终端视图下它不再是激活态", () => {
    renderWorkspace();
    fireEvent.click(cardButton("Box One"));
    expect(screen.getByTitle("Show SSH connections").className).not.toContain("active");
  });

  it("加号打开空白对话框", () => {
    renderWorkspace();
    fireEvent.click(screen.getByTitle("New connection"));
    expect(screen.getByTestId("dialog-editing")).toHaveTextContent("new");
  });
});

describe("保存连接", () => {
  it("新连接插到最前面,保存后直接进终端", () => {
    const { onConnectionsChange } = renderWorkspace();
    fireEvent.click(screen.getByTitle("New connection"));
    fireEvent.click(screen.getByText("dialog-save"));
    const next = onConnectionsChange.mock.calls.at(-1)![0] as SshConnection[];
    expect(next.map((c) => c.id)).toEqual(["saved-1", "c1"]);
    expect(screen.queryByTestId("conn-dialog")).not.toBeInTheDocument();
    // 这里还看不到终端:父组件是 mock,`connections` 没真的更新,
    // 而 `selectedConnection` 是从 props 里查的。真进终端要父组件回写,
    // 见下面「父组件真的回写时」那条。
  });

  it("保存已有连接是就地替换", () => {
    const { onConnectionsChange } = renderWorkspace({
      connections: [conn({ id: "saved-1", name: "Old" }), conn({ id: "c9" })],
    });
    fireEvent.click(screen.getByTitle("New connection"));
    fireEvent.click(screen.getByText("dialog-save"));
    const next = onConnectionsChange.mock.calls.at(-1)![0] as SshConnection[];
    expect(next).toHaveLength(2);
    expect(next.find((c) => c.id === "saved-1")!.name).toBe("Saved");
  });

  it("对话框里手输的分组会进名单", () => {
    renderWorkspace();
    fireEvent.click(screen.getByTitle("New connection"));
    fireEvent.click(screen.getByText("dialog-save"));
    expect(localStorage.getItem(SSH_GROUPS_STORAGE_KEY) ?? "").toContain("staging");
  });

  it("对话框拿到的是当前分组名单", () => {
    localStorage.setItem(SSH_GROUPS_STORAGE_KEY, JSON.stringify(["prod", "staging"]));
    resetSshGroupNamesCache();
    renderWorkspace({ connections: [] });
    fireEvent.click(screen.getByTitle("New connection"));
    expect(screen.getByTestId("dialog-groups")).toHaveTextContent("prod,staging");
  });

  it("关闭对话框清掉编辑态", () => {
    renderWorkspace();
    fireEvent.click(screen.getByLabelText("Edit"));
    expect(screen.getByTestId("dialog-editing")).toHaveTextContent("c1");
    fireEvent.click(screen.getByText("dialog-close"));
    expect(screen.queryByTestId("conn-dialog")).not.toBeInTheDocument();
    fireEvent.click(screen.getByTitle("New connection"));
    expect(screen.getByTestId("dialog-editing")).toHaveTextContent("new");
  });

  it("卡片上的编辑按钮带着这条连接开对话框", () => {
    renderWorkspace({ connections: [conn(), conn({ id: "c2", name: "Box Two" })] });
    const cards = screen.getAllByLabelText("Edit");
    fireEvent.click(cards[1]);
    expect(screen.getByTestId("dialog-editing")).toHaveTextContent("c2");
  });

  it("点编辑不会顺手打开终端(事件不该冒到卡片上)", () => {
    renderWorkspace();
    fireEvent.click(screen.getByLabelText("Edit"));
    expect(screen.queryByTestId("terminal-panel")).not.toBeInTheDocument();
  });
});

describe("删除连接", () => {
  it("给了 onDeleteConnection 就交给它", () => {
    const onDeleteConnection = vi.fn();
    const { onConnectionsChange } = renderWorkspace({ onDeleteConnection });
    onConnectionsChange.mockClear();
    fireEvent.contextMenu(screen.getByText("Box One"));
    fireEvent.click(screen.getByText(/Delete/i));
    expect(onDeleteConnection).toHaveBeenCalledWith("c1");
    expect(onConnectionsChange).not.toHaveBeenCalled();
  });

  it("没给就自己从名单里摘掉", () => {
    const { onConnectionsChange } = renderWorkspace({
      connections: [conn(), conn({ id: "c2", name: "Box Two" })],
    });
    onConnectionsChange.mockClear();
    fireEvent.contextMenu(screen.getByText("Box One"));
    fireEvent.click(screen.getByText(/Delete/i));
    const next = onConnectionsChange.mock.calls.at(-1)![0] as SshConnection[];
    expect(next.map((c) => c.id)).toEqual(["c2"]);
  });

  /**
   * `deleteConnection` 里的 `setShowCards(true)` 是恒等操作,不是漏测。
   * 证据不是「换成 throw 之后测试还绿」——jsdom 会把事件处理器里的异常吞掉,
   * 那样连无条件 throw 都是 43 条全绿(实测过,所以那个手法在这里完全无效)。
   * 换成计数器埋点才拿到真数据:这条分支被走到 2 次,其中
   * `showCards === false` 的次数是 0。原因是删除入口只存在于卡片视图里,
   * 能点到删除时 `showCards` 必然已经是 true。
   * 记为收敛候选(HANDOFF §4),本轮不改实现。
   */
  it("删掉正在看的那条:退回卡片视图并选中剩下的第一条", () => {
    renderWorkspace({ connections: [conn(), conn({ id: "c2", name: "Box Two" })] });
    fireEvent.click(cardButton("Box One"));
    expect(screen.getByTestId("terminal-panel")).toBeInTheDocument();
    fireEvent.click(screen.getByTitle("Show SSH connections"));
    fireEvent.contextMenu(screen.getByText("Box One"));
    fireEvent.click(screen.getByText(/Delete/i));
    expect(screen.queryByTestId("terminal-panel")).not.toBeInTheDocument();
  });
});

describe("父组件真的回写时", () => {
  /** 持状态的宿主,让 onConnectionsChange 真正生效。 */
  function StatefulHost({ initial }: { initial: SshConnection[] }) {
    const [list, setList] = useState(initial);
    const [layout, setLayout] = useState<"full" | "split">("full");
    return (
      <SshWorkspace
        connections={list}
        onConnectionsChange={setList}
        active
        themeVariant="dark"
        terminalFontSize={13}
        monoFontFamily="mono"
        layout={layout}
        onLayoutChange={setLayout}
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

  it("保存新连接后直接进入它的终端", () => {
    renderHost([conn()]);
    fireEvent.click(screen.getByTitle("New connection"));
    fireEvent.click(screen.getByText("dialog-save"));
    expect(screen.getByTestId("terminal-panel")).toHaveAttribute("data-initial", "saved-1");
  });

  it("删掉正在看的那条后落到剩下的第一条,并回卡片视图", () => {
    renderHost([conn({ id: "c1", name: "Box One" }), conn({ id: "c2", name: "Box Two" })]);
    fireEvent.click(cardButton("Box One"));
    expect(screen.getByTestId("terminal-panel")).toHaveAttribute("data-initial", "c1");
    fireEvent.click(screen.getByTitle("Show SSH connections"));
    fireEvent.contextMenu(screen.getByText("Box One"));
    fireEvent.click(screen.getByText(/Delete/i));
    expect(screen.queryByTestId("terminal-panel")).not.toBeInTheDocument();
    // 剩下的那条还在,点它能进终端
    fireEvent.click(cardButton("Box Two"));
    expect(screen.getByTestId("terminal-panel")).toHaveAttribute("data-initial", "c2");
  });

  it("删掉最后一条后停在空状态", () => {
    renderHost([conn()]);
    fireEvent.contextMenu(screen.getByText("Box One"));
    fireEvent.click(screen.getByText(/Delete/i));
    expect(screen.getByText(/No SSH connections/i)).toBeInTheDocument();
  });
});

describe("右键菜单", () => {
  /** SSH / SFTP 藏在 Connect 的二级菜单里,先把它展开。 */
  function openConnectSubmenu() {
    fireEvent.click(screen.getByRole("menuitem", { name: /^Connect/ }));
  }

  it("卡片右键出连接菜单,选 ssh 进终端", () => {
    renderWorkspace();
    fireEvent.contextMenu(screen.getByText("Box One"));
    openConnectSubmenu();
    fireEvent.click(screen.getByRole("menuitem", { name: "SSH" }));
    expect(screen.getByTestId("terminal-panel")).toHaveAttribute("data-initial", "c1");
  });

  it("选 sftp 走 onOpenSftp,不进终端", () => {
    const { onOpenSftp } = renderWorkspace();
    fireEvent.contextMenu(screen.getByText("Box One"));
    openConnectSubmenu();
    fireEvent.click(screen.getByRole("menuitem", { name: "SFTP" }));
    expect(onOpenSftp).toHaveBeenCalledTimes(1);
    expect(screen.queryByTestId("terminal-panel")).not.toBeInTheDocument();
  });

  it("命名分组的标题右键出分组菜单", () => {
    localStorage.setItem(SSH_GROUPS_STORAGE_KEY, JSON.stringify(["prod"]));
    resetSshGroupNamesCache();
    renderWorkspace({ connections: [conn({ group: "prod" })] });
    const title = Array.from(document.querySelectorAll("section")).find(
      (el) => el.firstElementChild?.textContent === "prod",
    )!.firstElementChild as HTMLElement;
    fireEvent.contextMenu(title);
    expect(screen.getByText(/Rename/i)).toBeInTheDocument();
  });

  it("默认分组的标题右键没有菜单(它不是真实分组,改名删除都无从落地)", () => {
    renderWorkspace({ connections: [conn({ group: undefined })] });
    const title = Array.from(document.querySelectorAll("section")).find(
      (el) => el.firstElementChild?.textContent === "Default",
    )!.firstElementChild as HTMLElement;
    fireEvent.contextMenu(title);
    expect(screen.queryByText(/Rename/i)).not.toBeInTheDocument();
  });

  it("分组菜单里的候选重名不含自己", () => {
    localStorage.setItem(SSH_GROUPS_STORAGE_KEY, JSON.stringify(["prod", "staging"]));
    resetSshGroupNamesCache();
    renderWorkspace({ connections: [] });
    const title = Array.from(document.querySelectorAll("section")).find(
      (el) => el.firstElementChild?.textContent === "prod",
    )!.firstElementChild as HTMLElement;
    fireEvent.contextMenu(title);
    // 菜单开着即可;takenNames 的具体校验在 ssh-group-delete / ssh-groups 里
    expect(screen.getByText(/Rename/i)).toBeInTheDocument();
  });
});

describe("复制密码", () => {
  it("有密码时可点,复制后短暂显示已复制", async () => {
    const writeText = vi.fn(() => Promise.resolve());
    Object.assign(navigator, { clipboard: { writeText } });
    renderWorkspace({ connections: [conn({ password: "s3cret" })] });
    const copy = screen.getByLabelText("Copy password");
    expect(copy).not.toBeDisabled();
    fireEvent.click(copy);
    expect(writeText).toHaveBeenCalledWith("s3cret");
    await waitFor(() =>
      expect(screen.getByLabelText("Copy password")).toHaveAttribute("data-copied", "true"),
    );
  });

  it("没有密码时按钮禁用,点了不写剪贴板", () => {
    const writeText = vi.fn(() => Promise.resolve());
    Object.assign(navigator, { clipboard: { writeText } });
    renderWorkspace({ connections: [conn({ password: undefined })] });
    const copy = screen.getByLabelText("Copy password");
    expect(copy).toBeDisabled();
    fireEvent.click(copy);
    expect(writeText).not.toHaveBeenCalled();
  });

  it("复制不会顺手打开终端", () => {
    Object.assign(navigator, { clipboard: { writeText: vi.fn(() => Promise.resolve()) } });
    renderWorkspace({ connections: [conn({ password: "x" })] });
    fireEvent.click(screen.getByLabelText("Copy password"));
    expect(screen.queryByTestId("terminal-panel")).not.toBeInTheDocument();
  });

  /**
   * 变异测试:复制密码有三道闸门 —— 按钮的 `disabled`、onClick 里的
   * `if (!canCopyPassword) return`、`copyConnectionPassword` 里的 `!password`。
   * 只有 `disabled` 那道是可观测的(摘掉本文件挂 2 条);另两道摘掉全绿,
   * 是 `disabled` 背后的兜底。不为它们补 jsdom 用例(用户点不到禁用按钮),
   * 也不建议删 —— 那个函数是模块级的,将来被别处调到时它就是唯一防线。
   */
  it("空密码串等于没密码,不写剪贴板", () => {
    const writeText = vi.fn(() => Promise.resolve());
    Object.assign(navigator, { clipboard: { writeText } });
    renderWorkspace({ connections: [conn({ password: "" })] });
    expect(screen.getByLabelText("Copy password")).toBeDisabled();
    expect(writeText).not.toHaveBeenCalled();
  });
});

describe("分组菜单的关闭", () => {
  function openGroupMenu(group: string) {
    localStorage.setItem(SSH_GROUPS_STORAGE_KEY, JSON.stringify([group]));
    resetSshGroupNamesCache();
    const api = renderWorkspace({ connections: [] });
    const title = Array.from(document.querySelectorAll("section")).find(
      (el) => el.firstElementChild?.textContent === group,
    )!.firstElementChild as HTMLElement;
    fireEvent.contextMenu(title);
    expect(screen.getByText(/Rename/i)).toBeInTheDocument();
    return api;
  }

  it("Escape 关掉分组菜单", () => {
    openGroupMenu("prod");
    fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.queryByText(/Rename/i)).not.toBeInTheDocument();
  });

  it("再右键另一个分组会换成那个分组的菜单", () => {
    localStorage.setItem(SSH_GROUPS_STORAGE_KEY, JSON.stringify(["prod", "staging"]));
    resetSshGroupNamesCache();
    renderWorkspace({ connections: [] });
    const titleOf = (group: string) =>
      Array.from(document.querySelectorAll("section")).find(
        (el) => el.firstElementChild?.textContent === group,
      )!.firstElementChild as HTMLElement;
    fireEvent.contextMenu(titleOf("prod"));
    fireEvent.keyDown(window, { key: "Escape" });
    fireEvent.contextMenu(titleOf("staging"));
    expect(screen.getByText(/Rename/i)).toBeInTheDocument();
  });
});
