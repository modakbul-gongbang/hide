import js from "@eslint/js";
import tseslint from "typescript-eslint";
import globals from "./eslint.globals.mjs";
import e2e from "../web/eslint.e2e.mjs";

export default tseslint.config(js.configs.recommended, ...tseslint.configs.recommended, globals, e2e, {
  ignores: ["dist/**", "out/**"],
});
