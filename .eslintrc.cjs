/** ESLint 8 legacy config (not flat config) — required because the "lint"
 * script in package.json uses `--ext ts,tsx`, which ESLint 8's flat-config
 * mode does not support (it errors on `--ext` unless legacy interop is
 * force-enabled). This is auto-detected without any flag, matching what the
 * installed eslint@^8.57.0 + the existing npm script actually expect.
 *
 * `npm run lint` (used in CI) runs with `--max-warnings 0` — a warning fails
 * the build, the same as an error. This was briefly dropped from the script
 * (050-Gold-Standard-Review.md F4: the gate was satisfied by lowering it
 * rather than by fixing the ~60 real `@typescript-eslint/no-explicit-any`
 * warnings that had accumulated, mostly at the JSON-RPC client boundary in
 * `src/lib/api.ts`). Restored, with those warnings fixed for real — either
 * typed properly or, where the JSON-RPC surface's response shape genuinely
 * isn't modeled per-method, routed through the single named, justified
 * `RpcValue` alias in `src/lib/api.ts` rather than a bare `any`. Keep it at
 * zero: a new `any` here should be a deliberate, visible choice (that alias,
 * or a scoped `eslint-disable-next-line` with a reason), never an
 * accumulating warning count CI silently tolerates.
 */
module.exports = {
  root: true,
  env: { browser: true, es2021: true, node: true },
  parser: "@typescript-eslint/parser",
  parserOptions: {
    ecmaVersion: "latest",
    sourceType: "module",
    ecmaFeatures: { jsx: true },
  },
  settings: {
    react: { version: "detect" },
  },
  extends: [
    "eslint:recommended",
    "plugin:@typescript-eslint/recommended",
    "plugin:react/recommended",
    "plugin:react-hooks/recommended",
  ],
  plugins: ["@typescript-eslint", "react", "react-hooks"],
  ignorePatterns: [
    "dist",
    "build",
    "target",
    "node_modules",
    "src-tauri",
    "*.config.js",
    "*.config.ts",
    "*.cjs",
    "playwright.config.ts",
  ],
  rules: {
    // React 17+ JSX transform (tsconfig.json: "jsx": "react-jsx") — no need
    // for `import React` in every file, and PropTypes are superseded by
    // TypeScript's own type checking throughout this codebase.
    "react/react-in-jsx-scope": "off",
    "react/prop-types": "off",

    // TS's own compiler already catches undefined/unused types; the ESLint
    // recommended rule duplicates and sometimes false-positives on generics.
    "no-undef": "off",

    // Matches tsconfig.json's own noUnusedLocals/noUnusedParameters: off,
    // since this codebase intentionally keeps some destructured-but-unused
    // parameters (event handler signatures, etc.) — flip both on together if
    // that tsconfig setting ever changes.
    "@typescript-eslint/no-unused-vars": [
      "warn",
      { argsIgnorePattern: "^_", varsIgnorePattern: "^_" },
    ],

    // `any` shows up at real integration boundaries in this codebase (JSON-RPC
    // payloads whose shape isn't fully typed yet) — a warning keeps it visible
    // without blocking CI on every occurrence already accepted elsewhere.
    "@typescript-eslint/no-explicit-any": "warn",
  },
  overrides: [
    {
      // Test files import their globals explicitly from "vitest" rather than
      // relying on ambient env globals, so no special env is needed here —
      // just a relaxed `any` rule for mock/fixture data.
      files: ["*.test.ts", "*.test.tsx"],
      rules: {
        "@typescript-eslint/no-explicit-any": "off",
      },
    },
  ],
};
