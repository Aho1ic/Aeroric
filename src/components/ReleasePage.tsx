import { X } from "lucide-react";
import { useI18n } from "../i18n";
import { NotificationBell } from "./NotificationBell";

export function ReleasePage({ onClose }: { onClose: () => void }) {
  const { t } = useI18n();

  return (
    <div
      style={{
        position: "fixed",
        inset: 0,
        zIndex: 9999,
        background: "var(--bg-primary)",
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
      }}
    >
      <div
        style={{
          width: "min(90vw, 700px)",
          maxHeight: "90vh",
          display: "flex",
          flexDirection: "column",
          background: "var(--bg-panel)",
          border: "1px solid var(--border-dim)",
          borderRadius: 12,
          overflow: "hidden",
        }}
      >
        <div
          style={{
            flexShrink: 0,
            height: 50,
            display: "flex",
            alignItems: "center",
            justifyContent: "space-between",
            padding: "0 20px",
            borderBottom: "1px solid var(--border-dim)",
            background: "var(--bg-sidebar)",
          }}
        >
          <h2
            style={{
              margin: 0,
              fontSize: 15,
              fontWeight: 600,
              color: "var(--text-primary)",
            }}
          >
            {t("release.title")}
          </h2>
          <button
            onClick={onClose}
            style={{
              width: 28,
              height: 28,
              display: "flex",
              alignItems: "center",
              justifyContent: "center",
              border: "1px solid var(--border-dim)",
              borderRadius: 6,
              background: "transparent",
              color: "var(--text-muted)",
              cursor: "pointer",
              transition: "all 0.15s ease",
            }}
            onMouseEnter={(e) => {
              e.currentTarget.style.background = "var(--bg-hover)";
              e.currentTarget.style.color = "var(--text-primary)";
            }}
            onMouseLeave={(e) => {
              e.currentTarget.style.background = "transparent";
              e.currentTarget.style.color = "var(--text-muted)";
            }}
            title={t("release.close")}
          >
            <X size={16} strokeWidth={2} />
          </button>
        </div>
        <div
          style={{
            flex: 1,
            minHeight: 0,
            overflow: "auto",
            padding: "20px",
          }}
        >
          <NotificationBell renderAsContent buttonStyle={{ display: "none" }} iconSize={14} />
        </div>
      </div>
    </div>
  );
}
