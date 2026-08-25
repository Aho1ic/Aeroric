import type { SshConnection } from "../../types";

/**
 * SSH 分组名单。
 *
 * 分组本身只是连接上的一个字符串字段,所以"还没有任何连接的分组"在
 * `ssh-connections.json` 里没有落脚点。这份名单专门存这种空分组,与
 * `projectGroups.ts` 对项目分组的做法一致 —— 纯 UI 归类,不进后端存储。
 */
export const SSH_GROUPS_STORAGE_KEY = "aeroric:sshGroups";

export function normalizeSshGroupName(value: unknown): string | null {
  if (typeof value !== "string") return null;
  const normalized = value.trim();
  return normalized ? normalized : null;
}

export function normalizeSshGroupNames(value: unknown): string[] {
  if (!Array.isArray(value)) return [];
  const names: string[] = [];
  const seen = new Set<string>();
  for (const item of value) {
    const name = normalizeSshGroupName(item);
    if (!name || seen.has(name)) continue;
    seen.add(name);
    names.push(name);
  }
  return names;
}

export function loadSshGroupNames(): string[] {
  try {
    return normalizeSshGroupNames(JSON.parse(localStorage.getItem(SSH_GROUPS_STORAGE_KEY) ?? "[]"));
  } catch {
    return [];
  }
}

export function saveSshGroupNames(names: string[]): void {
  const normalized = normalizeSshGroupNames(names);
  try {
    localStorage.setItem(SSH_GROUPS_STORAGE_KEY, JSON.stringify(normalized));
  } catch {
    // 存不下(隐私模式 / 配额)不该让建分组这个动作失败,本次会话内仍然可用。
  }
  // 落盘后广播:同一标签页内 localStorage 不触发 storage 事件,右侧栏与欢迎页
  // 的 SSH 视图可能同时挂载,不广播的话另一侧要等重新挂载才看得到新分组。
  cachedNames = normalized;
  for (const listener of listeners) listener();
}

// ── 订阅(供 useSyncExternalStore) ──────────────────────────────────────────
//
// 名单存在 localStorage 里,天然是"组件外部的可变状态"。让每个视图各存一份
// useState 会立刻不同步:一侧建组,另一侧毫不知情。这里用一个极小的 store 收口。

const listeners = new Set<() => void>();
/** getSnapshot 必须返回稳定引用,否则 useSyncExternalStore 会判定无限循环。 */
let cachedNames: string[] | null = null;

export function subscribeSshGroupNames(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

export function sshGroupNamesSnapshot(): string[] {
  if (!cachedNames) cachedNames = loadSshGroupNames();
  return cachedNames;
}

/** 仅供测试:清掉快照缓存,让下一次读取重新回到 localStorage。 */
export function resetSshGroupNamesCache(): void {
  cachedNames = null;
}

/**
 * 名单与连接实际用到的分组合并。
 *
 * 连接可能带着名单里没有的分组进来(手改 json、旧版本写入、导入的配置),
 * 那些分组必须照样显示,否则连接会凭空消失在列表里。
 */
export function mergeSshGroupNames(
  connections: SshConnection[],
  configuredNames: string[],
): string[] {
  const names = normalizeSshGroupNames(configuredNames);
  const known = new Set(names);
  for (const connection of connections) {
    const group = normalizeSshGroupName(connection.group);
    if (group && !known.has(group)) {
      known.add(group);
      names.push(group);
    }
  }
  return names;
}

/**
 * 删除分组:只摘掉分组名,组内连接回落到默认分组。
 *
 * 连接是用户的真实资产(含密码与主机信息),删一个归类标签绝不该带走它们。
 * 返回 null 表示没有连接需要改动,调用方可以跳过持久化。
 */
export function removeSshGroupFromConnections(
  connections: SshConnection[],
  groupName: string,
): SshConnection[] | null {
  const target = normalizeSshGroupName(groupName);
  if (!target) return null;
  let changed = false;
  const next = connections.map((connection) => {
    if (normalizeSshGroupName(connection.group) !== target) return connection;
    changed = true;
    const { group: _group, ...rest } = connection;
    return rest;
  });
  return changed ? next : null;
}

/**
 * 重命名分组:把组内连接的 `group` 一并改掉。
 *
 * 只改名单不改连接会让那些连接落到一个"名单里已经不存在"的分组下,
 * 虽然仍显示得出来,但用户会看到旧名字复活。返回 null 表示无连接需要改动。
 */
export function renameSshGroupInConnections(
  connections: SshConnection[],
  from: string,
  to: string,
): SshConnection[] | null {
  const source = normalizeSshGroupName(from);
  const target = normalizeSshGroupName(to);
  if (!source || !target || source === target) return null;
  let changed = false;
  const next = connections.map((connection) => {
    if (normalizeSshGroupName(connection.group) !== source) return connection;
    changed = true;
    return { ...connection, group: target };
  });
  return changed ? next : null;
}

/**
 * 名单内重命名,顺序保持不变。
 *
 * 返回 null 表示名单没有改动 —— 目标名已存在(不做静默合并),或者源名根本不在
 * 名单里(它只存在于连接上)。调用方要靠这个区分,不能拿返回值和入参比引用:
 * `.map` 无论是否命中都会产生新数组。
 */
export function renameSshGroupName(names: string[], from: string, to: string): string[] | null {
  const source = normalizeSshGroupName(from);
  const target = normalizeSshGroupName(to);
  if (!source || !target || source === target) return null;
  if (names.includes(target) || !names.includes(source)) return null;
  return names.map((name) => (name === source ? target : name));
}
