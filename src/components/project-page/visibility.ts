import type React from "react";

/**
 * Inactive projects remain mounted to preserve terminal/editor state, but must
 * not remain in layout. `visibility:hidden` still lays out every full ProjectPage
 * and wakes their ResizeObservers on each resize/drag; `display:none` preserves
 * the React tree while removing that N-project layout work.
 */
export function projectVisibilityStyle(visible: boolean): React.CSSProperties {
  return visible
    ? { display: "flex", pointerEvents: "auto", zIndex: 1 }
    : { display: "none", pointerEvents: "none", zIndex: 0 };
}
