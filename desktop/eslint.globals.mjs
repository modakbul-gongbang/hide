// The build script and the status page run as plain JavaScript in Node and
// in the page; declare their globals rather than disabling the rule. The e2e
// focus guard is CommonJS because Electron preloads it with `-r`.
export default [
  {
    files: ["scripts/**/*.mjs"],
    languageOptions: { globals: { process: "readonly", console: "readonly", fetch: "readonly", AbortSignal: "readonly" } },
  },
  {
    files: ["e2e/**/*.cjs"],
    languageOptions: { sourceType: "commonjs", globals: { require: "readonly", process: "readonly" } },
    rules: { "@typescript-eslint/no-require-imports": "off" },
  },
  {
    files: ["static/**/*.js"],
    languageOptions: { globals: { document: "readonly", location: "readonly", window: "readonly", URLSearchParams: "readonly" } },
  },
];
