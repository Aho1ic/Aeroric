import { useState, type ButtonHTMLAttributes, type CSSProperties, type HTMLAttributes } from "react";

export type ButtonVariant = "default" | "outline" | "secondary" | "ghost" | "destructive" | "link";
export type ButtonSize = "default" | "xs" | "sm" | "lg" | "icon" | "icon-xs" | "icon-sm" | "icon-lg";

type ButtonProps = ButtonHTMLAttributes<HTMLButtonElement> & {
  variant?: ButtonVariant;
  size?: ButtonSize;
  active?: boolean;
};

const baseButtonStyle: CSSProperties = {
  display: "inline-flex",
  alignItems: "center",
  justifyContent: "center",
  gap: 6,
  whiteSpace: "nowrap",
  userSelect: "none",
  border: "1px solid transparent",
  outline: "none",
  cursor: "pointer",
  fontFamily: "inherit",
  fontWeight: 600,
  lineHeight: 1,
  flexShrink: 0,
  transition:
    "background-color 0.15s ease, border-color 0.15s ease, color 0.15s ease, opacity 0.15s ease, box-shadow 0.15s ease",
};

const variantStyle: Record<ButtonVariant, CSSProperties> = {
  default: {
    background: "var(--primary-action-bg)",
    color: "var(--primary-action-fg)",
    borderColor: "var(--primary-action-bg)",
    boxShadow: "var(--shadow-xs)",
  },
  outline: {
    background: "var(--bg-card)",
    color: "var(--text-primary)",
    borderColor: "var(--border-medium)",
    boxShadow: "var(--shadow-xs)",
  },
  secondary: {
    background: "var(--bg-subtle)",
    color: "var(--text-secondary)",
    borderColor: "var(--border-dim)",
  },
  ghost: {
    background: "transparent",
    color: "var(--text-secondary)",
    borderColor: "transparent",
  },
  destructive: {
    background: "var(--danger-surface)",
    color: "var(--danger)",
    borderColor: "var(--danger-border)",
  },
  link: {
    background: "transparent",
    color: "var(--accent)",
    borderColor: "transparent",
    textDecoration: "underline",
    textUnderlineOffset: 4,
  },
};

const hoverVariantStyle: Partial<Record<ButtonVariant, CSSProperties>> = {
  default: {
    background: "var(--primary-action-hover)",
    borderColor: "var(--primary-action-hover)",
  },
  outline: {
    background: "var(--bg-hover)",
    color: "var(--text-primary)",
  },
  secondary: {
    background: "var(--bg-hover)",
    color: "var(--text-primary)",
  },
  ghost: {
    background: "var(--bg-hover)",
    color: "var(--text-primary)",
    borderColor: "var(--border-dim)",
  },
  destructive: {
    background: "color-mix(in srgb, var(--danger-surface) 78%, var(--bg-hover))",
  },
};

const sizeStyle: Record<ButtonSize, CSSProperties> = {
  default: {
    height: 32,
    minWidth: 76,
    padding: "0 12px",
    borderRadius: "var(--radius-md)",
    fontSize: 12.5,
  },
  xs: {
    height: 24,
    padding: "0 8px",
    borderRadius: "var(--radius-sm)",
    fontSize: 11,
    gap: 4,
  },
  sm: {
    height: 28,
    padding: "0 10px",
    borderRadius: "var(--radius-sm)",
    fontSize: 12,
    gap: 5,
  },
  lg: {
    height: 36,
    minWidth: 84,
    padding: "0 14px",
    borderRadius: "var(--radius-md)",
    fontSize: 13,
  },
  icon: {
    width: 32,
    height: 32,
    padding: 0,
    borderRadius: "var(--radius-md)",
  },
  "icon-xs": {
    width: 24,
    height: 24,
    padding: 0,
    borderRadius: "var(--radius-sm)",
  },
  "icon-sm": {
    width: 28,
    height: 28,
    padding: 0,
    borderRadius: "var(--radius-sm)",
  },
  "icon-lg": {
    width: 36,
    height: 36,
    padding: 0,
    borderRadius: "var(--radius-md)",
  },
};

export function Button({
  type = "button",
  variant = "default",
  size = "default",
  active = false,
  disabled = false,
  style,
  onMouseEnter,
  onMouseLeave,
  children,
  ...props
}: ButtonProps) {
  const [hovered, setHovered] = useState(false);
  const showHover = hovered && !disabled;
  const resolvedVariant: ButtonVariant = active ? "secondary" : variant;
  const resolvedStyle: CSSProperties = {
    ...baseButtonStyle,
    ...variantStyle[resolvedVariant],
    ...(showHover ? hoverVariantStyle[resolvedVariant] : null),
    ...sizeStyle[size],
    ...(active
      ? {
          color: "var(--control-active-fg)",
          background: "var(--control-active-bg)",
          borderColor: "var(--border-strong)",
        }
      : null),
    ...(disabled
      ? {
          opacity: 0.5,
          cursor: "not-allowed",
          pointerEvents: "none",
        }
      : null),
    ...style,
  };

  return (
    <button
      type={type}
      disabled={disabled}
      aria-disabled={disabled || undefined}
      style={resolvedStyle}
      onMouseEnter={(event) => {
        setHovered(true);
        onMouseEnter?.(event);
      }}
      onMouseLeave={(event) => {
        setHovered(false);
        onMouseLeave?.(event);
      }}
      {...props}
    >
      {children}
    </button>
  );
}

export function ButtonGroup({
  children,
  style,
  ...props
}: HTMLAttributes<HTMLDivElement>) {
  return (
    <div
      role={props.role ?? "group"}
      style={{
        display: "inline-flex",
        alignItems: "center",
        gap: 6,
        ...style,
      }}
      {...props}
    >
      {children}
    </div>
  );
}
