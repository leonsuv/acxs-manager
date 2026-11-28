import React from "react";
import { Loader2 } from "lucide-react";

function cn(...classes: Array<string | false | null | undefined>): string {
  return classes.filter(Boolean).join(" ");
}

// ============================================================================
// Button
// ============================================================================

export type ButtonVariant = "primary" | "secondary" | "ghost" | "danger";
export type ButtonSize = "sm" | "md" | "lg";

export interface ButtonProps
  extends React.ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: ButtonVariant;
  size?: ButtonSize;
  loading?: boolean;
  icon?: React.ReactNode;
}

export function Button({
  variant = "secondary",
  size = "md",
  loading = false,
  icon,
  children,
  className,
  disabled,
  ...rest
}: ButtonProps) {
  const sizeCls = {
    sm: "h-8 px-3 text-xs gap-1.5 rounded-lg",
    md: "h-9 px-4 text-[13px] gap-2 rounded-lg",
    lg: "h-11 px-5 text-sm gap-2.5 rounded-xl",
  }[variant === "primary" && size === "lg" ? "lg" : size];

  const variantCls = {
    primary:
      "bg-[var(--accent)] text-white font-semibold accent-glow hover:brightness-110 active:scale-[0.985]",
    secondary:
      "bg-white/[0.05] text-[var(--text-primary)] font-medium border border-white/[0.09] hover:bg-white/[0.09] hover:border-white/[0.16] active:scale-[0.985]",
    ghost:
      "bg-transparent text-[var(--text-secondary)] font-medium hover:text-[var(--text-primary)] hover:bg-white/[0.06] active:scale-[0.985]",
    danger:
      "bg-red-500/15 text-red-300 font-medium border border-red-500/30 hover:bg-red-500/25 hover:text-red-200 active:scale-[0.985]",
  }[variant];

  return (
    <button
      disabled={disabled || loading}
      className={cn(
        "inline-flex items-center justify-center whitespace-nowrap select-none transition-all duration-150 cursor-pointer",
        "disabled:opacity-45 disabled:pointer-events-none",
        sizeCls,
        variantCls,
        className
      )}
      {...rest}
    >
      {loading ? (
        <Loader2 className="w-3.5 h-3.5 animate-spin shrink-0" />
      ) : icon ? (
        <span className="shrink-0 flex items-center">{icon}</span>
      ) : null}
      {children}
    </button>
  );
}

// ============================================================================
// Card
// ============================================================================

export type CardVariant = "glass" | "elevated" | "interactive";

export interface CardProps extends React.HTMLAttributes<HTMLDivElement> {
  variant?: CardVariant;
  padding?: "none" | "sm" | "md" | "lg";
}

export function Card({
  variant = "glass",
  padding = "md",
  className,
  children,
  onClick,
  onKeyDown,
  role,
  tabIndex,
  ...rest
}: CardProps) {
  const padCls = {
    none: "",
    sm: "p-3",
    md: "p-4",
    lg: "p-5",
  }[padding];

  const variantCls = {
    glass: "glass-panel rounded-xl",
    elevated: "surface-elevated rounded-xl",
    interactive:
      "glass-panel rounded-xl transition-all duration-150 hover:border-white/[0.16] hover:bg-white/[0.04] hover:-translate-y-[1px] cursor-pointer",
  }[variant];

  const isClickable = Boolean(onClick);
  const handleKeyDown: React.KeyboardEventHandler<HTMLDivElement> = (e) => {
    onKeyDown?.(e);
    if (e.defaultPrevented || !onClick) return;
    if (e.target === e.currentTarget && (e.key === "Enter" || e.key === " ")) {
      e.preventDefault();
      onClick(e as unknown as React.MouseEvent<HTMLDivElement>);
    }
  };

  return (
    <div
      role={role ?? (isClickable ? "button" : undefined)}
      tabIndex={tabIndex ?? (isClickable ? 0 : undefined)}
      onClick={onClick}
      onKeyDown={isClickable || onKeyDown ? handleKeyDown : undefined}
      className={cn(
        variantCls,
        padCls,
        isClickable &&
          "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--accent)]",
        className
      )}
      {...rest}
    >
      {children}
    </div>
  );
}

// ============================================================================
// Slider (Sleek motorsport range slider with glowing active track & mono value)
// ============================================================================

export interface SliderProps {
  label?: string;
  value: number;
  min: number;
  max: number;
  step?: number;
  unit?: string;
  formatValue?: (val: number) => string;
  onChange: (value: number) => void;
  disabled?: boolean;
  className?: string;
}

