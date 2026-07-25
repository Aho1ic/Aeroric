import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type CSSProperties,
  type KeyboardEvent,
  type ReactNode,
} from "react";

export type AnimatedSelectionOrientation = "horizontal" | "vertical";
export type AnimatedSelectionVariant = "pill" | "underline";
export type AnimatedSelectionRole = "group" | "tablist";

interface IndicatorGeometry {
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface AnimatedSelectionTrackProps {
  value: string | number;
  children: ReactNode;
  ariaLabel: string;
  orientation?: AnimatedSelectionOrientation;
  variant?: AnimatedSelectionVariant;
  role?: AnimatedSelectionRole;
  className?: string;
  style?: CSSProperties;
  dataTestId?: string;
  dataSlot?: string;
}

/**
 * Shared moving-selection surface for segmented controls, navigation and tabs.
 * Children opt in with `data-animated-selection-item` and
 * `data-selection-value`. This keeps the indicator reusable for custom tab
 * content, including tabs with independent close buttons.
 */
export function AnimatedSelectionTrack({
  value,
  children,
  ariaLabel,
  orientation = "horizontal",
  variant = "pill",
  role = "group",
  className,
  style,
  dataTestId,
  dataSlot,
}: AnimatedSelectionTrackProps) {
  const rootRef = useRef<HTMLDivElement | null>(null);
  const previousIndexRef = useRef<number | null>(null);
  const frameRef = useRef<number | null>(null);
  const [geometry, setGeometry] = useState<IndicatorGeometry | null>(null);
  const [direction, setDirection] = useState<"forward" | "backward" | "none">("none");

  const measure = useCallback(() => {
    if (frameRef.current !== null) cancelAnimationFrame(frameRef.current);
    frameRef.current = requestAnimationFrame(() => {
      frameRef.current = null;
      const root = rootRef.current;
      if (!root) return;
      const items = Array.from(
        root.querySelectorAll<HTMLElement>("[data-animated-selection-item]"),
      );
      const activeIndex = items.findIndex((item) => item.dataset.selectionValue === String(value));
      const active = activeIndex >= 0 ? items[activeIndex] : null;
      if (!active) {
        setGeometry(null);
        previousIndexRef.current = null;
        return;
      }

      if (previousIndexRef.current !== null && previousIndexRef.current !== activeIndex) {
        setDirection(activeIndex > previousIndexRef.current ? "forward" : "backward");
      } else {
        setDirection("none");
      }
      previousIndexRef.current = activeIndex;

      const rootRect = root.getBoundingClientRect();
      const itemRect = active.getBoundingClientRect();
      const next =
        variant === "underline"
          ? {
              x: itemRect.left - rootRect.left + root.scrollLeft,
              y: itemRect.bottom - rootRect.top + root.scrollTop - 2,
              width: itemRect.width,
              height: 2,
            }
          : {
              x: itemRect.left - rootRect.left + root.scrollLeft,
              y: itemRect.top - rootRect.top + root.scrollTop,
              width: itemRect.width,
              height: itemRect.height,
            };
      setGeometry((current) => {
        if (
          current &&
          current.x === next.x &&
          current.y === next.y &&
          current.width === next.width &&
          current.height === next.height
        ) {
          return current;
        }
        return next;
      });
    });
  }, [value, variant]);

  useLayoutEffect(() => {
    measure();
  }, [measure]);

  useEffect(() => {
    const root = rootRef.current;
    if (!root) return;
    const resizeObserver =
      typeof ResizeObserver === "undefined" ? null : new ResizeObserver(() => measure());
    resizeObserver?.observe(root);
    root
      .querySelectorAll<HTMLElement>("[data-animated-selection-item]")
      .forEach((item) => resizeObserver?.observe(item));

    const mutationObserver =
      typeof MutationObserver === "undefined"
        ? null
        : new MutationObserver(() => {
            resizeObserver?.disconnect();
            resizeObserver?.observe(root);
            root
              .querySelectorAll<HTMLElement>("[data-animated-selection-item]")
              .forEach((item) => resizeObserver?.observe(item));
            measure();
          });
    mutationObserver?.observe(root, { childList: true, subtree: true });

    root.addEventListener("scroll", measure, { passive: true });
    window.addEventListener("resize", measure);
    return () => {
      if (frameRef.current !== null) cancelAnimationFrame(frameRef.current);
      resizeObserver?.disconnect();
      mutationObserver?.disconnect();
      root.removeEventListener("scroll", measure);
      window.removeEventListener("resize", measure);
    };
  }, [measure]);

  function handleKeyDown(event: KeyboardEvent<HTMLDivElement>) {
    const horizontalPrevious = event.key === "ArrowLeft";
    const horizontalNext = event.key === "ArrowRight";
    const verticalPrevious = event.key === "ArrowUp";
    const verticalNext = event.key === "ArrowDown";
    const previous =
      (orientation === "horizontal" && horizontalPrevious) ||
      (orientation === "vertical" && verticalPrevious);
    const next =
      (orientation === "horizontal" && horizontalNext) ||
      (orientation === "vertical" && verticalNext);
    if (!previous && !next && event.key !== "Home" && event.key !== "End") return;

    const root = rootRef.current;
    if (!root) return;
    const items = Array.from(
      root.querySelectorAll<HTMLElement>("[data-animated-selection-item]"),
    ).filter((item) => {
      const control =
        item instanceof HTMLButtonElement ? item : item.querySelector<HTMLButtonElement>("button");
      return control ? !control.disabled : false;
    });
    if (!items.length) return;
    const focusedIndex = items.findIndex(
      (item) => item === document.activeElement || item.contains(document.activeElement),
    );
    const activeIndex = items.findIndex((item) => item.dataset.selectionValue === String(value));
    const start = focusedIndex >= 0 ? focusedIndex : Math.max(0, activeIndex);
    let targetIndex = start;
    if (event.key === "Home") targetIndex = 0;
    else if (event.key === "End") targetIndex = items.length - 1;
    else if (previous) targetIndex = (start - 1 + items.length) % items.length;
    else if (next) targetIndex = (start + 1) % items.length;

    event.preventDefault();
    const target = items[targetIndex];
    const control =
      target instanceof HTMLButtonElement
        ? target
        : target?.querySelector<HTMLButtonElement>("button");
    control?.focus();
    control?.click();
  }

  return (
    <div
      ref={rootRef}
      role={role}
      aria-label={ariaLabel}
      aria-orientation={role === "tablist" ? orientation : undefined}
      className={`animated-selection animated-selection--${variant} animated-selection--${orientation}${className ? ` ${className}` : ""}`}
      style={style}
      data-testid={dataTestId}
      data-slot={dataSlot}
      data-direction={direction}
      onKeyDown={handleKeyDown}
    >
      <span
        aria-hidden="true"
        className="animated-selection__indicator"
        data-visible={geometry ? "true" : "false"}
        style={
          geometry
            ? {
                width: geometry.width,
                height: geometry.height,
                transform: `translate3d(${geometry.x}px, ${geometry.y}px, 0)`,
              }
            : undefined
        }
      />
      {children}
    </div>
  );
}

export interface AnimatedSelectionOption<T extends string | number> {
  value: T;
  label: ReactNode;
  ariaLabel?: string;
  title?: string;
  disabled?: boolean;
  style?: CSSProperties;
}

export interface AnimatedSelectionGroupProps<T extends string | number> {
  value: T;
  options: readonly AnimatedSelectionOption<T>[];
  onChange: (value: T) => void;
  ariaLabel: string;
  orientation?: AnimatedSelectionOrientation;
  variant?: AnimatedSelectionVariant;
  role?: AnimatedSelectionRole;
  className?: string;
  style?: CSSProperties;
  itemClassName?: string;
  itemStyle?: CSSProperties;
  equalWidth?: boolean;
}

export function AnimatedSelectionGroup<T extends string | number>({
  value,
  options,
  onChange,
  ariaLabel,
  orientation = "horizontal",
  variant = "pill",
  role = "group",
  className,
  style,
  itemClassName,
  itemStyle,
  equalWidth = false,
}: AnimatedSelectionGroupProps<T>) {
  return (
    <AnimatedSelectionTrack
      value={value}
      ariaLabel={ariaLabel}
      orientation={orientation}
      variant={variant}
      role={role}
      className={className}
      style={style}
    >
      {options.map((option) => {
        const selected = option.value === value;
        return (
          <button
            key={String(option.value)}
            type="button"
            role={role === "tablist" ? "tab" : undefined}
            aria-selected={role === "tablist" ? selected : undefined}
            aria-pressed={role === "group" ? selected : undefined}
            aria-label={option.ariaLabel}
            title={option.title}
            disabled={option.disabled}
            tabIndex={selected ? 0 : -1}
            data-animated-selection-item
            data-selection-value={String(option.value)}
            className={`animated-selection__item${itemClassName ? ` ${itemClassName}` : ""}`}
            style={{
              flex: equalWidth ? "1 1 0" : undefined,
              ...itemStyle,
              ...option.style,
            }}
            onClick={() => onChange(option.value)}
          >
            {option.label}
          </button>
        );
      })}
    </AnimatedSelectionTrack>
  );
}
