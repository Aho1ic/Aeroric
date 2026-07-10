import type { ChangeEvent, CSSProperties, ReactNode } from "react";
import { Eye, EyeOff } from "lucide-react";
import s from "../../styles";

const SQL_KEYWORDS = new Set([
  "ADD",
  "ALTER",
  "AND",
  "AS",
  "CREATE",
  "DEFAULT",
  "DELETE",
  "DROP",
  "EXISTS",
  "FOREIGN",
  "FROM",
  "IF",
  "INDEX",
  "INSERT",
  "INTO",
  "KEY",
  "NOT",
  "NULL",
  "ON",
  "PRIMARY",
  "REFERENCES",
  "SELECT",
  "SET",
  "TABLE",
  "UNIQUE",
  "UPDATE",
  "VALUES",
  "WHERE",
]);

function sqlTokenKind(token: string): "keyword" | "string" | "number" | "comment" | null {
  if (/^--/.test(token)) return "comment";
  if (/^'/.test(token) || /^"/.test(token)) return "string";
  if (/^\d+(?:\.\d+)?$/.test(token)) return "number";
  if (SQL_KEYWORDS.has(token.toUpperCase())) return "keyword";
  return null;
}

function sqlTokenColor(kind: NonNullable<ReturnType<typeof sqlTokenKind>>): string {
  if (kind === "keyword") return "var(--accent-strong)";
  if (kind === "string") return "var(--success)";
  if (kind === "number") return "var(--warning)";
  return "var(--text-hint)";
}

export function renderSqlTokens(sql: string): ReactNode[] {
  const tokens =
    sql.match(
      /--[^\n]*|'(?:''|[^'])*'|"(?:\\"|[^"])*"|\b\d+(?:\.\d+)?\b|\b[A-Za-z_][\w$]*\b|\s+|./g,
    ) ?? [];
  return tokens.map((token, index) => {
    const kind = sqlTokenKind(token);
    if (!kind) return token;
    return (
      <span key={`${index}:${token}`} data-sql-token={kind} style={{ color: sqlTokenColor(kind) }}>
        {token}
      </span>
    );
  });
}

export function PasswordInput({
  value,
  onChange,
  show,
  onToggle,
  style,
}: {
  value: string;
  onChange: (event: ChangeEvent<HTMLInputElement>) => void;
  show: boolean;
  onToggle: () => void;
  style?: CSSProperties;
}) {
  return (
    <div style={{ position: "relative", display: "flex", alignItems: "center" }}>
      <input
        style={{ ...style, paddingRight: 36 }}
        type={show ? "text" : "password"}
        value={value}
        onChange={onChange}
      />
      <button
        type="button"
        onClick={onToggle}
        style={{
          position: "absolute",
          right: 8,
          background: "transparent",
          border: "none",
          cursor: "pointer",
          color: "var(--text-hint)",
          padding: 2,
          display: "flex",
          alignItems: "center",
        }}
        aria-label={show ? "Hide password" : "Show password"}
      >
        {show ? <EyeOff size={14} /> : <Eye size={14} />}
      </button>
    </div>
  );
}

export function GuidancePanel({ title, message }: { title: string; message: string }) {
  return (
    <div style={s.databaseWorkspacePanel}>
      <div>
        <div style={s.databaseWorkspaceTitle}>{title}</div>
        <div style={s.databaseDialogHint}>{message}</div>
      </div>
    </div>
  );
}
