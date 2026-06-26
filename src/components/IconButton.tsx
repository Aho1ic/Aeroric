import { useState } from "react";
import type { ReactNode } from "react";

export function IconButton({
  icon,
  title,
  active = false,
  activeVariant = "filled",
  disabled = false,
  onClick,
  size = 36,
}: {
  icon: ReactNode;
  title?: string;
  active?: boolean;
  activeVariant?: "filled" | "icon";
  disabled?: boolean;
  onClick?: () => void;
  size?: number;
}) {
  const [hovered, setHovered] = useState(false);
  const showHover = hovered && !disabled && !active;
  const iconActive = active && activeVariant === "icon";
  const activeBackground = iconActive ? "var(--accent-subtle)" : "var(--control-selected-bg)";
  const activeBorder = iconActive ? "var(--accent-soft)" : "var(--border-strong)";
  const activeColor = iconActive ? "var(--accent-strong)" : "var(--control-selected-fg)";

  return (
    <button
      type="button"
      title={title}
      aria-pressed={active || undefined}
      disabled={disabled}
      onClick={onClick}
      onMouseEnter={() => setHovered(true)}
      onMouseLeave={() => setHovered(false)}
      style={{
        width: size,
        height: size,
        display: "inline-flex",
        alignItems: "center",
        justifyContent: "center",
        padding: 0,
        background: active ? activeBackground : showHover ? "var(--bg-hover)" : "transparent",
        border: `1px solid ${active ? activeBorder : showHover ? "var(--border-dim)" : "transparent"}`,
        borderRadius: "var(--radius-md)",
        boxSizing: "border-box",
        boxShadow: iconActive
          ? "inset 0 0 0 1px color-mix(in srgb, var(--accent-soft) 55%, transparent)"
          : "none",
        cursor: disabled ? "not-allowed" : "pointer",
        color: active ? activeColor : showHover ? "var(--text-primary)" : "var(--text-muted)",
        opacity: disabled ? 0.5 : 1,
        outline: "none",
        transition:
          "background-color 0.15s ease, border-color 0.15s ease, color 0.15s ease, opacity 0.15s ease, box-shadow 0.15s ease",
        flexShrink: 0,
      }}
    >
      {icon}
    </button>
  );
}
