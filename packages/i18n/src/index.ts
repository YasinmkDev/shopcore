/**
 * @shopcore/i18n — translations.
 * Spec 000: English only. All user-visible strings go through keys
 * (AGENTS.md hard rule); more locales land with the Phase 3 i18n spec.
 */
import en from "./en.json";

export const messages = { en } as const;
export type Locale = keyof typeof messages;
export type MessageKey = keyof (typeof messages)["en"];

export function t(key: MessageKey, locale: Locale = "en"): string {
  return messages[locale][key];
}
