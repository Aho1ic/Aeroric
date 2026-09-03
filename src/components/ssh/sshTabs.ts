import type { SshConnection } from "../../types";

/**
 * SSH 工作区的标签模型。
 *
 * `id` 必须区别于 `connectionId` —— 同一台主机可以开多个会话,拿 connectionId 当 key
 * 会让第二个标签复用第一个的 DOM 节点,两个会话抢一个 xterm。
 */
export interface SshTab {
  id: string;
  connectionId: string;
  title: string;
}

/** 与本地 shell 标签(`SHELL_TERMINAL_MAX_SESSIONS`)对齐。 */
export const SSH_TERMINAL_MAX_SESSIONS = 10;

let tabSeq = 0;

/** 仅供测试:序号影响 tab id,断言里需要可复位。 */
export function resetSshTabSeq(): void {
  tabSeq = 0;
}

export function createSshTabId(connectionId: string, now: number): string {
  tabSeq += 1;
  return `${connectionId}:${tabSeq}:${now}`;
}

/**
 * 同一台主机开第二个会话时给标题加序号。
 *
 * 只按 connectionId 数已有标签,不看标题本身 —— 用户可以把连接改名,按名字去重会在
 * 改名后错位。
 */
export function sshTabTitle(connection: SshConnection, existing: readonly SshTab[]): string {
  const sameHost = existing.filter((tab) => tab.connectionId === connection.id).length;
  return sameHost === 0 ? connection.name : `${connection.name} (${sameHost + 1})`;
}

/**
 * 头部「显示配置 / 还原终端」这一个按钮的两态。
 *
 * 没有任何标签时保持卡片可见:那时「还原到终端」无处可去,翻成 false 只会得到一个
 * 空面板,而眼前又没有别的开关能让卡片回来。
 */
export function toggleSshCardsView({
  showingCards,
  hasOpenTabs,
}: {
  showingCards: boolean;
  hasOpenTabs: boolean;
}): boolean {
  if (showingCards && hasOpenTabs) return false;
  return true;
}

export interface OpenSshTabResult {
  tabs: SshTab[];
  activeTabId: string | null;
  /** 撞上限而没能新建。调用方据此提示,不要静默吞掉。 */
  limitReached: boolean;
}

/**
 * 打开一个连接。
 *
 * 默认「聚焦已有标签」:点连接卡片时如果这台主机已经开着,就切过去而不是再连一条。
 * 想显式开同主机的第二个会话(头部 `+`)传 `forceNew`。
 */
export function openSshTab({
  tabs,
  connection,
  forceNew = false,
  now,
  maxTabs = SSH_TERMINAL_MAX_SESSIONS,
}: {
  tabs: readonly SshTab[];
  connection: SshConnection;
  forceNew?: boolean;
  now: number;
  maxTabs?: number;
}): OpenSshTabResult {
  if (!forceNew) {
    const existing = tabs.find((tab) => tab.connectionId === connection.id);
    if (existing) {
      return { tabs: [...tabs], activeTabId: existing.id, limitReached: false };
    }
  }
  if (tabs.length >= maxTabs) {
    // 到顶就什么都不动,连 activeTabId 也保持原样 —— 把焦点挪走会让用户以为开成功了。
    return {
      tabs: [...tabs],
      activeTabId: tabs[tabs.length - 1]?.id ?? null,
      limitReached: true,
    };
  }
  const tab: SshTab = {
    id: createSshTabId(connection.id, now),
    connectionId: connection.id,
    title: sshTabTitle(connection, tabs),
  };
  return { tabs: [...tabs, tab], activeTabId: tab.id, limitReached: false };
}

export interface CloseSshTabResult {
  tabs: SshTab[];
  activeTabId: string | null;
}

/**
 * 关掉一个标签,并决定焦点落到哪儿。
 *
 * 关的不是当前标签时焦点不动。关的是当前标签时优先落到右邻(和编辑器标签、shell 标签
 * 一致的直觉:关掉一个,后面那个顶上来),右边没有了才回退到左邻。
 */
export function closeSshTab({
  tabs,
  activeTabId,
  tabId,
}: {
  tabs: readonly SshTab[];
  activeTabId: string | null;
  tabId: string;
}): CloseSshTabResult {
  const index = tabs.findIndex((tab) => tab.id === tabId);
  if (index === -1) return { tabs: [...tabs], activeTabId };

  const next = tabs.filter((tab) => tab.id !== tabId);
  if (activeTabId !== tabId) return { tabs: next, activeTabId };
  return { tabs: next, activeTabId: next[Math.min(index, next.length - 1)]?.id ?? null };
}

/** 连接被删掉后清掉它的所有标签(同主机可能开着多个)。 */
export function pruneSshTabsForConnection({
  tabs,
  activeTabId,
  connectionId,
}: {
  tabs: readonly SshTab[];
  activeTabId: string | null;
  connectionId: string;
}): CloseSshTabResult {
  const next = tabs.filter((tab) => tab.connectionId !== connectionId);
  if (next.length === tabs.length) return { tabs: [...tabs], activeTabId };
  const activeSurvived = next.some((tab) => tab.id === activeTabId);
  return { tabs: next, activeTabId: activeSurvived ? activeTabId : (next[0]?.id ?? null) };
}
