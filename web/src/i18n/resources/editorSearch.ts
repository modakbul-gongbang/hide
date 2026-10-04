import type { Catalog, Catalogs } from "../schema";

export const editorSearchEnglish = {
  "editorSearch.replace": "Replace",
  "editorSearch.next": "Next",
  "editorSearch.previous": "Previous",
  "editorSearch.all": "All",
  "editorSearch.matchCase": "Match case",
  "editorSearch.regexp": "Regular expression",
  "editorSearch.byWord": "Whole word",
  "editorSearch.replaceAll": "Replace all",
  "editorSearch.goToLine": "Go to line",
  "editorSearch.go": "Go",
  "editorSearch.currentMatch": "Current match",
  "editorSearch.onLine": "On line",
  "editorSearch.controlCharacter": "Control character",
  "editorSearch.replacedMatch": "Replaced match on line {{line}}",
  "editorSearch.replacedMatches": "Matches replaced: {{matches}}",
} as const;

const ko = {
  "editorSearch.replace": "바꾸기",
  "editorSearch.next": "다음",
  "editorSearch.previous": "이전",
  "editorSearch.all": "모두",
  "editorSearch.matchCase": "대소문자 구분",
  "editorSearch.regexp": "정규식",
  "editorSearch.byWord": "단어 단위",
  "editorSearch.replaceAll": "모두 바꾸기",
  "editorSearch.goToLine": "줄로 이동",
  "editorSearch.go": "이동",
  "editorSearch.currentMatch": "현재 일치 항목",
  "editorSearch.onLine": "줄 번호",
  "editorSearch.controlCharacter": "제어 문자",
  "editorSearch.replacedMatch": "{{line}}번째 줄의 일치 항목을 바꿨습니다",
  "editorSearch.replacedMatches": "바꾼 항목 수: {{matches}}",
} satisfies Catalog<typeof editorSearchEnglish>;

const zhCN = {
  "editorSearch.replace": "替换",
  "editorSearch.next": "下一个",
  "editorSearch.previous": "上一个",
  "editorSearch.all": "全部",
  "editorSearch.matchCase": "区分大小写",
  "editorSearch.regexp": "正则表达式",
  "editorSearch.byWord": "全词匹配",
  "editorSearch.replaceAll": "全部替换",
  "editorSearch.goToLine": "转到行",
  "editorSearch.go": "转到",
  "editorSearch.currentMatch": "当前匹配项",
  "editorSearch.onLine": "行号",
  "editorSearch.controlCharacter": "控制字符",
  "editorSearch.replacedMatch": "已替换第 {{line}} 行的匹配项",
  "editorSearch.replacedMatches": "已替换的匹配项数：{{matches}}",
} satisfies Catalog<typeof editorSearchEnglish>;

const ja = {
  "editorSearch.replace": "置換",
  "editorSearch.next": "次へ",
  "editorSearch.previous": "前へ",
  "editorSearch.all": "すべて",
  "editorSearch.matchCase": "大文字と小文字を区別",
  "editorSearch.regexp": "正規表現",
  "editorSearch.byWord": "単語全体",
  "editorSearch.replaceAll": "すべて置換",
  "editorSearch.goToLine": "行に移動",
  "editorSearch.go": "移動",
  "editorSearch.currentMatch": "現在の一致箇所",
  "editorSearch.onLine": "行番号",
  "editorSearch.controlCharacter": "制御文字",
  "editorSearch.replacedMatch": "{{line}} 行目の一致箇所を置換しました",
  "editorSearch.replacedMatches": "置換した一致箇所の数: {{matches}}",
} satisfies Catalog<typeof editorSearchEnglish>;

export const editorSearchCatalogs = { en: editorSearchEnglish, ko, "zh-CN": zhCN, ja } satisfies Catalogs<typeof editorSearchEnglish>;
