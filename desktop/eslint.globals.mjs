// The build script and the status page run as plain JavaScript in Node and
// in the page; declare their globals rather than disabling the rule.
export default [
  {
    files: ["scripts/**/*.mjs"],
    languageOptions: { globals: { process: "readonly", console: "readonly" } },
  },
  {
    files: ["static/**/*.js"],
    languageOptions: { globals: { document: "readonly", location: "readonly", window: "readonly" } },
  },
];
