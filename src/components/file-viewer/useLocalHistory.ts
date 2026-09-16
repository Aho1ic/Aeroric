/**
 * 「文件本地历史」对话框的整簇状态:九个 useState、两个加载 effect 与一个
 * restore 动作同生共死 —— 打开时清场并拉全量条目 + 当前内容,选中条目变化时
 * 拉对应快照,恢复成功后回写快照与内容。原来内联在 FileViewer.tsx 里,形态与
 * hooks/useLocalShellSession 的"一簇状态一个 hook"相同。
 *
 * 与 FileViewer 的两条回边:
 * - `onOpen`:打开历史会顺带关掉编辑器右上角的"更多"菜单(原行为);
 * - `onRestored(path)`:恢复会改写磁盘上的文件,调用方要 bump 对应 tab 的
 *   reloadVersions 让编辑器重读,并清掉脏标记。
 */
import { useCallback, useEffect, useState } from "react";
import { invoke } from "../../lib/api/invoke";

import type { LocalHistoryEntry, LocalHistorySnapshot } from "../../types";
import { confirm } from "../../lib/appDialog";

interface UseLocalHistoryOptions {
  projectPath: string;
  t: (key: string, vars?: Record<string, string>) => string;
  onOpen?: () => void;
  onRestored?: (path: string) => void;
}

export function useLocalHistory({ projectPath, t, onOpen, onRestored }: UseLocalHistoryOptions) {
  const [target, setTarget] = useState<{ path: string; name: string } | null>(null);
  const [entries, setEntries] = useState<LocalHistoryEntry[]>([]);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [snapshot, setSnapshot] = useState<LocalHistorySnapshot | null>(null);
  const [currentContent, setCurrentContent] = useState("");
  const [loading, setLoading] = useState(false);
  const [snapshotLoading, setSnapshotLoading] = useState(false);
  const [restoring, setRestoring] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const open = useCallback(
    (tab: { path: string; name: string }) => {
      onOpen?.();
      setTarget({ path: tab.path, name: tab.name });
      setEntries([]);
      setSelectedId(null);
      setSnapshot(null);
      setCurrentContent("");
      setError(null);
    },
    [onOpen],
  );

  const close = useCallback(() => {
    setTarget(null);
    setEntries([]);
    setSelectedId(null);
    setSnapshot(null);
    setCurrentContent("");
    setError(null);
  }, []);

  useEffect(() => {
    if (!target) return;
    let cancelled = false;
    setLoading(true);
    setError(null);
    setSnapshot(null);
    setSelectedId(null);

    void Promise.all([
      invoke<LocalHistoryEntry[]>("list_local_history", {
        projectPath,
        filePath: target.path,
      }),
      invoke<string>("read_file_content", {
        projectPath,
        path: target.path,
      }),
    ])
      .then(([loadedEntries, loadedCurrentContent]) => {
        if (cancelled) return;
        setEntries(loadedEntries);
        setCurrentContent(loadedCurrentContent);
        setSelectedId(loadedEntries[0]?.id ?? null);
      })
      .catch((err) => {
        if (cancelled) return;
        setEntries([]);
        setCurrentContent("");
        setError(String(err));
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });

    return () => {
      cancelled = true;
    };
  }, [target, projectPath]);

  useEffect(() => {
    if (!target || !selectedId) {
      setSnapshot(null);
      return;
    }
    let cancelled = false;
    setSnapshotLoading(true);
    setError(null);

    void invoke<LocalHistorySnapshot>("read_local_history_entry", {
      projectPath,
      filePath: target.path,
      entryId: selectedId,
    })
      .then((loadedSnapshot) => {
        if (!cancelled) setSnapshot(loadedSnapshot);
      })
      .catch((err) => {
        if (!cancelled) {
          setSnapshot(null);
          setError(String(err));
        }
      })
      .finally(() => {
        if (!cancelled) setSnapshotLoading(false);
      });

    return () => {
      cancelled = true;
    };
  }, [selectedId, target, projectPath]);

  const restore = useCallback(async () => {
    if (!target || !snapshot || restoring) return;
    const confirmed = await confirm(t("file.localHistoryRestoreConfirm"), {
      title: t("file.localHistory"),
      kind: "warning",
    });
    if (!confirmed) return;
    setRestoring(true);
    setError(null);
    try {
      const restored = await invoke<LocalHistorySnapshot>("restore_local_history_entry", {
        projectPath,
        filePath: target.path,
        entryId: snapshot.entry.id,
      });
      setSnapshot(restored);
      setCurrentContent(restored.content);
      onRestored?.(target.path);
      const reloadedEntries = await invoke<LocalHistoryEntry[]>("list_local_history", {
        projectPath,
        filePath: target.path,
      });
      setEntries(reloadedEntries);
    } catch (err) {
      setError(String(err));
    } finally {
      setRestoring(false);
    }
  }, [onRestored, restoring, snapshot, target, projectPath, t]);

  return {
    target,
    entries,
    selectedId,
    snapshot,
    currentContent,
    loading,
    snapshotLoading,
    restoring,
    error,
    setSelectedId,
    open,
    close,
    restore,
  };
}
