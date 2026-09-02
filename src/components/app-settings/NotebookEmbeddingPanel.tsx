/**
 * 随手记 RAG 的 embedding provider 设置。
 *
 * 这一页填的三个字段决定 AI 面板去连哪个 embedding 服务;在它出现之前
 * `NotebookPanel` 只能用硬编码的本机 Ollama(计划 §3.3 的 P7 延后项)。
 *
 * 与 `ProxyPanel` 同一套骨架(load → 编辑 → 测试 → 保存 → 派发
 * `APP_SETTINGS_CHANGED_EVENT`),只有 key 那一格不同,而那一格的三条规则是这个面板
 * 唯一需要动脑子的地方:
 *
 * 1. **输入框永远从空的开始。** 后端不回明文(`secrets.rs` 的不变量 1),所以没法像
 *    代理密码那样把已存的值填回来。已经存过的话由旁边那行状态文字说明。
 *
 * 2. **留空 = 不动。** 承接第 1 条:如果「保存时把框里的东西原样写下去」,那么一个只想
 *    改模型名的用户会顺手把 key 抹掉。于是清除必须是一个单独的动作。
 *
 * 3. **「测试连接」送框里那个 key。** 用户刚粘进来还没保存的那个才是他想测的。后端只在
 *    key 为空时才去钥匙串补(`with_stored_key`),所以框里有东西时测的就是它,框里空着
 *    时测的是已保存的那个 —— 两种都是对的。
 */
import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Check, TriangleAlert } from "lucide-react";

import { useI18n } from "../../i18n";
import s from "../../styles";
import { settingsForm } from "../../styles/panelChrome";
import { Button } from "../ui/Button";
import {
  clearNotebookEmbeddingKey,
  notebookEmbeddingKeyStatus,
  probeRagEmbed,
  setNotebookEmbeddingKey,
  type EmbedProvider,
} from "../notebook/noteRag";
import {
  APP_SETTINGS_CHANGED_EVENT,
  type AppSettings,
  type NotebookEmbeddingSettings,
} from "./types";

/** 与 Rust 的 `NotebookEmbeddingSettings::default` 一致。 */
const defaultEmbeddingSettings: NotebookEmbeddingSettings = {
  provider: "ollama",
  base_url: "http://127.0.0.1:11434",
  model: "nomic-embed-text",
};

const PROVIDERS: EmbedProvider[] = ["ollama", "openAi"];

/**
 * provider → 下拉里那一项的文案 key。
 *
 * 写成字面量表而不是 `` t(`...${provider}`) ``:`i18n-keys.test.ts` 只能静态检查字面量
 * key,拼出来的 key 漏翻译时它是绿的,而 UI 上会露出原始串。
 */
const PROVIDER_LABEL_KEY: Record<EmbedProvider, string> = {
  ollama: "appSettings.notebookEmbedding.providerOllama",
  openAi: "appSettings.notebookEmbedding.providerOpenAi",
};

/** 换 provider 时把地址与模型也带过去 —— 不然用户得自己想起来那两个都要改。 */
const PROVIDER_PRESETS: Record<EmbedProvider, { baseUrl: string; model: string }> = {
  ollama: { baseUrl: "http://127.0.0.1:11434", model: "nomic-embed-text" },
  openAi: { baseUrl: "https://api.openai.com/v1", model: "text-embedding-3-small" },
};

function embeddingOf(settings: AppSettings | null): NotebookEmbeddingSettings {
  return { ...defaultEmbeddingSettings, ...(settings?.notebook_embedding_settings ?? {}) };
}

function embeddingEqual(a: NotebookEmbeddingSettings, b: NotebookEmbeddingSettings): boolean {
  return a.provider === b.provider && a.base_url === b.base_url && a.model === b.model;
}

type TestState = { ok: boolean; message: string };

