import js from "@eslint/js";
import tseslint from "typescript-eslint";
import reactHooks from "eslint-plugin-react-hooks";
import jsxA11y from "eslint-plugin-jsx-a11y";

export default tseslint.config(
  js.configs.recommended,
  ...tseslint.configs.strictTypeChecked,
  ...tseslint.configs.stylisticTypeChecked,
  {
    languageOptions: {
      parserOptions: { projectService: true, tsconfigRootDir: import.meta.dirname },
    },
  },
  // The config file itself is not part of the TS program; lint it without
  // type-aware rules.
  { files: ["eslint.config.js"], ...tseslint.configs.disableTypeChecked },
  // The release-bundle verifier is a plain node script, not part of
  // the TS program either; same treatment, plus `no-undef` off — its
  // node globals (console, process, URL) are not in the browser env
  // this config assumes, and node fails at runtime on a bad name.
  {
    files: ["scripts/verify-release-bundle.mjs"],
    ...tseslint.configs.disableTypeChecked,
    // Spread last would replace `rules` wholesale (the bug that kept
    // `no-undef` on); merge instead.
    rules: { ...tseslint.configs.disableTypeChecked.rules, "no-undef": "off" },
  },
  reactHooks.configs.flat["recommended-latest"],
  jsxA11y.flatConfigs.recommended,
  {
    // Empty interfaces are deliberate extension points (e.g. ShopCoreEvents:
    // event payload types land with the specs that emit them).
    rules: { "@typescript-eslint/no-empty-object-type": ["error", { allowInterfaces: "always" }] },
  },
  { ignores: ["dist", "dist-e2e", "node_modules", "public"] },
);
