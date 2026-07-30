/**
 * 只读文件查看:project.readFile,按行虚拟渲染(FlatList),
 * 超限内容由服务端截断并提示。
 */

import { Stack, useLocalSearchParams } from "expo-router";
import { useCallback, useEffect, useMemo, useState } from "react";
import { FlatList, Platform, Pressable, ScrollView, StyleSheet, Text, View } from "react-native";
import { t } from "../src/i18n";
import { useConnection } from "../src/state/connection-context";
import type { ReadFileResult } from "../src/types";
import { theme } from "../src/ui/theme";

const MONO = Platform.select({ ios: "Menlo", default: "monospace" });

export default function FileViewScreen() {
  const params = useLocalSearchParams<{ projectId?: string; path?: string; name?: string }>();
  const projectId = typeof params.projectId === "string" ? params.projectId : "";
  const path = typeof params.path === "string" ? params.path : "";
  const title = typeof params.name === "string" && params.name ? params.name : t("files.title");
  const { request, status } = useConnection();
  const [result, setResult] = useState<ReadFileResult | null>(null);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(() => {
    if (status !== "online" || !projectId || !path) return;
    setError(null);
    request<ReadFileResult>("project.readFile", { projectId, path })
      .then(setResult)
      .catch((err) => setError(err instanceof Error ? err.message : String(err)));
  }, [path, projectId, request, status]);

  useEffect(() => {
    if (result === null) refresh();
  }, [refresh, result]);

  const lines = useMemo(() => (result?.content ?? "").split("\n"), [result?.content]);

  return (
    <View style={styles.screen}>
      <Stack.Screen options={{ title }} />
      {error ? (
        <View style={styles.center}>
          <Text style={styles.errorText}>{error}</Text>
          <Pressable style={styles.retryButton} onPress={refresh}>
            <Text style={styles.retryText}>{t("common.retry")}</Text>
          </Pressable>
        </View>
      ) : !result ? (
        <View style={styles.center}>
          <Text style={styles.hint}>{t("common.loading")}</Text>
        </View>
      ) : (
        <>
          {result.truncated ? (
            <Text style={styles.truncatedBanner}>
              {t("files.truncated", { kb: Math.round((result.content?.length ?? 0) / 1024) })}
            </Text>
          ) : null}
          <ScrollView horizontal contentContainerStyle={styles.hScroll}>
            <FlatList
              data={lines}
              keyExtractor={(_, index) => String(index)}
              initialNumToRender={60}
              windowSize={11}
              renderItem={({ item }) => (
                <Text style={styles.codeLine} numberOfLines={1}>
                  {item || " "}
                </Text>
              )}
            />
          </ScrollView>
        </>
      )}
    </View>
  );
}

const styles = StyleSheet.create({
  screen: { flex: 1, backgroundColor: theme.bg },
  center: { flex: 1, alignItems: "center", justifyContent: "center", gap: 12, padding: 24 },
  hint: { color: theme.textSecondary, fontSize: 13 },
  errorText: { color: theme.danger, fontSize: 13, textAlign: "center", lineHeight: 19 },
  retryButton: {
    borderWidth: 1,
    borderColor: theme.border,
    borderRadius: 8,
    paddingHorizontal: 18,
    paddingVertical: 8,
  },
  retryText: { color: theme.accent, fontSize: 13.5 },
  truncatedBanner: {
    color: theme.warning,
    fontSize: 11.5,
    paddingHorizontal: 12,
    paddingVertical: 6,
    backgroundColor: theme.bgCard,
  },
  hScroll: { minWidth: "100%" },
  codeLine: {
    fontFamily: MONO,
    fontSize: 11.5,
    lineHeight: 17,
    color: theme.textSecondary,
    paddingHorizontal: 12,
  },
});
