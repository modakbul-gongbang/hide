import js from "@eslint/js";
import tseslint from "typescript-eslint";
import globals from "./eslint.globals.mjs";

export default tseslint.config(js.configs.recommended, ...tseslint.configs.recommended, globals, {
  ignores: ["dist/**", "out/**"],
});
