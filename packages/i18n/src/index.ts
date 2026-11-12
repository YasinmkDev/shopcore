/**
 * @shopcore/i18n — translations.
 * Spec 000: English only. All user-visible strings go through keys
 * (AGENTS.md hard rule); more locales land with the Phase 3 i18n spec.
 *
 * Interpolation (spec 001c-1): values may contain `{name}` placeholders,
 * filled from the optional `vars` record — e.g.
 * `t("sell.announce.added", { qty: "2", name: "Milk 1L" })`.
 * Unknown placeholders are left as-is (never silently dropped).
 */
import en from "./en.json";

export const messages = { en } as const;
export type Locale = keyof typeof messages;
export type MessageKey = keyof (typeof messages)["en"];
export type MessageVars = Record<string, string | number>;

export function t(key: MessageKey, vars?: MessageVars, locale: Locale = "en"): string {
  const template = messages[locale][key];
  if (vars === undefined) {
    return template;
  }
  return template.replace(/\{(\w+)\}/g, (match, name: string) =>
    name in vars ? String(vars[name]) : match,
  );
}
