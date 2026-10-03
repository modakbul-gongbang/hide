import { commandsCatalogs, commandsEnglish } from "./resources/commands";
import { boardCatalogs, boardEnglish } from "./resources/board";
import { cleanupCatalogs, cleanupEnglish } from "./resources/cleanup";
import { commonCatalogs, commonEnglish } from "./resources/common";
import { devicesCatalogs, devicesEnglish } from "./resources/devices";
import { issueSettingsCatalogs, issueSettingsEnglish } from "./resources/issueSettings";
import { issuesCatalogs, issuesEnglish } from "./resources/issues";
import { mobileCatalogs, mobileEnglish } from "./resources/mobile";
import { mobileSetupCatalogs, mobileSetupEnglish } from "./resources/mobileSetup";
import { nativeCatalogs, nativeEnglish } from "./resources/native";
import { prWorkCatalogs, prWorkEnglish } from "./resources/prWork";
import { prListCatalogs, prListEnglish } from "./resources/prList";
import { requestsCatalogs, requestsEnglish } from "./resources/requests";
import { settingsCatalogs, settingsEnglish } from "./resources/settings";
import { workspaceCatalogs, workspaceEnglish } from "./resources/workspace";
import type { Catalogs } from "./schema";

export const english = {
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
  ...requestsEnglish,
  ...issuesEnglish,
  ...prWorkEnglish,
  ...boardEnglish,
  ...prListEnglish,
} as const;

export type MessageKey = keyof typeof english;

export const catalogs = {
  en: english,
  ko: {
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
    ...requestsCatalogs.ko,
    ...issuesCatalogs.ko,
    ...prWorkCatalogs.ko,
    ...boardCatalogs.ko,
    ...prListCatalogs.ko,
  },
  "zh-CN": {
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
    ...requestsCatalogs["zh-CN"],
    ...issuesCatalogs["zh-CN"],
    ...prWorkCatalogs["zh-CN"],
    ...boardCatalogs["zh-CN"],
    ...prListCatalogs["zh-CN"],
  },
  ja: {
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
    ...requestsCatalogs.ja,
    ...issuesCatalogs.ja,
    ...prWorkCatalogs.ja,
    ...boardCatalogs.ja,
    ...prListCatalogs.ja,
  },
} satisfies Catalogs<typeof english>;
