/** 移动端 Agent 配置:内置/自定义统一支持编辑、检测模型、代理与兼容桥接。 */
import { useCallback, useMemo, useState, type Dispatch, type SetStateAction } from "react";
import {
  ActivityIndicator,
  Alert,
  FlatList,
  Modal,
  Pressable,
  ScrollView,
  StyleSheet,
  Text,
  TextInput,
  View,
} from "react-native";
import { useFocusEffect } from "expo-router";
import { t } from "../src/i18n";
import { useConnection } from "../src/state/connection-context";
import { AnimatedSelection } from "../src/ui/AnimatedSelection";
import type { AgentConfigEntry } from "../src/types";
import { radii, spacing, theme, typography } from "../src/ui/theme";

type Provider = "anthropic" | "openai";
type ViewMode = "bar" | "grid";
type AgentKind = "codex" | "claude_code";

interface Draft {
  name: string;
  baseUrl: string;
  apiKey: string;
  modelsText: string;
  kind: AgentKind;
  bridge: boolean;
  proxy: boolean;
}

function normalizeModels(models: string[]): string[] {
  const result: string[] = [];
  const seen = new Set<string>();
  for (const raw of models) {
    const model = raw.trim();
    const key = model.toLocaleLowerCase();
    if (!model || seen.has(key)) continue;
    seen.add(key);
    result.push(model);
  }
  return result;
}

function providerOf(agent: Pick<AgentConfigEntry, "codexLike">): Provider {
  return agent.codexLike ? "openai" : "anthropic";
}

function toDraft(agent?: AgentConfigEntry, kind: AgentKind = "codex"): Draft {
  return {
    name: agent?.label ?? "",
    baseUrl: agent?.baseUrl ?? "",
    apiKey: agent?.apiKey ?? "",
    modelsText: (agent?.models ?? []).join("\n"),
    kind: agent ? (agent.codexLike ? "codex" : "claude_code") : kind,
    bridge: Boolean(agent?.enableChatCompletionsProxy),
    proxy: Boolean(agent?.proxyEnabled),
  };
}

function parseTextModels(text: string): string[] {
  return normalizeModels(text.split(/[\n,]/));
}

function ToggleRow({
  label,
  hint,
  value,
  onChange,
}: {
  label: string;
  hint: string;
  value: boolean;
  onChange: (value: boolean) => void;
}) {
  return (
    <Pressable style={styles.toggleRow} onPress={() => onChange(!value)} accessibilityRole="switch" accessibilityState={{ checked: value }}>
      <View style={styles.toggleCopy}>
        <Text style={styles.fieldLabel}>{label}</Text>
        <Text style={styles.hint}>{hint}</Text>
      </View>
      <View style={[styles.switchTrack, value && styles.switchTrackActive]}>
        <View style={[styles.switchThumb, value && styles.switchThumbActive]} />
      </View>
    </Pressable>
  );
}