export function Slider({
  label,
  value,
  min,
  max,
  step = 1,
  unit = "",
  formatValue,
  onChange,
  disabled = false,
  className,
}: SliderProps) {
  const safeValue = typeof value === "number" && Number.isFinite(value) ? value : min;
  const pct =
    max > min ? Math.min(100, Math.max(0, ((safeValue - min) / (max - min)) * 100)) : 0;
  const display = formatValue ? formatValue(safeValue) : `${safeValue}${unit}`;

  return (
    <div className={cn("flex flex-col gap-1.5", disabled && "opacity-50", className)}>
      {(label || display) && (
        <div className="flex items-center justify-between gap-2 text-xs">
          {label ? (
            <span className="text-[var(--text-secondary)] font-medium">{label}</span>
          ) : (
            <span />
          )}
          <span className="font-mono text-[12px] text-[var(--text-primary)] tabular-nums bg-white/[0.04] border border-white/[0.06] px-1.5 py-0.5 rounded">
            {display}
          </span>
        </div>
      )}
      <div className="relative h-5 flex items-center">
        {/* Track backdrop */}
        <div className="absolute inset-x-0 h-[4px] rounded-full bg-white/[0.08] overflow-hidden pointer-events-none">
          <div
            className="h-full rounded-full transition-[width] duration-75"
            style={{
              width: `${pct}%`,
              background:
                "linear-gradient(90deg, rgba(var(--accent-rgb), 0.72) 0%, var(--accent) 100%)",
              boxShadow: "0 0 10px var(--accent-glow)",
            }}
          />
        </div>
        <input
          type="range"
          min={min}
          max={max}
          step={step}
          value={safeValue}
          disabled={disabled}
          onChange={(e) => onChange(Number(e.target.value))}
          className="acxs-range relative z-10 w-full h-5 m-0"
        />
      </div>
    </div>
  );
}

// ============================================================================
// Toggle (Smooth spring pill switch)
// ============================================================================

export interface ToggleProps {
  checked: boolean;
  onChange: (checked: boolean) => void;
  label?: string;
  description?: string;
  disabled?: boolean;
  className?: string;
}

export function Toggle({
  checked,
  onChange,
  label,
  description,
  disabled = false,
  className,
}: ToggleProps) {
  return (
    <label
      className={cn(
        "flex items-center justify-between gap-3 select-none",
        disabled ? "opacity-45 cursor-not-allowed" : "cursor-pointer",
        className
      )}
    >
      {(label || description) && (
        <div className="flex flex-col">
          {label && (
            <span className="text-[13px] font-medium text-[var(--text-primary)] leading-tight">
              {label}
            </span>
          )}
          {description && (
            <span className="text-[11px] text-[var(--text-muted)] mt-0.5">
              {description}
            </span>
          )}
        </div>
      )}
      <button
        type="button"
        role="switch"
        aria-checked={checked}
        disabled={disabled}
        onClick={() => !disabled && onChange(!checked)}
        className={cn(
          "relative inline-flex h-[22px] w-[40px] shrink-0 items-center rounded-full border transition-colors duration-200 cursor-pointer",
          checked
            ? "bg-[var(--accent)] border-[var(--accent-border)]"
            : "bg-white/[0.08] border-white/[0.1]"
        )}
        style={checked ? { boxShadow: "0 0 12px var(--accent-glow)" } : undefined}
      >
        <span
          className={cn(
            "inline-block h-4 w-4 rounded-full bg-white shadow-sm transition-transform duration-200 ease-[cubic-bezier(0.34,1.56,0.64,1)]",
            checked ? "translate-x-[20px]" : "translate-x-[2px]"
          )}
        />
      </button>
    </label>
  );
}

// ============================================================================
// SegmentedControl
// ============================================================================

export interface SegmentedOption<T extends string> {
  value: T;
  label: string;
  icon?: React.ReactNode;
}

export interface SegmentedControlProps<T extends string> {
  options: Array<SegmentedOption<T>>;
  value: T;
  onChange: (value: T) => void;
  size?: "sm" | "md";
  className?: string;
}

export function SegmentedControl<T extends string>({
  options,
  value,
  onChange,
  size = "md",
  className,
}: SegmentedControlProps<T>) {
  return (
    <div
      className={cn(
        "inline-flex items-center p-1 rounded-xl bg-black/40 border border-white/[0.07] gap-0.5",
        className
      )}
    >
      {options.map((opt) => {
        const active = opt.value === value;
        return (
          <button
            key={opt.value}
            type="button"
            onClick={() => onChange(opt.value)}
            className={cn(
              "inline-flex items-center justify-center gap-1.5 rounded-lg font-medium transition-all duration-150 cursor-pointer select-none",
              size === "sm" ? "h-7 px-2.5 text-xs" : "h-8 px-3 text-[13px]",
              active
                ? "bg-[var(--bg-elevated)] text-[var(--text-primary)] border border-white/[0.12] shadow-sm"
                : "text-[var(--text-secondary)] hover:text-[var(--text-primary)] border border-transparent"
            )}
          >
            {opt.icon}
            {opt.label}
          </button>
        );
      })}
    </div>
  );
}

// ============================================================================
// Badge
// ============================================================================

export type BadgeVariant =
  | "accent"
  | "neutral"
  | "success"
  | "warning"
  | "danger"
  | "info";

export interface BadgeProps extends React.HTMLAttributes<HTMLSpanElement> {
  variant?: BadgeVariant;
  dot?: boolean;
}

