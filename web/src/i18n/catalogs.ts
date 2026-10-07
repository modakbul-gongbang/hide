import { commandsCatalogs, commandsEnglish } from "./resources/commands";
import { boardCatalogs, boardEnglish } from "./resources/board";
import { cleanupCatalogs, cleanupEnglish } from "./resources/cleanup";
import { commonCatalogs, commonEnglish } from "./resources/common";
import { agentPresentationCatalogs, agentPresentationEnglish } from "./resources/agentPresentation";
import { devicesCatalogs, devicesEnglish } from "./resources/devices";
import { agentsCatalogs, agentsEnglish } from "./resources/agents";
import { hideAiCatalogs, hideAiEnglish } from "./resources/hideAi";
import { issueSettingsCatalogs, issueSettingsEnglish } from "./resources/issueSettings";
import { historyCatalogs, historyEnglish } from "./resources/history";
import { explorerCatalogs, explorerEnglish } from "./resources/explorer";
import { panesCatalogs, panesEnglish } from "./resources/panes";
import { sessionsCatalogs, sessionsEnglish } from "./resources/sessions";
import { searchCatalogs, searchEnglish } from "./resources/search";
import { documentsCatalogs, documentsEnglish } from "./resources/documents";
import { editorSearchCatalogs, editorSearchEnglish } from "./resources/editorSearch";
import { shellCatalogs, shellEnglish } from "./resources/shell";
import { issuesCatalogs, issuesEnglish } from "./resources/issues";
import { mobileCatalogs, mobileEnglish } from "./resources/mobile";
import { mobileSetupCatalogs, mobileSetupEnglish } from "./resources/mobileSetup";
import { nativeCatalogs, nativeEnglish } from "./resources/native";
import { overviewCatalogs, overviewEnglish } from "./resources/overview";
import { factoryCatalogs, factoryEnglish } from "./resources/factory";
import { prWorkCatalogs, prWorkEnglish } from "./resources/prWork";
import { prListCatalogs, prListEnglish } from "./resources/prList";
import { linksCatalogs, linksEnglish } from "./resources/links";
import { requestsCatalogs, requestsEnglish } from "./resources/requests";
import { settingsCatalogs, settingsEnglish } from "./resources/settings";
import { workspaceCatalogs, workspaceEnglish } from "./resources/workspace";
import type { Catalogs } from "./schema";

export const english = {
  ...shellEnglish,
  ...editorSearchEnglish,
  ...documentsEnglish,
  ...searchEnglish,
  ...panesEnglish,
  ...explorerEnglish,
  ...historyEnglish,
  ...sessionsEnglish,
  ...agentPresentationEnglish,
  ...commonEnglish,
  ...commandsEnglish,
  ...nativeEnglish,
  ...mobileEnglish,
  ...settingsEnglish,
  ...cleanupEnglish,
  ...devicesEnglish,
  ...mobileSetupEnglish,
  ...workspaceEnglish,
  ...issueSettingsEnglish,
  ...hideAiEnglish,
  ...agentsEnglish,
  ...requestsEnglish,
  ...issuesEnglish,
  ...prWorkEnglish,
  ...boardEnglish,
  ...prListEnglish,
  ...linksEnglish,
  ...overviewEnglish,
  ...factoryEnglish,
} as const;

export type MessageKey = keyof typeof english;

export const catalogs = {
  en: english,
  ko: {
    ...shellCatalogs.ko,
    ...editorSearchCatalogs.ko,
    ...documentsCatalogs.ko,
    ...searchCatalogs.ko,
    ...panesCatalogs.ko,
    ...explorerCatalogs.ko,
    ...historyCatalogs.ko,
    ...sessionsCatalogs.ko,
    ...agentPresentationCatalogs.ko,
    ...commonCatalogs.ko,
    ...commandsCatalogs.ko,
    ...nativeCatalogs.ko,
    ...mobileCatalogs.ko,
    ...settingsCatalogs.ko,
    ...cleanupCatalogs.ko,
    ...devicesCatalogs.ko,
    ...mobileSetupCatalogs.ko,
    ...workspaceCatalogs.ko,
    ...issueSettingsCatalogs.ko,
    ...hideAiCatalogs.ko,
    ...agentsCatalogs.ko,
    ...requestsCatalogs.ko,
    ...issuesCatalogs.ko,
    ...prWorkCatalogs.ko,
    ...boardCatalogs.ko,
    ...prListCatalogs.ko,
    ...linksCatalogs.ko,
    ...overviewCatalogs.ko,
    ...factoryCatalogs.ko,
  },
  "zh-CN": {
    ...shellCatalogs["zh-CN"],
    ...editorSearchCatalogs["zh-CN"],
    ...documentsCatalogs["zh-CN"],
    ...searchCatalogs["zh-CN"],
    ...panesCatalogs["zh-CN"],
    ...explorerCatalogs["zh-CN"],
    ...historyCatalogs["zh-CN"],
    ...sessionsCatalogs["zh-CN"],
    ...agentPresentationCatalogs["zh-CN"],
    ...commonCatalogs["zh-CN"],
    ...commandsCatalogs["zh-CN"],
    ...nativeCatalogs["zh-CN"],
    ...mobileCatalogs["zh-CN"],
    ...settingsCatalogs["zh-CN"],
    ...cleanupCatalogs["zh-CN"],
    ...devicesCatalogs["zh-CN"],
    ...mobileSetupCatalogs["zh-CN"],
    ...workspaceCatalogs["zh-CN"],
    ...issueSettingsCatalogs["zh-CN"],
    ...hideAiCatalogs["zh-CN"],
    ...agentsCatalogs["zh-CN"],
    ...requestsCatalogs["zh-CN"],
    ...issuesCatalogs["zh-CN"],
    ...prWorkCatalogs["zh-CN"],
    ...boardCatalogs["zh-CN"],
    ...prListCatalogs["zh-CN"],
    ...linksCatalogs["zh-CN"],
    ...overviewCatalogs["zh-CN"],
    ...factoryCatalogs["zh-CN"],
  },
  ja: {
    ...shellCatalogs.ja,
    ...editorSearchCatalogs.ja,
    ...documentsCatalogs.ja,
    ...searchCatalogs.ja,
    ...panesCatalogs.ja,
    ...explorerCatalogs.ja,
    ...historyCatalogs.ja,
    ...sessionsCatalogs.ja,
    ...agentPresentationCatalogs.ja,
    ...commonCatalogs.ja,
    ...commandsCatalogs.ja,
    ...nativeCatalogs.ja,
    ...mobileCatalogs.ja,
    ...settingsCatalogs.ja,
    ...cleanupCatalogs.ja,
    ...devicesCatalogs.ja,
    ...mobileSetupCatalogs.ja,
    ...workspaceCatalogs.ja,
    ...issueSettingsCatalogs.ja,
    ...hideAiCatalogs.ja,
    ...agentsCatalogs.ja,
    ...requestsCatalogs.ja,
    ...issuesCatalogs.ja,
    ...prWorkCatalogs.ja,
    ...boardCatalogs.ja,
    ...prListCatalogs.ja,
    ...linksCatalogs.ja,
    ...overviewCatalogs.ja,
    ...factoryCatalogs.ja,
  },
} satisfies Catalogs<typeof english>;
