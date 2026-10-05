import js from "@eslint/js";
import tseslint from "typescript-eslint";
import e2e from "./eslint.e2e.mjs";

export default tseslint.config(js.configs.recommended, ...tseslint.configs.recommended, e2e, {
  ignores: ["dist/**", "src/generated/**"],
});
