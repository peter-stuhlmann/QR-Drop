import js from "@eslint/js";
import globals from "globals";
import jsxA11y from "eslint-plugin-jsx-a11y-x";
import reactHooks from "eslint-plugin-react-hooks";
import tseslint from "typescript-eslint";

export default tseslint.config(
  { ignores: ["dist", "src-tauri", "landing", "node_modules"] },
  {
    extends: [js.configs.recommended, ...tseslint.configs.recommended],
    files: ["**/*.{ts,tsx}"],
    languageOptions: { ecmaVersion: 2022, globals: globals.browser },
    plugins: { "react-hooks": reactHooks },
    rules: { ...reactHooks.configs.recommended.rules },
  },
  {
    // Accessibility (CLAUDE.md §7.2): labels, roles, keyboard use.
    extends: [jsxA11y.configs.strict],
    files: ["src/**/*.tsx"],
    // Checked like the element they render, e.g. an icon-only Button
    // without aria-label.
    settings: { "jsx-a11y-x": { components: { Button: "button" } } },
    rules: {
      // Off in "strict", but exactly what icon buttons need.
      "jsx-a11y-x/control-has-associated-label": "error",
      "jsx-a11y-x/no-aria-hidden-on-focusable": "error",
    },
  },
);
