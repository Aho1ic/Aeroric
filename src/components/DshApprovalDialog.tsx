import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { AlertTriangle, Check, X } from "lucide-react";
import { useI18n } from "../i18n";
import { Button } from "./ui/Button";

export interface DshApprovalRequest {
  rpcId: string;
  sessionId: string;
  approvalId: string;
  toolName: string;
  callId?: string;
  reason?: string;
}

export function DshApprovalDialog({
  request,
  onClose,
}: {
  request: DshApprovalRequest | null;
  onClose: () => void;
}) {
  const { t } = useI18n();
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (request) {
      setSubmitting(false);
      setError(null);
    }
  }, [request]);

  if (!request) return null;

  async function handleRespond(outcome: "allowed-once" | "rejected") {
    if (submitting || !request) return;
    setSubmitting(true);
    setError(null);

    try {
      await invoke("respond_dsh_server_request", {
        rpcId: request.rpcId,
        sessionId: request.sessionId,
        result: {
          ok: true,
          value: {
            sessionId: request.sessionId,
            approvalId: request.approvalId,
            outcome,
          },
        },
      });
      onClose();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
      setSubmitting(false);
    }
  }

  return (
    <div
      style={{
        position: "fixed",
        inset: 0,
        zIndex: "var(--z-overlay)",
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        background: "color-mix(in srgb, var(--bg-panel) 16%, transparent)",
        backdropFilter: "blur(12px)",
      }}
      onClick={(e) => {
        if (e.target === e.currentTarget && !submitting) void handleRespond("rejected");
      }}
    >
      <div
        role="dialog"
        aria-labelledby="dsh-approval-title"
        aria-describedby="dsh-approval-description"
        style={{
          width: "min(520px, calc(100vw - 32px))",
          maxHeight: "calc(100vh - 64px)",
          display: "flex",
          flexDirection: "column",
          background: "var(--bg-card)",
          border: "1px solid var(--border-medium)",
          borderRadius: "var(--radius-lg, 14px)",
          boxShadow: "var(--shadow-dialog, 0 16px 48px rgba(0,0,0,0.24))",
          overflow: "hidden",
        }}
      >
        <div
          style={{
            display: "flex",
            alignItems: "center",
            gap: 12,
            padding: "18px 20px",
            borderBottom: "1px solid var(--border-dim)",
            background:
              "color-mix(in srgb, var(--warning-subtle, rgba(251, 191, 36, 0.1)) 60%, transparent)",
          }}
        >
          <AlertTriangle size={20} color="var(--warning, #f59e0b)" aria-hidden />
          <h2
            id="dsh-approval-title"
            style={{
              flex: 1,
              margin: 0,
              fontSize: 15,
              fontWeight: 600,
              color: "var(--text-primary)",
            }}
          >
            {t("dsh.approvalTitle")}
          </h2>
        </div>

        <div
          id="dsh-approval-description"
          style={{
            flex: 1,
            minHeight: 0,
            padding: "20px 20px",
            overflow: "auto",
          }}
        >
          <div style={{ marginBottom: 16 }}>
            <div style={{ fontSize: 12, color: "var(--text-secondary)", marginBottom: 6 }}>
              {t("dsh.approvalToolLabel")}
            </div>
            <div
              style={{
                padding: "8px 12px",
                background: "var(--bg-panel)",
                border: "1px solid var(--border-dim)",
                borderRadius: "var(--radius-sm)",
                fontFamily: "var(--font-mono)",
                fontSize: 13,
                color: "var(--text-primary)",
              }}
            >
              {request.toolName}
            </div>
          </div>

          {request.reason && (
            <div style={{ marginBottom: 16 }}>
              <div style={{ fontSize: 12, color: "var(--text-secondary)", marginBottom: 6 }}>
                {t("dsh.approvalReasonLabel")}
              </div>
              <div
                style={{
                  padding: "10px 12px",
                  background: "var(--bg-panel)",
                  border: "1px solid var(--border-dim)",
                  borderRadius: "var(--radius-sm)",
                  fontSize: 13,
                  lineHeight: 1.5,
                  color: "var(--text-primary)",
                  whiteSpace: "pre-wrap",
                  wordBreak: "break-word",
                }}
              >
                {request.reason}
              </div>
            </div>
          )}

          {error && (
            <div
              role="alert"
              style={{
                padding: "10px 12px",
                background: "var(--danger-subtle, rgba(239, 68, 68, 0.1))",
                border: "1px solid var(--danger, #ef4444)",
                borderRadius: "var(--radius-sm)",
                fontSize: 12,
                color: "var(--danger, #ef4444)",
              }}
            >
              {error}
            </div>
          )}
        </div>

        <div
          style={{
            display: "flex",
            alignItems: "center",
            justifyContent: "flex-end",
            gap: 10,
            padding: "14px 20px",
            borderTop: "1px solid var(--border-dim)",
            background: "color-mix(in srgb, var(--bg-card) 94%, transparent)",
          }}
        >
          <Button
            variant="outline"
            size="sm"
            icon={X}
            disabled={submitting}
            onClick={() => handleRespond("rejected")}
          >
            {t("dsh.approvalReject")}
          </Button>
          <Button
            variant="default"
            size="sm"
            icon={Check}
            disabled={submitting}
            onClick={() => handleRespond("allowed-once")}
          >
            {submitting ? t("dsh.approvalSubmitting") : t("dsh.approvalAllow")}
          </Button>
        </div>
      </div>
    </div>
  );
}
