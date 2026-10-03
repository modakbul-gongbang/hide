import { commandsCatalogs, commandsEnglish } from "./resources/commands";
import { commonCatalogs, commonEnglish } from "./resources/common";
import { mobileCatalogs, mobileEnglish } from "./resources/mobile";
import { nativeCatalogs, nativeEnglish } from "./resources/native";
import type { Catalogs } from "./schema";

export const english = {
  ...commonEnglish,
  ...commandsEnglish,
  ...nativeEnglish,
  ...mobileEnglish,
} as const;

export type MessageKey = keyof typeof english;

export const catalogs = {
  en: english,
  ko: { ...commonCatalogs.ko, ...commandsCatalogs.ko, ...nativeCatalogs.ko, ...mobileCatalogs.ko },
  "zh-CN": { ...commonCatalogs["zh-CN"], ...commandsCatalogs["zh-CN"], ...nativeCatalogs["zh-CN"], ...mobileCatalogs["zh-CN"] },
  ja: { ...commonCatalogs.ja, ...commandsCatalogs.ja, ...nativeCatalogs.ja, ...mobileCatalogs.ja },
} satisfies Catalogs<typeof english>;
