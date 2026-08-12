import type {
  CSSProperties,
  HTMLAttributes,
  InputHTMLAttributes,
  ReactNode,
  SelectHTMLAttributes,
  TextareaHTMLAttributes,
} from "react";
import { Button, type ButtonSize, type ButtonVariant } from "./Button";

export function Field({
  label,
  hint,
  error,
  children,
  style,
}: {
  label: ReactNode;
  hint?: ReactNode;
  error?: ReactNode;
  children: ReactNode;
  style?: CSSProperties;
}) {
  return (
    <label style={{ display: "grid", gap: 6, color: "var(--ds-text-secondary)", ...style }}>
      <span style={{ fontSize: "var(--font-size-meta)", fontWeight: 650 }}>{label}</span>
      {children}
      {error ? (
        <span role="alert" style={{ color: "var(--ds-danger)", fontSize: 11 }}>
          {error}
        </span>
      ) : hint ? (
        <span style={{ color: "var(--ds-text-muted)", fontSize: 11 }}>{hint}</span>
      ) : null}
    </label>
  );
}

const controlStyle: CSSProperties = {
  minHeight: 32,
  boxSizing: "border-box",
  border: "1px solid var(--ds-border)",
  borderRadius: "var(--radius-md)",
  background: "var(--ds-surface)",
  color: "var(--ds-text)",
  font: "inherit",
};

export function TextInput({ style, ...props }: InputHTMLAttributes<HTMLInputElement>) {
  return <input style={{ ...controlStyle, padding: "0 10px", ...style }} {...props} />;
}

export function Select({ style, ...props }: SelectHTMLAttributes<HTMLSelectElement>) {
  return <select style={{ ...controlStyle, padding: "0 10px", ...style }} {...props} />;
}

export function TextArea({ style, ...props }: TextareaHTMLAttributes<HTMLTextAreaElement>) {
  return (
    <textarea style={{ ...controlStyle, padding: 10, resize: "vertical", ...style }} {...props} />
  );
}

export function Badge({
  tone = "neutral",
  children,
  style,
}: {
  tone?: "neutral" | "accent" | "success" | "warning" | "danger";
  children: ReactNode;
  style?: CSSProperties;
}) {
  const colors = {
    neutral: ["var(--ds-surface-muted)", "var(--ds-text-secondary)"],
    accent: ["var(--ds-accent-soft)", "var(--ds-accent)"],
    success: ["color-mix(in srgb, var(--ds-success) 14%, transparent)", "var(--ds-success)"],
    warning: ["color-mix(in srgb, var(--ds-warning) 14%, transparent)", "var(--ds-warning)"],
    danger: ["color-mix(in srgb, var(--ds-danger) 14%, transparent)", "var(--ds-danger)"],
  } as const;
  return (
    <span
      style={{
        display: "inline-flex",
        alignItems: "center",
        minHeight: 20,
        padding: "0 7px",
        borderRadius: "var(--radius-pill)",
        background: colors[tone][0],
        color: colors[tone][1],
        fontSize: 11,
        fontWeight: 650,
        ...style,
      }}
    >
      {children}
    </span>
  );
}

export function Spinner({ label }: { label?: string }) {
  return <span className="ui-spinner" role="status" aria-label={label} />;
}

export function EmptyState({
  icon,
  title,
  description,
  action,
}: {
  icon?: ReactNode;
  title: ReactNode;
  description?: ReactNode;
  action?: ReactNode;
}) {
  return (
    <div
      style={{
        minHeight: 160,
        display: "grid",
        placeItems: "center",
        alignContent: "center",
        gap: 8,
        padding: 24,
        textAlign: "center",
        color: "var(--ds-text-muted)",
      }}
    >
      {icon}
      <strong style={{ color: "var(--ds-text)", fontSize: 14 }}>{title}</strong>
      {description && (
        <span style={{ maxWidth: 420, fontSize: 12, lineHeight: 1.5 }}>{description}</span>
      )}
      {action}
    </div>
  );
}

export function Toolbar({ style, ...props }: HTMLAttributes<HTMLDivElement>) {
  return (
    <div
      role={props.role ?? "toolbar"}
      style={{ display: "flex", alignItems: "center", gap: 6, minHeight: 40, ...style }}
      {...props}
    />
  );
}

export function IconButton({
  label,
  children,
  active,
  variant = "ghost",
  size = "icon",
  ...props
}: React.ComponentProps<typeof Button> & {
  label: string;
  children: ReactNode;
  active?: boolean;
  variant?: ButtonVariant;
  size?: ButtonSize;
}) {
  return (
    <Button
      aria-label={label}
      title={label}
      active={active}
      variant={variant}
      size={size}
      {...props}
    >
      {children}
    </Button>
  );
}
