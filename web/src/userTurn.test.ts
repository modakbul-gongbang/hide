import fs from "node:fs";
import path from "node:path";
import { expect, it } from "vitest";
import { USER_TURN_KINDS } from "./snapshot";

it("types every native user-turn kind emitted by the snapshot contract", () => {
  const contract = JSON.parse(fs.readFileSync(path.resolve(__dirname, "../../contracts/snapshot-wire-enums.json"), "utf8"));
  expect(USER_TURN_KINDS).toEqual(contract.user_turn_kind);
});
