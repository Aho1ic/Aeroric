import { useEffect, useMemo, useRef, useState } from "react";
import { useI18n } from "../i18n";
import { zLayers } from "../styles/zLayers";
import type { DshMenuState } from "../dshInputTriggers";
import type { DshTriggerSource } from "../hooks/useDshTriggerMenu";

/** Design cap on the list height; clamped at runtime to the space above the composer. */
const MAX_HEIGHT = 320;

/** Smallest useful list height — below this the clamp stops shrinking and scrolls instead. */
const MIN_HEIGHT = 96;

/** DOM id of one option row (the `aria-activedescendant` target). */
function optionId(source: string, index: number): string {
  return `dsh-trigger-option-${source}-${index}`;
}

/**
 * Grouped candidate menu for the caret-driven `/` and `@` triggers.
 *
 * Combobox pattern, as in the Harness: focus never leaves the textarea, so rows
 * pick on `mousedown` with the default prevented (a `click` handler would fire
 * after the focus steal) and the highlight is exposed through
 * `aria-activedescendant` on the listbox instead of DOM focus.
 */
export function DshTriggerMenu({
  state,
  sources,
  onPick,
  onDismiss,
}: {
  state: DshMenuState;
  sources: readonly DshTriggerSource[];
  onPick: (source: string, index: number) => void;
  onDismiss: () => void;
}) {
  const { t } = useI18n();
  const listRef = useRef<HTMLDivElement>(null);
  const [maxHeight, setMaxHeight] = useState(MAX_HEIGHT);
  const highlight = state.open ? state.highlight : null;
  const labels = useMemo(() => {
    const map = new Map<string, string>();
    for (const source of sources) map.set(source.name, source.labelKey);
    return map;
  }, [sources]);

  // The list is bottom-anchored above the composer, so the design cap has to be
  // clamped to the space above it — re-measured on every state change because
  // the anchor moves when the composer grows.
  useEffect(() => {
    const node = listRef.current;
    if (node === null) return;
    const { bottom } = node.getBoundingClientRect();
    // Nothing measurable (no layout engine, or an unmounted anchor) keeps the cap.
    if (bottom <= 0) return;
    setMaxHeight(Math.max(MIN_HEIGHT, Math.min(MAX_HEIGHT, Math.round(bottom - 8))));
  }, [state]);

  // Focus stays in the textarea, so the browser never scrolls the active option
  // into view on a keyboard move — do it here.
  useEffect(() => {
    if (highlight === null) return;
    const row = document.getElementById(optionId(highlight.source, highlight.index));
    if (row && typeof row.scrollIntoView === "function") row.scrollIntoView({ block: "nearest" });
  }, [highlight]);

  // Dismiss on a pointer outside the menu AND outside the composer card:
  // clicking the textarea or the send row must not take the menu down.
  useEffect(() => {
    if (!state.open) return;
    const onPointerDown = (event: PointerEvent): void => {
      if (!(event.target instanceof Node)) return;
      const list = listRef.current;
      if (list === null || list.contains(event.target)) return;
      if (list.closest("[data-composer-card]")?.contains(event.target)) return;
      onDismiss();
    };
    document.addEventListener("pointerdown", onPointerDown, true);
    return () => document.removeEventListener("pointerdown", onPointerDown, true);
  }, [state.open, onDismiss]);

  if (!state.open) return null;

  return (
    <div
      ref={listRef}
      className="dsh-trigger-menu"
      style={{ maxHeight, zIndex: zLayers.dropdownInline }}
      role="listbox"
      aria-label={t("dsh.trigger.suggestions")}
      aria-activedescendant={
        highlight === null ? undefined : optionId(highlight.source, highlight.index)
      }
    >
      <div className="dsh-trigger-viewport">
        {state.groups.map((group) => {
          if (group.status === "ready" && group.items.length === 0) return null;
          return (
            <div key={group.source} className="dsh-trigger-group" data-source={group.source}>
              <div className="dsh-trigger-group-title" role="presentation">
                {t(labels.get(group.source) ?? group.source)}
              </div>
              {group.status === "pending" ? (
                <div className="dsh-trigger-loading">{t("dsh.trigger.loading")}</div>
              ) : (
                group.items.map((item, index) => {
                  const active =
                    highlight !== null &&
                    highlight.source === group.source &&
                    highlight.index === index;
                  return (
                    <button
                      key={`${group.source}:${item.name}`}
                      id={optionId(group.source, index)}
                      type="button"
                      role="option"
                      aria-selected={active}
                      className="dsh-trigger-item"
                      data-active={active}
                      onMouseDown={(event) => {
                        event.preventDefault();
                        onPick(group.source, index);
                      }}
                    >
                      <span className="dsh-trigger-item-name">{item.name}</span>
                      {item.hint !== undefined && (
                        <span className="dsh-trigger-item-hint">{item.hint}</span>
                      )}
                      {item.description !== undefined && (
                        <span className="dsh-trigger-item-description">{item.description}</span>
                      )}
                    </button>
                  );
                })
              )}
            </div>
          );
        })}
      </div>
    </div>
  );
}
