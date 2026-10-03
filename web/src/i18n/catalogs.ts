import { commandsCatalogs, commandsEnglish } from "./resources/commands";
import { cleanupCatalogs, cleanupEnglish } from "./resources/cleanup";
import { commonCatalogs, commonEnglish } from "./resources/common";
import { mobileCatalogs, mobileEnglish } from "./resources/mobile";
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
} as const;

export type MessageKey = keyof typeof english;

export const catalogs = {
  en: english,
  ko: { ...commonCatalogs.ko, ...commandsCatalogs.ko, ...nativeCatalogs.ko, ...mobileCatalogs.ko, ...settingsCatalogs.ko, ...cleanupCatalogs.ko },
  "zh-CN": { ...commonCatalogs["zh-CN"], ...commandsCatalogs["zh-CN"], ...nativeCatalogs["zh-CN"], ...mobileCatalogs["zh-CN"], ...settingsCatalogs["zh-CN"], ...cleanupCatalogs["zh-CN"] },
  ja: { ...commonCatalogs.ja, ...commandsCatalogs.ja, ...nativeCatalogs.ja, ...mobileCatalogs.ja, ...settingsCatalogs.ja, ...cleanupCatalogs.ja },
} satisfies Catalogs<typeof english>;
