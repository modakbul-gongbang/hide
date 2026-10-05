import { EditorState } from "@codemirror/state";
import type { TFunction } from "i18next";

/**
 * CodeMirror's own interface text, keyed by the English phrase it asks for.
 * Its `$1` marks where a line or a count goes.
 */
export function editorPhrases(t: TFunction<"translation">) {
  return EditorState.phrases.of({
    Find: t("documents.find"),
    Replace: t("editorSearch.replace"),
    replace: t("editorSearch.replaceButton"),
    "replace all": t("editorSearch.replaceAll"),
    next: t("editorSearch.next"),
    previous: t("editorSearch.previous"),
    all: t("editorSearch.all"),
    "match case": t("editorSearch.matchCase"),
    regexp: t("editorSearch.regexp"),
    "by word": t("editorSearch.byWord"),
    close: t("editorSearch.close"),
    "Go to line": t("editorSearch.goToLine"),
    go: t("editorSearch.go"),
    "current match": t("editorSearch.currentMatch"),
    "on line": t("editorSearch.onLine"),
    "Control character": t("editorSearch.controlCharacter"),
    "replaced match on line $": t("editorSearch.replacedMatch", { line: "$1" }),
    "replaced $ matches": t("editorSearch.replacedMatches", { matches: "$1" }),
  });
}
