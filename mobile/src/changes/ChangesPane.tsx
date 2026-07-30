/**
 * 任务详情「变更」tab:工作区 git 变更列表 + 内联 unified diff 查看。
 * 数据来自只读 RPC git.changes / git.diff(任务在 worktree 跑就看 worktree)。
 * SSH / WSL 项目不可用,引导回桌面查看。
 */

import { router } from "expo-router";
import { useCallback, useEffect, useState } from "react";
import { FlatList, Platform, Pressable, ScrollView, StyleSheet, Text, View } from "react-native";
import { t } from "../i18n";
import { useConnection } from "../state/connection-context";
import type { GitChangesResult, GitDiffResult, GitFileChange } from "../types";
import { theme } from "../ui/theme";

const MONO = Platform.select({ ios: "Menlo", default: "monospace" });

/** 单文件 diff 渲染的行数上限(防超大 diff 卡死渲染)。 */
const MAX_DIFF_LINES = 800;

interface DiffState {
  loading: boolean;
  error: string | null;
  lines: string[] | null;
  truncated: boolean;
}

function statusBadge(change: GitFileChange): { label: string; color: string } {
  const label = change.status === "untracked" ? "??" : change.status.slice(0, 1).toUpperCase();
  const color =
    change.status === "deleted"
      ? theme.danger
      : change.status === "untracked" || change.status === "added"
        ? theme.success
        : theme.accent;
  return { label, color };
}

function diffLineStyle(line: string) {
  if (line.startsWith("+") && !line.startsWith("+++")) return styles.diffAdd;
  if (line.startsWith("-") && !line.startsWith("---")) return styles.diffDel;
  if (line.startsWith("@@")) return styles.diffHunk;
  if (line.startsWith("diff ") || line.startsWith("index ")) return styles.diffMeta;
  return styles.diffCtx;
}

export function ChangesPane({
  projectId,
  taskId,
  active,
}: {
  projectId: string;
  taskId: string;
  active: boolean;
}) {
  const { request, status } = useConnection();
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [unavailable, setUnavailable] = useState<"ssh" | "wsl" | null>(null);
  const [changes, setChanges] = useState<GitFileChange[] | null>(null);
  const [expanded, setExpanded] = useState<string | null>(null);
  const [diffs, setDiffs] = useState<Record<string, DiffState>>({});

  const refresh = useCallback(() => {
    if (status !== "online" || !projectId) return;
    setLoading(true);
    setError(null);
    request<GitChangesResult>("git.changes", { projectId, taskId })
      .then((result) => {
        if (!result.available) {
          setUnavailable(result.reason ?? "ssh");
          setChanges(null);
          return;
        }
        setUnavailable(null);
        setChanges(result.changes ?? []);
      })
      .catch((err) => setError(err instanceof Error ? err.message : String(err)))
      .finally(() => setLoading(false));
  }, [projectId, request, status, taskId]);

  // 首次激活 & 重新上线时拉取
  useEffect(() => {
    if (active && changes === null && status === "online") refresh();
  }, [active, changes, refresh, status]);

  const toggleDiff = useCallback(
    (change: GitFileChange) => {
      const key = `${change.staged ? "s" : "w"}:${change.path}`;
      if (expanded === key) {
        setExpanded(null);
        return;
      }
      setExpanded(key);
      if (diffs[key]?.lines || diffs[key]?.loading) return;
      setDiffs((prev) => ({
        ...prev,
        [key]: { loading: true, error: null, lines: null, truncated: false },
      }));
      request<GitDiffResult>("git.diff", {
        projectId,
        taskId,
        path: change.path,
        staged: change.staged,
      })
        .then((result) => {
          const text = result.available ? (result.diff ?? "") : "";
          const allLines = text.length > 0 ? text.split("\n") : [];
          const truncated = allLines.length > MAX_DIFF_LINES;
          setDiffs((prev) => ({
            ...prev,
            [key]: {
              loading: false,
              error: null,
              lines: truncated ? allLines.slice(0, MAX_DIFF_LINES) : allLines,
              truncated,
            },
          }));
        })
        .catch((err) =>
          setDiffs((prev) => ({
            ...prev,
            [key]: {
              loading: false,
              error: err instanceof Error ? err.message : String(err),
              lines: null,
              truncated: false,
            },
          })),
        );
    },
    [diffs, expanded, projectId, request, taskId],
  );

  if (unavailable) {
    return (
      <View style={styles.center}>
        <Text style={styles.hint}>
          {unavailable === "wsl" ? t("changes.unavailable.wsl") : t("changes.unavailable.ssh")}
        </Text>
      </View>
    );
  }

  return (
    <FlatList
      style={styles.list}
      contentContainerStyle={styles.listContent}
      data={changes ?? []}
      keyExtractor={(item) => `${item.staged ? "s" : "w"}:${item.path}`}
      refreshing={loading}
      onRefresh={refresh}
      ListHeaderComponent={
        <Pressable
          style={styles.browseRow}
          onPress={() =>
            router.push({ pathname: "/files", params: { projectId } })
          }
        >
          <Text style={styles.browseText}>{t("changes.browseFiles")} ›</Text>
        </Pressable>
      }
      ListEmptyComponent={
        <View style={styles.center}>
          <Text style={styles.hint}>
            {error ? error : loading ? t("common.loading") : t("changes.empty")}
          </Text>
          {error ? (
            <Pressable style={styles.retryButton} onPress={refresh}>
              <Text style={styles.retryText}>{t("common.retry")}</Text>
            </Pressable>
          ) : null}
        </View>
      }
      renderItem={({ item }) => {
        const key = `${item.staged ? "s" : "w"}:${item.path}`;
        const badge = statusBadge(item);
        const diff = diffs[key];
        const isOpen = expanded === key;
        return (
          <View style={styles.fileCard}>
            <Pressable style={styles.fileRow} onPress={() => toggleDiff(item)}>
              <Text style={[styles.fileBadge, { color: badge.color }]}>{badge.label}</Text>
              <Text style={styles.filePath} numberOfLines={1}>
                {item.path}
              </Text>
              {item.staged ? <Text style={styles.stagedTag}>{t("changes.staged")}</Text> : null}
              <Text style={styles.chevron}>{isOpen ? "▾" : "▸"}</Text>
            </Pressable>
            {isOpen ? (
              <View style={styles.diffWrap}>
                {diff?.loading ? <Text style={styles.hint}>{t("common.loading")}</Text> : null}
                {diff?.error ? <Text style={styles.errorText}>{diff.error}</Text> : null}
                {diff?.lines && diff.lines.length === 0 ? (
                  <Text style={styles.hint}>{t("changes.diffEmpty")}</Text>
                ) : null}
                {diff?.lines && diff.lines.length > 0 ? (
                  <ScrollView horizontal showsHorizontalScrollIndicator>
                    <View>
                      {diff.lines.map((line, index) => (
                        <Text
                          key={index}
                          style={[styles.diffLine, diffLineStyle(line)]}
                          numberOfLines={1}
                        >
                          {line || " "}
                        </Text>
                      ))}
                      {diff.truncated ? (
                        <Text style={[styles.diffLine, styles.diffMeta]}>…</Text>
                      ) : null}
                    </View>
                  </ScrollView>
                ) : null}
              </View>
            ) : null}
          </View>
        );
      }}
    />
  );
}

