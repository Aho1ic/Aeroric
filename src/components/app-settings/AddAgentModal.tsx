import { X } from "lucide-react";
import { useI18n } from "../../i18n";
import { AddAgentPanel } from "./AddAgentPanel";

export function AddAgentModal({
  onClose,
  onSaved,
}: {
  onClose: () => void;
  onSaved: (agentId: string) => void;
}) {
  const { t } = useI18n();

  return (
    <div
      role="presentation"
      style={{
        position: "fixed",
        inset: 0,
        zIndex: 3000,
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        background: "rgba(0,0,0,0.36)",
      }}
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-label={t("appSettings.addAgentInline")}
        style={{
          width: "min(560px, calc(100vw - 48px))",
          maxHeight: "min(600px, calc(100vh - 80px))",
          display: "flex",
          flexDirection: "column",
          border: "1px solid color-mix(in srgb, var(--border-medium) 72%, #ffffff 28%)",
          borderRadius: 28,
          background: "color-mix(in srgb, var(--bg-card) 52%, transparent)",
          backdropFilter: "blur(44px) saturate(1.4)",
          WebkitBackdropFilter: "blur(44px) saturate(1.4)",
          boxShadow: "var(--shadow-popover)",
          overflow: "hidden",
        }}
      >
        <div
          style={{
            display: "flex",
            alignItems: "center",
            justifyContent: "space-between",
            padding: "14px 18px",
            borderBottom: "1px solid var(--border-dim)",
            flexShrink: 0,
          }}
        >
          <span style={{ fontSize: 14, fontWeight: 700, color: "var(--text-primary)" }}>
            {t("appSettings.addAgentInline")}
          </span>
          <button
            type="button"
            onClick={onClose}
            style={{
              width: 26,
              height: 26,
              display: "inline-flex",
              alignItems: "center",
              justifyContent: "center",
              border: "none",
              borderRadius: 6,
              background: "transparent",
              color: "var(--text-secondary)",
              cursor: "pointer",
            }}
            title={t("common.close")}
          >
            <X size={15} />
          </button>
        </div>
        <div style={{ flex: 1, minHeight: 0, overflow: "auto" }}>
          <AddAgentPanel
            onSaved={(id) => {
              onSaved(id);
              onClose();
            }}
          />
        </div>
      </div>
    </div>
  );
}
