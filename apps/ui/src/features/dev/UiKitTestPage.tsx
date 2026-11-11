import type { ReactElement } from "react";
import { t } from "@shopcore/i18n";
import { Button, TextField, useMotionTier } from "@shopcore/ui-kit";
import type {
  ButtonSize,
  ButtonVariant,
  MotionOverride,
  ThemeSetting,
} from "@shopcore/ui-kit";
import { useTheme } from "../../lib/theme";

/**
 * Dev-only test page (spec 000b, req. 13): every Button/TextField variant
 * and state in both themes. NOT in shipped navigation — App renders it
 * only in dev builds at `#ui-kit-test`, via a dynamic import (the chunk
 * is dead-code-eliminated from production builds).
 *
 * The motion tier is passed in as a prop: App owns the single
 * useMotionTier() instance, so this page never starts a second sampler.
 */
export interface UiKitTestPageProps {
  motion: ReturnType<typeof useMotionTier>;
}

const VARIANT_LABELS: Record<
  ButtonVariant,
  | "dev.uikit.variant.primary"
  | "dev.uikit.variant.secondary"
  | "dev.uikit.variant.ghost"
  | "dev.uikit.variant.danger"
> = {
  primary: "dev.uikit.variant.primary",
  secondary: "dev.uikit.variant.secondary",
  ghost: "dev.uikit.variant.ghost",
  danger: "dev.uikit.variant.danger",
};

const SIZE_LABELS: Record<
  ButtonSize,
  "dev.uikit.size.sm" | "dev.uikit.size.md" | "dev.uikit.size.lg"
> = {
  sm: "dev.uikit.size.sm",
  md: "dev.uikit.size.md",
  lg: "dev.uikit.size.lg",
};

const TIER_LABELS = {
  0: "dev.uikit.motion.full",
  1: "dev.uikit.motion.reduced",
  2: "dev.uikit.motion.minimal",
} as const;

const VARIANTS: ButtonVariant[] = ["primary", "secondary", "ghost", "danger"];
const SIZES: ButtonSize[] = ["sm", "md", "lg"];

const THEME_OPTIONS: { value: ThemeSetting; labelKey: "theme.light" | "theme.dark" | "theme.auto" }[] = [
  { value: "light", labelKey: "theme.light" },
  { value: "dark", labelKey: "theme.dark" },
  { value: "auto", labelKey: "theme.auto" },
];

const MOTION_OVERRIDES: {
  value: MotionOverride | null;
  labelKey:
    | "dev.uikit.motion.auto"
    | "dev.uikit.motion.full"
    | "dev.uikit.motion.reduced"
    | "dev.uikit.motion.off";
}[] = [
  { value: null, labelKey: "dev.uikit.motion.auto" },
  { value: "full", labelKey: "dev.uikit.motion.full" },
  { value: "reduced", labelKey: "dev.uikit.motion.reduced" },
  { value: "off", labelKey: "dev.uikit.motion.off" },
];

export function UiKitTestPage({ motion }: UiKitTestPageProps): ReactElement {
  const { theme, setTheme } = useTheme();
  return (
    <div className="flex min-h-screen flex-col bg-bg px-6 py-8 text-text">
      <h1 className="mb-6 text-2xl font-semibold">{t("dev.uikit.title")}</h1>

      <section aria-label={t("dev.uikit.theme")} className="mb-8">
        <h2 className="mb-2 text-lg font-semibold">{t("dev.uikit.theme")}</h2>
        <div role="group" aria-label={t("theme.label")} className="flex items-center gap-1">
          {THEME_OPTIONS.map((option) => (
            <Button
              key={option.value}
              size="sm"
              variant={theme === option.value ? "secondary" : "ghost"}
              aria-pressed={theme === option.value}
              onClick={() => {
                setTheme(option.value);
              }}
            >
              {t(option.labelKey)}
            </Button>
          ))}
        </div>
      </section>

      <section aria-label={t("dev.uikit.motion")} className="mb-8">
        <h2 className="mb-2 text-lg font-semibold">{t("dev.uikit.motion")}</h2>
        <p className="mb-2 text-muted">
          {t("dev.uikit.motion.tier")}: {t(TIER_LABELS[motion.tier])}
        </p>
        <div role="group" aria-label={t("dev.uikit.motion")} className="flex items-center gap-1">
          {MOTION_OVERRIDES.map((option) => (
            <Button
              key={option.labelKey}
              size="sm"
              variant={motion.override === option.value ? "secondary" : "ghost"}
              aria-pressed={motion.override === option.value}
              onClick={() => {
                motion.setOverride(option.value);
              }}
            >
              {t(option.labelKey)}
            </Button>
          ))}
        </div>
      </section>

      <section aria-label={t("dev.uikit.buttons")} className="mb-8">
        <h2 className="mb-2 text-lg font-semibold">{t("dev.uikit.buttons")}</h2>
        {VARIANTS.map((variant) => (
          <div key={variant} className="mb-4">
            <h3 className="mb-2 font-medium">{t(VARIANT_LABELS[variant])}</h3>
            {SIZES.map((size) => (
              <div key={size} className="mb-2 flex flex-wrap items-center gap-2">
                <span className="w-20 text-sm text-muted">{t(SIZE_LABELS[size])}</span>
                <Button variant={variant} size={size}>
                  {t("dev.uikit.state.default")}
                </Button>
                <Button variant={variant} size={size} disabled>
                  {t("dev.uikit.state.disabled")}
                </Button>
                <Button variant={variant} size={size} loading>
                  {t("dev.uikit.state.loading")}
                </Button>
                {size === "sm" && (
                  <span className="text-sm text-muted">{t("dev.uikit.note.backoffice")}</span>
                )}
              </div>
            ))}
          </div>
        ))}
      </section>

      <section aria-label={t("dev.uikit.textfields")}>
        <h2 className="mb-2 text-lg font-semibold">{t("dev.uikit.textfields")}</h2>
        <div className="grid max-w-lg gap-4">
          <TextField label={t("dev.uikit.field.label")} />
          <TextField
            label={t("dev.uikit.field.label")}
            helperText={t("dev.uikit.field.helper")}
            placeholder={t("dev.uikit.field.placeholder")}
          />
          <TextField label={t("dev.uikit.field.required")} required />
          <TextField label={t("dev.uikit.field.disabled")} disabled />
          <TextField label={t("dev.uikit.field.label")} error={t("dev.uikit.field.error")} />
        </div>
      </section>
    </div>
  );
}
