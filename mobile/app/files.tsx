/**
 * 只读文件浏览:project.files 列目录,目录下钻走同路由 push(返回键天然回退),
 * 点文件进 file-view。SSH / WSL 项目提示回桌面查看。
 */

import { router, Stack, useLocalSearchParams } from "expo-router";
import { useCallback, useEffect, useState } from "react";
import { FlatList, Pressable, StyleSheet, Text, View } from "react-native";
import { t } from "../src/i18n";
import { useConnection } from "../src/state/connection-context";
import type { FsEntryView, ProjectFilesResult } from "../src/types";
import { theme } from "../src/ui/theme";

export default function FilesScreen() {
  const params = useLocalSearchParams<{ projectId?: string; path?: string; name?: string }>();
  const projectId = typeof params.projectId === "string" ? params.projectId : "";
  const path = typeof params.path === "string" ? params.path : "";
  const title = typeof params.name === "string" && params.name ? params.name : t("files.title");
  const { request, status } = useConnection();
  const [entries, setEntries] = useState<FsEntryView[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [unavailable, setUnavailable] = useState<"ssh" | "wsl" | null>(null);
  const [loading, setLoading] = useState(false);

  const refresh = useCallback(() => {
    if (status !== "online" || !projectId) return;
    setLoading(true);
    setError(null);
    request<ProjectFilesResult>("project.files", path ? { projectId, path } : { projectId })
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
  }, [path, projectId, request, status]);

  useEffect(() => {
    if (entries === null) refresh();
  }, [entries, refresh]);

  return (
    <View style={styles.screen}>
      <Stack.Screen options={{ title }} />
      {unavailable ? (
        <View style={styles.center}>
          <Text style={styles.hint}>
            {unavailable === "wsl" ? t("changes.unavailable.wsl") : t("changes.unavailable.ssh")}
          </Text>
        </View>
      ) : (
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
                <Pressable style={styles.retryButton} onPress={refresh}>
                  <Text style={styles.retryText}>{t("common.retry")}</Text>
                </Pressable>
              ) : null}
            </View>
          }
          renderItem={({ item }) => (
            <Pressable
              style={styles.row}
              onPress={() =>
                router.push({
                  pathname: item.is_dir ? "/files" : "/file-view",
                  params: { projectId, path: item.path, name: item.name },
                })
              }
            >
              <Text style={styles.icon}>{item.is_dir ? "📁" : "📄"}</Text>
              <Text
                style={[styles.name, item.is_gitignored && styles.ignored]}
                numberOfLines={1}
              >
                {item.name}
              </Text>
              {item.is_dir ? <Text style={styles.chevron}>›</Text> : null}
            </Pressable>
          )}
        />
      )}
    </View>
  );
}

const styles = StyleSheet.create({
  screen: { flex: 1, backgroundColor: theme.bg },
  listContent: { paddingVertical: 6 },
  center: { alignItems: "center", justifyContent: "center", paddingVertical: 48, gap: 12 },
  hint: { color: theme.textSecondary, fontSize: 13, textAlign: "center", lineHeight: 19 },
  retryButton: {
    borderWidth: 1,
    borderColor: theme.border,
    borderRadius: 8,
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
  icon: { fontSize: 15 },
  name: { flex: 1, color: theme.text, fontSize: 14 },
  ignored: { color: theme.textHint },
  chevron: { color: theme.textHint, fontSize: 16 },
});
