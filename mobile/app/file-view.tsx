/**
 * 文件查看 / 编辑:project.readFile 读取,project.writeFile 保存。
 * 查看态用「竖向 ScrollView + 横向 ScrollView + 整块 Text」渲染(避免 VirtualizedList 嵌套告警);
 * 编辑态用 multiline TextInput。被截断的文件只读,防止保存时截掉尾部内容。
 */

import { Stack, useLocalSearchParams } from "expo-router";
import { useCallback, useEffect, useState } from "react";
import {
  ActivityIndicator,
  Alert,
  Platform,
  ScrollView,
  StyleSheet,
  Text,
  TextInput,
  View,
} from "react-native";
import { t } from "../src/i18n";
import { useConnection } from "../src/state/connection-context";
import type { ReadFileResult, WriteFileResult } from "../src/types";
import { HeaderActions } from "../src/ui/HeaderIconButton";
import { AnimatedPressable } from "../src/ui/AnimatedPressable";
import { radii, spacing, theme } from "../src/ui/theme";
import { useKeyboardInset } from "../src/ui/use-keyboard-inset";

const MONO = Platform.select({ ios: "Menlo", default: "monospace" });

export default function FileViewScreen() {
  const params = useLocalSearchParams<{ projectId?: string; path?: string; name?: string }>();
  const projectId = typeof params.projectId === "string" ? params.projectId : "";
  const path = typeof params.path === "string" ? params.path : "";
  const title = typeof params.name === "string" && params.name ? params.name : t("files.title");
  const { request, status, capabilitiesReady, hasCapability } = useConnection();
  const [result, setResult] = useState<ReadFileResult | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState("");
  const [saving, setSaving] = useState(false);
  const keyboardInset = useKeyboardInset();
  const readSupported = !capabilitiesReady || hasCapability("files.read");
  const writeSupported = !capabilitiesReady || hasCapability("files.write");

  const refresh = useCallback(() => {
    if (status !== "online" || !projectId || !path) return;
    if (!readSupported) {
      setResult(null);
      setError(t("files.unsupported"));
      return;
    }
    setError(null);
    request<ReadFileResult>("project.readFile", { projectId, path })
      .then(setResult)
      .catch((err) => setError(err instanceof Error ? err.message : String(err)));
  }, [path, projectId, readSupported, request, status]);

  useEffect(() => {
    if (result === null) refresh();
  }, [refresh, result]);

  const content = result?.content ?? "";
  const truncated = !!result?.truncated;
  const canEdit = !!result && result.available !== false && !truncated && writeSupported;

  const startEdit = useCallback(() => {
    if (!writeSupported) {
      Alert.alert(t("files.readOnlyUnsupported"));
      return;
    }
    setDraft(content);
    setEditing(true);
  }, [content, writeSupported]);

  const cancelEdit = useCallback(() => {
    setEditing(false);
    setDraft("");
  }, []);

  const save = useCallback(() => {
    if (saving) return;
    if (!writeSupported) {
      Alert.alert(t("files.saveFailed"), t("files.readOnlyUnsupported"));
      return;
    }
    setSaving(true);
    const next = draft;
    request<WriteFileResult>("project.writeFile", { projectId, path, content: next })
      .then((res) => {
        if (res.available === false) throw new Error(t("changes.unavailable.ssh"));
        setResult((prev) => (prev ? { ...prev, content: next } : prev));
        setEditing(false);
        setDraft("");
        Alert.alert(t("files.saved"));
      })
      .catch((err) =>
        Alert.alert(t("files.saveFailed"), err instanceof Error ? err.message : String(err)),
      )
      .finally(() => setSaving(false));
  }, [draft, path, projectId, request, saving, writeSupported]);

  return (
    <View style={[styles.screen, { paddingBottom: keyboardInset }]}>
      <Stack.Screen
        options={{
          title,
          headerRight: () =>
            !result ? null : (
              <HeaderActions>
                {saving ? (
                  <ActivityIndicator size="small" color={theme.accent} />
                ) : editing ? (
                  <>
                    <AnimatedPressable hitSlop={8} onPress={cancelEdit}>
                      <Text style={styles.headerLink}>{t("files.cancelEdit")}</Text>
                    </AnimatedPressable>
                    <AnimatedPressable hitSlop={8} onPress={save}>
                      <Text style={[styles.headerLink, styles.headerLinkStrong]}>
                        {t("files.save")}
                      </Text>
                    </AnimatedPressable>
                  </>
                ) : (
                  <AnimatedPressable hitSlop={8} disabled={!canEdit} onPress={startEdit}>
                    <Text style={[styles.headerLink, !canEdit && styles.headerLinkDisabled]}>
                      {t("files.edit")}
                    </Text>
                  </AnimatedPressable>
                )}
              </HeaderActions>
            ),
        }}
      />
      {error ? (
        <View style={styles.center}>
          <Text style={styles.errorText}>{error}</Text>
          <AnimatedPressable style={styles.retryButton} onPress={refresh}>
            <Text style={styles.retryText}>{t("common.retry")}</Text>
          </AnimatedPressable>
        </View>
      ) : !result ? (
        <View style={styles.center}>
          <Text style={styles.hint}>{t("common.loading")}</Text>
        </View>
      ) : (
        <>
          {truncated ? (
            <Text style={styles.truncatedBanner}>
              {t("files.truncated", { kb: Math.round(content.length / 1024) })} ·{" "}
              {t("files.readOnlyTruncated")}
            </Text>
          ) : null}
          {!truncated && !writeSupported ? (
            <Text style={styles.truncatedBanner}>{t("files.readOnlyUnsupported")}</Text>
          ) : null}
          {editing ? (
            <TextInput
              style={styles.editor}
              value={draft}
              onChangeText={setDraft}
              multiline
              autoCorrect={false}
              autoCapitalize="none"
              spellCheck={false}
              textAlignVertical="top"
            />
          ) : (
            <ScrollView contentContainerStyle={styles.vScroll}>
              <ScrollView horizontal contentContainerStyle={styles.hScroll}>
                <Text style={styles.code} selectable>
                  {content || " "}
                </Text>
              </ScrollView>
            </ScrollView>
          )}
        </>
      )}
    </View>
  );
}

const styles = StyleSheet.create({
  screen: { flex: 1, backgroundColor: theme.bg },
  headerLink: { color: theme.accent, fontSize: 13.5, fontWeight: "600" },
  headerLinkStrong: { fontWeight: "700" },
  headerLinkDisabled: { color: theme.textHint },
  center: { flex: 1, alignItems: "center", justifyContent: "center", gap: 12, padding: 24 },
  hint: { color: theme.textSecondary, fontSize: 13 },
  errorText: { color: theme.danger, fontSize: 13, textAlign: "center", lineHeight: 19 },
  retryButton: {
    borderWidth: 1,
    borderColor: theme.border,
    borderRadius: radii.button,
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
  vScroll: { paddingVertical: spacing.sm },
  hScroll: { minWidth: "100%" },
  code: {
    fontFamily: MONO,
    fontSize: 11.5,
    lineHeight: 17,
    color: theme.textSecondary,
    paddingHorizontal: 12,
  },
  editor: {
    flex: 1,
    fontFamily: MONO,
    fontSize: 12,
    lineHeight: 18,
    color: theme.text,
    paddingHorizontal: 12,
    paddingVertical: spacing.sm,
  },
});
