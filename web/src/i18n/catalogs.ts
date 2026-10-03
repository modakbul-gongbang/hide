import { commandsCatalogs, commandsEnglish } from "./resources/commands";
import { cleanupCatalogs, cleanupEnglish } from "./resources/cleanup";
import { commonCatalogs, commonEnglish } from "./resources/common";
import { devicesCatalogs, devicesEnglish } from "./resources/devices";
import { mobileCatalogs, mobileEnglish } from "./resources/mobile";
import { mobileSetupCatalogs, mobileSetupEnglish } from "./resources/mobileSetup";
import { nativeCatalogs, nativeEnglish } from "./resources/native";
import { settingsCatalogs, settingsEnglish } from "./resources/settings";
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
  },
} satisfies Catalogs<typeof english>;
