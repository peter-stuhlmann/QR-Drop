import type { ButtonHTMLAttributes, ReactNode } from "react";

type Variant = "primary" | "glass";

interface Props extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: Variant;
  icon?: ReactNode;
  /** Full width. */
  block?: boolean;
  /** 44 px tall call to action instead of the 32 px default. */
  large?: boolean;
}

/** Look per variant; the colours live in index.css so hover states can override them. */
const LOOK: Record<Variant, string> = {
  primary: "accent-fill font-semibold hover:brightness-105",
  glass: "glass-control text-[var(--text-primary)]",
};

/**
 * Pill button. Without children it is a round icon button; pass an `aria-label` then.
 */
export function Button({
  variant = "glass",
  icon,
  block,
  large,
  className = "",
  children,
  ...rest
}: Props) {
  const base =
    "focus-ring inline-flex items-center justify-center gap-2 rounded-full font-medium transition-[transform,background-color,filter] duration-150 active:scale-[0.97] disabled:opacity-50";
  const size = large
    ? "h-11 px-5 text-body font-semibold"
    : children
      ? "h-8 px-4 text-caption"
      : "size-8";
  return (
    <button
      type="button"
      className={`${base} ${size} ${LOOK[variant]} ${block ? "w-full" : ""} ${className}`}
      {...rest}
    >
      {icon}
      {children}
    </button>
  );
}
