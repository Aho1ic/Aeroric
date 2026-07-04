import { useId } from "react";
import { Button } from "./Button";

export function ConfirmDialog({
  title,
  message,
  cancelLabel,
  confirmLabel,
  confirmingLabel,
  confirming = false,
  destructive = false,
  onCancel,
  onConfirm,
}: {
  title: string;
  message: string;
  cancelLabel: string;
  confirmLabel: string;
  confirmingLabel?: string;
  confirming?: boolean;
  destructive?: boolean;
  onCancel: () => void;
  onConfirm: () => void;
}) {
  const titleId = useId();

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
      onMouseDown={(event) => {
        if (event.target === event.currentTarget && !confirming) {
          onCancel();
        }
      }}
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        style={{
          width: "min(420px, calc(100vw - 32px))",
          border: "1px solid var(--border-medium)",
          borderRadius: 8,
          background: "var(--bg-card)",
          boxShadow: "var(--shadow-popover)",
          padding: 18,
        }}
      >
        <div
          id={titleId}
          style={{
            fontSize: 15,
            fontWeight: 700,
            color: "var(--text-primary)",
            marginBottom: 8,
          }}
        >
          {title}
        </div>
        <div style={{ fontSize: 13, lineHeight: 1.55, color: "var(--text-secondary)" }}>
          {message}
        </div>
        <div
          style={{
            display: "flex",
            justifyContent: "flex-end",
            gap: 8,
            marginTop: 18,
          }}
        >
          <Button variant="outline" size="sm" onClick={onCancel} disabled={confirming}>
            {cancelLabel}
          </Button>
          <Button
            variant={destructive ? "destructive" : "default"}
            size="sm"
            onClick={onConfirm}
            disabled={confirming}
          >
            {confirming ? (confirmingLabel ?? confirmLabel) : confirmLabel}
          </Button>
        </div>
      </div>
    </div>
  );
}
