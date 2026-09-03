import { beforeEach, describe, expect, it } from "vitest";
import type { SshConnection } from "../types";
import {
  SSH_TERMINAL_MAX_SESSIONS,
  closeSshTab,
  createSshTabId,
  openSshTab,
  pruneSshTabsForConnection,
  resetSshTabSeq,
  sshTabTitle,
  toggleSshCardsView,
  type SshTab,
} from "../components/ssh/sshTabs";

/**
 * SSH 标签的状态机。这里全是纯函数,所以能把「点已开的卡片该聚焦还是新建」「关掉当前
 * 标签焦点落哪儿」这类容易漂的判断锁死,不用连带跑终端初始化。
 */

function connection(id: string, name = id): SshConnection {
  return {
    id,
    name,
    host: `${id}.example.com`,
    port: 22,
    username: "root",
  } as SshConnection;
}

function tabOf(id: string, connectionId: string, title = connectionId): SshTab {
  return { id, connectionId, title };
}

beforeEach(() => {
  resetSshTabSeq();
});

describe("toggleSshCardsView", () => {
  it("看卡片且有已开标签时回到终端", () => {
    expect(toggleSshCardsView({ showingCards: true, hasOpenTabs: true })).toBe(false);
  });

  it("看终端时切到卡片", () => {
    expect(toggleSshCardsView({ showingCards: false, hasOpenTabs: true })).toBe(true);
  });

  // 没有标签时「还原到终端」无处可去,翻成 false 会得到一个空面板。
  it("没有任何标签时保持卡片可见", () => {
    expect(toggleSshCardsView({ showingCards: true, hasOpenTabs: false })).toBe(true);
    expect(toggleSshCardsView({ showingCards: false, hasOpenTabs: false })).toBe(true);
  });
});

describe("createSshTabId", () => {
  it("同一连接同一时刻也给出不同 id", () => {
    const a = createSshTabId("conn-1", 1000);
    const b = createSshTabId("conn-1", 1000);
    expect(a).not.toBe(b);
  });
});

describe("sshTabTitle", () => {
  it("首个标签用连接名", () => {
    expect(sshTabTitle(connection("c1", "prod"), [])).toBe("prod");
  });

  it("同主机第二个起加序号", () => {
    const tabs = [tabOf("t1", "c1", "prod")];
    expect(sshTabTitle(connection("c1", "prod"), tabs)).toBe("prod (2)");
    expect(sshTabTitle(connection("c1", "prod"), [...tabs, tabOf("t2", "c1")])).toBe("prod (3)");
  });

  // 按 connectionId 数而不是按标题:用户改名后按名字去重会错位。
  it("改名后仍按连接身份计数", () => {
    const tabs = [tabOf("t1", "c1", "旧名")];
    expect(sshTabTitle(connection("c1", "新名"), tabs)).toBe("新名 (2)");
  });

  it("不同主机各自从无序号开始", () => {
    const tabs = [tabOf("t1", "c1", "prod")];
    expect(sshTabTitle(connection("c2", "stage"), tabs)).toBe("stage");
  });
});

describe("openSshTab", () => {
  it("未开过的连接新建标签并聚焦", () => {
    const result = openSshTab({ tabs: [], connection: connection("c1", "prod"), now: 1 });
    expect(result.tabs).toHaveLength(1);
    expect(result.tabs[0].connectionId).toBe("c1");
    expect(result.tabs[0].title).toBe("prod");
    expect(result.activeTabId).toBe(result.tabs[0].id);
    expect(result.limitReached).toBe(false);
  });

  // 已确认的取舍:点卡片是聚焦,不是再连一条。
  it("已开着的连接只聚焦,不新建", () => {
    const tabs = [tabOf("t1", "c1"), tabOf("t2", "c2")];
    const result = openSshTab({ tabs, connection: connection("c1"), now: 2 });
    expect(result.tabs).toHaveLength(2);
    expect(result.activeTabId).toBe("t1");
  });

  it("forceNew 才开同主机的第二个会话", () => {
    const tabs = [tabOf("t1", "c1", "prod")];
    const result = openSshTab({
      tabs,
      connection: connection("c1", "prod"),
      forceNew: true,
      now: 3,
    });
    expect(result.tabs).toHaveLength(2);
    expect(result.tabs[1].title).toBe("prod (2)");
    expect(result.activeTabId).toBe(result.tabs[1].id);
  });

  it("撞上限时不新建并报告", () => {
    const tabs = Array.from({ length: SSH_TERMINAL_MAX_SESSIONS }, (_, i) =>
      tabOf(`t${i}`, `c${i}`),
    );
    const result = openSshTab({ tabs, connection: connection("new"), now: 4 });
    expect(result.tabs).toHaveLength(SSH_TERMINAL_MAX_SESSIONS);
    expect(result.limitReached).toBe(true);
  });

  // 到顶时把焦点挪走会让用户以为开成功了。
  it("撞上限时焦点不跳到别的标签", () => {
    const tabs = Array.from({ length: SSH_TERMINAL_MAX_SESSIONS }, (_, i) =>
      tabOf(`t${i}`, `c${i}`),
    );
    const result = openSshTab({ tabs, connection: connection("new"), now: 5 });
    expect(result.activeTabId).toBe(`t${SSH_TERMINAL_MAX_SESSIONS - 1}`);
  });

  it("上限之内聚焦已有标签不受上限影响", () => {
    const tabs = Array.from({ length: SSH_TERMINAL_MAX_SESSIONS }, (_, i) =>
      tabOf(`t${i}`, `c${i}`),
    );
    const result = openSshTab({ tabs, connection: connection("c0"), now: 6 });
    expect(result.limitReached).toBe(false);
    expect(result.activeTabId).toBe("t0");
  });

  it("不改动传入的数组", () => {
    const tabs = [tabOf("t1", "c1")];
    openSshTab({ tabs, connection: connection("c2"), now: 7 });
    expect(tabs).toHaveLength(1);
  });
});

