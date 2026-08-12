import {
  Children,
  createContext,
  isValidElement,
  useContext,
  useState,
  type ButtonHTMLAttributes,
  type CSSProperties,
  type HTMLAttributes,
  type ReactNode,
} from "react";
import type { LucideIcon } from "lucide-react";
import { AnimatedSelectionTrack } from "./AnimatedSelection";

export type ButtonVariant = "default" | "outline" | "secondary" | "ghost" | "destructive" | "link";
export type ButtonSize =
  | "default"
  | "xs"
  | "sm"
  | "lg"
  | "icon"
  | "icon-xs"
  | "icon-sm"
  | "icon-lg";

export type ButtonProps = ButtonHTMLAttributes<HTMLButtonElement> & {
  variant?: ButtonVariant;
  size?: ButtonSize;
  icon?: LucideIcon;
  iconPosition?: "start" | "end";
  active?: boolean;
};

export type ButtonGroupProps = HTMLAttributes<HTMLDivElement> & { children?: ReactNode };
export type MenuItemProps = Omit<ButtonProps, "variant" | "size" | "role"> & {
  destructive?: boolean;
};

const ButtonGroupContext = createContext(false);

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
  boxSizing: "border-box",
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
    background: "var(--bg-input)",
    color: "var(--text-primary)",
    borderColor: "var(--border-medium)",
    boxShadow: "var(--shadow-xs)",
  },
  secondary: {
    background: "var(--secondary)",
    color: "var(--secondary-foreground)",
    borderColor: "var(--border-dim)",
  },
  ghost: {
    background: "transparent",
    color: "var(--text-secondary)",
    borderColor: "transparent",
  },
  destructive: {
    background: "var(--destructive)",
    color: "var(--destructive-foreground)",
    borderColor: "var(--destructive)",
    boxShadow: "var(--shadow-xs)",
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
    borderColor: "var(--border-strong)",
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
    background: "color-mix(in srgb, var(--destructive) 88%, black)",
    borderColor: "color-mix(in srgb, var(--destructive) 88%, black)",
  },
  link: {
    color: "var(--accent-hover)",
  },
};

const sizeStyle: Record<ButtonSize, CSSProperties> = {
  default: {
    height: 32,
    minWidth: 76,
    padding: "0 14px",
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
    height: 38,
    minWidth: 84,
    padding: "0 16px",
    borderRadius: "var(--radius-md)",
    fontSize: 13,
  },
  icon: {
    width: 32,
    minWidth: 32,
    height: 32,
    padding: 0,
    borderRadius: "var(--radius-md)",
  },
  "icon-xs": {
    width: 24,
    minWidth: 24,
    height: 24,
    padding: 0,
    borderRadius: "var(--radius-sm)",
  },
  "icon-sm": {
    width: 28,
    minWidth: 28,
    height: 28,
    padding: 0,
    borderRadius: "var(--radius-sm)",
  },
  "icon-lg": {
    width: 36,
    minWidth: 36,
    height: 36,
    padding: 0,
    borderRadius: "var(--radius-md)",
  },
};

const iconSizes: Record<ButtonSize, number> = {
  default: 16,
  xs: 12,
  sm: 14,
  lg: 16,
  icon: 16,
  "icon-xs": 12,
  "icon-sm": 14,
  "icon-lg": 16,
};

export function Button({
  type = "button",
  variant = "default",
  size = "default",
  icon: Icon,
  iconPosition = "start",
  active = false,
  disabled = false,
  style,
  onMouseEnter,
  onMouseLeave,
  onFocus,
  onBlur,
  children,
  ...props
}: ButtonProps) {
  const [hovered, setHovered] = useState(false);
  const [focused, setFocused] = useState(false);
  const showHover = hovered && !disabled;
  const isIconOnly = size.startsWith("icon") || (!children && Icon);
  const iconElement = Icon ? <Icon size={iconSizes[size]} aria-hidden /> : null;
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
    ...(focused && !disabled
      ? {
          borderColor: "var(--ring)",
          boxShadow: "0 0 0 3px color-mix(in srgb, var(--ring) 18%, transparent)",
        }
      : null),
    ...(disabled
      ? {
          opacity: 0.48,
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
      onFocus={(event) => {
        setFocused(true);
        onFocus?.(event);
      }}
      onBlur={(event) => {
        setFocused(false);
        onBlur?.(event);
      }}
      {...props}
    >
      {iconPosition === "start" ? iconElement : null}
      {!isIconOnly ? children : null}
      {iconPosition === "end" ? iconElement : null}
    </button>
  );
}

export function IconButton(props: Omit<ButtonProps, "children">) {
  return <Button variant="ghost" size="icon" {...props} />;
}

export function ButtonGroup({
  children,
  style,
  className,
  role,
  "aria-label": ariaLabel,
}: ButtonGroupProps) {
  const items = Children.toArray(children);
  const activeIndex = items.findIndex(
    (child) => isValidElement<ButtonProps>(child) && child.props.active,
  );
  return (
    <AnimatedSelectionTrack
      value={activeIndex}
      ariaLabel={typeof ariaLabel === "string" ? ariaLabel : ""}
      role={role === "tablist" ? "tablist" : "group"}
      className={className}
      dataSlot="button-group"
      style={{
        ...style,
      }}
    >
      {items.map((child, index) => (
        <span
          key={isValidElement(child) && child.key != null ? child.key : index}
          data-animated-selection-item
          data-selection-value={String(index)}
          style={{ position: "relative", zIndex: 1, display: "inline-flex" }}
        >
          <ButtonGroupContext.Provider value>{child}</ButtonGroupContext.Provider>
        </span>
      ))}
    </AnimatedSelectionTrack>
  );
}

export function SegmentedButton({
  active = false,
  variant = "ghost",
  size = "sm",
  ...props
}: ButtonProps) {
  const grouped = useContext(ButtonGroupContext);
  return (
    <Button
      active={grouped ? false : active}
      aria-pressed={active}
      variant={grouped ? "ghost" : active ? "secondary" : variant}
      size={size}
      {...props}
      style={{
        background: grouped ? "transparent" : undefined,
        color: grouped ? (active ? "var(--control-active-fg)" : "var(--text-muted)") : undefined,
        ...props.style,
      }}
    />
  );
}

export function MenuItem({ destructive = false, icon, style, children, ...props }: MenuItemProps) {
  return (
    <Button
      role="menuitem"
      variant={destructive ? "destructive" : "ghost"}
      size="sm"
      icon={icon}
      style={{
        width: "100%",
        justifyContent: "flex-start",
        borderRadius: 8,
        background: destructive ? "var(--danger-subtle, rgba(239, 68, 68, 0.1))" : undefined,
        color: destructive ? "var(--danger, #ef4444)" : undefined,
        ...style,
      }}
      {...props}
    >
      {children}
    </Button>
  );
}

export function DialogFooterButton({ variant = "outline", size = "sm", ...props }: ButtonProps) {
  return <Button variant={variant} size={size} {...props} />;
}
