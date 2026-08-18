/** 移动端 Agent 配置:内置/自定义统一支持编辑、获取可用模型、代理与兼容桥接。 */
import { useCallback, useMemo, useState, type Dispatch, type SetStateAction } from "react";
import {
  ActivityIndicator,
  Alert,
  FlatList,
  ScrollView,
  StyleSheet,
  Text,
  TextInput,
  View,
} from "react-native";
import { useFocusEffect } from "expo-router";
import { BrainCircuit, Check, LayoutGrid, Plus, Rows3, Search } from "lucide-react-native";
import { t } from "../src/i18n";
import { agentFamilyOf } from "../src/agent-family";
import { useConnection } from "../src/state/connection-context";
import { AnimatedPressable } from "../src/ui/AnimatedPressable";
import { AnimatedSelection } from "../src/ui/AnimatedSelection";
import { ANTHROPIC_BRAND, AnthropicIcon, OpenAIIcon } from "../src/ui/brand-icons";
import { fuzzyMatch } from "../src/ui/fuzzy-match";
import type { AgentConfigEntry } from "../src/types";
import { radii, spacing, theme, typography } from "../src/ui/theme";

type Provider = "anthropic" | "openai" | "deepseek";
type ViewMode = "bar" | "grid";
type AgentKind = "codex" | "claude_code" | "dsh";