describe("closeSshTab", () => {
  it("关非当前标签时焦点不动", () => {
    const tabs = [tabOf("t1", "c1"), tabOf("t2", "c2"), tabOf("t3", "c3")];
    const result = closeSshTab({ tabs, activeTabId: "t1", tabId: "t3" });
    expect(result.tabs.map((tab) => tab.id)).toEqual(["t1", "t2"]);
    expect(result.activeTabId).toBe("t1");
  });

  // 与编辑器标签、shell 标签一致:关掉一个,后面那个顶上来。
  it("关当前标签时右邻顶上来", () => {
    const tabs = [tabOf("t1", "c1"), tabOf("t2", "c2"), tabOf("t3", "c3")];
    const result = closeSshTab({ tabs, activeTabId: "t2", tabId: "t2" });
    expect(result.activeTabId).toBe("t3");
  });

  it("关最后一个标签时回退到左邻", () => {
    const tabs = [tabOf("t1", "c1"), tabOf("t2", "c2")];
    const result = closeSshTab({ tabs, activeTabId: "t2", tabId: "t2" });
    expect(result.activeTabId).toBe("t1");
  });

  it("关掉唯一标签后没有焦点", () => {
    const result = closeSshTab({ tabs: [tabOf("t1", "c1")], activeTabId: "t1", tabId: "t1" });
    expect(result.tabs).toEqual([]);
    expect(result.activeTabId).toBeNull();
  });

  it("关不存在的标签是空操作", () => {
    const tabs = [tabOf("t1", "c1")];
    const result = closeSshTab({ tabs, activeTabId: "t1", tabId: "nope" });
    expect(result.tabs.map((tab) => tab.id)).toEqual(["t1"]);
    expect(result.activeTabId).toBe("t1");
  });
});

describe("pruneSshTabsForConnection", () => {
  it("删连接时清掉它的全部标签", () => {
    const tabs = [tabOf("t1", "c1"), tabOf("t2", "c1"), tabOf("t3", "c2")];
    const result = pruneSshTabsForConnection({ tabs, activeTabId: "t1", connectionId: "c1" });
    expect(result.tabs.map((tab) => tab.id)).toEqual(["t3"]);
    expect(result.activeTabId).toBe("t3");
  });

  it("当前标签没被删则焦点不动", () => {
    const tabs = [tabOf("t1", "c1"), tabOf("t2", "c2")];
    const result = pruneSshTabsForConnection({ tabs, activeTabId: "t2", connectionId: "c1" });
    expect(result.activeTabId).toBe("t2");
  });

  it("没有匹配标签时是空操作", () => {
    const tabs = [tabOf("t1", "c1")];
    const result = pruneSshTabsForConnection({ tabs, activeTabId: "t1", connectionId: "c9" });
    expect(result.tabs.map((tab) => tab.id)).toEqual(["t1"]);
    expect(result.activeTabId).toBe("t1");
  });

  it("删掉全部标签后没有焦点", () => {
    const tabs = [tabOf("t1", "c1"), tabOf("t2", "c1")];
    const result = pruneSshTabsForConnection({ tabs, activeTabId: "t1", connectionId: "c1" });
    expect(result.tabs).toEqual([]);
    expect(result.activeTabId).toBeNull();
  });
});
