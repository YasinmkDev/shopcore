/**
 * ScanInput (spec 001c-1, req. 2-3, amendment 5): a plain focused input
 * + Enter. No burst detection yet — that arrives with spec 004.
 * The parent owns the value and the submit/escape/arrow behaviour;
 * this component owns focus (always focused, refocused after every
 * action by the parent) and the input's accessible label.
 */
import { useEffect } from "react";
import type { ReactElement, RefObject } from "react";
import { t } from "@shopcore/i18n";

export interface ScanInputProps {
  value: string;
  onChange: (value: string) => void;
  /** Enter: submit the scan/search. */
  onSubmit: () => void;
  /** Escape: clear the input / close the results. */
  onEscape: () => void;
  /** Arrow keys: move the search highlight. */
  onArrowUp: () => void;
  onArrowDown: () => void;
  /** `+` / `-`: adjust the selected line's quantity by one unit. */
  onAdjustQty: (deltaMilli: number) => void;
  /** Delete: remove the selected line. */
  onDelete: () => void;
  /** Ctrl+Z: restore the last removed line. */
  onRestore: () => void;
  disabled?: boolean;
  inputRef: RefObject<HTMLInputElement | null>;
}

export function ScanInput({
  value,
  onChange,
  onSubmit,
  onEscape,
  onArrowUp,
  onArrowDown,
  onAdjustQty,
  onDelete,
  onRestore,
  disabled = false,
  inputRef,
}: ScanInputProps): ReactElement {
  // The scan input is always focused (req. 11: focus return < 100 ms).
  // Imperative focus on mount, not the autoFocus attribute (a11y).
  useEffect(() => {
    inputRef.current?.focus();
  }, [inputRef]);

  return (
    <input
      ref={inputRef}
      type="text"
      inputMode="search"
      autoComplete="off"
      aria-label={t("sell.scan.label")}
      placeholder={t("sell.scan.placeholder")}
      value={value}
      disabled={disabled}
      onChange={(event) => {
        onChange(event.target.value);
      }}
      onKeyDown={(event) => {
        if (event.key === "Enter") {
          event.preventDefault();
          onSubmit();
        } else if (event.key === "Escape") {
          event.preventDefault();
          onEscape();
        } else if (event.key === "ArrowUp") {
          event.preventDefault();
          onArrowUp();
        } else if (event.key === "ArrowDown") {
          event.preventDefault();
          onArrowDown();
        } else if (event.key === "+" || event.key === "=") {
          event.preventDefault();
          onAdjustQty(1000);
        } else if (event.key === "-" || event.key === "_") {
          event.preventDefault();
          onAdjustQty(-1000);
        } else if (event.key === "Delete") {
          event.preventDefault();
          onDelete();
        } else if (event.key.toLowerCase() === "z" && (event.ctrlKey || event.metaKey)) {
          event.preventDefault();
          onRestore();
        }
        // F9 / F6 are intentionally NOT bound in 001c-1 (amendment 2).
      }}
      className="h-12 w-full rounded-md border border-border-input bg-surface px-4 text-base text-text outline-none placeholder:text-faint focus-visible:ring-2 focus-visible:ring-focus-ring"
    />
  );
}
