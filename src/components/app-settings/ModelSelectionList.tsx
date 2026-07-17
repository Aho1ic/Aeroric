import { Search, X } from "lucide-react";
import { useMemo, useState } from "react";
import { useI18n } from "../../i18n";

function fuzzyTokenMatches(value: string, token: string): boolean {
  if (value.includes(token)) return true;
  let tokenIndex = 0;
  for (const character of value) {
    if (character === token[tokenIndex]) tokenIndex += 1;
    if (tokenIndex === token.length) return true;
  }
  return false;
}

export function filterAgentModels(models: string[], query: string): string[] {
  const tokens = query.trim().toLocaleLowerCase().split(/\s+/).filter(Boolean);
  if (tokens.length === 0) return models;
  return models.filter((model) => {
    const normalized = model.toLocaleLowerCase();
    return tokens.every((token) => fuzzyTokenMatches(normalized, token));
  });
}

export function ModelSelectionList({
  models,
  selectedModels,
  onToggle,
}: {
  models: string[];
  selectedModels: string[];
  onToggle: (model: string) => void;
}) {
  const { t } = useI18n();
  const [query, setQuery] = useState("");
  const filteredModels = useMemo(() => filterAgentModels(models, query), [models, query]);

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
      <div style={{ position: "relative" }}>
        <Search
          size={13}
          aria-hidden="true"
          style={{
            position: "absolute",
            left: 11,
            top: "50%",
            transform: "translateY(-50%)",
            color: "var(--text-hint)",
            pointerEvents: "none",
          }}
        />
        <input
          type="search"
          aria-label={t("appSettings.searchModels")}
          value={query}
          onChange={(event) => setQuery(event.currentTarget.value)}
          placeholder={t("appSettings.searchModelsPlaceholder")}
          spellCheck={false}
          style={{
            width: "100%",
            height: 32,
            padding: "5px 36px 5px 32px",
            border: "1px solid var(--border-medium)",
            borderRadius: 999,
            outline: "none",
            background: "var(--bg-input)",
            color: "var(--text-primary)",
            fontFamily: "var(--font-mono)",
            fontSize: 12,
            boxSizing: "border-box",
          }}
        />
        {query && (
          <button
            type="button"
            aria-label={t("common.clear")}
            title={t("common.clear")}
            onClick={() => setQuery("")}
            style={{
              position: "absolute",
              right: 7,
              top: "50%",
              transform: "translateY(-50%)",
              width: 22,
              height: 22,
              display: "inline-flex",
              alignItems: "center",
              justifyContent: "center",
              border: "none",
              borderRadius: "50%",
              background: "transparent",
              color: "var(--text-hint)",
              cursor: "pointer",
            }}
          >
            <X size={12} />
          </button>
        )}
      </div>

      <div
        role="group"
        aria-label={t("appSettings.availableModels")}
        style={{
          display: "grid",
          gridTemplateColumns: "repeat(auto-fit, minmax(180px, 1fr))",
          gap: 7,
          maxHeight: 220,
          overflow: "auto",
          border: "1px solid var(--border-dim)",
          borderRadius: 14,
          padding: 9,
          background:
            "linear-gradient(145deg, color-mix(in srgb, var(--bg-subtle) 96%, var(--accent) 4%), var(--bg-subtle))",
        }}
      >
        {filteredModels.length > 0 ? (
          filteredModels.map((item) => {
            const selected = selectedModels.includes(item);
            return (
              <label
                key={item}
                style={{
                  display: "flex",
                  alignItems: "center",
                  gap: 8,
                  minWidth: 0,
                  minHeight: 30,
                  padding: "4px 7px",
                  border: `1px solid ${
                    selected
                      ? "color-mix(in srgb, var(--accent) 38%, var(--border-dim))"
                      : "transparent"
                  }`,
                  borderRadius: 9,
                  background: selected
                    ? "color-mix(in srgb, var(--accent) 8%, var(--bg-card))"
                    : "transparent",
                  color: selected ? "var(--text-primary)" : "var(--text-secondary)",
                  cursor: "pointer",
                  boxSizing: "border-box",
                }}
              >
                <input
                  type="checkbox"
                  checked={selected}
                  onChange={() => onToggle(item)}
                  style={{ accentColor: "var(--accent)", flexShrink: 0 }}
                />
                <span
                  style={{
                    minWidth: 0,
                    overflow: "hidden",
                    textOverflow: "ellipsis",
                    whiteSpace: "nowrap",
                    fontFamily: "var(--font-mono)",
                    fontSize: 12,
                  }}
                  title={item}
                >
                  {item}
                </span>
              </label>
            );
          })
        ) : (
          <div
            style={{
              gridColumn: "1 / -1",
              padding: "18px 10px",
              textAlign: "center",
              color: "var(--text-hint)",
              fontSize: 12,
            }}
          >
            {t("appSettings.noMatchingModels")}
          </div>
        )}
      </div>
    </div>
  );
}
