import { useState } from "react";
import type { CSSProperties } from "react";
import { ChevronRight } from "lucide-react";
import type { AgentOption } from "../../agents";
import { isBuiltInAgent } from "../../agents";
import { useI18n } from "../../i18n";
import type { ThemeVariant } from "../../types";

type ViewMode = "card" | "bar";

const cardStyle: CSSProperties = {
  border: "1px solid transparent",
  borderRadius: 10,
  background: "transparent",
  overflow: "hidden",
  transition: "border-color 0.12s, background 0.12s",
  cursor: "pointer",
};

const barStyle: CSSProperties = {
  border: "1px solid transparent",
  borderRadius: 6,
  background: "transparent",
  overflow: "hidden",
  transition: "border-color 0.12s, background 0.12s",
  cursor: "pointer",
};

const summaryBaseStyle: CSSProperties = {
  cursor: "pointer",
  display: "flex",
  alignItems: "center",
  gap: 10,
  userSelect: "none",
  transition: "background 0.1s",
};

function maskApiKey(key?: string): string {
  if (!key) return "—";
  if (key.length <= 8) return "••••";
  return key.slice(0, 4) + "••••" + key.slice(-4);
}

function maskBaseUrl(url?: string): string {
  if (!url) return "—";
  try {
    const u = new URL(url);
    return u.hostname;
  } catch {
    return url.length > 30 ? url.slice(0, 30) + "…" : url;
  }
}

export function AgentCardItem({
  option,
  viewMode,
  themeVariant,
  logo,
  baseUrl,
  apiKey,
  version,
  onClick,
}: {
  option: AgentOption;
  viewMode: ViewMode;
  themeVariant: ThemeVariant;
  logo: string;
  baseUrl?: string;
  apiKey?: string;
  version?: string;
  onClick?: () => void;
}) {
  const { t } = useI18n();
  const [hovered, setHovered] = useState(false);
  const isBuiltIn = isBuiltInAgent(option.value);

  const containerStyle = viewMode === "card" ? cardStyle : barStyle;
  const summaryPadding = viewMode === "card" ? "12px 14px" : "8px 12px";

  return (
    <div
      style={{
        ...containerStyle,
        borderColor: hovered ? "var(--border-medium)" : undefined,
        background: hovered ? "var(--bg-subtle)" : undefined,
      }}
      onMouseEnter={() => setHovered(true)}
      onMouseLeave={() => setHovered(false)}
    >
      <div
        style={{ ...summaryBaseStyle, padding: summaryPadding }}
        onClick={onClick}
      >
        <img
          src={logo}
          alt=""
          style={{
            width: viewMode === "card" ? 22 : 16,
            height: viewMode === "card" ? 22 : 16,
            borderRadius: 4,
            flexShrink: 0,
            filter:
              themeVariant === "dark" && option.codexLike
                ? "invert(1) brightness(1.35)"
                : undefined,
          }}
        />
        <div style={{ flex: 1, minWidth: 0, display: "flex", flexDirection: "column", gap: 2 }}>
          <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
            <span
              style={{
                fontSize: viewMode === "card" ? 13 : 12,
                fontWeight: 600,
                color: "var(--text-primary)",
                overflow: "hidden",
                textOverflow: "ellipsis",
                whiteSpace: "nowrap",
              }}
            >
              {option.label}
            </span>
            <span
              style={{
                fontSize: 10,
                fontWeight: 500,
                padding: "1px 5px",
                borderRadius: 4,
                background: isBuiltIn
                  ? "color-mix(in srgb, var(--accent) 12%, transparent)"
                  : "color-mix(in srgb, var(--text-hint) 12%, transparent)",
                color: isBuiltIn ? "var(--accent)" : "var(--text-hint)",
                flexShrink: 0,
              }}
            >
              {isBuiltIn ? t("appSettings.builtIn") : t("appSettings.custom")}
            </span>
          </div>
          {viewMode === "card" && (
            <div
              style={{
                fontSize: 11,
                color: "var(--text-hint)",
                display: "flex",
                gap: 12,
                overflow: "hidden",
              }}
            >
              {baseUrl && (
                <span style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
                  {maskBaseUrl(baseUrl)}
                </span>
              )}
              {apiKey && <span>Key: {maskApiKey(apiKey)}</span>}
              {version && <span>{version}</span>}
            </div>
          )}
        </div>
        {viewMode === "bar" && (
          <div
            style={{
              fontSize: 11,
              color: "var(--text-hint)",
              display: "flex",
              gap: 10,
              flexShrink: 0,
            }}
          >
            {baseUrl && <span>{maskBaseUrl(baseUrl)}</span>}
            {version && <span>{version}</span>}
          </div>
        )}
        <span style={{ flexShrink: 0, color: "var(--text-hint)" }}>
          <ChevronRight size={14} />
        </span>
      </div>
    </div>
  );
}