export function Badge({
  variant = "neutral",
  dot = false,
  className,
  children,
  ...rest
}: BadgeProps) {
  const styles: Record<BadgeVariant, string> = {
    accent:
      "bg-[var(--accent-soft)] text-[var(--accent)] border-[var(--accent-border)]",
    neutral: "bg-white/[0.06] text-[var(--text-secondary)] border-white/[0.09]",
    success: "bg-emerald-500/15 text-emerald-300 border-emerald-500/30",
    warning: "bg-amber-500/15 text-amber-300 border-amber-500/30",
    danger: "bg-red-500/15 text-red-300 border-red-500/30",
    info: "bg-cyan-500/15 text-cyan-300 border-cyan-500/30",
  };

  const dotColors: Record<BadgeVariant, string> = {
    accent: "bg-[var(--accent)]",
    neutral: "bg-slate-400",
    success: "bg-emerald-400",
    warning: "bg-amber-400",
    danger: "bg-red-400",
    info: "bg-cyan-400",
  };

  return (
    <span
      className={cn(
        "inline-flex items-center gap-1.5 px-2 py-0.5 rounded-md border text-[11px] font-medium tracking-wide whitespace-nowrap",
        styles[variant],
        className
      )}
      {...rest}
    >
      {dot && <span className={cn("w-1.5 h-1.5 rounded-full", dotColors[variant])} />}
      {children}
    </span>
  );
}

// ============================================================================
// EmptyState
// ============================================================================

export interface EmptyStateProps {
  icon?: React.ReactNode;
  title: string;
  description?: string;
  action?: React.ReactNode;
  className?: string;
}

export function EmptyState({
  icon,
  title,
  description,
  action,
  className,
}: EmptyStateProps) {
  return (
    <div
      className={cn(
        "glass-panel rounded-2xl p-8 flex flex-col items-center justify-center text-center gap-3",
        className
      )}
    >
      {icon && (
        <div className="w-11 h-11 rounded-xl bg-white/[0.04] border border-white/[0.08] flex items-center justify-center text-[var(--text-secondary)]">
          {icon}
        </div>
      )}
      <div className="space-y-1 max-w-md">
        <h3 className="text-sm font-semibold text-[var(--text-primary)]">{title}</h3>
        {description && (
          <p className="text-xs text-[var(--text-muted)] leading-relaxed">
            {description}
          </p>
        )}
      </div>
      {action && <div className="mt-1">{action}</div>}
    </div>
  );
}

// ============================================================================
// Skeleton
// ============================================================================

export interface SkeletonProps {
  className?: string;
}

export function Skeleton({ className }: SkeletonProps) {
  return <div className={cn("acxs-skeleton rounded-lg", className)} />;
}

// ============================================================================
// PageHeader
// ============================================================================

export interface PageHeaderProps {
  eyebrow?: string;
  title: string;
  subtitle?: string;
  actions?: React.ReactNode;
  className?: string;
}

export function PageHeader({
  eyebrow,
  title,
  subtitle,
  actions,
  className,
}: PageHeaderProps) {
  return (
    <div
      className={cn(
        "flex flex-wrap items-end justify-between gap-4 pb-4 border-b border-white/[0.06]",
        className
      )}
    >
      <div>
        {eyebrow && (
          <div className="text-[11px] font-mono uppercase tracking-[0.14em] text-[var(--accent)] mb-1">
            {eyebrow}
          </div>
        )}
        <h1 className="font-display text-3xl font-bold uppercase tracking-wider text-[var(--text-primary)] leading-none">
          {title}
        </h1>
        {subtitle && (
          <p className="text-[13px] text-[var(--text-secondary)] mt-1.5">
            {subtitle}
          </p>
        )}
      </div>
      {actions && <div className="flex items-center gap-2">{actions}</div>}
    </div>
  );
}

// ============================================================================
// KpiTile
// ============================================================================

export interface KpiTileProps {
  label: string;
  value: string | number;
  unit?: string;
  sublabel?: string;
  icon?: React.ReactNode;
  accent?: boolean;
  className?: string;
}

export function KpiTile({
  label,
  value,
  unit,
  sublabel,
  icon,
  accent = false,
  className,
}: KpiTileProps) {
  return (
    <div
      className={cn(
        "glass-panel rounded-xl p-4 flex flex-col justify-between gap-2 relative overflow-hidden",
        className
      )}
    >
      {accent && (
        <div
          className="absolute inset-x-0 top-0 h-[2px]"
          style={{
            background:
              "linear-gradient(90deg, transparent 0%, var(--accent) 50%, transparent 100%)",
          }}
        />
      )}
      <div className="flex items-center justify-between gap-2">
        <span className="text-[11px] font-medium uppercase tracking-[0.08em] text-[var(--text-muted)]">
          {label}
        </span>
        {icon && <span className="text-[var(--text-muted)]">{icon}</span>}
      </div>
      <div className="flex items-baseline gap-1.5">
        <span className="font-mono text-2xl font-semibold text-[var(--text-primary)] tabular-nums tracking-tight">
          {value}
        </span>
        {unit && (
          <span className="font-mono text-xs text-[var(--text-muted)]">{unit}</span>
        )}
      </div>
      {sublabel && (
        <span className="text-[11px] text-[var(--text-secondary)] truncate">
          {sublabel}
        </span>
      )}
    </div>
  );
}
