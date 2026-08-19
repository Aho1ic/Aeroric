/**
 * 文件浏览面板:project.files 列目录。目录下钻走内部 path state(不 push 路由),
 * 因此既能作为独立页面,也能嵌在任务详情页的「文件」tab 里。
 * 点文件进 file-view(可查看 / 编辑)。SSH / WSL 项目提示回桌面查看。
 */

import { router } from "expo-router";
import { ChevronLeft, FileText, Folder } from "lucide-react-native";
import { useCallback, useEffect, useState } from "react";
import { FlatList, StyleSheet, Text, View } from "react-native";
import { t } from "../i18n";
import { useConnection } from "../state/connection-context";
import type { FsEntryView, ProjectFilesResult } from "../types";
import { radii, spacing, theme } from "../ui/theme";
import { AnimatedPressable } from "../ui/AnimatedPressable";

function parentOf(path: string): string {
  const parts = path.split("/").filter(Boolean);
  parts.pop();
  return parts.join("/");
}

export function FilesPane({
  projectId,
  active,
  initialPath = "",
}: {
  projectId: string;
  active: boolean;
  initialPath?: string;
}) {
  const { request, status, capabilitiesReady, hasCapability } = useConnection();
  const [path, setPath] = useState(initialPath);
  const [entries, setEntries] = useState<FsEntryView[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [unavailable, setUnavailable] = useState<"ssh" | "wsl" | null>(null);
  const [loading, setLoading] = useState(false);
  const [unsupported, setUnsupported] = useState(false);

  const load = useCallback(
    (target: string) => {
      if (status !== "online" || !projectId) return;
      if (capabilitiesReady && !hasCapability("files.read")) {
        setUnsupported(true);
        setEntries(null);
        setLoading(false);
        return;
      }
      setUnsupported(false);
      setLoading(true);
      setError(null);
      request<ProjectFilesResult>(
        "project.files",
        target ? { projectId, path: target } : { projectId },
      )
        .then((result) => {
          if (!result.available) {
            setUnavailable(result.reason ?? "ssh");
            return;
          }
          setUnavailable(null);
          setEntries(result.entries ?? []);
        })
        .catch((err) => setError(err instanceof Error ? err.message : String(err)))
        .finally(() => setLoading(false));
    },
    [capabilitiesReady, hasCapability, projectId, request, status],
  );

  useEffect(() => {
    if (active && entries === null) load(path);
  }, [active, entries, load, path]);

  const navigate = useCallback((target: string) => {
    setPath(target);
    setEntries(null);
  }, []);

  const refresh = useCallback(() => load(path), [load, path]);

  if (unavailable) {
    return (
      <View style={styles.center}>
        <Text style={styles.hint}>
          {unavailable === "wsl" ? t("changes.unavailable.wsl") : t("changes.unavailable.ssh")}
        </Text>
      </View>
    );
  }
  if (unsupported) {
    return (
      <View style={styles.center}>
        <Text style={styles.hint}>{t("files.unsupported")}</Text>
      </View>
    );
  }

  return (
    <View style={styles.pane}>
      {path ? (
        <AnimatedPressable
          style={styles.crumbBar}
          accessibilityRole="button"
          accessibilityLabel={t("files.back")}
          onPress={() => navigate(parentOf(path))}
        >
          <ChevronLeft size={16} color={theme.accent} />
          <Text style={styles.crumbText} numberOfLines={1} ellipsizeMode="head">
            {path}
          </Text>
        </AnimatedPressable>
      ) : null}
      <FlatList
        data={entries ?? []}
        keyExtractor={(item) => item.path}
        refreshing={loading}
        onRefresh={refresh}
        contentContainerStyle={styles.listContent}
        ListEmptyComponent={
          <View style={styles.center}>
            <Text style={styles.hint}>
              {error ? error : loading ? t("common.loading") : t("files.empty")}
            </Text>
            {error ? (
              <AnimatedPressable style={styles.retryButton} onPress={refresh}>
                <Text style={styles.retryText}>{t("common.retry")}</Text>
              </AnimatedPressable>
            ) : null}
          </View>
        }
        renderItem={({ item }) => (
          <AnimatedPressable
            style={styles.row}
            onPress={() =>
              item.is_dir
                ? navigate(item.path)
                : router.push({
                    pathname: "/file-view",
                    params: { projectId, path: item.path, name: item.name },
                  })
            }
          >
            {item.is_dir ? (
              <Folder size={15} color={theme.accent} />
            ) : (
              <FileText size={15} color={theme.textSecondary} />
            )}
            <Text style={[styles.name, item.is_gitignored && styles.ignored]} numberOfLines={1}>
              {item.name}
            </Text>
            {item.is_dir ? <Text style={styles.chevron}>›</Text> : null}
          </AnimatedPressable>
        )}
      />
    </View>
  );
}

const styles = StyleSheet.create({
  pane: { flex: 1, backgroundColor: theme.bg },
  crumbBar: {
    flexDirection: "row",
    alignItems: "center",
    gap: spacing.xs,
    paddingHorizontal: spacing.md,
    paddingVertical: spacing.sm,
    backgroundColor: theme.bgCard,
    borderBottomWidth: StyleSheet.hairlineWidth,
    borderBottomColor: theme.border,
  },
  crumbText: { flex: 1, color: theme.textSecondary, fontSize: 12.5 },
  listContent: { paddingVertical: 6 },
  center: { alignItems: "center", justifyContent: "center", paddingVertical: 48, gap: 12 },
  hint: { color: theme.textSecondary, fontSize: 13, textAlign: "center", lineHeight: 19 },
  retryButton: {
    borderWidth: 1,
    borderColor: theme.border,
    borderRadius: radii.button,
    paddingHorizontal: 18,
    paddingVertical: 8,
  },
  retryText: { color: theme.accent, fontSize: 13.5 },
  row: {
    flexDirection: "row",
    alignItems: "center",
    gap: 10,
    paddingHorizontal: 16,
    paddingVertical: 11,
    borderBottomWidth: StyleSheet.hairlineWidth,
    borderBottomColor: theme.border,
  },
  name: { flex: 1, color: theme.text, fontSize: 14 },
  ignored: { color: theme.textHint },
  chevron: { color: theme.textHint, fontSize: 16 },
});
