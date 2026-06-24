import { invoke } from "@tauri-apps/api/core";
import { Check, Eye, Search, X } from "lucide-react";
import { useMemo, useRef, useState } from "react";
import type { CSSProperties, ReactNode } from "react";
import { useI18n } from "../../i18n";
import type {
  ReplacePreview,
  ReplaceSummary,
  TextSearchMatch,
  TextSearchOptions,
} from "../../types";
import {
  buildTextSearchOptions,
  canApplyReplacementPreview,
  flattenReplacePreview,
  groupSearchMatches,
  searchMatchPreview,
} from "./searchState";

export function SearchPanel({
  projectPath,
  width,
  onOpenMatch,
}: {
  projectPath: string;
  width: number;
  onOpenMatch: (match: TextSearchMatch) => void;
}) {
  const { t } = useI18n();
  const [query, setQuery] = useState("");
  const [replacement, setReplacement] = useState("");
  const [includeGlob, setIncludeGlob] = useState("");
  const [excludeGlob, setExcludeGlob] = useState("");
  const [caseSensitive, setCaseSensitive] = useState(false);
  const [regex, setRegex] = useState(false);
  const [wholeWord, setWholeWord] = useState(false);
  const [matches, setMatches] = useState<TextSearchMatch[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [preview, setPreview] = useState<ReplacePreview | null>(null);
  const [previewKey, setPreviewKey] = useState<string | null>(null);
  const [previewLoading, setPreviewLoading] = useState(false);
  const [previewError, setPreviewError] = useState<string | null>(null);
  const [applying, setApplying] = useState(false);
  const [summary, setSummary] = useState<ReplaceSummary | null>(null);
  const searchRunRef = useRef(0);
  const previewRunRef = useRef(0);

  const options = useMemo(
    () =>
      buildTextSearchOptions({
        caseSensitive,
        regex,
        wholeWord,
        includeGlob,
        excludeGlob,
        limit: 300,
      }),
    [caseSensitive, excludeGlob, includeGlob, regex, wholeWord],
  );
  const groups = useMemo(() => groupSearchMatches(matches), [matches]);
  const currentPreviewKey = useMemo(
    () => createPreviewKey(query, replacement, options),
    [options, query, replacement],
  );
  const canApplyPreview =
    canApplyReplacementPreview(preview, query, replacement) &&
    previewKey === currentPreviewKey &&
    !previewLoading &&
    !applying;

  const invalidateSearch = () => {
    searchRunRef.current += 1;
    setLoading(false);
    setMatches([]);
    setError(null);
  };

  const invalidatePreview = () => {
    previewRunRef.current += 1;
    setPreview(null);
    setPreviewKey(null);
    setPreviewError(null);
    setPreviewLoading(false);
    setApplying(false);
    setSummary(null);
  };

  const updateSearchInput = (value: string) => {
    setQuery(value);
    invalidateSearch();
    invalidatePreview();
  };

  const updateSearchOption = (setter: (value: boolean) => void, value: boolean) => {
    setter(value);
    invalidateSearch();
    invalidatePreview();
  };

  const runSearch = async () => {
    const text = query.trim();
    const runId = searchRunRef.current + 1;
    searchRunRef.current = runId;
    if (!text) {
      setMatches([]);
      setError(null);
      setLoading(false);
      return;
    }
    setLoading(true);
    setError(null);
    try {
      const results = await invoke<TextSearchMatch[]>("search_text", {
        projectPath,
        query: text,
        options,
      });
      if (runId !== searchRunRef.current) return;
      setMatches(results);
    } catch (err) {
      if (runId !== searchRunRef.current) return;
      setMatches([]);
      setError(String(err));
    } finally {
      if (runId === searchRunRef.current) setLoading(false);
    }
  };

  const runPreview = async () => {
    const text = query.trim();
    const runId = previewRunRef.current + 1;
    previewRunRef.current = runId;
    if (!text) {
      setPreview(null);
      setPreviewKey(null);
      setPreviewError(null);
      setPreviewLoading(false);
      setSummary(null);
      return;
    }
    setPreviewLoading(true);
    setPreviewError(null);
    setSummary(null);
    const nextPreviewKey = createPreviewKey(text, replacement, options);
    try {
      const nextPreview = await invoke<ReplacePreview>("replace_text_preview", {
        projectPath,
        query: text,
        replacement,
        options,
      });
      if (runId !== previewRunRef.current) return;
      setPreview(nextPreview);
      setPreviewKey(nextPreviewKey);
    } catch (err) {
      if (runId !== previewRunRef.current) return;
      setPreview(null);
      setPreviewKey(null);
      setPreviewError(String(err));
    } finally {
      if (runId === previewRunRef.current) setPreviewLoading(false);
    }
  };

  const applyPreview = async () => {
    if (!preview || !canApplyPreview) return;
    const replacements = flattenReplacePreview(preview);
    if (replacements.length === 0) return;
    const runId = previewRunRef.current;
    setApplying(true);
    setPreviewError(null);
    try {
      const result = await invoke<ReplaceSummary>("apply_text_replacements", {
        projectPath,
        replacements,
      });
      if (runId !== previewRunRef.current) return;
      setSummary(result);
      setPreview(null);
      setPreviewKey(null);
      setMatches([]);
      searchRunRef.current += 1;
    } catch (err) {
      if (runId !== previewRunRef.current) return;
      setPreviewError(String(err));
    } finally {
      setApplying(false);
    }
  };

  return (
    <div
      style={{
        width,
        minHeight: 0,
        display: "flex",
        flexDirection: "column",
        borderLeft: "1px solid var(--border-dim)",
        background: "var(--bg-panel)",
      }}
    >
      <div style={panelHeaderStyle}>
        <Search size={14} />
        <span>{t("search.title")}</span>
      </div>

      <div style={{ padding: 10, borderBottom: "1px solid var(--border-dim)" }}>
        <InputRow
          value={query}
          icon={<Search size={13} color="var(--text-hint)" />}
          onChange={updateSearchInput}
          onClear={() => updateSearchInput("")}
          onEnter={runSearch}
          placeholder={t("search.placeholder")}
          clearLabel={t("common.clear")}
        />
        <InputRow
          value={replacement}
          onChange={(value) => {
            setReplacement(value);
            invalidatePreview();
          }}
          onClear={() => {
            setReplacement("");
            invalidatePreview();
          }}
          onEnter={runPreview}
          placeholder={t("search.replacePlaceholder")}
          clearLabel={t("common.clear")}
          style={{ marginTop: 6 }}
        />
        <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 6, marginTop: 6 }}>
          <MiniInput
            value={includeGlob}
            onChange={(value) => {
              setIncludeGlob(value);
              invalidateSearch();
              invalidatePreview();
            }}
            placeholder={t("search.includePlaceholder")}
          />
          <MiniInput
            value={excludeGlob}
            onChange={(value) => {
              setExcludeGlob(value);
              invalidateSearch();
              invalidatePreview();
            }}
            placeholder={t("search.excludePlaceholder")}
          />
        </div>
        <div style={{ display: "flex", gap: 6, marginTop: 8, flexWrap: "wrap" }}>
          <Toggle
            label="Aa"
            title={t("search.caseSensitive")}
            value={caseSensitive}
            onChange={(value) => updateSearchOption(setCaseSensitive, value)}
          />
          <Toggle
            label=".*"
            title={t("search.regex")}
            value={regex}
            onChange={(value) => updateSearchOption(setRegex, value)}
          />
          <Toggle
            label="W"
            title={t("search.wholeWord")}
            value={wholeWord}
            onChange={(value) => updateSearchOption(setWholeWord, value)}
          />
          <button
            type="button"
            onClick={() => void runSearch()}
            disabled={loading}
            style={actionButtonStyle(loading, true)}
          >
            <Search size={12} />
            <span>{loading ? t("common.loading") : t("search.run")}</span>
          </button>
          <button
            type="button"
            onClick={() => void runPreview()}
            disabled={previewLoading}
            style={actionButtonStyle(previewLoading)}
          >
            <Eye size={12} />
            <span>{previewLoading ? t("common.loading") : t("search.preview")}</span>
          </button>
          <button
            type="button"
            onClick={() => void applyPreview()}
            disabled={!canApplyPreview}
            style={actionButtonStyle(!canApplyPreview)}
          >
            <Check size={12} />
            <span>{applying ? t("search.applying") : t("search.apply")}</span>
          </button>
        </div>
      </div>

      <div style={{ flex: 1, minHeight: 0, overflowY: "auto", padding: 8 }}>
        <SectionHeader label={t("search.results")} count={matches.length} />
        {error ? (
          <div style={emptyStyle}>{t("search.failed", { error })}</div>
        ) : loading ? (
          <div style={emptyStyle}>{t("common.loading")}</div>
        ) : groups.length === 0 ? (
          <div style={emptyStyle}>{t("search.empty")}</div>
        ) : (
          groups.map((group) => (
            <SearchResultGroup
              key={group.path}
              name={group.name}
              path={group.path}
              count={group.matches.length}
            >
              {group.matches.map((match) => (
                <MatchButton
                  key={`${match.path}:${match.line}:${match.column}`}
                  match={match}
                  onOpenMatch={onOpenMatch}
                />
              ))}
            </SearchResultGroup>
          ))
        )}

        {(previewLoading || previewError || preview || summary) && (
          <div style={{ marginTop: 14, paddingTop: 10, borderTop: "1px solid var(--border-dim)" }}>
            <SectionHeader label={t("search.replacePreview")} count={preview?.totalMatches ?? 0} />
            {summary && (
              <div style={statusStyle}>
                {t("search.applySummary", {
                  applied: summary.replacementsApplied,
                  skipped: summary.replacementsSkipped,
                  files: summary.filesChanged,
                })}
              </div>
            )}
            {previewError ? (
              <div style={emptyStyle}>{t("search.previewFailed", { error: previewError })}</div>
            ) : previewLoading ? (
              <div style={emptyStyle}>{t("common.loading")}</div>
            ) : preview && preview.files.length === 0 ? (
              <div style={emptyStyle}>{t("search.previewEmpty")}</div>
            ) : (
              preview?.files.map((file) => (
                <SearchResultGroup
                  key={file.path}
                  name={file.name}
                  path={file.path}
                  count={file.matches.length}
                >
                  {file.matches.map((match) => (
                    <PreviewMatchButton
                      key={`${match.path}:${match.start}:${match.end}`}
                      match={match}
                      emptyLabel={t("search.emptyReplacement")}
                      onOpenMatch={onOpenMatch}
                    />
                  ))}
                </SearchResultGroup>
              ))
            )}
            {preview?.truncated && (
              <div style={statusStyle}>
                {t("search.truncated", { count: preview.totalMatches })}
              </div>
            )}
          </div>
        )}
      </div>
    </div>
  );
}

