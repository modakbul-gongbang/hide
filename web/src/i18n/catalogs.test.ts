import { describe, expect, it } from "vitest";
import { catalogs, english } from "./catalogs";
import { commandsEnglish } from "./resources/commands";
import { boardEnglish } from "./resources/board";
import { cleanupEnglish } from "./resources/cleanup";
import { commonEnglish } from "./resources/common";
import { agentPresentationEnglish } from "./resources/agentPresentation";
import { devicesEnglish } from "./resources/devices";
import { issueSettingsEnglish } from "./resources/issueSettings";
import { historyEnglish } from "./resources/history";
import { explorerEnglish } from "./resources/explorer";
import { panesEnglish } from "./resources/panes";
import { sessionsEnglish } from "./resources/sessions";
import { searchEnglish } from "./resources/search";
import { documentsEnglish } from "./resources/documents";
import { issuesEnglish } from "./resources/issues";
import { mobileEnglish } from "./resources/mobile";
import { mobileSetupEnglish } from "./resources/mobileSetup";
import { nativeEnglish } from "./resources/native";
import { overviewEnglish } from "./resources/overview";
import { prWorkEnglish } from "./resources/prWork";
import { prListEnglish } from "./resources/prList";
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
      documentsEnglish,
      searchEnglish,
      panesEnglish,
      explorerEnglish,
      historyEnglish,
      sessionsEnglish,
      agentPresentationEnglish,
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
      issuesEnglish,
      prWorkEnglish,
      boardEnglish,
      prListEnglish,
      overviewEnglish,
    ].flatMap((schema) => Object.keys(schema));
    expect(new Set(keys).size).toBe(keys.length);
  });
});
