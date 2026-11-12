/**
 * NumPad (spec 001c-2): the tender keypad for the payment panel.
 * Digits, `00`, decimal point, clear, backspace. Purely presentational
 * — the parent owns the tender string and decides what each key does.
 * Every key is a real button (keyboard- and touch-operable).
 */
import type { ReactElement } from "react";

export type NumPadKey = "0" | "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "00" | "." | "C" | "⌫";

const KEYS: NumPadKey[] = ["1", "2", "3", "4", "5", "6", "7", "8", "9", "00", "0", ".", "C", "⌫"];

export interface NumPadProps {
  onKey: (key: NumPadKey) => void;
  disabled?: boolean;
}

export function NumPad({ onKey, disabled = false }: NumPadProps): ReactElement {
  return (
    <div className="grid grid-cols-3 gap-2" role="group" aria-label="Number pad">
      {KEYS.map((key) => (
        <button
          key={key}
          type="button"
          disabled={disabled}
          onClick={() => {
            onKey(key);
          }}
          className="h-12 rounded-md border border-border-strong bg-surface text-lg font-medium tabular-nums outline-none hover:bg-surface-subtle focus-visible:ring-2 focus-visible:ring-focus-ring disabled:cursor-not-allowed disabled:opacity-50"
        >
          {key}
        </button>
      ))}
    </div>
  );
}
