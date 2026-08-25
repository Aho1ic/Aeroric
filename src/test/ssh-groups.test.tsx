import { fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { I18nProvider } from "../i18n";
import { SshConnectionList } from "../components/ssh/SshConnectionList";
import {
  SSH_GROUPS_STORAGE_KEY,
  loadSshGroupNames,
  mergeSshGroupNames,
  removeSshGroupFromConnections,
  renameSshGroupInConnections,
  renameSshGroupName,
  resetSshGroupNamesCache,
  saveSshGroupNames,
} from "../components/ssh/sshGroups";
import type { SshConnection } from "../types";

function connection(overrides: Partial<SshConnection> = {}): SshConnection {
  return {
    id: "conn-1",
    name: "prod",
    host: "prod.example.com",
    port: 22,
    username: "deploy",
    createdAt: 1,
    ...overrides,
  };
}

function renderList(props: Partial<React.ComponentProps<typeof SshConnectionList>> = {}) {
  return render(
    <I18nProvider>
      <SshConnectionList
        connections={[]}
        selectedId={null}
        onSelect={vi.fn()}
        onCreate={vi.fn()}
        onEdit={vi.fn()}
        onDelete={vi.fn()}
        {...props}
      />
    </I18nProvider>,
  );
}

describe("SSH group names storage", () => {
  beforeEach(() => {
    localStorage.clear();
    resetSshGroupNamesCache();
  });

  it("round-trips through localStorage so empty groups survive a reload", () => {
    saveSshGroupNames(["edge", "staging"]);

    expect(loadSshGroupNames()).toEqual(["edge", "staging"]);
  });

  it("drops blanks and duplicates instead of rendering phantom groups", () => {
    saveSshGroupNames([" edge ", "edge", "", "   ", "staging"]);

    expect(loadSshGroupNames()).toEqual(["edge", "staging"]);
  });

  it("survives corrupted storage rather than throwing on mount", () => {
    localStorage.setItem(SSH_GROUPS_STORAGE_KEY, "{not json");

    expect(loadSshGroupNames()).toEqual([]);
  });

  /// 手改 json 或导入的配置可能带着名单里没有的分组,那些连接不能凭空消失。
  it("keeps groups that only exist on connections", () => {
    const merged = mergeSshGroupNames([connection({ group: "imported" })], ["edge"]);

    expect(merged).toEqual(["edge", "imported"]);
  });
});

describe("removeSshGroupFromConnections", () => {
  it("clears the label but keeps every connection", () => {
    const connections = [
      connection({ id: "a", group: "edge" }),
      connection({ id: "b", group: "staging" }),
    ];

    const next = removeSshGroupFromConnections(connections, "edge");

    expect(next).toHaveLength(2);
    expect(next?.[0]).not.toHaveProperty("group");
    expect(next?.[1]).toMatchObject({ id: "b", group: "staging" });
  });

  it("reports no change when nothing referenced the group", () => {
    expect(removeSshGroupFromConnections([connection({ group: "edge" })], "staging")).toBeNull();
    expect(removeSshGroupFromConnections([connection({ group: "edge" })], "  ")).toBeNull();
  });
});

describe("renameSshGroupName", () => {
  it("renames in place and keeps the order", () => {
    expect(renameSshGroupName(["edge", "staging"], "edge", "border")).toEqual([
      "border",
      "staging",
    ]);
  });

  /// 调用方靠 null 区分"没改动",不能比引用 —— `.map` 无论是否命中都产生新数组。
  it("reports no change when the source is absent or the target is taken", () => {
    expect(renameSshGroupName([], "edge", "border")).toBeNull();
    expect(renameSshGroupName(["edge", "staging"], "edge", "staging")).toBeNull();
    expect(renameSshGroupName(["edge"], "edge", "edge")).toBeNull();
    expect(renameSshGroupName(["edge"], "edge", "   ")).toBeNull();
  });
});

describe("renameSshGroupInConnections", () => {
  it("moves every connection in the group to the new name", () => {
    const next = renameSshGroupInConnections(
      [connection({ id: "a", group: "edge" }), connection({ id: "b", group: "staging" })],
      "edge",
      "border",
    );

    expect(next?.[0]).toMatchObject({ id: "a", group: "border" });
    expect(next?.[1]).toMatchObject({ id: "b", group: "staging" });
  });

  it("reports no change when nothing referenced the group", () => {
    expect(renameSshGroupInConnections([connection()], "edge", "border")).toBeNull();
    expect(renameSshGroupInConnections([connection({ group: "edge" })], "edge", "edge")).toBeNull();
  });
});

describe("SSH connection list groups", () => {
  beforeEach(() => {
    localStorage.clear();
    resetSshGroupNamesCache();
  });

  it("creates a group from the header button", async () => {
    const user = userEvent.setup();
    const onCreateGroup = vi.fn();
    renderList({ onCreateGroup });

    await user.click(screen.getByRole("button", { name: "New group" }));
    await user.type(screen.getByLabelText("Group name"), "edge");
    await user.keyboard("{Enter}");

    expect(onCreateGroup).toHaveBeenCalledWith("edge");
  });

  it("ignores a blank name so the list cannot grow an unnamed group", async () => {
    const user = userEvent.setup();
    const onCreateGroup = vi.fn();
    renderList({ onCreateGroup });

    await user.click(screen.getByRole("button", { name: "New group" }));
    await user.type(screen.getByLabelText("Group name"), "   ");
    await user.keyboard("{Enter}");

    expect(onCreateGroup).not.toHaveBeenCalled();
  });

  it("refuses a duplicate name instead of listing the group twice", async () => {
    const user = userEvent.setup();
    const onCreateGroup = vi.fn();
    renderList({ groupNames: ["edge"], onCreateGroup });

    await user.click(screen.getByRole("button", { name: "New group" }));
    await user.type(screen.getByLabelText("Group name"), "edge");
    await user.keyboard("{Enter}");

    expect(onCreateGroup).not.toHaveBeenCalled();
  });

  it("abandons the draft on Escape", async () => {
    const user = userEvent.setup();
    const onCreateGroup = vi.fn();
    renderList({ onCreateGroup });

    await user.click(screen.getByRole("button", { name: "New group" }));
    await user.type(screen.getByLabelText("Group name"), "edge");
    await user.keyboard("{Escape}");

    expect(onCreateGroup).not.toHaveBeenCalled();
    expect(screen.queryByLabelText("Group name")).not.toBeInTheDocument();
  });

  /// 空分组必须看得见,否则建完分组界面毫无变化,用户以为没生效。
  it("shows a group that has no connections yet", () => {
    renderList({ groupNames: ["edge"] });

    expect(screen.getByText("edge")).toBeInTheDocument();
    expect(screen.getByText("No connections in this group yet")).toBeInTheDocument();
  });

  it("keeps ungrouped connections under the default group", () => {
    renderList({ connections: [connection()], groupNames: ["edge"] });

    expect(screen.getByText("edge")).toBeInTheDocument();
    expect(screen.getByText("Default")).toBeInTheDocument();
  });

  it("creates a connection prefilled into the group from the group header", async () => {
    const user = userEvent.setup();
    const onCreateInGroup = vi.fn();
    renderList({ groupNames: ["edge"], onCreateInGroup });

    await user.click(screen.getByRole("button", { name: "New connection in edge" }));

    expect(onCreateInGroup).toHaveBeenCalledWith("edge");
  });

  it("deletes a group from its header", async () => {
    const user = userEvent.setup();
    const onDeleteGroup = vi.fn();
    renderList({
      connections: [connection({ group: "edge" })],
      groupNames: ["edge"],
      onDeleteGroup,
    });

    await user.click(screen.getByRole("button", { name: /Delete group edge/ }));

    expect(onDeleteGroup).toHaveBeenCalledWith("edge");
  });

  /// 默认分组不是真实分组,给它删除按钮会让人以为能删掉那些连接。
  it("offers no delete action on the default group", () => {
    renderList({ connections: [connection()], onDeleteGroup: vi.fn() });

    expect(screen.queryByRole("button", { name: /Delete group/ })).not.toBeInTheDocument();
  });

  /// 宿主没接分组回调时(SshWorkspace 那条路径),不该冒出建分组入口。
  it("hides the group button when the host wires no handler", () => {
    renderList();

    expect(screen.queryByRole("button", { name: "New group" })).not.toBeInTheDocument();
  });
});

describe("SSH group context menu", () => {
  beforeEach(() => {
    localStorage.clear();
    resetSshGroupNamesCache();
  });

  function openGroupMenu(group: string) {
    fireEvent.contextMenu(screen.getByText(group), { clientX: 60, clientY: 60 });
  }

  it("renames a group from the right-click menu", async () => {
    const user = userEvent.setup();
    const onRenameGroup = vi.fn();
    renderList({ groupNames: ["edge"], onRenameGroup, onDeleteGroup: vi.fn() });

    openGroupMenu("edge");
    await user.click(screen.getByRole("menuitem", { name: "Rename group" }));
    const input = screen.getByLabelText("Rename group");
    await user.clear(input);
    await user.type(input, "border");
    await user.keyboard("{Enter}");

    expect(onRenameGroup).toHaveBeenCalledWith("edge", "border");
  });

  /// 重名会把两个分组悄悄合并,那不是重命名该有的结果。
  it("refuses to rename onto an existing group", async () => {
    const user = userEvent.setup();
    const onRenameGroup = vi.fn();
    renderList({ groupNames: ["edge", "staging"], onRenameGroup, onDeleteGroup: vi.fn() });

    openGroupMenu("edge");
    await user.click(screen.getByRole("menuitem", { name: "Rename group" }));
    const input = screen.getByLabelText("Rename group");
    await user.clear(input);
    await user.type(input, "staging");
    await user.keyboard("{Enter}");

    expect(onRenameGroup).not.toHaveBeenCalled();
  });

  it("ignores a blank rename", async () => {
    const user = userEvent.setup();
    const onRenameGroup = vi.fn();
    renderList({ groupNames: ["edge"], onRenameGroup, onDeleteGroup: vi.fn() });

    openGroupMenu("edge");
    await user.click(screen.getByRole("menuitem", { name: "Rename group" }));
    const input = screen.getByLabelText("Rename group");
    await user.clear(input);
    await user.keyboard("{Enter}");

    expect(onRenameGroup).not.toHaveBeenCalled();
  });

  it("deletes a group from the right-click menu", async () => {
    const user = userEvent.setup();
    const onDeleteGroup = vi.fn();
    renderList({ groupNames: ["edge"], onDeleteGroup, onRenameGroup: vi.fn() });

    openGroupMenu("edge");
    await user.click(screen.getByRole("menuitem", { name: "Delete group" }));

    expect(onDeleteGroup).toHaveBeenCalledWith("edge");
  });

  it("states that deleting keeps the connections", async () => {
    renderList({ groupNames: ["edge"], onDeleteGroup: vi.fn(), onRenameGroup: vi.fn() });

    openGroupMenu("edge");

    expect(
      await screen.findByText(/Connections move back to the default group/),
    ).toBeInTheDocument();
  });

  /// 默认分组没有可改的名字,右键不该弹菜单。
  it("does not open on the default group", () => {
    renderList({
      connections: [connection()],
      onDeleteGroup: vi.fn(),
      onRenameGroup: vi.fn(),
    });

    openGroupMenu("Default");

    expect(screen.queryByRole("menu")).not.toBeInTheDocument();
  });
});