function AgentEditor({
  draft,
  setDraft,
  availableModels,
  selectedModels,
  setAvailableModels,
  setSelectedModels,
  canChangeKind,
  agentId,
  showBridge,
  request,
  detecting,
  setDetecting,
  modelInput,
  setModelInput,
}: {
  draft: Draft;
  setDraft: Dispatch<SetStateAction<Draft>>;
  availableModels: string[];
  selectedModels: string[];
  setAvailableModels: Dispatch<SetStateAction<string[]>>;
  setSelectedModels: Dispatch<SetStateAction<string[]>>;
  canChangeKind: boolean;
  agentId?: string;
  showBridge: boolean;
  request: ReturnType<typeof useConnection>["request"];
  detecting: boolean;
  setDetecting: (value: boolean) => void;
  modelInput: string;
  setModelInput: (value: string) => void;
}) {
  const detect = () => {
    if ((!agentId && (!draft.baseUrl.trim() || !draft.apiKey.trim())) || detecting) return;
    setDetecting(true);
    const promise = agentId && (!draft.baseUrl.trim() || !draft.apiKey.trim())
      ? request<{ models: string[] }>("agents.models", { agent: agentId })
      : request<{ models: string[] }>("agentConfig.detectModels", {
          kind: draft.kind,
          baseUrl: draft.baseUrl.trim(),
          apiKey: draft.apiKey.trim(),
        });
    promise
      .then((result) => {
        const models = normalizeModels(result.models ?? []);
        setAvailableModels(models);
        setSelectedModels(models);
        setDraft((previous) => ({ ...previous, modelsText: models.join("\n") }));
      })
      .catch((error) => Alert.alert(t("agentConfig.detectFailed"), error instanceof Error ? error.message : String(error)))
      .finally(() => setDetecting(false));
  };

  const addModel = () => {
    const model = modelInput.trim();
    if (!model) return;
    const nextModels = normalizeModels([...selectedModels, model]);
    setAvailableModels((previous) => normalizeModels([...previous, model]));
    setSelectedModels(nextModels);
    setDraft((previous) => ({ ...previous, modelsText: nextModels.join("\n") }));
    setModelInput("");
  };

  const toggleModel = (model: string) => {
    const nextModels = selectedModels.some(
      (item) => item.toLocaleLowerCase() === model.toLocaleLowerCase(),
    )
      ? selectedModels.filter((item) => item.toLocaleLowerCase() !== model.toLocaleLowerCase())
      : [...selectedModels, model];
    setSelectedModels(nextModels);
    setDraft((previous) => ({ ...previous, modelsText: nextModels.join("\n") }));
  };

  return (
    <View style={styles.editor}>
      {canChangeKind ? (
        <>
          <Text style={styles.fieldLabel}>{t("agentConfig.kind")}</Text>
          <AnimatedSelection
            value={draft.kind}
            options={[
              { value: "codex", label: t("agentConfig.codex") },
              { value: "claude_code", label: t("agentConfig.claude") },
            ]}
            onChange={(kind) => setDraft((previous) => ({ ...previous, kind }))}
          />
        </>
      ) : null}
      {canChangeKind ? (
        <>
          <Text style={styles.fieldLabel}>{t("agentConfig.name")}</Text>
          <TextInput
            style={styles.input}
            value={draft.name}
            onChangeText={(name) => setDraft((previous) => ({ ...previous, name }))}
            placeholder={t("agentConfig.nameHint")}
            placeholderTextColor={theme.textHint}
            autoCapitalize="none"
          />
        </>
      ) : null}
      <Text style={styles.fieldLabel}>{t("agentConfig.baseUrl")}</Text>
      <TextInput
        style={styles.input}
        value={draft.baseUrl}
        onChangeText={(baseUrl) => setDraft((previous) => ({ ...previous, baseUrl }))}
        autoCapitalize="none"
        autoCorrect={false}
        keyboardType="url"
        placeholder="https://api.example.com/v1"
        placeholderTextColor={theme.textHint}
      />
      <Text style={styles.fieldLabel}>{t("agentConfig.apiKey")}</Text>
      <TextInput
        style={styles.input}
        value={draft.apiKey}
        onChangeText={(apiKey) => setDraft((previous) => ({ ...previous, apiKey }))}
        autoCapitalize="none"
        autoCorrect={false}
        secureTextEntry
        placeholder="sk-…"
        placeholderTextColor={theme.textHint}
      />
      <Text style={styles.fieldLabel}>{t("agentConfig.models")}</Text>
      <TextInput
        style={[styles.input, styles.inputMultiline]}
        value={draft.modelsText}
        onChangeText={(modelsText) => {
          setDraft((previous) => ({ ...previous, modelsText }));
          setSelectedModels(parseTextModels(modelsText));
        }}
        autoCapitalize="none"
        autoCorrect={false}
        multiline
        placeholder={t("agentConfig.modelsHint")}
        placeholderTextColor={theme.textHint}
      />
      <View style={styles.modelActionRow}>
        <Pressable
          style={[styles.outlineButton, ((!agentId && (!draft.baseUrl.trim() || !draft.apiKey.trim())) || detecting) && styles.buttonDisabled]}
          disabled={(!agentId && (!draft.baseUrl.trim() || !draft.apiKey.trim())) || detecting}
          onPress={detect}
        >
          {detecting ? <ActivityIndicator size="small" color={theme.accent} /> : <Text style={styles.outlineButtonText}>{t("agentConfig.detect")}</Text>}
        </Pressable>
        <TextInput
          style={[styles.input, styles.modelInput]}
          value={modelInput}
          onChangeText={setModelInput}
          autoCapitalize="none"
          autoCorrect={false}
          placeholder={t("agentConfig.addModel")}
          placeholderTextColor={theme.textHint}
          onSubmitEditing={addModel}
        />
        <Pressable style={styles.outlineButton} onPress={addModel}>
          <Text style={styles.outlineButtonText}>+</Text>
        </Pressable>
      </View>
      {availableModels.length > 0 ? (
        <View style={styles.scannedModels}>
          <Text style={styles.hint}>{t("agentConfig.scannedModels")}</Text>
          {availableModels.map((model) => {
            const selected = selectedModels.some((item) => item.toLocaleLowerCase() === model.toLocaleLowerCase());
            return (
              <Pressable key={model} style={[styles.modelCheckRow, selected && styles.modelCheckRowActive]} onPress={() => toggleModel(model)}>
                <View style={[styles.checkbox, selected && styles.checkboxActive]} />
                <Text style={styles.modelCheckText} numberOfLines={1} ellipsizeMode="tail">{model}</Text>
              </Pressable>
            );
          })}
        </View>
      ) : null}
      {draft.kind === "codex" && showBridge ? (
        <ToggleRow
          label={t("agentConfig.bridge")}
          hint={t("agentConfig.bridgeHint")}
          value={draft.bridge}
          onChange={(bridge) => setDraft((previous) => ({ ...previous, bridge }))}
        />
      ) : null}
      <ToggleRow
        label={t("agentConfig.proxy")}
        hint={t("agentConfig.proxyHint")}
        value={draft.proxy}
        onChange={(proxy) => setDraft((previous) => ({ ...previous, proxy }))}
      />
    </View>
  );
}

