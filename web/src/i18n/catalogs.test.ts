import { describe, expect, it } from "vitest";
import { catalogs, english } from "./catalogs";
import { commandsEnglish } from "./resources/commands";
import { cleanupEnglish } from "./resources/cleanup";
import { commonEnglish } from "./resources/common";
import { devicesEnglish } from "./resources/devices";
import { issueSettingsEnglish } from "./resources/issueSettings";
import { mobileEnglish } from "./resources/mobile";
import { mobileSetupEnglish } from "./resources/mobileSetup";
import { nativeEnglish } from "./resources/native";
import { requestsEnglish } from "./resources/requests";
import { settingsEnglish } from "./resources/settings";
import { workspaceEnglish } from "./resources/workspace";
import { validateCatalogs } from "./schema";

describe("interface resources", () => {
  it("provides every key and insertion in all four languages", () => {
    expect(() => validateCatalogs(english, catalogs)).not.toThrow();
  });

  it("keeps domain keys separate so composition cannot overwrite a message", () => {
    const keys = [
      commonEnglish,
      commandsEnglish,
      nativeEnglish,
      mobileEnglish,
      settingsEnglish,
      cleanupEnglish,
      devicesEnglish,
      mobileSetupEnglish,
      workspaceEnglish,
      issueSettingsEnglish,
      requestsEnglish,
    ].flatMap((schema) => Object.keys(schema));
    expect(new Set(keys).size).toBe(keys.length);
  });
});
