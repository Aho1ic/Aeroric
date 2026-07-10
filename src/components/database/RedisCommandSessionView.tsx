import { useI18n } from "../../i18n";
import { DbxButton } from "./DbxButton";

export interface RedisCommandHistoryEntry {
  id: number;
  prompt: string;
  command: string;
  output: string;
  error: boolean;
}

interface RedisCommandSessionViewProps {
  commandDb: number;
  commandText: string;
  commandHistory: RedisCommandHistoryEntry[];
  commandRunning: boolean;
  onCommandTextChange: (value: string) => void;
  onRunCommand: () => void | Promise<void>;
}

export function RedisCommandSessionView({
  commandDb,
  commandText,
  commandHistory,
  commandRunning,
  onCommandTextChange,
  onRunCommand,
}: RedisCommandSessionViewProps) {
  const { t } = useI18n();
  return (
    <div
      style={{
        minHeight: 200,
        maxHeight: 280,
        display: "flex",
        flexDirection: "column",
        border: "1px solid var(--border-dim)",
        borderRadius: 8,
        overflow: "hidden",
        background: "#171b21",
        color: "#d8dee9",
        fontFamily: "var(--font-mono)",
        fontSize: 12,
        lineHeight: 1.5,
      }}
      onClick={() => document.getElementById("redis-command-input")?.focus()}
    >
      <div style={{ flex: 1, minHeight: 0, overflow: "auto", padding: 12 }}>
        <div style={{ marginBottom: 8, color: "#94a3b8" }}>{t("database.redisCommandWelcome")}</div>
        {commandHistory.map((entry) => (
          <div key={entry.id} style={{ marginBottom: 8 }}>
            <div
              style={{
                display: "flex",
                alignItems: "flex-start",
                gap: 8,
                whiteSpace: "pre-wrap",
                wordBreak: "break-word",
              }}
            >
              <span style={{ flexShrink: 0, color: "#d7ba7d" }}>{entry.prompt}</span>
              <span style={{ minWidth: 0, color: "#e5e7eb" }}>{entry.command}</span>
            </div>
            {entry.output && (
              <pre
                style={{
                  margin: 0,
                  color: entry.error ? "#ff6b6b" : "#cbd5e1",
                  fontFamily: "var(--font-mono)",
                  fontSize: 12,
                  whiteSpace: "pre-wrap",
                  wordBreak: "break-word",
                }}
              >
                {entry.output}
              </pre>
            )}
          </div>
        ))}
      </div>
      <form
        style={{
          minHeight: 36,
          display: "flex",
          alignItems: "center",
          gap: 8,
          borderTop: "1px solid rgba(255, 255, 255, 0.1)",
          padding: "0 12px",
        }}
        onSubmit={(event) => {
          event.preventDefault();
          void onRunCommand();
        }}
      >
        <span style={{ flexShrink: 0, color: "#d7ba7d" }}>db{commandDb}&gt;</span>
        <input
          id="redis-command-input"
          aria-label={t("database.redisCommand")}
          style={{
            flex: 1,
            minWidth: 0,
            border: "none",
            outline: "none",
            background: "transparent",
            color: "#e5e7eb",
            fontFamily: "var(--font-mono)",
            fontSize: 12,
          }}
          value={commandText}
          onChange={(event) => onCommandTextChange(event.target.value)}
          disabled={commandRunning}
          autoComplete="off"
          spellCheck={false}
          placeholder="GET user:1"
        />
        <DbxButton
          type="submit"
          variant="default"
          size="sm"
          disabled={!commandText.trim() || commandRunning}
        >
          {t("database.redisRunCommand")}
        </DbxButton>
      </form>
    </div>
  );
}
