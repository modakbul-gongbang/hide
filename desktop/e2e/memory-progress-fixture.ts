// Synthetic GitHub answers and card receipts in the fixture's private home.
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import type { HerdrFixture } from "../../web/e2e/herdr-fixture";
import { fixtureProgram } from "../../web/e2e/platform-fixture";
import type { Isolated } from "./fixture";

type FixturePull = { number: number; title: string; headRefName: string; baseRefName: string; state: string; reviewDecision: string; isDraft: boolean; statusCheckRollup: Record<string, string>[]; url: string; mergedAt: string | null; createdAt: string; updatedAt: string; closingIssuesReferences: never[] };

export function progressGithub(herdr: HerdrFixture) {
  const file = path.join(herdr.root, "memory-pulls.json");
  const pulls: FixturePull[] = [{ number: 72, title: "Existing document PR", headRefName: "fixture-shared-pr", baseRefName: "main", state: "OPEN", reviewDecision: "REVIEW_REQUIRED", isDraft: false, statusCheckRollup: [], url: "https://github.com/acme/memory-fixture/pull/72", mergedAt: null, createdAt: "2026-10-13T00:00:00Z", updatedAt: "2026-10-13T00:00:00Z", closingIssuesReferences: [] }];
  const write = () => fs.writeFileSync(file, JSON.stringify(pulls));
  write();
  fixtureProgram(path.join(herdr.root, "bin"), "gh", `
const fs = require("node:fs");
const args = process.argv.slice(2), key = args.slice(0, 2).join(" ");
const pulls = JSON.parse(fs.readFileSync(${JSON.stringify(file)}, "utf8"));
if (key === "auth status") process.exit(0);
if (key === "api user") { console.log("example-user"); process.exit(0); }
if (process.argv.slice(2).join(" ").includes("review-requested:@me")) { console.log("[]"); process.exit(0); }
if (process.argv.slice(2).join(" ").includes("HidePrFacts")) { console.log('{"data":{"repository":{"pullRequests":{"nodes":[]}}}}'); process.exit(0); }
if (key === "repo view") { console.log('{"nameWithOwner":"acme/memory-fixture"}'); process.exit(0); }
if (key === "issue list") { console.log("[]"); process.exit(0); }
if (key === "pr list") { console.log(JSON.stringify(pulls)); process.exit(0); }
if (key === "pr view") {
  const pull = pulls.find(p => String(p.number) === args[2] || p.headRefName === args[2]);
  if (pull) { console.log(JSON.stringify(pull)); process.exit(0); }
}
console.error("unsupported fixture gh command: " + args.join(" ")); process.exit(1);
`);
  return {
    open(branch: string) {
      pulls.push({ ...pulls[0]!, number: 73, title: "Promote two Memory rules", headRefName: branch, url: "https://github.com/acme/memory-fixture/pull/73", createdAt: new Date().toISOString(), statusCheckRollup: [{ __typename: "CheckRun", status: "COMPLETED", conclusion: "FAILURE", name: "verify" }] });
      write();
    },
    settle(number: number, state: "MERGED" | "CLOSED") {
      const pull = pulls.find((pull) => pull.number === number);
      if (!pull) throw new Error(`no fixture PR ${number}`);
      pull.state = state;
      if (state === "MERGED") pull.mergedAt = "2026-10-13T00:10:00Z";
      write();
    },
  };
}

export function placedProgressBatch(run: Isolated): { branch: string; pane: string } {
  const promotion = JSON.parse(execFileSync("python3", ["-c", String.raw`
import json,sqlite3,sys
db=sqlite3.connect(sys.argv[1],timeout=5)
promotion=db.execute("SELECT branch,agent_id FROM card_promotions WHERE item_id='fixture-m1' AND state='running' AND agent_id IS NOT NULL").fetchone()
if not promotion: raise RuntimeError('live promotion not reserved')
db.execute("UPDATE card_promotions SET target_path='AGENTS.md' WHERE item_id IN ('fixture-m1','fixture-m2')")
db.commit()
print(json.dumps(dict(branch=promotion[0],agent_id=promotion[1])))
`, path.join(run.env.HIDE_STATE_DIR!, "project-memory.sqlite3")], { encoding: "utf8", timeout: 10_000 })) as { branch: string; agent_id: string };
  const ledger = JSON.parse(fs.readFileSync(path.join(run.env.HIDE_STATE_DIR!, "delivery-ledger.json"), "utf8")) as { agents: { id: string; pane: string }[] };
  const agent = ledger.agents.find((agent) => agent.id === promotion.agent_id);
  if (!agent) throw new Error("The fixture's promotion has no bound ledger agent");
  return { branch: promotion.branch, pane: agent.pane };
}
