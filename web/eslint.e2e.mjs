// The lint every Playwright spec in web/e2e and desktop/e2e passes.
// eslint-plugin-playwright states the generic rules; hide-e2e states the two this repository was burned by.
// docs/TESTING.md owns the reasons. The size budget of one test is checked by web/scripts/check-e2e-test-size.mjs.
import playwright from "eslint-plugin-playwright";
import hide from "./eslint-rules/hide-e2e.mjs";

export default {
  files: ["e2e/**/*.ts"],
  plugins: { playwright, "hide-e2e": hide },
  rules: {
    "playwright/no-wait-for-timeout": "error",
    "playwright/no-networkidle": "error",
    "playwright/no-force-option": "error",
    "playwright/no-skipped-test": ["error", { allowConditional: true }],
    "playwright/no-focused-test": "error",
    "playwright/no-page-pause": "error",
    "playwright/no-standalone-expect": "error",
    "playwright/prefer-web-first-assertions": "error",
    "playwright/missing-playwright-await": "error",
    "playwright/no-useless-await": "error",
    "hide-e2e/no-action-in-poll": "error",
    "hide-e2e/reopen-after-restart-through-blank": "error",
  },
};
