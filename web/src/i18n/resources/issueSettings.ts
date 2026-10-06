import type { Catalog, Catalogs } from "../schema";

export const issueSettingsEnglish = {
  "issueSettings.local": "Local",
  "issueSettings.closesInstruction": "Link the issue in pull requests",
  "issueSettings.auto": "Automatic",
  "issueSettings.autoResolved": "Automatic ({{source}})",
  "issueSettings.githubRepository": "GitHub · {{repository}}",
  "issueSettings.ghNotInstalled": "gh isn't installed",
  "issueSettings.ghNotLoggedIn": "Not signed in to gh",
  "issueSettings.readFailed": "Couldn't read",
  "issueSettings.sourceMenu": "Issue source",
} as const;

const ko = {
  "issueSettings.local": "Local",
  "issueSettings.closesInstruction": "PR에 이슈 연결",
  "issueSettings.auto": "자동",
  "issueSettings.autoResolved": "자동 ({{source}})",
  "issueSettings.githubRepository": "GitHub · {{repository}}",
  "issueSettings.ghNotInstalled": "gh 설치 안 됨",
  "issueSettings.ghNotLoggedIn": "gh 로그인 안 됨",
  "issueSettings.readFailed": "읽기 실패",
  "issueSettings.sourceMenu": "이슈 출처",
} satisfies Catalog<typeof issueSettingsEnglish>;

const zhCN = {
  "issueSettings.local": "本地",
  "issueSettings.closesInstruction": "在拉取请求中关联议题",
  "issueSettings.auto": "自动",
  "issueSettings.autoResolved": "自动（{{source}}）",
  "issueSettings.githubRepository": "GitHub · {{repository}}",
  "issueSettings.ghNotInstalled": "未安装 gh",
  "issueSettings.ghNotLoggedIn": "未登录 gh",
  "issueSettings.readFailed": "读取失败",
  "issueSettings.sourceMenu": "议题来源",
} satisfies Catalog<typeof issueSettingsEnglish>;

const ja = {
  "issueSettings.local": "ローカル",
  "issueSettings.closesInstruction": "PRにIssueをリンク",
  "issueSettings.auto": "自動",
  "issueSettings.autoResolved": "自動（{{source}}）",
  "issueSettings.githubRepository": "GitHub · {{repository}}",
  "issueSettings.ghNotInstalled": "ghが未インストール",
  "issueSettings.ghNotLoggedIn": "ghに未ログイン",
  "issueSettings.readFailed": "読み込み失敗",
  "issueSettings.sourceMenu": "Issueの保存先",
} satisfies Catalog<typeof issueSettingsEnglish>;

export const issueSettingsCatalogs = { en: issueSettingsEnglish, ko, "zh-CN": zhCN, ja } satisfies Catalogs<typeof issueSettingsEnglish>;
