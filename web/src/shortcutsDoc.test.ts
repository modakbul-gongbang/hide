import fs from "node:fs";
import path from "node:path";
import { describe, expect, it } from "vitest";
import type { HostKind } from "./host";
import { REGISTRY, displayCommand, systemRegistry, type KeySystem } from "./shortcuts";

// docs/UI_BEHAVIOR.md prints every command's chord in both hosts on both
// systems; a row the registry no longer answers to tells the operator a key
// the window does not take.
const GUIDE = fs.readFileSync(path.resolve(__dirname, "../../docs/UI_BEHAVIOR.md"), "utf8");
const SECTION = GUIDE.split(/^### Every command$/m)[1]!.split(/^## /m)[0]!;
const COLUMNS: { host: HostKind; system: KeySystem }[] = [
  { host: "electron", system: "mac" },
  { host: "electron", system: "pc" },
  { host: "browser", system: "mac" },
  { host: "browser", system: "pc" },
];

const docRows = new Map(
  SECTION.split("\n")
    .filter((line) => line.startsWith("|"))
    .slice(2)
    .map((line) => {
      const [name, ...chords] = line.split("|").slice(1, -1).map((cell) => /^(`+) ?(.*?) ?\1$/.exec(cell.trim())?.[2] ?? cell.trim());
      return [name!.replace(/ \(exception\)$/, ""), chords] as const;
    }),
);

/** The registry's side of the table: a numbered family is one row, "1 … 9" in its first and last chord. */
function registryRows(): Map<string, string[]> {
  const rows = new Map<string, string[]>();
  for (const command of REGISTRY) {
    const folded = /^(.*) [1-9]$/.exec(command.title);
    const name = folded ? `${folded[1]} 1-9` : command.title;
    const chords = COLUMNS.map(({ host, system }) => displayCommand(command.id, host, systemRegistry(system), system) || "none");
    const row = rows.get(name);
    rows.set(name, row && folded?.[0].endsWith("9") ? row.map((chord, column) => (chord === "none" ? chord : `${chord} … ${chords[column]}`)) : row ?? chords);
  }
  return rows;
}

describe("docs/UI_BEHAVIOR.md, Every command", () => {
  it("prints each command's chord as the registry shows it, in both hosts on both systems", () => {
    const table = (rows: Map<string, string[]>) => [...rows].map(([name, chords]) => `${name} | ${chords.join(" | ")}`).sort();
    expect(table(docRows)).toEqual(table(registryRows()));
  });
});
