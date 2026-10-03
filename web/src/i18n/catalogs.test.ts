import { describe, expect, it } from "vitest";
import { catalogs, english } from "./catalogs";
import { commandsEnglish } from "./resources/commands";
import { cleanupEnglish } from "./resources/cleanup";
import { commonEnglish } from "./resources/common";
import { mobileEnglish } from "./resources/mobile";
import { nativeEnglish } from "./resources/native";
import { settingsEnglish } from "./resources/settings";
import { validateCatalogs } from "./schema";

describe("interface resources", () => {
  it("provides every key and insertion in all four languages", () => {
    expect(() => validateCatalogs(english, catalogs)).not.toThrow();
  });

  it("keeps domain keys separate so composition cannot overwrite a message", () => {
    const keys = [commonEnglish, commandsEnglish, nativeEnglish, mobileEnglish, settingsEnglish, cleanupEnglish].flatMap((schema) => Object.keys(schema));
    expect(new Set(keys).size).toBe(keys.length);
  });
});
