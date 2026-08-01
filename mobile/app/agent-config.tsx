/**
 * Agent 配置页:远程读写桌面端自定义 agent 的接入配置(接口地址 / API Key / 模型列表)。
 * 内置 claude / codex 没有这些字段,只列出并提示去电脑端改。
 *
 * ⚠️ API Key 默认掩码,只有用户主动点「显示」才明文渲染 —— 与后端
 * agent_config_rpc.rs 的安全说明配套。
 */
import { useCallback, useState } from "react";
import {
  ActivityIndicator,
  Alert,
  FlatList,
  Pressable,
  StyleSheet,
  Text,
  TextInput,
  View,
} from "react-native";
import { useFocusEffect } from "expo-router";
import { t } from "../src/i18n";
import { useConnection } from "../src/state/connection-context";
import type { AgentConfigEntry } from "../src/types";
import { radii, spacing, theme, typography } from "../src/ui/theme";

interface Draft {
  baseUrl: string;
  apiKey: string;
  models: string;
}

function toDraft(agent: AgentConfigEntry): Draft {
  return {
    baseUrl: agent.baseUrl ?? "",
    apiKey: agent.apiKey ?? "",
    models: (agent.models ?? []).join("\n"),
  };
}

export default function AgentConfigScreen() {
  const { request, status } = useConnection();
  const [agents, setAgents] = useState<AgentConfigEntry[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [editingId, setEditingId] = useState<string | null>(null);
  const [draft, setDraft] = useState<Draft>({ baseUrl: "", apiKey: "", models: "" });
  const [revealKey, setRevealKey] = useState(false);
  const [saving, setSaving] = useState(false);

  const refresh = useCallback(() => {
    setLoading(true);
    request<{ agents: AgentConfigEntry[] }>("agentConfig.list")
      .then((res) => {
        setAgents(res.agents ?? []);
        setError(null);
      })
      .catch((err) => setError(err instanceof Error ? err.message : String(err)))
      .finally(() => setLoading(false));
  }, [request]);

  useFocusEffect(
    useCallback(() => {
      refresh();
    }, [refresh]),
  );

  const startEdit = (agent: AgentConfigEntry) => {
    setEditingId(agent.id);
    setDraft(toDraft(agent));
    setRevealKey(false);
  };

  const save = (agent: AgentConfigEntry) => {
    if (saving) return;
    setSaving(true);
    const models = draft.models
      .split("\n")
      .map((line) => line.trim())
      .filter((line) => line.length > 0);
    request("agentConfig.save", {
      id: agent.id,
      baseUrl: draft.baseUrl.trim(),
      apiKey: draft.apiKey.trim(),
      models,
    })
      .then(() => {
        setEditingId(null);
        refresh();
      })
      .catch((err) => {
        Alert.alert(t("hosts.saveFailed"), err instanceof Error ? err.message : String(err));
      })
      .finally(() => setSaving(false));
  };

  return (
    <View style={styles.screen}>
      <FlatList
        data={agents}
        keyExtractor={(item) => item.id}
        contentContainerStyle={styles.list}
        renderItem={({ item }) => {
          const isEditing = item.id === editingId;
          return (
            <View style={styles.card}>
              <View style={styles.cardHead}>
                <View style={styles.cardTitleWrap}>
                  <Text style={styles.cardTitle} numberOfLines={1}>
                    {item.label}
                  </Text>
                  <Text style={styles.cardMeta} numberOfLines={1}>
                    {item.editable
                      ? (item.baseUrl || "—")
                      : t("agentConfig.builtinReadOnly")}
                  </Text>
                </View>
                {item.editable ? (
                  <Pressable
                    hitSlop={10}
                    accessibilityRole="button"
                    onPress={() => (isEditing ? setEditingId(null) : startEdit(item))}
                  >
                    <Text style={styles.actionText}>
                      {isEditing ? t("agentConfig.collapse") : t("agentConfig.edit")}
                    </Text>
                  </Pressable>
                ) : null}
              </View>

              {isEditing ? (
                <View style={styles.editArea}>
                  <Text style={styles.fieldLabel}>{t("agentConfig.baseUrl")}</Text>
                  <TextInput
                    style={styles.input}
                    value={draft.baseUrl}
                    onChangeText={(baseUrl) => setDraft((prev) => ({ ...prev, baseUrl }))}
                    autoCapitalize="none"
                    autoCorrect={false}
                    keyboardType="url"
                    placeholder="https://api.example.com/v1"
                    placeholderTextColor={theme.textHint}
                  />

                  <View style={styles.fieldLabelRow}>
                    <Text style={styles.fieldLabel}>{t("agentConfig.apiKey")}</Text>
                    <Pressable
                      hitSlop={10}
                      accessibilityRole="button"
                      onPress={() => setRevealKey((prev) => !prev)}
                    >
                      <Text style={styles.actionText}>
                        {revealKey ? t("agentConfig.hideKey") : t("agentConfig.showKey")}
                      </Text>
                    </Pressable>
                  </View>
                  <TextInput
                    style={styles.input}
                    value={draft.apiKey}
                    onChangeText={(apiKey) => setDraft((prev) => ({ ...prev, apiKey }))}
                    autoCapitalize="none"
                    autoCorrect={false}
                    secureTextEntry={!revealKey}
                    placeholder="sk-…"
                    placeholderTextColor={theme.textHint}
                  />
                  <Text style={styles.hint}>{t("agentConfig.keyHint")}</Text>

                  <Text style={styles.fieldLabel}>{t("agentConfig.models")}</Text>
                  <TextInput
                    style={[styles.input, styles.inputMultiline]}
                    value={draft.models}
                    onChangeText={(models) => setDraft((prev) => ({ ...prev, models }))}
                    autoCapitalize="none"
                    autoCorrect={false}
                    multiline
                    placeholder={t("agentConfig.modelsHint")}
                    placeholderTextColor={theme.textHint}
                  />

                  <Pressable
                    style={[styles.saveButton, saving && styles.buttonDisabled]}
                    disabled={saving}
                    accessibilityRole="button"
                    onPress={() => save(item)}
                  >
                    {saving ? (
                      <ActivityIndicator size="small" color={theme.onAccent} />
                    ) : (
                      <Text style={styles.saveButtonText}>{t("agentConfig.save")}</Text>
                    )}
                  </Pressable>
                </View>
              ) : null}
            </View>
          );
        }}
        ListEmptyComponent={
          loading ? (
            <ActivityIndicator style={styles.loading} color={theme.textSecondary} />
          ) : (
            <Text style={styles.emptyText}>
              {error ?? (status === "online" ? t("agentConfig.empty") : t("agentConfig.offline"))}
            </Text>
          )
        }
        refreshing={loading && agents.length > 0}
        onRefresh={refresh}
      />
    </View>
  );
}

const styles = StyleSheet.create({
  screen: { flex: 1, backgroundColor: theme.bg },
  list: { padding: spacing.md, gap: spacing.sm },
  card: {
    backgroundColor: theme.bgCard,
    borderRadius: radii.card,
    borderWidth: StyleSheet.hairlineWidth,
    borderColor: theme.border,
  },
  cardHead: {
    flexDirection: "row",
    alignItems: "center",
    gap: spacing.md,
    padding: spacing.md,
  },
  cardTitleWrap: { flex: 1, gap: 3 },
  cardTitle: { color: theme.text, fontSize: 15, fontWeight: "600" },
  cardMeta: { color: theme.textHint, fontSize: typography.metaSize },
  actionText: { color: theme.accent, fontSize: 12.5, fontWeight: "600" },
  editArea: {
    borderTopWidth: StyleSheet.hairlineWidth,
    borderTopColor: theme.border,
    padding: spacing.md,
    gap: spacing.sm,
  },
  fieldLabel: { color: theme.textSecondary, fontSize: typography.metaSize, fontWeight: "600" },
  fieldLabelRow: { flexDirection: "row", alignItems: "center", justifyContent: "space-between" },
  input: {
    borderRadius: radii.input,
    borderWidth: 1,
    borderColor: theme.border,
    backgroundColor: theme.bg,
    color: theme.text,
    paddingHorizontal: spacing.sm + 2,
    paddingVertical: spacing.sm,
    fontSize: 12.5,
  },
  inputMultiline: { minHeight: 84, textAlignVertical: "top" },
  hint: { color: theme.textHint, fontSize: typography.labelSize, lineHeight: 16 },
  saveButton: {
    marginTop: spacing.xs,
    backgroundColor: theme.accent,
    borderRadius: radii.button,
    paddingVertical: 10,
    alignItems: "center",
    justifyContent: "center",
    minHeight: 40,
  },
  buttonDisabled: { opacity: 0.45 },
  saveButtonText: { color: theme.onAccent, fontSize: 13.5, fontWeight: "600" },
  emptyText: {
    color: theme.textSecondary,
    fontSize: 13.5,
    textAlign: "center",
    marginTop: 40,
    paddingHorizontal: spacing.xl,
    lineHeight: 20,
  },
  loading: { marginTop: 40 },
});
