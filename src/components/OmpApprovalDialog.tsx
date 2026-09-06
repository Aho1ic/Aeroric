import { useEffect, useState } from "react";
import { AlertTriangle, Check, X } from "lucide-react";
import { useI18n } from "../i18n";
import { Button } from "./ui/Button";
import { cancelledOmpUiResponse, respondOmpUiRequest, type OmpUiRequest } from "../ompUiRequests";

/**
 * omp 审批弹窗(auto_edit / always-ask 模式下 omp 的工具确认,extension_ui_request
 * 的 `confirm` 方法)。回应帧为 `{confirmed: boolean}`。
 */
export function OmpApprovalDialog({
  request,
  onClose,
}: {
  request: OmpUiRequest | null;
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

  async function handleRespond(confirmed: boolean) {
    if (submitting || !request) return;
    setSubmitting(true);
    setError(null);
    try {
      await respondOmpUiRequest(request, { confirmed });
      onClose();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
      setSubmitting(false);
    }
  }

  async function handleCancel() {
    if (submitting || !request) return;
    setSubmitting(true);
    try {
      await respondOmpUiRequest(request, cancelledOmpUiResponse());
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
        if (e.target === e.currentTarget && !submitting) void handleCancel();
      }}
    >
      <div
        role="dialog"
        aria-labelledby="omp-approval-title"
        aria-describedby="omp-approval-description"
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
            id="omp-approval-title"
            style={{
              flex: 1,
              margin: 0,
              fontSize: 15,
              fontWeight: 600,
              color: "var(--text-primary)",
            }}
          >
            {request.title?.trim() || t("omp.approval.title")}
          </h2>
        </div>

        <div
          id="omp-approval-description"
          style={{
            flex: 1,
            minHeight: 0,
            padding: "20px 20px",
            overflow: "auto",
          }}
        >
          {request.message && (
            <div
              style={{
                marginBottom: 16,
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
              {request.message}
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
            onClick={() => void handleRespond(false)}
          >
            {t("omp.approval.reject")}
          </Button>
          <Button
            variant="default"
            size="sm"
            icon={Check}
            disabled={submitting}
            onClick={() => void handleRespond(true)}
          >
            {submitting ? t("omp.approval.submitting") : t("omp.approval.allow")}
          </Button>
        </div>
      </div>
    </div>
  );
}
