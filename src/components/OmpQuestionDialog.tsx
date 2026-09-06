import { useEffect, useState } from "react";
import { Check, HelpCircle, X } from "lucide-react";
import { useI18n } from "../i18n";
import { Button } from "./ui/Button";
import { cancelledOmpUiResponse, respondOmpUiRequest, type OmpUiRequest } from "../ompUiRequests";

/**
 * omp 提问弹窗(extension_ui_request 的 `select`/`input`/`editor` 方法,
 * 含 `ask` 工具发起的提问)。select 回 `{value: <label>}`,input/editor 回
 * `{value: <text>}`,取消回 `{cancelled: true}`。
 */
export function OmpQuestionDialog({
  request,
  onClose,
}: {
  request: OmpUiRequest | null;
  onClose: () => void;
}) {
  const { t } = useI18n();
  const [selected, setSelected] = useState<string | null>(null);
  const [text, setText] = useState("");
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (request) {
      setSelected(null);
      // editor 场景保留 prefill 的首尾空白;仅当完全缺省时才给空串。
      setText(request.prefill ?? "");
      setSubmitting(false);
      setError(null);
    }
  }, [request]);

  if (!request) return null;

  const isSelect = request.method === "select" && (request.options?.length ?? 0) > 0;
  const isText = request.method === "input" || request.method === "editor";

  async function handleSubmit() {
    if (submitting || !request) return;
    const value = request.method === "select" ? selected : text.trim();
    if (value == null || value === "") return;
    setSubmitting(true);
    setError(null);
    try {
      await respondOmpUiRequest(request, { value });
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
        aria-labelledby="omp-question-title"
        style={{
          width: "min(600px, calc(100vw - 32px))",
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
              "color-mix(in srgb, var(--accent-subtle, rgba(59, 130, 246, 0.1)) 60%, transparent)",
          }}
        >
          <HelpCircle size={20} color="var(--accent)" aria-hidden />
          <h2
            id="omp-question-title"
            style={{
              flex: 1,
              margin: 0,
              fontSize: 15,
              fontWeight: 600,
              color: "var(--text-primary)",
            }}
          >
            {request.title?.trim() || t("omp.question.title")}
          </h2>
        </div>

        <div
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
                fontSize: 13,
                lineHeight: 1.5,
                color: "var(--text-primary)",
                whiteSpace: "pre-wrap",
                marginBottom: 14,
              }}
            >
              {request.message}
            </div>
          )}

          {isSelect && (
            <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
              {request.options!.map((option, index) => {
                const isSelected = selected === option;
                const description = request.optionDetails?.[index]?.description;
                return (
                  <button
                    key={option}
                    type="button"
                    onClick={() => setSelected(option)}
                    style={{
                      display: "flex",
                      alignItems: "flex-start",
                      gap: 10,
                      padding: "10px 12px",
                      border: `1px solid ${isSelected ? "var(--accent)" : "var(--border-medium)"}`,
                      borderRadius: "var(--radius-sm)",
                      background: isSelected
                        ? "color-mix(in srgb, var(--accent) 8%, transparent)"
                        : "var(--bg-input)",
                      cursor: "pointer",
                      textAlign: "left",
                      transition: "all 0.15s ease",
                    }}
                  >
                    <div
                      style={{
                        width: 18,
                        height: 18,
                        flexShrink: 0,
                        marginTop: 1,
                        display: "flex",
                        alignItems: "center",
                        justifyContent: "center",
                        border: `2px solid ${isSelected ? "var(--accent)" : "var(--border-strong)"}`,
                        borderRadius: "50%",
                        background: isSelected ? "var(--accent)" : "transparent",
                        transition: "all 0.15s ease",
                      }}
                    >
                      {isSelected && <Check size={12} color="white" strokeWidth={3} />}
                    </div>
                    <div style={{ flex: 1, minWidth: 0 }}>
                      <div
                        style={{
                          fontSize: 13,
                          fontWeight: 600,
                          color: "var(--text-primary)",
                          marginBottom: description ? 4 : 0,
                        }}
                      >
                        {option}
                      </div>
                      {description && (
                        <div
                          style={{
                            fontSize: 12,
                            lineHeight: 1.4,
                            color: "var(--text-secondary)",
                          }}
                        >
                          {description}
                        </div>
                      )}
                    </div>
                  </button>
                );
              })}
            </div>
          )}

          {isText && (
            <textarea
              value={text}
              onChange={(event) => setText(event.target.value)}
              placeholder={request.placeholder ?? t("omp.question.textPlaceholder")}
              rows={request.method === "editor" ? 6 : 3}
              autoFocus
              style={{
                width: "100%",
                resize: "vertical",
                padding: "10px 12px",
                border: "1px solid var(--border-medium)",
                borderRadius: "var(--radius-sm)",
                background: "var(--bg-input)",
                color: "var(--text-primary)",
                fontSize: 13,
                lineHeight: 1.5,
                fontFamily: "inherit",
                outline: "none",
              }}
            />
          )}

          {error && (
            <div
              role="alert"
              style={{
                marginTop: 16,
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
          <Button variant="outline" size="sm" icon={X} disabled={submitting} onClick={handleCancel}>
            {t("omp.question.cancel")}
          </Button>
          <Button
            variant="default"
            size="sm"
            icon={Check}
            disabled={
              submitting || (request.method === "select" ? selected == null : text.trim() === "")
            }
            onClick={handleSubmit}
          >
            {submitting ? t("omp.question.submitting") : t("omp.question.submit")}
          </Button>
        </div>
      </div>
    </div>
  );
}
