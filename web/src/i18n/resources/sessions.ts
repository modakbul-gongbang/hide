import type { Catalog, Catalogs } from "../schema";

// Source paths, branches, session titles, snippets and conversation text are caller data.

export const sessionsEnglish = {
  "sessions.project": "Sessions of {{project}}",
  "sessions.days_one": "{{count}} day",
  "sessions.days_other": "{{count}} days",
  "sessions.count_one": "{{count}} session",
  "sessions.count_other": "{{count}} sessions",
  "sessions.filtered_one": "{{shown}} of {{count}} session",
  "sessions.filtered_other": "{{shown}} of {{count}} sessions",
  "sessions.copyFailed": "Could not copy",
} as const;

const ko = {
  "sessions.project": "{{project}}의 세션",
  "sessions.days_one": "{{count}}일",
  "sessions.days_other": "{{count}}일",
  "sessions.count_one": "세션 {{count}}개",
  "sessions.count_other": "세션 {{count}}개",
  "sessions.filtered_one": "세션 {{count}}개 중 {{shown}}개",
  "sessions.filtered_other": "세션 {{count}}개 중 {{shown}}개",
  "sessions.copyFailed": "복사하지 못했습니다",
} satisfies Catalog<typeof sessionsEnglish>;

const zhCN = {
  "sessions.project": "{{project}} 的会话",
  "sessions.days_one": "{{count}} 天",
  "sessions.days_other": "{{count}} 天",
  "sessions.count_one": "{{count}} 个会话",
  "sessions.count_other": "{{count}} 个会话",
  "sessions.filtered_one": "{{count}} 个会话，显示 {{shown}} 个",
  "sessions.filtered_other": "{{count}} 个会话中显示 {{shown}} 个",
  "sessions.copyFailed": "未能复制",
} satisfies Catalog<typeof sessionsEnglish>;

const ja = {
  "sessions.project": "{{project}}のセッション",
  "sessions.days_one": "{{count}} 日",
  "sessions.days_other": "{{count}} 日",
  "sessions.count_one": "セッション {{count}} 件",
  "sessions.count_other": "セッション {{count}} 件",
  "sessions.filtered_one": "セッション {{count}} 件中 {{shown}} 件を表示",
  "sessions.filtered_other": "セッション {{count}} 件中 {{shown}} 件を表示",
  "sessions.copyFailed": "コピーできませんでした",
} satisfies Catalog<typeof sessionsEnglish>;

export const sessionsCatalogs = { en: sessionsEnglish, ko, "zh-CN": zhCN, ja } satisfies Catalogs<typeof sessionsEnglish>;
