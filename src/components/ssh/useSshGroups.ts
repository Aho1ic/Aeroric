import { useCallback, useMemo, useSyncExternalStore } from "react";
import type { SshConnection } from "../../types";
import {
  loadSshGroupNames,
  mergeSshGroupNames,
  normalizeSshGroupName,
  removeSshGroupFromConnections,
  renameSshGroupInConnections,
  renameSshGroupName,
  saveSshGroupNames,
  sshGroupNamesSnapshot,
  subscribeSshGroupNames,
} from "./sshGroups";

/**
 * SSH 分组的读写口子,右侧栏列表与欢迎页视图共用。
 *
 * 抽出来的理由是同步:名单存在 localStorage,两个视图可能同时挂载,各自持一份
 * useState 会立刻不同步(一侧建组另一侧看不到)。这里统一走
 * `useSyncExternalStore` 订阅同一份快照。
 */
export function useSshGroups(
  connections: SshConnection[],
  saveConnections: (next: SshConnection[]) => void,
) {
  const storedGroupNames = useSyncExternalStore(
    subscribeSshGroupNames,
    sshGroupNamesSnapshot,
    sshGroupNamesSnapshot,
  );

  const groups = useMemo(
    () => mergeSshGroupNames(connections, storedGroupNames),
    [connections, storedGroupNames],
  );

  const createGroup = useCallback((groupName: string) => {
    const normalized = normalizeSshGroupName(groupName);
    if (!normalized) return;
    // 每次都从最新落盘状态出发,避免同一 tick 内的连续建组互相覆盖
    // (保存一条带新分组的连接会连着触发两次)。
    const current = loadSshGroupNames();
    if (current.includes(normalized)) return;
    saveSshGroupNames([...current, normalized]);
  }, []);

  /// 只摘分组标签,组内连接回落到默认分组 —— 连接是用户资产,删归类不该带走它们。
  const deleteGroup = useCallback(
    (groupName: string) => {
      const normalized = normalizeSshGroupName(groupName);
      if (!normalized) return;
      saveSshGroupNames(loadSshGroupNames().filter((name) => name !== normalized));
      const next = removeSshGroupFromConnections(connections, normalized);
      if (next) saveConnections(next);
    },
    [connections, saveConnections],
  );

  const renameGroup = useCallback(
    (from: string, to: string) => {
      const source = normalizeSshGroupName(from);
      const target = normalizeSshGroupName(to);
      if (!source || !target || source === target) return;
      const current = loadSshGroupNames();
      const renamed = renameSshGroupName(current, source, target);
      if (renamed) {
        saveSshGroupNames(renamed);
      } else if (!current.includes(target)) {
        // 源名不在名单里(它只存在于连接上)时,把新名字登记进来 ——
        // 否则之后移走最后一条连接,这个分组就消失了。
        saveSshGroupNames([...current, target]);
      }
      const next = renameSshGroupInConnections(connections, source, target);
      if (next) saveConnections(next);
    },
    [connections, saveConnections],
  );

  return { groups, createGroup, deleteGroup, renameGroup };
}
