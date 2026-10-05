// TEMPORARY: proves the Playwright retry and the flaky report on this pull request. Removed before merge.
import { expect, test } from "@playwright/test";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";

test("flaky demo: fails the first attempt and passes the retry", async () => {
  const marker = path.join(os.tmpdir(), "hide-flaky-demo-marker");
  if (!fs.existsSync(marker)) {
    fs.writeFileSync(marker, "ran");
    expect("first attempt").toBe("retry");
  }
});
