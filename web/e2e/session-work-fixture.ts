// Native transcripts and a repository-local provider/GitHub fixture. The
// daemon still reads, judges, persists and queries through its production workers.
import fs from "node:fs";
import path from "node:path";
import { execFileSync } from "node:child_process";
import { declareParent, labelAgent, type FixtureLabel, type HerdrFixture } from "./herdr-fixture";
import { fixtureProgram } from "./platform-fixture";

export const FIRST_GOAL = "세션 기록 화면과 결과물 연결 구현";
export const NEXT_GOAL = "놓친 일 이슈 생성과 검색 검증";
export const CHILD_GOAL = "자식 세션 결과물 연결 검증";
export const MISSED = "좁은 열에서 긴 한국어 요청의 줄바꿈을 확인해야 합니다";

export function workFixture(herdr: HerdrFixture, uncertainCreate = false) {
  const cwd = path.join(herdr.root, "fixture");
  const git = (args: string[]) => execFileSync("git", ["-c", "user.name=e2e", "-c", "user.email=e2e@example.invalid", "-c", "commit.gpgsign=false", ...args], { cwd });
  // Match the GitHub fixture's head branch on every system, regardless of
  // the account's init.defaultBranch; its open PR keeps the session active.
  git(["init", "--initial-branch=main"]);
  fs.writeFileSync(path.join(cwd, "README.md"), "# fixture\n");
  git(["add", "README.md"]);
  git(["commit", "-m", "initial"]);
  git(["remote", "add", "origin", "https://github.com/acme/repo.git"]);
  const bin = path.join(herdr.root, "gh-bin");
  const calls = path.join(herdr.root, "gh-calls.jsonl");
  const issue = path.join(herdr.root, "created-issue.json");
  const repositoryUnavailable = path.join(herdr.root, "repository-unavailable");
  const clock = path.join(herdr.root, "work-clock.json");
  fs.writeFileSync(clock, JSON.stringify({ first: Date.now(), next: Date.now() }));
  fixtureProgram(bin, "gh", `
const fs = require("node:fs");
const args = process.argv.slice(2);
const calls = ${JSON.stringify(calls)}, issueFile = ${JSON.stringify(issue)};
fs.appendFileSync(calls, JSON.stringify(args) + "\\n");
const out = v => process.stdout.write(JSON.stringify(v) + "\\n");
const clock = JSON.parse(fs.readFileSync(${JSON.stringify(clock)}, "utf8"));
const created = fs.existsSync(issueFile) ? JSON.parse(fs.readFileSync(issueFile, "utf8")) : null;
const pull = (number, title, merged, failed, at) => ({number,title,state:merged?"MERGED":"OPEN",url:"https://github.com/acme/repo/pull/"+number,headRefName:"main",headRefOid:"1".repeat(40),isCrossRepository:false,baseRefName:"main",reviewDecision:"REVIEW_REQUIRED",isDraft:false,mergedAt:merged?new Date(at+1000).toISOString():null,closedAt:null,createdAt:new Date(at+500).toISOString(),updatedAt:new Date(at+1000).toISOString(),closingIssuesReferences:[],statusCheckRollup:[{__typename:"CheckRun",name:"verify",status:"COMPLETED",conclusion:failed?"FAILURE":"SUCCESS"}]});
const pulls = [pull(31,"기록 화면 구현",true,false,clock.first),pull(32,"이슈와 검색 검증",false,true,clock.next),pull(34,"자식 연결 검증",true,false,clock.first+200)];
const tracked = {number:41,title:"기록 화면 후속 검증",state:"OPEN",url:"https://github.com/acme/repo/issues/41",createdAt:new Date(clock.first+500).toISOString(),updatedAt:new Date(clock.first+1000).toISOString(),labels:[]};
const [a,b] = args;
if(a==="auth"&&b==="status") process.exit(0);
if(a==="api"&&b==="user") {process.stdout.write("example-user\\n");process.exit(0);}
if (process.argv.slice(2).join(" ").includes("review-requested:@me")) { process.stdout.write("[]\\n"); process.exit(0); }
if (process.argv.slice(2).join(" ").includes("HidePrFacts")) { process.stdout.write('{"data":{"repository":{"pullRequests":{"nodes":[]}}}}\\n'); process.exit(0); }
if(a==="repo"&&b==="view") {if(fs.existsSync(${JSON.stringify(repositoryUnavailable)})) {process.stderr.write("fixture GitHub unavailable\\n");process.exit(1);} out({nameWithOwner:"acme/repo",id:"R_fixture"});process.exit(0);}
if(a==="pr"&&b==="list") {out(pulls.filter(p=>args.includes("all")|| (args.includes("merged")?p.state==="MERGED":p.state==="OPEN")));process.exit(0);}
const open=created&&created.state==="OPEN"?[created]:[];
if(a==="issue"&&b==="list") {out([tracked,...open]);process.exit(0);}
if(a==="issue"&&b==="close") {if(created){created.state="CLOSED";fs.writeFileSync(issueFile,JSON.stringify(created));}process.exit(0);}
if(a==="issue"&&b==="create") {
 const title=args[args.indexOf("--title")+1];
 const value={number:77,title,url:"https://github.com/acme/repo/issues/77",state:"OPEN",createdAt:new Date().toISOString(),updatedAt:new Date().toISOString(),labels:[]};
 fs.writeFileSync(issueFile,JSON.stringify(value));
 if(${JSON.stringify(uncertainCreate)}) {process.stderr.write("fixture response lost\\n");process.exit(1);}
 process.stdout.write(value.url+"\\n");process.exit(0);
}
if(a==="api"&&b==="graphql") {
 const query=args.join(" ");
 if(query.includes("states:[OPEN,CLOSED]")) out({data:{repository:{issues:{nodes:created?[created]:[],pageInfo:{hasNextPage:false}}}}});
 else if(query.includes("issues(first:100")) out({data:{repository:{issues:{nodes:open,pageInfo:{hasNextPage:false}}}}});
 else if(query.includes("issue(number:")) out({data:{r0:{issue:query.includes("77")?created:tracked}}});
 else out({data:{}});
 process.exit(0);
}
if(a==="search") {out([]);process.exit(0);}
process.stderr.write("unsupported fixture gh arguments\\n");process.exit(1);
`);
  const env = { PATH: `${bin}${path.delimiter}${herdr.fixturePath}` };
  const setClock = (first: number, next: number) => fs.writeFileSync(clock, JSON.stringify({ first, next }));
  return { env, calls, issue, repositoryUnavailable, clock: setClock };
}