const styles = StyleSheet.create({
  list: { flex: 1, backgroundColor: theme.bg },
  listContent: { padding: 12, gap: 8 },
  center: { alignItems: "center", justifyContent: "center", paddingVertical: 48, gap: 12 },
  hint: { color: theme.textSecondary, fontSize: 13, textAlign: "center", lineHeight: 19 },
  errorText: { color: theme.danger, fontSize: 12.5 },
  retryButton: {
    borderWidth: 1,
    borderColor: theme.border,
    borderRadius: 8,
    paddingHorizontal: 18,
    paddingVertical: 8,
  },
  retryText: { color: theme.accent, fontSize: 13.5 },
  browseRow: { paddingVertical: 6, paddingHorizontal: 2, marginBottom: 4 },
  browseText: { color: theme.accent, fontSize: 13.5, fontWeight: "600" },
  fileCard: {
    backgroundColor: theme.bgCard,
    borderRadius: 10,
    borderWidth: StyleSheet.hairlineWidth,
    borderColor: theme.border,
    marginBottom: 8,
    overflow: "hidden",
  },
  fileRow: { flexDirection: "row", alignItems: "center", gap: 8, padding: 12 },
  fileBadge: { fontFamily: MONO, fontSize: 12.5, fontWeight: "700", width: 22 },
  filePath: { flex: 1, color: theme.text, fontSize: 13 },
  stagedTag: { color: theme.textHint, fontSize: 11 },
  chevron: { color: theme.textHint, fontSize: 12 },
  diffWrap: {
    borderTopWidth: StyleSheet.hairlineWidth,
    borderTopColor: theme.border,
    padding: 10,
  },
  diffLine: { fontFamily: MONO, fontSize: 11.5, lineHeight: 16 },
  diffAdd: { color: theme.success },
  diffDel: { color: theme.danger },
  diffHunk: { color: theme.accent },
  diffMeta: { color: theme.textHint },
  diffCtx: { color: theme.textSecondary },
});