interface Draft {
  name: string;
  baseUrl: string;
  apiKey: string;
  clearApiKey: boolean;
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

function providerOf(agent: Pick<AgentConfigEntry, "family" | "codexLike">): Provider {
  const family = agentFamilyOf(agent);
  return family === "dsh" ? "deepseek" : family === "codex" ? "openai" : "anthropic";
}

function toDraft(agent?: AgentConfigEntry, kind: AgentKind = "codex"): Draft {
  return {
    name: agent?.label ?? "",
    baseUrl: agent?.baseUrl ?? "",
    apiKey: "",
    clearApiKey: false,
    kind: agent
      ? agentFamilyOf(agent) === "dsh"
        ? "dsh"
        : agentFamilyOf(agent) === "codex"
          ? "codex"
          : "claude_code"
      : kind,
    bridge: Boolean(agent?.enableChatCompletionsProxy),
    proxy: Boolean(agent?.proxyEnabled),
  };
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
    <AnimatedPressable
      style={styles.toggleRow}
      onPress={() => onChange(!value)}
      accessibilityRole="switch"
      accessibilityState={{ checked: value }}
    >
      <View style={styles.toggleCopy}>
        <Text style={styles.fieldLabel}>{label}</Text>
        <Text style={styles.hint}>{hint}</Text>
      </View>
      <View style={[styles.switchTrack, value && styles.switchTrackActive]}>
        <View style={[styles.switchThumb, value && styles.switchThumbActive]} />
      </View>
    </AnimatedPressable>
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
  apiKeyConfigured,
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
  apiKeyConfigured: boolean;
  showBridge: boolean;
  request: ReturnType<typeof useConnection>["request"];
  detecting: boolean;
  setDetecting: (value: boolean) => void;
  modelInput: string;
  setModelInput: (value: string) => void;
}) {
  const hasUsableApiKey =
    Boolean(draft.apiKey.trim()) || (Boolean(agentId) && apiKeyConfigured && !draft.clearApiKey);
  const detectDisabled = !draft.baseUrl.trim() || !hasUsableApiKey || detecting;

  const detect = () => {
    if (detectDisabled) return;
    setDetecting(true);
    const promise = request<{ models: string[] }>("agentConfig.detectModels", {
      ...(agentId ? { id: agentId } : {}),
      kind: draft.kind,
      baseUrl: draft.baseUrl.trim(),
      ...(draft.apiKey.trim() ? { apiKey: draft.apiKey.trim() } : {}),
    });
    promise
      .then((result) => {
        const scanned = normalizeModels(result.models ?? []);
        // 不动 selectedModels:保留用户此前的勾选。已勾选但本次未扫描到的模型
        // 追加到末尾,避免它们从列表消失后无法取消。
        setAvailableModels(normalizeModels([...scanned, ...selectedModels]));
      })
      .catch((error) =>
        Alert.alert(
          t("agentConfig.detectFailed"),
          error instanceof Error ? error.message : String(error),
        ),
      )
      .finally(() => setDetecting(false));
  };

  const addModel = () => {
    const model = modelInput.trim();
    if (!model) return;
    setAvailableModels((previous) => normalizeModels([...previous, model]));
    setSelectedModels((previous) => normalizeModels([...previous, model]));
    setModelInput("");
  };

  const toggleModel = (model: string) => {
    setSelectedModels((previous) =>
      previous.some((item) => item.toLocaleLowerCase() === model.toLocaleLowerCase())
        ? previous.filter((item) => item.toLocaleLowerCase() !== model.toLocaleLowerCase())
        : [...previous, model],
    );
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
              { value: "dsh", label: t("agentConfig.dsh") },
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
        onChangeText={(apiKey) =>
          setDraft((previous) => ({
            ...previous,
            apiKey,
            clearApiKey: apiKey.trim() ? false : previous.clearApiKey,
          }))
        }
        autoCapitalize="none"
        autoCorrect={false}
        secureTextEntry
        placeholder={
          agentId && apiKeyConfigured ? t("agentConfig.apiKeyConfiguredPlaceholder") : "sk-…"
        }
        placeholderTextColor={theme.textHint}
      />
      {agentId && apiKeyConfigured ? (
        <>
          <Text style={styles.hint}>{t("agentConfig.apiKeyConfiguredHint")}</Text>
          <ToggleRow
            label={t("agentConfig.clearApiKey")}
            hint={t("agentConfig.clearApiKeyHint")}
            value={draft.clearApiKey}
            onChange={(clearApiKey) =>
              setDraft((previous) => ({
                ...previous,
                clearApiKey,
                apiKey: clearApiKey ? "" : previous.apiKey,
              }))
            }
          />
        </>
      ) : null}
      <View style={styles.modelActionRow}>
        <AnimatedPressable
          style={[styles.outlineButton, detectDisabled && styles.buttonDisabled]}
          disabled={detectDisabled}
          onPress={detect}
        >
          {detecting ? (
            <ActivityIndicator size="small" color={theme.accent} />
          ) : (
            <Text style={styles.outlineButtonText}>{t("agentConfig.detect")}</Text>
          )}
        </AnimatedPressable>
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
        <AnimatedPressable
          style={styles.outlineButton}
          onPress={addModel}
          accessibilityLabel={t("agentConfig.addModel")}
        >
          <Plus size={16} color={theme.text} />
        </AnimatedPressable>
      </View>
      {availableModels.length > 0 ? (
        <View style={styles.scannedModels}>
          <View style={styles.scannedHead}>
            <Text style={styles.fieldLabel}>{t("agentConfig.scannedModels")}</Text>
            <Text style={styles.hint}>
              {t("agentConfig.scannedSelected", {
                selected: String(selectedModels.length),
                total: String(availableModels.length),
              })}
            </Text>
          </View>
          {/* 独立可滚动区:高度上限落在 ScrollView 上,外层 overflow:hidden 裁剪,
              模型再多也不会溢出压住下方的代理开关与取消/保存。 */}
          <ScrollView
            style={styles.scannedScroll}
            contentContainerStyle={styles.scannedList}
            nestedScrollEnabled
            keyboardShouldPersistTaps="handled"
          >
            {availableModels.map((model) => {
              const selected = selectedModels.some(
                (item) => item.toLocaleLowerCase() === model.toLocaleLowerCase(),
              );
              return (
                <AnimatedPressable
                  key={model}
                  style={[styles.modelCheckRow, selected && styles.modelCheckRowActive]}
                  onPress={() => toggleModel(model)}
                  accessibilityRole="checkbox"
                  accessibilityState={{ checked: selected }}
                >
                  <View style={[styles.checkbox, selected && styles.checkboxActive]}>
                    {selected ? <Check size={11} color={theme.onAccent} strokeWidth={3} /> : null}
                  </View>
                  <Text style={styles.modelCheckText} numberOfLines={1} ellipsizeMode="tail">
                    {model}
                  </Text>
                </AnimatedPressable>
              );
            })}
          </ScrollView>
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
  const [query, setQuery] = useState("");
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

  useFocusEffect(
    useCallback(() => {
      refresh();
    }, [refresh]),
  );

  const providerAgents = useMemo(
    () => agents.filter((agent) => providerOf(agent) === provider),
    [agents, provider],
  );

  const visibleAgents = useMemo(
    () => providerAgents.filter((agent) => fuzzyMatch(agent.label, query)),
    [providerAgents, query],
  );

  const startEdit = (agent: AgentConfigEntry) => {
    setEditingId(agent.id);
    setDraft(toDraft(agent));
    setAvailableModels(normalizeModels(agent.models ?? []));
    setSelectedModels(normalizeModels(agent.models ?? []));
    setModelInput("");
  };

  const startCreate = () => {
    const kind: AgentKind =
      provider === "openai" ? "codex" : provider === "deepseek" ? "dsh" : "claude_code";
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
    const models = normalizeModels(selectedModels);
    if (
      (!agent && (!draft.name.trim() || !draft.baseUrl.trim() || !draft.apiKey.trim())) ||
      models.length === 0
    ) {
      Alert.alert(t("agentConfig.createFailed"), t("newTask.modelRequired"));
      return;
    }
    setSaving(true);
    const payload = {
      ...(agent ? { id: agent.id } : {}),
      ...(agent ? {} : { label: draft.name.trim(), kind: draft.kind }),
      baseUrl: draft.baseUrl.trim(),
      ...(draft.apiKey.trim() ? { apiKey: draft.apiKey.trim() } : {}),
      ...(agent && draft.clearApiKey ? { clearApiKey: true } : {}),
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
      .catch((err) =>
        Alert.alert(
          agent ? t("hosts.saveFailed") : t("agentConfig.createFailed"),
          err instanceof Error ? err.message : String(err),
        ),
      )
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
        apiKeyConfigured={Boolean(agent?.apiKeyConfigured)}
        showBridge={agent?.id !== "codex"}
        request={request}
        detecting={detecting}
        setDetecting={setDetecting}
        modelInput={modelInput}
        setModelInput={setModelInput}
      />
      <View style={styles.editorActions}>
        <AnimatedPressable style={styles.cancelButton} onPress={closeEditor}>
          <Text style={styles.cancelButtonText}>{t("hosts.cancel")}</Text>
        </AnimatedPressable>
        <AnimatedPressable
          style={[styles.saveButton, saving && styles.buttonDisabled]}
          disabled={saving}
          onPress={() => save(agent)}
        >
          {saving ? (
            <ActivityIndicator size="small" color={theme.onAccent} />
          ) : (
            <Text style={styles.saveButtonText}>
              {agent ? t("agentConfig.save") : t("agentConfig.create")}
            </Text>
          )}
        </AnimatedPressable>
      </View>
    </View>
  );

  return (
    <View style={styles.screen}>
      <FlatList
        key={`${provider}-${viewMode}-${editingId ? "editing" : "browsing"}`}
        data={visibleAgents}
        numColumns={editingId ? 1 : viewMode === "grid" ? 2 : 1}
        keyExtractor={(item) => item.id}
        columnWrapperStyle={!editingId && viewMode === "grid" ? styles.gridRow : undefined}
        contentContainerStyle={styles.list}
        ListHeaderComponent={
          <View style={styles.toolbar}>
            <AnimatedSelection
              value={provider}
              options={[
                {
                  value: "anthropic",
                  label: t("agentConfig.anthropic"),
                  icon: (
                    <AnthropicIcon
                      size={16}
                      color={provider === "anthropic" ? theme.onAccent : ANTHROPIC_BRAND}
                    />
                  ),
                },
                {
                  value: "openai",
                  label: t("agentConfig.openai"),
                  icon: (
                    <OpenAIIcon
                      size={16}
                      color={provider === "openai" ? theme.onAccent : theme.text}
                    />
                  ),
                },
                {
                  value: "deepseek",
                  label: t("agentConfig.deepseek"),
                  icon: (
                    <BrainCircuit
                      size={16}
                      color={provider === "deepseek" ? theme.onAccent : theme.accent}
                    />
                  ),
                },
              ]}
              onChange={setProvider}
              showDividers
            />
            <View style={styles.toolbarRow}>
              <View style={styles.searchBox}>
                <Search size={15} color={theme.textHint} />
                <TextInput
                  style={styles.searchInput}
                  value={query}
                  onChangeText={setQuery}
                  placeholder={t("agentConfig.searchPlaceholder")}
                  placeholderTextColor={theme.textHint}
                  autoCapitalize="none"
                  autoCorrect={false}
                  clearButtonMode="while-editing"
                  returnKeyType="search"
                />
              </View>
              <AnimatedSelection
                value={viewMode}
                options={[
                  {
                    value: "bar",
                    label: t("agentConfig.barView"),
                    icon: (
                      <Rows3
                        size={16}
                        color={viewMode === "bar" ? theme.onAccent : theme.textSecondary}
                      />
                    ),
                  },
                  {
                    value: "grid",
                    label: t("agentConfig.gridView"),
                    icon: (
                      <LayoutGrid
                        size={16}
                        color={viewMode === "grid" ? theme.onAccent : theme.textSecondary}
                      />
                    ),
                  },
                ]}
                onChange={setViewMode}
                compact
                iconOnly
              />
              <AnimatedPressable style={styles.addButton} onPress={startCreate}>
                <Plus size={16} color={theme.onAccent} />
                <Text style={styles.addButtonText}>{t("agentConfig.add")}</Text>
              </AnimatedPressable>
            </View>
            {createOpen ? renderEditor() : null}
          </View>
        }
        renderItem={({ item }) => {
          const editing = item.id === editingId;
          return (
            <View
              style={[
                styles.card,
                viewMode === "grid" && !editingId && styles.gridCard,
                editing && styles.editingCard,
              ]}
            >
              <View style={styles.cardHead}>
                <View style={styles.cardTitleWrap}>
                  <Text style={styles.cardTitle} numberOfLines={1}>
                    {item.label}
                  </Text>
                  <Text style={styles.cardMeta} numberOfLines={1}>
                    {item.baseUrl ||
                      (agentFamilyOf(item) === "dsh"
                        ? t("agentConfig.deepseek")
                        : agentFamilyOf(item) === "codex"
                          ? t("agentConfig.openai")
                          : t("agentConfig.anthropic"))}
                  </Text>
                </View>
                <AnimatedPressable
                  hitSlop={10}
                  style={styles.actionButton}
                  onPress={() => (editing ? closeEditor() : startEdit(item))}
                >
                  <Text style={styles.actionText}>
                    {editing ? t("agentConfig.collapse") : t("agentConfig.edit")}
                  </Text>
                </AnimatedPressable>
              </View>
              {editing ? renderEditor(item) : null}
            </View>
          );
        }}
        ListEmptyComponent={
          loading ? (
            <ActivityIndicator style={styles.loading} color={theme.textSecondary} />
          ) : (
            <Text style={styles.emptyText}>
              {error ??
                (query.trim() && providerAgents.length > 0
                  ? t("agentConfig.noMatch")
                  : status === "online"
                    ? t("agentConfig.empty")
                    : t("agentConfig.offline"))}
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
  list: { padding: spacing.md, gap: spacing.sm, paddingBottom: 36 },
  toolbar: { gap: spacing.sm, marginBottom: spacing.sm },
  toolbarRow: { flexDirection: "row", alignItems: "center", gap: spacing.sm },
  searchBox: {
    flex: 1,
    minWidth: 0,
    minHeight: 40,
    flexDirection: "row",
    alignItems: "center",
    gap: 6,
    paddingHorizontal: spacing.sm,
    borderRadius: radii.input,
    borderWidth: StyleSheet.hairlineWidth,
    borderColor: theme.border,
    backgroundColor: theme.bgElevated,
  },
  searchInput: {
    flex: 1,
    minWidth: 0,
    color: theme.text,
    fontSize: 13.5,
    paddingVertical: 0,
    includeFontPadding: false,
  },
  addButton: {
    minHeight: 40,
    flexDirection: "row",
    alignItems: "center",
    justifyContent: "center",
    gap: 6,
    borderRadius: radii.button,
    backgroundColor: theme.accent,
    paddingHorizontal: spacing.md,
  },
  addButtonText: { color: theme.onAccent, fontSize: 14, fontWeight: "700" },
  gridRow: { gap: spacing.sm },
  card: {
    flex: 1,
    backgroundColor: theme.bgCard,
    borderRadius: radii.card,
    borderWidth: StyleSheet.hairlineWidth,
    borderColor: theme.border,
    marginBottom: spacing.sm,
    overflow: "hidden",
  },
  gridCard: { maxWidth: "50%" },
  editingCard: { maxWidth: "100%", width: "100%" },
  cardHead: { flexDirection: "row", alignItems: "center", gap: spacing.md, padding: spacing.md },
  cardTitleWrap: { flex: 1, minWidth: 0, gap: 3 },
  cardTitle: { color: theme.text, fontSize: 15, fontWeight: "600" },
  cardMeta: { color: theme.textHint, fontSize: typography.metaSize },
  actionButton: {
    minHeight: 32,
    alignItems: "center",
    justifyContent: "center",
    paddingHorizontal: spacing.xs,
  },
  actionText: {
    color: theme.accent,
    fontSize: typography.metaSize,
    fontWeight: "600",
    includeFontPadding: false,
  },
  editorWrap: {
    borderTopWidth: StyleSheet.hairlineWidth,
    borderTopColor: theme.border,
    padding: spacing.md,
    gap: spacing.md,
  },
  editor: { gap: spacing.sm },
  fieldLabel: { color: theme.textSecondary, fontSize: typography.metaSize, fontWeight: "700" },
  hint: { color: theme.textHint, fontSize: typography.metaSize, lineHeight: 18 },
  input: {
    minHeight: 40,
    borderRadius: radii.input,
    borderWidth: StyleSheet.hairlineWidth,
    borderColor: theme.border,
    backgroundColor: theme.bgElevated,
    color: theme.text,
    paddingHorizontal: spacing.md,
    paddingVertical: spacing.sm,
    fontSize: 13,
  },
  modelActionRow: { flexDirection: "row", alignItems: "center", gap: spacing.sm },
  modelInput: { flex: 1, minWidth: 0 },
  outlineButton: {
    minHeight: 40,
    minWidth: 40,
    alignItems: "center",
    justifyContent: "center",
    paddingHorizontal: spacing.sm,
    borderRadius: radii.button,
    borderWidth: StyleSheet.hairlineWidth,
    borderColor: theme.accentBorder,
    backgroundColor: theme.accentSoft,
  },
  outlineButtonText: { color: theme.text, fontSize: 12.5, fontWeight: "600" },
  scannedModels: {
    borderRadius: radii.row,
    borderWidth: StyleSheet.hairlineWidth,
    borderColor: theme.border,
    backgroundColor: theme.bgElevated,
    overflow: "hidden",
  },
  scannedHead: {
    flexDirection: "row",
    alignItems: "center",
    justifyContent: "space-between",
    gap: spacing.sm,
    paddingHorizontal: spacing.sm,
    paddingTop: spacing.sm,
    paddingBottom: spacing.xs,
  },
  // ≈ 5.5 行(36 + 4 gap),超出后区域内部滚动
  scannedScroll: { maxHeight: 224 },
  scannedList: { gap: 4, padding: 4 },
  modelCheckRow: {
    minHeight: 36,
    flexDirection: "row",
    alignItems: "center",
    gap: spacing.sm,
    paddingHorizontal: spacing.sm,
    borderRadius: radii.button,
  },
  modelCheckRowActive: { backgroundColor: theme.accentSoft },
  modelCheckText: {
    flex: 1,
    minWidth: 0,
    color: theme.text,
    fontSize: 12.5,
    includeFontPadding: false,
    textAlignVertical: "center",
  },
  checkbox: {
    width: 18,
    height: 18,
    borderRadius: 5,
    borderWidth: 1.5,
    borderColor: theme.border,
    alignItems: "center",
    justifyContent: "center",
  },
  checkboxActive: { backgroundColor: theme.accent, borderColor: theme.accent },
  toggleRow: {
    flexDirection: "row",
    alignItems: "center",
    gap: spacing.md,
    paddingVertical: spacing.xs,
  },
  toggleCopy: { flex: 1, gap: 2 },
  switchTrack: {
    width: 42,
    height: 24,
    borderRadius: 12,
    justifyContent: "center",
    padding: 3,
    backgroundColor: theme.border,
  },
  switchTrackActive: { backgroundColor: theme.accent },
  switchThumb: { width: 18, height: 18, borderRadius: 9, backgroundColor: theme.textSecondary },
  switchThumbActive: { alignSelf: "flex-end", backgroundColor: theme.onAccent },
  editorActions: { flexDirection: "row", justifyContent: "flex-end", gap: spacing.sm },
  cancelButton: {
    minHeight: 40,
    alignItems: "center",
    justifyContent: "center",
    paddingHorizontal: spacing.md,
  },
  cancelButtonText: { color: theme.textSecondary, fontSize: 13, fontWeight: "600" },
  saveButton: {
    minHeight: 40,
    minWidth: 84,
    alignItems: "center",
    justifyContent: "center",
    paddingHorizontal: spacing.md,
    borderRadius: radii.button,
    backgroundColor: theme.accent,
  },
  saveButtonText: { color: theme.onAccent, fontSize: 13, fontWeight: "700" },
  buttonDisabled: { opacity: 0.45 },
  loading: { marginTop: spacing.xl },
  emptyText: { color: theme.textHint, textAlign: "center", padding: spacing.xl, lineHeight: 20 },
});
