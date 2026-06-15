import type React from "react";

export function projectVisibilityStyle(visible: boolean): React.CSSProperties {
  return {
    display: "flex",
    visibility: visible ? "visible" : "hidden",
    pointerEvents: visible ? "auto" : "none",
    zIndex: visible ? 1 : 0,
  };
}
