import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Check, HelpCircle, X } from "lucide-react";
import { useI18n } from "../i18n";
import { Button } from "./ui/Button";

interface AskUserQuestionOption {
  label: string;
  description?: string;
}

interface AskUserQuestionItem {
  id: string;
  question: string;
  detail?: string;
  header?: string;
  options?: AskUserQuestionOption[];
  multiSelect?: boolean;
}

export interface DshQuestionRequest {
  rpcId: string;
  sessionId: string;
  questions: AskUserQuestionItem[];
}

interface QuestionAnswer {
  id: string;
  selected: string[];
  custom?: string;
}

export function DshQuestionDialog({
  request,
  onClose,
}: {
  request: DshQuestionRequest | null;
  onClose: () => void;
}) {
  const { t } = useI18n();
  const [answers, setAnswers] = useState<Map<string, QuestionAnswer>>(new Map());
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (request) {
      const initialAnswers = new Map<string, QuestionAnswer>();
      request.questions.forEach((q) => {
        initialAnswers.set(q.id, { id: q.id, selected: [] });
      });
      setAnswers(initialAnswers);
      setSubmitting(false);
      setError(null);
    }
  }, [request]);

  if (!request) return null;

  function toggleOption(questionId: string, optionLabel: string, multiSelect: boolean) {
    setAnswers((prev) => {
      const next = new Map(prev);
      const answer = next.get(questionId) ?? { id: questionId, selected: [] };
      if (multiSelect) {
        const selected = answer.selected.includes(optionLabel)
          ? answer.selected.filter((label) => label !== optionLabel)
          : [...answer.selected, optionLabel];
        next.set(questionId, { ...answer, selected });
      } else {
        next.set(questionId, { ...answer, selected: [optionLabel] });
      }
      return next;
    });
  }

  function setCustomAnswer(questionId: string, custom: string) {
    setAnswers((prev) => {
      const next = new Map(prev);
      const answer = next.get(questionId) ?? { id: questionId, selected: [] };
      next.set(questionId, { ...answer, custom: custom.trim() || undefined });
      return next;
    });
  }

  async function handleSubmit() {
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
            answer: {
              answers: Array.from(answers.values()),
            },
          },
        },
      });
      onClose();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
      setSubmitting(false);
    }
  }

  async function handleCancel() {
    if (submitting || !request) return;
    setSubmitting(true);
    setError(null);
    try {
      await invoke("respond_dsh_server_request", {
        rpcId: request.rpcId,
        sessionId: request.sessionId,
        result: {
          ok: false,
          error: {
            code: "cancelled",
            message: "the user closed this question request",
            details: {},
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
        if (e.target === e.currentTarget && !submitting) void handleCancel();
      }}
    >
      <div
        role="dialog"
        aria-labelledby="dsh-question-title"
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
            id="dsh-question-title"
            style={{
              flex: 1,
              margin: 0,
              fontSize: 15,
              fontWeight: 600,
              color: "var(--text-primary)",
            }}
          >
            {t("dsh.questionTitle")}
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
          {request.questions.map((question, qIndex) => {
            const answer = answers.get(question.id);
            const hasOptions = question.options && question.options.length > 0;

            return (
              <div
                key={question.id}
                style={{
                  marginBottom: qIndex < request.questions.length - 1 ? 24 : 0,
                }}
              >
                {question.header && (
                  <div
                    style={{
                      fontSize: 11,
                      fontWeight: 700,
                      color: "var(--text-hint)",
                      marginBottom: 6,
                      textTransform: "uppercase",
                      letterSpacing: "0.5px",
                    }}
                  >
                    {question.header}
                  </div>
                )}

                <div
                  style={{
                    fontSize: 13.5,
                    fontWeight: 600,
                    color: "var(--text-primary)",
                    marginBottom: question.detail ? 8 : 12,
                  }}
                >
                  {question.question}
                </div>

                {question.detail && (
                  <div
                    style={{
                      fontSize: 12.5,
                      lineHeight: 1.5,
                      color: "var(--text-secondary)",
                      marginBottom: 12,
                      whiteSpace: "pre-wrap",
                    }}
                  >
                    {question.detail}
                  </div>
                )}

                {hasOptions && (
                  <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
                    {question.options!.map((option) => {
                      const isSelected = answer?.selected.includes(option.label) ?? false;
                      return (
                        <button
                          key={option.label}
                          type="button"
                          onClick={() =>
                            toggleOption(question.id, option.label, question.multiSelect ?? false)
                          }
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
                              borderRadius: question.multiSelect ? "var(--radius-xs, 4px)" : "50%",
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
                                marginBottom: option.description ? 4 : 0,
                              }}
                            >
                              {option.label}
                            </div>
                            {option.description && (
                              <div
                                style={{
                                  fontSize: 12,
                                  lineHeight: 1.4,
                                  color: "var(--text-secondary)",
                                }}
                              >
                                {option.description}
                              </div>
                            )}
                          </div>
                        </button>
                      );
                    })}
                  </div>
                )}

                <div style={{ marginTop: 10 }}>
                  <input
                    type="text"
                    placeholder={t("dsh.questionCustomPlaceholder")}
                    value={answer?.custom ?? ""}
                    onChange={(e) => setCustomAnswer(question.id, e.target.value)}
                    style={{
                      width: "100%",
                      height: 34,
                      padding: "0 12px",
                      border: "1px solid var(--border-medium)",
                      borderRadius: "var(--radius-sm)",
                      background: "var(--bg-input)",
                      color: "var(--text-primary)",
                      fontSize: 13,
                      fontFamily: "inherit",
                      outline: "none",
                      transition: "border-color 0.15s ease, box-shadow 0.15s ease",
                    }}
                    onFocus={(e) => {
                      e.currentTarget.style.borderColor = "var(--ring)";
                      e.currentTarget.style.boxShadow =
                        "0 0 0 3px color-mix(in srgb, var(--ring) 18%, transparent)";
                    }}
                    onBlur={(e) => {
                      e.currentTarget.style.borderColor = "var(--border-medium)";
                      e.currentTarget.style.boxShadow = "none";
                    }}
                  />
                </div>
              </div>
            );
          })}

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
            {t("dsh.questionCancel")}
          </Button>
          <Button
            variant="default"
            size="sm"
            icon={Check}
            disabled={submitting}
            onClick={handleSubmit}
          >
            {submitting ? t("dsh.questionSubmitting") : t("dsh.questionSubmit")}
          </Button>
        </div>
      </div>
    </div>
  );
}