function createPreviewKey(query: string, replacement: string, options: TextSearchOptions): string {
  return JSON.stringify({ query: query.trim(), replacement, options });
}

function InputRow({
  value,
  icon,
  onChange,
  onClear,
  onEnter,
  placeholder,
  clearLabel,
  style,
}: {
  value: string;
  icon?: ReactNode;
  onChange: (value: string) => void;
  onClear: () => void;
  onEnter: () => void | Promise<void>;
  placeholder: string;
  clearLabel: string;
  style?: CSSProperties;
}) {
  return (
    <div style={{ ...inputRowStyle, ...style }}>
      {icon}
      <input
        value={value}
        onChange={(event) => onChange(event.currentTarget.value)}
        onKeyDown={(event) => {
          if (event.key === "Enter") void onEnter();
        }}
        placeholder={placeholder}
        style={inputStyle}
      />
      {value && (
        <button type="button" aria-label={clearLabel} onClick={onClear} style={iconButtonStyle}>
          <X size={12} />
        </button>
      )}
    </div>
  );
}

function MiniInput({
  value,
  onChange,
  placeholder,
}: {
  value: string;
  onChange: (value: string) => void;
  placeholder: string;
}) {
  return (
    <input
      value={value}
      onChange={(event) => onChange(event.currentTarget.value)}
      placeholder={placeholder}
      style={miniInputStyle}
    />
  );
}

