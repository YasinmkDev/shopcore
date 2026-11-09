import js from "@eslint/js";
import tseslint from "typescript-eslint";

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
  { ignores: ["node_modules"] },
);
