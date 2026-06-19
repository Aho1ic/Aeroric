import { useState } from "react";
import type { ReactNode } from "react";

export function IconButton({
  icon,
  title,
  active = false,
  activeVariant = "filled",
  disabled = false,
  onClick,
  size = 32,
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
  const iconOnlyActive = active && activeVariant === "icon";

  return (
    <button
      title={title}
      disabled={disabled}
      onClick={onClick}
      onMouseEnter={() => setHovered(true)}
      onMouseLeave={() => setHovered(false)}
      style={{
        width: size,
        height: size,
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        background:
          active && !iconOnlyActive
            ? "var(--control-active-bg)"
            : showHover
              ? "var(--bg-hover)"
              : "none",
        border: "none",
        borderRadius: 6,
        cursor: disabled ? "not-allowed" : "pointer",
        color: iconOnlyActive
          ? "var(--accent)"
          : active
            ? "var(--control-active-fg)"
            : showHover
              ? "var(--text-muted)"
              : "var(--text-hint)",
        opacity: disabled ? 0.4 : 1,
        transition: "background 0.12s, color 0.12s",
        flexShrink: 0,
      }}
    >
      {icon}
    </button>
  );
}
