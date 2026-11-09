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
  reactHooks.configs.flat["recommended-latest"],
  jsxA11y.flatConfigs.recommended,
  {
    // Empty interfaces are deliberate extension points (e.g. ShopCoreEvents:
    // event payload types land with the specs that emit them).
    rules: { "@typescript-eslint/no-empty-object-type": ["error", { allowInterfaces: "always" }] },
  },
  { ignores: ["dist", "node_modules"] },
);
