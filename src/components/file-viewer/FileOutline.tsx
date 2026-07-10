import { useMemo, useState, type CSSProperties, type ReactNode } from "react";
import { ChevronRight, List } from "lucide-react";
import type { LspSymbol } from "../../types";
import { useI18n } from "../../i18n";
import { outlineSymbolDepth, outlineSymbolKey } from "./lspOutline";
import type { TocEntry } from "./markdownPreview";

const outlineMessageStyle: CSSProperties = {
  padding: "7px 8px",
  color: "var(--text-hint)",
  fontSize: 11.5,
  lineHeight: 1.35,
};

const outlineErrorStyle: CSSProperties = {
  ...outlineMessageStyle,
  color: "var(--warning)",
};

const breadcrumbStyle: CSSProperties = {
  minWidth: 0,
  overflow: "hidden",
  display: "flex",
  alignItems: "center",
  gap: 1,
  color: "var(--text-muted)",
  fontFamily: "var(--font-mono)",
  fontSize: 11,
  whiteSpace: "nowrap",
};

const breadcrumbSegmentStyle: CSSProperties = {
  minWidth: 0,
  display: "inline-flex",
  alignItems: "center",
  overflow: "hidden",
  flexShrink: 1,
};

const breadcrumbSymbolButtonStyle: CSSProperties = {
  ...breadcrumbSegmentStyle,
  border: "none",
  background: "transparent",
  color: "var(--text-secondary)",
  padding: 0,
  cursor: "pointer",
  font: "inherit",
};

const breadcrumbSeparatorStyle: CSSProperties = {
  flexShrink: 0,
  color: "var(--text-hint)",
};

const breadcrumbTextStyle: CSSProperties = {
  minWidth: 0,
  overflow: "hidden",
  textOverflow: "ellipsis",
  whiteSpace: "nowrap",
};

const stickyScrollStyle: CSSProperties = {
  minHeight: 26,
  display: "flex",
  alignItems: "center",
  gap: 2,
  padding: "0 8px",
  borderBottom: "1px solid var(--border-dim)",
  background: "var(--bg-panel)",
  color: "var(--text-muted)",
  fontFamily: "var(--font-mono)",
  fontSize: 11,
  whiteSpace: "nowrap",
  overflow: "hidden",
  flexShrink: 0,
};

const stickyScrollButtonStyle: CSSProperties = {
  minWidth: 0,
  display: "inline-flex",
  alignItems: "center",
  gap: 2,
  border: "none",
  background: "transparent",
  color: "var(--text-secondary)",
  padding: 0,
  cursor: "pointer",
  font: "inherit",
};

function CollapsibleOutline({ label, children }: { label: string; children: ReactNode }) {
  const [open, setOpen] = useState(true);
  return (
    <div className={`md-toc${open ? "" : " md-toc-collapsed"}`}>
      <button
        type="button"
        className="md-toc-toggle"
        onClick={() => setOpen((current) => !current)}
        title={label}
      >
        {open ? <List size={13} /> : <ChevronRight size={13} />}
        <span>{label}</span>
      </button>
      {open ? children : null}
    </div>
  );
}

export function MarkdownToc({
  toc,
  activeId,
  onJump,
}: {
  toc: TocEntry[];
  activeId: string | null;
  onJump: (id: string) => void;
}) {
  const { t } = useI18n();
  const minDepth = useMemo(() => Math.min(...toc.map((entry) => entry.depth)), [toc]);

  return (
    <CollapsibleOutline label={t("file.outline")}>
      <nav className="md-toc-list">
        {toc.map((entry) => (
          <button
            key={entry.id}
            type="button"
            data-depth={Math.min(entry.depth - minDepth + 1, 6)}
            className={`md-toc-item${activeId === entry.id ? " active" : ""}`}
            onClick={() => onJump(entry.id)}
            title={entry.text}
          >
            {entry.text}
          </button>
        ))}
      </nav>
    </CollapsibleOutline>
  );
}

export function CodeOutline({
  symbols,
  activeKeys,
  loading,
  error,
  truncated,
  onJump,
}: {
  symbols: LspSymbol[];
  activeKeys: Set<string>;
  loading: boolean;
  error: string | null;
  truncated: boolean;
  onJump: (symbol: LspSymbol) => void;
}) {
  const { t } = useI18n();

  return (
    <CollapsibleOutline label={t("file.outline")}>
      <nav className="md-toc-list" aria-label={t("file.outline")}>
        {loading && symbols.length === 0 ? (
          <div style={outlineMessageStyle}>{t("file.outlineLoading")}</div>
        ) : error ? (
          <div style={outlineErrorStyle}>{t("file.outlineFailed", { error })}</div>
        ) : symbols.length === 0 ? (
          <div style={outlineMessageStyle}>{t("file.noOutlineSymbols")}</div>
        ) : (
          <>
            {truncated ? (
              <div style={outlineMessageStyle}>
                {t("file.outlineTruncated", { count: String(symbols.length) })}
              </div>
            ) : null}
            {symbols.map((symbol) => {
              const key = outlineSymbolKey(symbol);
              return (
                <button
                  key={key}
                  type="button"
                  data-depth={Math.min(outlineSymbolDepth(symbol, symbols), 6)}
                  className={`md-toc-item${activeKeys.has(key) ? " active" : ""}`}
                  onClick={() => onJump(symbol)}
                  title={[symbol.containerName, symbol.detail, symbol.name]
                    .filter(Boolean)
                    .join(" · ")}
                >
                  {symbol.name}
                </button>
              );
            })}
          </>
        )}
      </nav>
    </CollapsibleOutline>
  );
}

export function FileBreadcrumbs({
  pathSegments,
  symbols,
  label,
  onJump,
}: {
  pathSegments: { label: string; title: string }[];
  symbols: LspSymbol[];
  label: string;
  onJump: (symbol: LspSymbol) => void;
}) {
  return (
    <nav aria-label={label} style={breadcrumbStyle}>
      {pathSegments.map((segment, index) => (
        <span
          key={`${segment.title}:${index}`}
          style={breadcrumbSegmentStyle}
          title={segment.title}
        >
          {index > 0 ? <ChevronRight size={10} style={breadcrumbSeparatorStyle} /> : null}
          <span style={breadcrumbTextStyle}>{segment.label}</span>
        </span>
      ))}
      {symbols.map((symbol) => (
        <button
          key={outlineSymbolKey(symbol)}
          type="button"
          style={breadcrumbSymbolButtonStyle}
          title={symbol.detail ?? symbol.name}
          onClick={() => onJump(symbol)}
        >
          <ChevronRight size={10} style={breadcrumbSeparatorStyle} />
          <span style={breadcrumbTextStyle}>{symbol.name}</span>
        </button>
      ))}
    </nav>
  );
}

export function CodeStickyScroll({
  symbols,
  label,
  onJump,
}: {
  symbols: LspSymbol[];
  label: string;
  onJump: (symbol: LspSymbol) => void;
}) {
  if (symbols.length === 0) return null;
  return (
    <nav aria-label={label} style={stickyScrollStyle}>
      {symbols.map((symbol, index) => (
        <button
          key={outlineSymbolKey(symbol)}
          type="button"
          style={stickyScrollButtonStyle}
          title={symbol.detail ?? symbol.name}
          onClick={() => onJump(symbol)}
        >
          {index > 0 ? <ChevronRight size={10} style={breadcrumbSeparatorStyle} /> : null}
          <span style={breadcrumbTextStyle}>{symbol.name}</span>
        </button>
      ))}
    </nav>
  );
}
