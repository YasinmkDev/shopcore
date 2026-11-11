import { useId } from "react";
import type { InputHTMLAttributes, ReactElement } from "react";

/**
 * TextField (see context/10-ui-system.md, component registry).
 * Label, helper text, placeholder, required marker, disabled and error
 * states. Errors render inline next to the field in plain language, with
 * `aria-invalid` + `aria-describedby`. The border uses the --border-input
 * token (≥ 3:1 against the surface, spec 000b); the focus ring is 2 px +
 * offset (token --focus-ring) and is never removed via CSS.
 * 44 px tall (h-11) so it meets the POS target. RTL-safe (logical
 * properties only — no physical left/right utilities).
 */
export interface TextFieldProps extends Omit<InputHTMLAttributes<HTMLInputElement>, "size"> {
  label: string;
  helperText?: string;
  /** Plain-language error text; shown inline and linked via aria-describedby. */
  error?: string;
}

export function TextField({
  label,
  helperText,
  error,
  required = false,
  disabled = false,
  id,
  className = "",
  ...rest
}: TextFieldProps): ReactElement {
  const generatedId = useId().replace(/:/g, "");
  const inputId = id ?? `textfield-${generatedId}`;
  const errorId = `${inputId}-error`;
  const helperId = `${inputId}-helper`;
  const describedBy =
    error !== undefined ? errorId : helperText !== undefined ? helperId : undefined;

  return (
    <div className="flex flex-col gap-1">
      <label htmlFor={inputId} className="text-sm font-medium text-text">
        {label}
        {required && (
          <span aria-hidden="true" className="text-danger">
            {" *"}
          </span>
        )}
      </label>
      <input
        id={inputId}
        type="text"
        required={required}
        disabled={disabled}
        aria-invalid={error !== undefined || undefined}
        aria-describedby={describedBy}
        className={`h-11 w-full rounded border bg-surface px-3 text-base text-text placeholder:text-faint focus-visible:outline-2 focus-visible:outline-solid focus-visible:outline-focus-ring focus-visible:outline-offset-2 disabled:cursor-not-allowed disabled:bg-surface-subtle disabled:text-muted ${error !== undefined ? "border-danger" : "border-border-input"} ${className}`}
        {...rest}
      />
      {error !== undefined ? (
        <p id={errorId} className="text-sm text-danger">
          {error}
        </p>
      ) : helperText !== undefined ? (
        <p id={helperId} className="text-sm text-muted">
          {helperText}
        </p>
      ) : null}
    </div>
  );
}