export function NotebookEmbeddingPanel() {
  const { t } = useI18n();
  const [settings, setSettings] = useState<NotebookEmbeddingSettings>(defaultEmbeddingSettings);
  const [original, setOriginal] = useState<NotebookEmbeddingSettings>(defaultEmbeddingSettings);
  const [keyStored, setKeyStored] = useState(false);
  const [keyDraft, setKeyDraft] = useState("");
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [saved, setSaved] = useState(false);
  const [clearing, setClearing] = useState(false);
  const [testing, setTesting] = useState(false);
  const [testState, setTestState] = useState<TestState | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    // 两条请求都要,但状态查失败不该挡住设置本身 —— 那一位只影响一行提示文字。
    Promise.all([
      invoke<AppSettings>("load_app_settings"),
      notebookEmbeddingKeyStatus().catch(() => false),
    ])
      .then(([loaded, stored]) => {
        if (cancelled) return;
        const next = embeddingOf(loaded);
        setSettings(next);
        setOriginal(next);
        setKeyStored(stored);
      })
      .catch((e) => {
        if (!cancelled) setError(String(e));
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, []);

  // 任何改动都让上一次的测试结论失效:拿旧结论判断新配置比没有结论更糟。
  function update(patch: Partial<NotebookEmbeddingSettings>) {
    setTestState(null);
    setSettings((prev) => ({ ...prev, ...patch }));
  }

  function handleProviderChange(provider: EmbedProvider) {
    const preset = PROVIDER_PRESETS[provider];
    // 只有还停在上一个 provider 的预设值上才跟着换。用户自己填过的地址不许被覆盖。
    const previous = PROVIDER_PRESETS[settings.provider];
    update({
      provider,
      base_url: settings.base_url === previous.baseUrl ? preset.baseUrl : settings.base_url,
      model: settings.model === previous.model ? preset.model : settings.model,
    });
  }

  async function handleTest() {
    setTesting(true);
    setTestState(null);
    setError(null);
    try {
      // 见模块注释第 3 条。
      const dim = await probeRagEmbed({
        provider: settings.provider,
        baseUrl: settings.base_url,
        model: settings.model,
        apiKey: keyDraft,
      });
      setTestState({ ok: true, message: t("appSettings.notebookEmbedding.testOk", { dim }) });
    } catch (e) {
      setTestState({
        ok: false,
        message: t("appSettings.notebookEmbedding.testFailed", { error: String(e) }),
      });
    } finally {
      setTesting(false);
    }
  }

  async function handleSave() {
    setSaving(true);
    setSaved(false);
    setError(null);
    try {
      const updated = await invoke<AppSettings>("update_notebook_embedding_settings", {
        notebookEmbeddingSettings: settings,
      });
      // key 也写在 try 里、在「保存成功」之前:钥匙串写失败时用户该看到那条错误,而不是
      // 一个对勾加一个仍然连不上的 provider。留空表示不动它(见模块注释第 2 条)。
      if (keyDraft.trim()) {
        await setNotebookEmbeddingKey(keyDraft);
        setKeyStored(true);
        setKeyDraft("");
      }
      const next = embeddingOf(updated);
      setSettings(next);
      setOriginal(next);
      window.dispatchEvent(new Event(APP_SETTINGS_CHANGED_EVENT));
      setSaved(true);
      setTimeout(() => setSaved(false), 2000);
    } catch (e) {
      setError(String(e));
    } finally {
      setSaving(false);
    }
  }

  async function handleClearKey() {
    setClearing(true);
    setError(null);
    try {
      await clearNotebookEmbeddingKey();
      setKeyStored(false);
      setKeyDraft("");
      setTestState(null);
    } catch (e) {
      setError(String(e));
    } finally {
      setClearing(false);
    }
  }

  const needsKey = settings.provider !== "ollama";
  const busy = loading || saving || testing || clearing;
  const isDirty = !embeddingEqual(settings, original) || keyDraft.trim().length > 0;

  return (
    <>
      <div
        style={{
          ...s.settingsBody,
          display: "flex",
          flexDirection: "column",
          gap: 14,
          padding: "18px 20px 14px",
        }}
      >
        {error && <div style={{ color: "var(--danger)", fontSize: 12.5 }}>{error}</div>}
        {loading && (
          <div style={{ color: "var(--text-hint)", fontSize: 13 }}>{t("common.loading")}</div>
        )}

        <div>
          <label style={settingsForm.label} htmlFor="notebook-embedding-provider">
            {t("appSettings.notebookEmbedding.provider")}
          </label>
          <select
            id="notebook-embedding-provider"
            style={{
              ...settingsForm.input,
              fontFamily: "inherit",
              opacity: loading ? 0.65 : 1,
              cursor: loading ? "wait" : "pointer",
            }}
            value={settings.provider}
            onChange={(e) => handleProviderChange(e.target.value as EmbedProvider)}
            disabled={loading}
          >
            {PROVIDERS.map((provider) => (
              <option key={provider} value={provider}>
                {t(PROVIDER_LABEL_KEY[provider])}
              </option>
            ))}
          </select>
          <div style={settingsForm.hint}>{t("appSettings.notebookEmbedding.providerHint")}</div>
        </div>

        <div>
          <label style={settingsForm.label} htmlFor="notebook-embedding-base-url">
            {t("appSettings.notebookEmbedding.baseUrl")}
          </label>
          <input
            id="notebook-embedding-base-url"
            style={{
              ...settingsForm.input,
              opacity: loading ? 0.65 : 1,
              cursor: loading ? "wait" : "text",
            }}
            value={settings.base_url}
            onChange={(e) => update({ base_url: e.target.value })}
            placeholder={PROVIDER_PRESETS[settings.provider].baseUrl}
            disabled={loading}
            spellCheck={false}
          />
          <div style={settingsForm.hint}>{t("appSettings.notebookEmbedding.baseUrlHint")}</div>
        </div>

        <div>
          <label style={settingsForm.label} htmlFor="notebook-embedding-model">
            {t("appSettings.notebookEmbedding.model")}
          </label>
          <input
            id="notebook-embedding-model"
            style={{
              ...settingsForm.input,
              opacity: loading ? 0.65 : 1,
              cursor: loading ? "wait" : "text",
            }}
            value={settings.model}
            onChange={(e) => update({ model: e.target.value })}
            placeholder={PROVIDER_PRESETS[settings.provider].model}
            disabled={loading}
            spellCheck={false}
          />
          <div style={settingsForm.hint}>{t("appSettings.notebookEmbedding.modelHint")}</div>
        </div>

        <div>
          <label style={settingsForm.label} htmlFor="notebook-embedding-key">
            {t("appSettings.notebookEmbedding.apiKey")}
          </label>
          <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
            <input
              id="notebook-embedding-key"
              type="password"
              style={{
                ...settingsForm.input,
                opacity: loading ? 0.65 : 1,
                cursor: loading ? "wait" : "text",
              }}
              value={keyDraft}
              onChange={(e) => {
                setTestState(null);
                setKeyDraft(e.target.value);
              }}
              placeholder={
                keyStored
                  ? t("appSettings.notebookEmbedding.apiKeyStored")
                  : t("appSettings.notebookEmbedding.apiKeyEmpty")
              }
              disabled={loading}
              autoComplete="new-password"
              spellCheck={false}
            />
            {keyStored && (
              <Button variant="outline" size="sm" onClick={handleClearKey} disabled={busy}>
                {clearing ? t("common.loading") : t("appSettings.notebookEmbedding.apiKeyClear")}
              </Button>
            )}
          </div>
          <div style={settingsForm.hint}>
            {needsKey
              ? t("appSettings.notebookEmbedding.apiKeyHint")
              : t("appSettings.notebookEmbedding.apiKeyUnusedHint")}
          </div>
        </div>
      </div>

      <div style={s.settingsFooter}>
        {testState && (
          <span
            role="status"
            style={{
              display: "flex",
              alignItems: "center",
              gap: 4,
              marginRight: "auto",
              minWidth: 0,
              fontSize: 12,
              color: testState.ok ? "var(--success)" : "var(--danger)",
            }}
          >
            {testState.ok ? (
              <Check size={12} style={{ flexShrink: 0 }} />
            ) : (
              <TriangleAlert size={12} style={{ flexShrink: 0 }} />
            )}
            <span style={{ overflow: "hidden", textOverflow: "ellipsis" }}>
              {testState.message}
            </span>
          </span>
        )}
        {saved && (
          <span
            style={{
              display: "flex",
              alignItems: "center",
              gap: 4,
              fontSize: 12,
              color: "var(--success)",
            }}
          >
            <Check size={12} /> {t("common.saved")}
          </span>
        )}
        <Button
          variant="outline"
          size="sm"
          onClick={handleTest}
          disabled={busy || !settings.base_url.trim() || !settings.model.trim()}
        >
          {testing
            ? t("appSettings.notebookEmbedding.testing")
            : t("appSettings.notebookEmbedding.test")}
        </Button>
        <Button variant="default" size="sm" onClick={handleSave} disabled={busy || !isDirty}>
          {saving ? t("common.saving") : t("common.save")}
        </Button>
      </div>
    </>
  );
}