function Toggle({
  label,
  title,
  value,
  onChange,
}: {
  label: string;
  title: string;
  value: boolean;
  onChange: (value: boolean) => void;
}) {
  return (
    <button
      type="button"
      title={title}
      onClick={() => onChange(!value)}
      style={{
        height: 24,
        minWidth: 28,
        border: "1px solid var(--border-dim)",
        borderRadius: 6,
        background: value ? "var(--control-active-bg)" : "transparent",
        color: value ? "var(--control-active-fg)" : "var(--text-muted)",
        fontSize: 11,
        fontWeight: 650,
        cursor: "pointer",
      }}
    >
      {label}
    </button>
  );
}

function SectionHeader({ label, count }: { label: string; count: number }) {
  return (
    <div style={sectionHeaderStyle}>
      <span>{label}</span>
      <span style={sectionCountStyle}>{count}</span>
    </div>
  );
}

function SearchResultGroup({
  name,
  path,
  count,
  children,
}: {
  name: string;
  path: string;
  count: number;
  children: ReactNode;
}) {
  return (
    <div style={{ marginBottom: 10 }}>
      <div title={path} style={fileHeaderStyle}>
        {name}
        <span style={{ marginLeft: 6, color: "var(--text-hint)", fontWeight: 500 }}>{count}</span>
      </div>
      {children}
    </div>
  );
}

function MatchButton({
  match,
  onOpenMatch,
}: {
  match: TextSearchMatch;
  onOpenMatch: (match: TextSearchMatch) => void;
}) {
  return (
    <button type="button" onClick={() => onOpenMatch(match)} style={matchButtonStyle}>
      <span style={lineCellStyle}>
        {match.line}:{match.column}
      </span>
      <span style={previewTextStyle}>{searchMatchPreview(match)}</span>
    </button>
  );
}

