/**
 * @shopcore/ui-kit — design tokens and registry components.
 * Spec 000: StatusPill only. Spec 000b adds Button, IconButton, TextField,
 * SettingsStore and useMotionTier.
 * Tokens live in tokens.css (verbatim copy from context/10-ui-system.md,
 * plus the --border-input token added by spec 000b).
 */
export { StatusPill } from "./StatusPill";
export type { StatusPillProps, StatusPillStatus } from "./StatusPill";
export { Button, IconButton } from "./Button";
export type { ButtonProps, ButtonSize, ButtonVariant, IconButtonProps } from "./Button";
export { TextField } from "./TextField";
export type { TextFieldProps } from "./TextField";
export {
  createMemorySettingsStore,
  localStorageSettingsStore,
} from "./SettingsStore";
export type {
  MotionOverride,
  MotionTierValue,
  SettingsStore,
  ThemeSetting,
} from "./SettingsStore";
export { useMotionTier } from "./useMotionTier";
export type { MotionTier, MotionTierDeps } from "./useMotionTier";
