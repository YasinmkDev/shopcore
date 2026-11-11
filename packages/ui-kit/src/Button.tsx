import { LoaderCircle } from "lucide-react";
import type { ButtonHTMLAttributes, ReactElement, ReactNode } from "react";

/**
 * Button (see context/10-ui-system.md, component registry).
 * Variants primary/secondary/ghost/danger; sizes sm/md/lg. The label is
 * always text (never icon alone — use IconButton for that). Press is
 * transform-only (scale .98, 80 ms, token --dur-instant) and off under
 * `prefers-reduced-motion`. md/lg meet the 44 px POS target; sm is
 * back-office only (labelled as such on the dev test page).
 * Keyboard: a native <button>, so Enter/Space activation is the browser's
 * (jsdom does not implement key-based activation — see Button.test.tsx).
 */
export type ButtonVariant = "primary" | "secondary" | "ghost" | "danger";
export type ButtonSize = "sm" | "md" | "lg";

export interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: ButtonVariant;
  size?: ButtonSize;
  /** Shows a spinner and disables the button while the action runs. */
  loading?: boolean;
}

const VARIANT_CLASSES: Record<ButtonVariant, string> = {
  primary: "bg-accent text-on-accent hover:bg-accent-hover active:bg-accent-active",
  secondary:
    "border border-border-strong bg-surface text-text hover:bg-surface-subtle",
  ghost: "bg-transparent text-text hover:bg-surface-subtle",
  /* No --danger-hover token exists; hover dims the whole button instead. */
  danger: "bg-danger text-on-accent hover:opacity-90",
};

const SIZE_CLASSES: Record<ButtonSize, string> = {
  sm: "h-8 min-w-8 px-3 text-sm",
  md: "h-11 min-w-11 px-4 text-base",
  lg: "h-12 min-w-12 px-5 text-base",
};

export function Button({
  variant = "primary",
  size = "md",
  loading = false,
  disabled = false,
  className = "",
  children,
  ...rest
}: ButtonProps): ReactElement {
  const isDisabled = disabled || loading;
  return (
    <button
      type="button"
      disabled={isDisabled}
      aria-busy={loading || undefined}
      className={`inline-flex items-center justify-center gap-2 rounded font-medium select-none transition-transform duration-instant focus-visible:outline-2 focus-visible:outline-solid focus-visible:outline-focus-ring focus-visible:outline-offset-2 active:scale-[0.98] motion-reduce:transition-none motion-reduce:active:scale-100 disabled:cursor-not-allowed disabled:opacity-50 ${VARIANT_CLASSES[variant]} ${SIZE_CLASSES[size]} ${className}`}
      {...rest}
    >
      {loading && <LoaderCircle size={16} strokeWidth={1.5} aria-hidden="true" className="animate-spin" />}
      {children}
    </button>
  );
}

export interface IconButtonProps extends Omit<
  ButtonHTMLAttributes<HTMLButtonElement>,
  "aria-label" | "children"
> {
  variant?: ButtonVariant;
  size?: ButtonSize;
  loading?: boolean;
  /** Required: icon-only buttons need a screen-reader label AND a tooltip. */
  "aria-label": string;
  icon: ReactNode;
}

const ICON_SIZE_CLASSES: Record<ButtonSize, string> = {
  sm: "h-8 w-8",
  md: "h-11 w-11",
  lg: "h-12 w-12",
};

export function IconButton({
  variant = "ghost",
  size = "md",
  loading = false,
  disabled = false,
  className = "",
  icon,
  ...rest
}: IconButtonProps): ReactElement {
  const label = rest["aria-label"];
  if (typeof label !== "string" || label.length === 0) {
    throw new Error("IconButton requires a non-empty aria-label (screen-reader label + tooltip).");
  }
  const isDisabled = disabled || loading;
  return (
    <button
      type="button"
      disabled={isDisabled}
      aria-busy={loading || undefined}
      title={label}
      className={`inline-flex items-center justify-center rounded font-medium select-none transition-transform duration-instant focus-visible:outline-2 focus-visible:outline-solid focus-visible:outline-focus-ring focus-visible:outline-offset-2 active:scale-[0.98] motion-reduce:transition-none motion-reduce:active:scale-100 disabled:cursor-not-allowed disabled:opacity-50 ${VARIANT_CLASSES[variant]} ${ICON_SIZE_CLASSES[size]} ${className}`}
      {...rest}
    >
      {loading ? (
        <LoaderCircle size={16} strokeWidth={1.5} aria-hidden="true" className="animate-spin" />
      ) : (
        icon
      )}
    </button>
  );
}
