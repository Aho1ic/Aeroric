/**
 * DBX-style button component for Aeroric database module
 * Matches dbx button visual specifications
 */

import type { ReactNode, ButtonHTMLAttributes, HTMLAttributes } from "react";
import type { LucideIcon } from "lucide-react";

// Button variants matching dbx
type DbxButtonVariant = "default" | "outline" | "secondary" | "ghost" | "destructive" | "link";

// Button sizes matching dbx
type DbxButtonSize = "default" | "xs" | "sm" | "lg" | "icon" | "icon-xs" | "icon-sm" | "icon-lg";

interface DbxButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: DbxButtonVariant;
  size?: DbxButtonSize;
  icon?: LucideIcon;
  iconPosition?: "start" | "end";
  active?: boolean;
  children?: ReactNode;
}

interface DbxButtonGroupProps extends HTMLAttributes<HTMLDivElement> {
  children?: ReactNode;
}

interface DbxMenuItemProps extends Omit<DbxButtonProps, "variant" | "size" | "role"> {
  destructive?: boolean;
}

// Base styles matching dbx buttonVariants
const baseStyles: React.CSSProperties = {
  display: "inline-flex",
  alignItems: "center",
  justifyContent: "center",
  gap: 6,
  whiteSpace: "nowrap",
  userSelect: "none",
  transition: "all 0.15s ease",
  outline: "none",
  border: "1px solid transparent",
  cursor: "pointer",
  fontWeight: 500,
  fontFamily: "inherit",
  lineHeight: 1,
  flexShrink: 0,
};

// Variant styles
const variantStyles: Record<DbxButtonVariant, React.CSSProperties> = {
  default: {
    background: "var(--accent)",
    color: "var(--accent-foreground)",
  },
  outline: {
    background: "var(--bg-card)",
    color: "var(--text-primary)",
    borderColor: "var(--border-medium)",
  },
  secondary: {
    background: "var(--bg-subtle)",
    color: "var(--text-secondary)",
  },
  ghost: {
    background: "transparent",
    color: "var(--text-primary)",
  },
  destructive: {
    background: "var(--danger-subtle, rgba(239, 68, 68, 0.1))",
    color: "var(--danger, #ef4444)",
  },
  link: {
    background: "transparent",
    color: "var(--accent)",
    textDecoration: "underline",
    textUnderlineOffset: 4,
  },
};

// Size styles
const sizeStyles: Record<DbxButtonSize, React.CSSProperties> = {
  default: {
    height: 32,
    padding: "0 10px",
    borderRadius: 10,
    fontSize: 14,
    gap: 6,
  },
  xs: {
    height: 24,
    padding: "0 8px",
    borderRadius: 8,
    fontSize: 12,
    gap: 4,
  },
  sm: {
    height: 28,
    padding: "0 10px",
    borderRadius: 10,
    fontSize: 12.8,
    gap: 4,
  },
  lg: {
    height: 36,
    padding: "0 10px",
    borderRadius: 10,
    fontSize: 14,
    gap: 6,
  },
  icon: {
    width: 32,
    height: 32,
    padding: 0,
    borderRadius: 10,
  },
  "icon-xs": {
    width: 24,
    height: 24,
    padding: 0,
    borderRadius: 8,
  },
  "icon-sm": {
    width: 28,
    height: 28,
    padding: 0,
    borderRadius: 10,
  },
  "icon-lg": {
    width: 36,
    height: 36,
    padding: 0,
    borderRadius: 10,
  },
};

// Icon sizes matching dbx
const iconSizes: Record<DbxButtonSize, number> = {
  default: 16,
  xs: 12,
  sm: 14,
  lg: 16,
  icon: 16,
  "icon-xs": 12,
  "icon-sm": 14,
  "icon-lg": 16,
};

export function DbxButton({
  variant = "default",
  size = "default",
  icon: Icon,
  iconPosition = "start",
  active = false,
  disabled = false,
  children,
  className,
  style: customStyle,
  ...props
}: DbxButtonProps) {
  const isIconOnly = size.startsWith("icon") || (!children && Icon);

  const style: React.CSSProperties = {
    ...baseStyles,
    ...variantStyles[variant],
    ...sizeStyles[size],
    ...(active ? { background: "var(--bg-active, rgba(0,0,0,0.1))" } : {}),
    ...(disabled ? { opacity: 0.5, cursor: "not-allowed", pointerEvents: "none" } : {}),
    ...customStyle,
  };

  const iconSize = iconSizes[size];
  const iconElement = Icon ? <Icon size={iconSize} /> : null;

  return (
    <button
      type="button"
      className={className}
      style={style}
      disabled={disabled}
      aria-disabled={disabled}
      {...props}
    >
      {iconElement && iconPosition === "start" && iconElement}
      {!isIconOnly && children}
      {iconElement && iconPosition === "end" && iconElement}
    </button>
  );
}

export function DbxIconButton(props: Omit<DbxButtonProps, "children">) {
  return <DbxButton variant="ghost" size="icon" {...props} />;
}

export function DbxButtonGroup({ children, style, ...props }: DbxButtonGroupProps) {
  return (
    <div
      data-slot="button-group"
      role={props.role ?? "group"}
      style={{
        display: "inline-flex",
        alignItems: "center",
        gap: 4,
        padding: 2,
        borderRadius: 12,
        border: "1px solid var(--border-subtle)",
        background: "var(--bg-subtle)",
        ...style,
      }}
      {...props}
    >
      {children}
    </div>
  );
}

export function DbxSegmentedButton({
  active = false,
  variant = "ghost",
  size = "sm",
  ...props
}: DbxButtonProps) {
  return (
    <DbxButton
      active={active}
      aria-pressed={active}
      variant={active ? "secondary" : variant}
      size={size}
      {...props}
    />
  );
}

export function DbxMenuItem({
  destructive = false,
  icon,
  style,
  children,
  ...props
}: DbxMenuItemProps) {
  return (
    <DbxButton
      role="menuitem"
      variant={destructive ? "destructive" : "ghost"}
      size="sm"
      icon={icon}
      style={{
        width: "100%",
        justifyContent: "flex-start",
        borderRadius: 8,
        color: destructive ? "var(--danger, #ef4444)" : undefined,
        ...style,
      }}
      {...props}
    >
      {children}
    </DbxButton>
  );
}

export function DbxDialogFooterButton({
  variant = "outline",
  size = "sm",
  ...props
}: DbxButtonProps) {
  return <DbxButton variant={variant} size={size} {...props} />;
}

// Export types for external use
export type {
  DbxButtonVariant,
  DbxButtonSize,
  DbxButtonProps,
  DbxButtonGroupProps,
  DbxMenuItemProps,
};