export function seedFirstWork(herdr: HerdrFixture, at: number) {
  const session = labelAgent(herdr, herdr.panes[0], {
    task: FIRST_GOAL, at, end: "done", toolOutputs: ["https://github.com/acme/repo/pull/31", "https://github.com/acme/repo/issues/41"],
    work: { task: { kind: "new", name: "기록 화면 구현" }, missed: [{ kind: "follow_up", text: MISSED }], resolved: [], output_task_id: null },
  });
  const child = labelAgent(herdr, herdr.panes[1], {
    task: CHILD_GOAL, at: at + 200, end: "done", toolOutputs: ["https://github.com/acme/repo/pull/34"],
    work: { task: { kind: "new", name: "자식 연결 검증" }, missed: [], resolved: [], output_task_id: null },
  });
  declareParent(herdr, herdr.panes[1], herdr.panes[0]);
  return { session, child };
}

export function nextWork(at: number): FixtureLabel {
  return { task: NEXT_GOAL, at, end: "unfinished", toolOutputs: ["https://github.com/acme/repo/pull/32"],
    work: { task: { kind: "new", name: "이슈와 검색 검증" }, missed: [{ kind: "problem", text: "다른 저장소의 이슈 템플릿이 비어 있어 따로 고쳐야 합니다" }], resolved: [], output_task_id: null } };
}