export default function AgentConfigScreen() {
  const { request, status } = useConnection();
  const [agents, setAgents] = useState<AgentConfigEntry[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [provider, setProvider] = useState<Provider>("anthropic");
  const [viewMode, setViewMode] = useState<ViewMode>("bar");
  const [editingId, setEditingId] = useState<string | null>(null);
  const [draft, setDraft] = useState<Draft>(() => toDraft(undefined));
  const [availableModels, setAvailableModels] = useState<string[]>([]);
  const [selectedModels, setSelectedModels] = useState<string[]>([]);
  const [modelInput, setModelInput] = useState("");
  const [detecting, setDetecting] = useState(false);
  const [saving, setSaving] = useState(false);
  const [createOpen, setCreateOpen] = useState(false);

  const refresh = useCallback(() => {
    setLoading(true);
    request<{ agents: AgentConfigEntry[] }>("agentConfig.list")
      .then((result) => {
        setAgents(result.agents ?? []);
        setError(null);
      })
      .catch((err) => setError(err instanceof Error ? err.message : String(err)))
      .finally(() => setLoading(false));
  }, [request]);

  useFocusEffect(useCallback(() => { refresh(); }, [refresh]));

  const visibleAgents = useMemo(() => agents.filter((agent) => providerOf(agent) === provider), [agents, provider]);

  const startEdit = (agent: AgentConfigEntry) => {
    setEditingId(agent.id);
    setDraft(toDraft(agent));
    setAvailableModels(normalizeModels(agent.models ?? []));
    setSelectedModels(normalizeModels(agent.models ?? []));
    setModelInput("");
  };

  const startCreate = () => {
    const kind: AgentKind = provider === "openai" ? "codex" : "claude_code";
    setDraft(toDraft(undefined, kind));
    setAvailableModels([]);
    setSelectedModels([]);
    setModelInput("");
    setEditingId(null);
    setCreateOpen(true);
  };

  const closeEditor = () => {
    setEditingId(null);
    setCreateOpen(false);
  };

  const save = (agent?: AgentConfigEntry) => {
    if (saving) return;
    const models = parseTextModels(draft.modelsText);
    if ((!agent && !draft.name.trim()) || models.length === 0) {
      Alert.alert(t("agentConfig.createFailed"), t("newTask.modelRequired"));
      return;
    }
    setSaving(true);
    const payload = {
      ...(agent ? { id: agent.id } : {}),
      ...(agent ? {} : { label: draft.name.trim(), kind: draft.kind }),
      baseUrl: draft.baseUrl.trim(),
      apiKey: draft.apiKey.trim(),
      models,
      enableChatCompletionsProxy: draft.kind === "codex" && draft.bridge,
      proxyEnabled: draft.proxy,
    };
    request(agent ? "agentConfig.save" : "agentConfig.create", payload)
      .then(() => {
        closeEditor();
        refresh();
        if (!agent) Alert.alert(t("agentConfig.created"));
      })
      .catch((err) => Alert.alert(agent ? t("hosts.saveFailed") : t("agentConfig.createFailed"), err instanceof Error ? err.message : String(err)))
      .finally(() => setSaving(false));
  };

  const renderEditor = (agent?: AgentConfigEntry) => (
    <View style={styles.editorWrap}>
      <AgentEditor
        draft={draft}
        setDraft={setDraft}
        availableModels={availableModels}
        selectedModels={selectedModels}
        setAvailableModels={setAvailableModels}
        setSelectedModels={setSelectedModels}
        canChangeKind={!agent}
        agentId={agent?.id}
        showBridge={agent?.id !== "codex"}
        request={request}
        detecting={detecting}
        setDetecting={setDetecting}
        modelInput={modelInput}
        setModelInput={setModelInput}
      />
      <View style={styles.editorActions}>
        <Pressable style={styles.cancelButton} onPress={closeEditor}>
          <Text style={styles.cancelButtonText}>{t("hosts.cancel")}</Text>
        </Pressable>
        <Pressable style={[styles.saveButton, saving && styles.buttonDisabled]} disabled={saving} onPress={() => save(agent)}>
          {saving ? <ActivityIndicator size="small" color={theme.onAccent} /> : <Text style={styles.saveButtonText}>{agent ? t("agentConfig.save") : t("agentConfig.create")}</Text>}
        </Pressable>
      </View>
    </View>
  );

  return (
    <View style={styles.screen}>
      <FlatList
        key={`${provider}-${viewMode}`}
        data={visibleAgents}
        numColumns={viewMode === "grid" ? 2 : 1}
        keyExtractor={(item) => item.id}
        columnWrapperStyle={viewMode === "grid" ? styles.gridRow : undefined}
        contentContainerStyle={styles.list}
        ListHeaderComponent={
          <View style={styles.toolbar}>
            <AnimatedSelection
              value={provider}
              options={[
                { value: "anthropic", label: t("agentConfig.anthropic") },
                { value: "openai", label: t("agentConfig.openai") },
              ]}
              onChange={setProvider}
            />
            <View style={styles.toolbarRow}>
              <AnimatedSelection
                value={viewMode}
                options={[
                  { value: "bar", label: t("agentConfig.barView") },
                  { value: "grid", label: t("agentConfig.gridView") },
                ]}
                onChange={setViewMode}
              />
              <Pressable style={styles.addButton} onPress={startCreate}>
                <Text style={styles.addButtonText}>＋ {t("agentConfig.add")}</Text>
              </Pressable>
            </View>
            {createOpen ? renderEditor() : null}
          </View>
        }
        renderItem={({ item }) => {
          const editing = item.id === editingId;
          return (
            <View style={[styles.card, viewMode === "grid" && styles.gridCard]}>
              <View style={styles.cardHead}>
                <View style={styles.cardTitleWrap}>
                  <Text style={styles.cardTitle} numberOfLines={1}>{item.label}</Text>
                  <Text style={styles.cardMeta} numberOfLines={1}>
                    {item.baseUrl || (item.codexLike ? t("agentConfig.openai") : t("agentConfig.anthropic"))}
                  </Text>
                </View>
                <Pressable hitSlop={10} onPress={() => (editing ? closeEditor() : startEdit(item))}>
                  <Text style={styles.actionText}>{editing ? t("agentConfig.collapse") : t("agentConfig.edit")}</Text>
                </Pressable>
              </View>
              {editing ? renderEditor(item) : null}
            </View>
          );
        }}
        ListEmptyComponent={
          loading ? <ActivityIndicator style={styles.loading} color={theme.textSecondary} /> : <Text style={styles.emptyText}>{error ?? (status === "online" ? t("agentConfig.empty") : t("agentConfig.offline"))}</Text>
        }
        refreshing={loading && agents.length > 0}
        onRefresh={refresh}
      />
    </View>
  );
}

const styles = StyleSheet.create({
  screen: { flex: 1, backgroundColor: theme.bg },
  list: { padding: spacing.md, gap: spacing.sm, paddingBottom: 36 },
  toolbar: { gap: spacing.sm, marginBottom: spacing.sm },
  toolbarRow: { flexDirection: "row", alignItems: "center", gap: spacing.sm },
  addButton: { flex: 1, minHeight: 34, alignItems: "center", justifyContent: "center", borderRadius: radii.button, backgroundColor: theme.accent, paddingHorizontal: spacing.sm },
  addButtonText: { color: theme.onAccent, fontSize: 13, fontWeight: "700" },
  gridRow: { gap: spacing.sm },
  card: { flex: 1, backgroundColor: theme.bgCard, borderRadius: radii.card, borderWidth: StyleSheet.hairlineWidth, borderColor: theme.border, marginBottom: spacing.sm, overflow: "hidden" },
  gridCard: { maxWidth: "50%" },
  cardHead: { flexDirection: "row", alignItems: "center", gap: spacing.md, padding: spacing.md },
  cardTitleWrap: { flex: 1, minWidth: 0, gap: 3 },
  cardTitle: { color: theme.text, fontSize: 15, fontWeight: "600" },
  cardMeta: { color: theme.textHint, fontSize: typography.metaSize },
  actionText: { color: theme.accent, fontSize: typography.metaSize, fontWeight: "600" },
  editorWrap: { borderTopWidth: StyleSheet.hairlineWidth, borderTopColor: theme.border, padding: spacing.md, gap: spacing.md },
  editor: { gap: spacing.sm },
  fieldLabel: { color: theme.textSecondary, fontSize: typography.metaSize, fontWeight: "700" },
  hint: { color: theme.textHint, fontSize: typography.metaSize, lineHeight: 18 },
  input: { minHeight: 40, borderRadius: radii.input, borderWidth: StyleSheet.hairlineWidth, borderColor: theme.border, backgroundColor: theme.bgElevated, color: theme.text, paddingHorizontal: spacing.md, paddingVertical: spacing.sm, fontSize: 13 },
  inputMultiline: { minHeight: 76, textAlignVertical: "top" },
  modelActionRow: { flexDirection: "row", alignItems: "center", gap: spacing.sm },
  modelInput: { flex: 1, minWidth: 0 },
  outlineButton: { minHeight: 38, alignItems: "center", justifyContent: "center", paddingHorizontal: spacing.sm, borderRadius: radii.button, borderWidth: StyleSheet.hairlineWidth, borderColor: theme.border, backgroundColor: theme.bgElevated },
  outlineButtonText: { color: theme.text, fontSize: 12.5, fontWeight: "600" },
  scannedModels: { maxHeight: 210, gap: 4, padding: 4, borderRadius: radii.row, backgroundColor: theme.bgElevated },
  modelCheckRow: { minHeight: 36, flexDirection: "row", alignItems: "center", gap: spacing.sm, paddingHorizontal: spacing.sm, borderRadius: radii.button },
  modelCheckRowActive: { backgroundColor: "#2454a0" },
  modelCheckText: { flex: 1, minWidth: 0, color: theme.text, fontSize: 12.5 },
  checkbox: { width: 15, height: 15, borderRadius: 4, borderWidth: 1.5, borderColor: theme.border },
  checkboxActive: { backgroundColor: theme.accent, borderColor: theme.accent },
  toggleRow: { flexDirection: "row", alignItems: "center", gap: spacing.md, paddingVertical: spacing.xs },
  toggleCopy: { flex: 1, gap: 2 },
  switchTrack: { width: 42, height: 24, borderRadius: 12, justifyContent: "center", padding: 3, backgroundColor: theme.border },
  switchTrackActive: { backgroundColor: theme.accent },
  switchThumb: { width: 18, height: 18, borderRadius: 9, backgroundColor: theme.textSecondary },
  switchThumbActive: { alignSelf: "flex-end", backgroundColor: theme.onAccent },
  editorActions: { flexDirection: "row", justifyContent: "flex-end", gap: spacing.sm },
  cancelButton: { minHeight: 40, justifyContent: "center", paddingHorizontal: spacing.md },
  cancelButtonText: { color: theme.textSecondary, fontSize: 13, fontWeight: "600" },
  saveButton: { minHeight: 40, minWidth: 84, alignItems: "center", justifyContent: "center", paddingHorizontal: spacing.md, borderRadius: radii.button, backgroundColor: theme.accent },
  saveButtonText: { color: theme.onAccent, fontSize: 13, fontWeight: "700" },
  buttonDisabled: { opacity: 0.45 },
  loading: { marginTop: spacing.xl },
  emptyText: { color: theme.textHint, textAlign: "center", padding: spacing.xl, lineHeight: 20 },
});