function PreviewMatchButton({
  match,
  emptyLabel,
  onOpenMatch,
}: {
  match: TextSearchMatch & { replacementText: string; start: number; end: number };
  emptyLabel: string;
  onOpenMatch: (match: TextSearchMatch) => void;
}) {
  return (
    <button type="button" onClick={() => onOpenMatch(match)} style={matchButtonStyle}>
      <span style={lineCellStyle}>
        {match.line}:{match.column}
      </span>
      <span style={{ ...previewTextStyle, display: "flex", flexDirection: "column", gap: 2 }}>
        <span style={singleLineTextStyle}>{searchMatchPreview(match)}</span>
        <span style={{ ...singleLineTextStyle, color: "var(--success, #3fb950)" }}>
          {"-> "}
          {match.replacementText || emptyLabel}
        </span>
      </span>
    </button>
  );
}

function actionButtonStyle(disabled: boolean, alignEnd = false): CSSProperties {
  return {
    height: 24,
    marginLeft: alignEnd ? "auto" : 0,
    display: "inline-flex",
    alignItems: "center",
    gap: 4,
    border: "1px solid var(--border-dim)",
    borderRadius: 6,
    background: "var(--control-active-bg)",
    color: "var(--control-active-fg)",
    fontSize: 11,
    fontWeight: 650,
    opacity: disabled ? 0.55 : 1,
    cursor: disabled ? "not-allowed" : "pointer",
  };
}

const panelHeaderStyle: CSSProperties = {
  height: 38,
  display: "flex",
  alignItems: "center",
  gap: 8,
  padding: "0 10px",
  borderBottom: "1px solid var(--border-dim)",
  fontSize: 12,
  fontWeight: 650,
};

const inputRowStyle: CSSProperties = {
  height: 28,
  display: "flex",
  alignItems: "center",
  gap: 6,
  border: "1px solid var(--border-dim)",
  borderRadius: 6,
  padding: "0 8px",
  background: "var(--bg-input)",
};

const inputStyle: CSSProperties = {
  flex: 1,
  minWidth: 0,
  border: "none",
  outline: "none",
  background: "transparent",
  color: "var(--text-primary)",
  fontSize: 12,
};

const miniInputStyle: CSSProperties = {
  minWidth: 0,
  height: 26,
  border: "1px solid var(--border-dim)",
  borderRadius: 6,
  outline: "none",
  padding: "0 8px",
  background: "var(--bg-input)",
  color: "var(--text-primary)",
  fontSize: 11,
};

const iconButtonStyle: CSSProperties = {
  width: 20,
  height: 20,
  display: "inline-flex",
  alignItems: "center",
  justifyContent: "center",
  border: "none",
  borderRadius: 5,
  background: "transparent",
  color: "var(--text-hint)",
  cursor: "pointer",
};

const sectionHeaderStyle: CSSProperties = {
  display: "flex",
  alignItems: "center",
  justifyContent: "space-between",
  marginBottom: 8,
  color: "var(--text-muted)",
  fontSize: 11,
  fontWeight: 650,
};

const sectionCountStyle: CSSProperties = {
  color: "var(--text-hint)",
  fontFamily: "var(--font-mono)",
  fontWeight: 500,
};

const fileHeaderStyle: CSSProperties = {
  fontSize: 11.5,
  fontWeight: 650,
  color: "var(--text-primary)",
  overflow: "hidden",
  textOverflow: "ellipsis",
  whiteSpace: "nowrap",
  marginBottom: 4,
};

const matchButtonStyle: CSSProperties = {
  width: "100%",
  display: "grid",
  gridTemplateColumns: "44px minmax(0, 1fr)",
  gap: 6,
  padding: "5px 6px",
  border: "none",
  borderRadius: 6,
  background: "transparent",
  color: "var(--text-muted)",
  textAlign: "left",
  cursor: "pointer",
};

const lineCellStyle: CSSProperties = {
  fontFamily: "var(--font-mono)",
  fontSize: 10.5,
};

const previewTextStyle: CSSProperties = {
  minWidth: 0,
  overflow: "hidden",
  textOverflow: "ellipsis",
  whiteSpace: "nowrap",
  fontFamily: "var(--font-mono)",
  fontSize: 10.5,
};

const singleLineTextStyle: CSSProperties = {
  overflow: "hidden",
  textOverflow: "ellipsis",
  whiteSpace: "nowrap",
};

const emptyStyle: CSSProperties = {
  padding: "28px 8px",
  color: "var(--text-muted)",
  textAlign: "center",
  fontSize: 12,
};

const statusStyle: CSSProperties = {
  marginBottom: 8,
  color: "var(--text-muted)",
  fontSize: 11,
};
