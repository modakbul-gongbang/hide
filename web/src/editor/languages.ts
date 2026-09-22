// Lazy language packs for the editor (PRD B4, D-07): the core names the
// language (`files::language_for`), and this maps that name to a pack loaded
// only when a document of that kind opens. A missing pack is not a failure:
// the document still edits, just without highlighting.

import type { Extension } from "@codemirror/state";
import { StreamLanguage } from "@codemirror/language";

type Loader = () => Promise<Extension>;

/** The language names the core produces, and the extension it may carry for
 * the cases a name alone cannot decide (JSX, TSX). */
const PACKS: Record<string, Loader> = {
  rust: async () => (await import("@codemirror/lang-rust")).rust(),
  json: async () => (await import("@codemirror/lang-json")).json(),
  jsonc: async () => (await import("@codemirror/lang-json")).json(),
  jsonl: async () => (await import("@codemirror/lang-json")).json(),
  markdown: async () => (await import("@codemirror/lang-markdown")).markdown(),
  python: async () => (await import("@codemirror/lang-python")).python(),
  html: async () => (await import("@codemirror/lang-html")).html(),
  htm: async () => (await import("@codemirror/lang-html")).html(),
  css: async () => (await import("@codemirror/lang-css")).css(),
  scss: async () => (await import("@codemirror/lang-css")).css(),
  sass: async () => (await import("@codemirror/lang-css")).css(),
  less: async () => (await import("@codemirror/lang-css")).css(),
  yaml: async () => (await import("@codemirror/lang-yaml")).yaml(),
  yml: async () => (await import("@codemirror/lang-yaml")).yaml(),
  go: async () => (await import("@codemirror/lang-go")).go(),
  java: async () => (await import("@codemirror/lang-java")).java(),
  php: async () => (await import("@codemirror/lang-php")).php(),
  sql: async () => (await import("@codemirror/lang-sql")).sql(),
  xml: async () => (await import("@codemirror/lang-xml")).xml(),
  c: async () => (await import("@codemirror/lang-cpp")).cpp(),
  cc: async () => (await import("@codemirror/lang-cpp")).cpp(),
  cpp: async () => (await import("@codemirror/lang-cpp")).cpp(),
  cxx: async () => (await import("@codemirror/lang-cpp")).cpp(),
  h: async () => (await import("@codemirror/lang-cpp")).cpp(),
  hpp: async () => (await import("@codemirror/lang-cpp")).cpp(),
  shell: async () => shellMode(),
  bash: async () => shellMode(),
  zsh: async () => shellMode(),
  fish: async () => shellMode(),
};

async function shellMode(): Promise<Extension> {
  const mode = await import("@codemirror/legacy-modes/mode/shell");
  return StreamLanguage.define(mode.shell);
}

/** The modes that ship as a Lezer StreamLanguage rather than a pack. */
const STREAMS: Record<string, Loader> = {
  ini: async () => StreamLanguage.define((await import("@codemirror/legacy-modes/mode/properties")).properties),
  toml: async () => StreamLanguage.define((await import("@codemirror/legacy-modes/mode/toml")).toml),
  makefile: async () => StreamLanguage.define((await import("@codemirror/legacy-modes/mode/cmake")).cmake),
  cmake: async () => StreamLanguage.define((await import("@codemirror/legacy-modes/mode/cmake")).cmake),
  dockerfile: async () => StreamLanguage.define((await import("@codemirror/legacy-modes/mode/dockerfile")).dockerFile),
  ruby: async () => StreamLanguage.define((await import("@codemirror/legacy-modes/mode/ruby")).ruby),
};

/** The JS family needs the file's own extension: the core names `typescript`
 * for both `.ts` and `.tsx`, and only the extension says JSX is in play. */
function javascript(path: string): Loader {
  const lower = path.toLowerCase();
  const typescript = lower.endsWith(".ts") || lower.endsWith(".tsx");
  const jsx = lower.endsWith(".jsx") || lower.endsWith(".tsx");
  return async () => (await import("@codemirror/lang-javascript")).javascript({ typescript, jsx });
}

/**
 * The pack for a document, or null when none is known. The core's language
 * name decides first; a name this table does not know falls back to the file
 * extension, which covers a document whose language is a bare extension.
 */
export function languageLoader(language: string | null, path: string): Loader | null {
  const lower = path.toLowerCase();
  if (lower.endsWith(".js") || lower.endsWith(".jsx") || lower.endsWith(".ts") || lower.endsWith(".tsx") || lower.endsWith(".mjs") || lower.endsWith(".cjs")) {
    return javascript(path);
  }
  if (language) {
    const named = PACKS[language];
    if (named) return named;
    const streamed = STREAMS[language];
    if (streamed) return streamed;
  }
  return null;
}
