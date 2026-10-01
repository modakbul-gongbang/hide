import { expect, type Locator, type Page } from "@playwright/test";
import { spawn, type ChildProcess } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { stripVTControlCharacters } from "node:util";

export function openServerButton(page: Page) {
  // Expanded panels retain the covered agent toolbar in an inert subtree.
  // Role queries can still include it; only the active control can be used.
  return page.locator('[data-open-server]:not([inert] *)');
}

export function writeConversation(home: string, cwd: string, id = "search-session"): string {
  const directory = path.join(home, ".codex/sessions/2026/10/01");
  fs.mkdirSync(directory, { recursive: true });
  const at = new Date().toISOString();
  const message = (role: string, text: string) => ({ type: "response_item", timestamp: at, payload: { type: "message", role, content: [{ type: role === "user" ? "input_text" : "output_text", text }] } });
  const lines = [{ type: "session_meta", payload: { id, cwd } }, message("user", "Review conversation history")];
  for (let i = 0; i < 30; i++) lines.push(message("assistant", `Earlier answer ${i}\n${"Readable conversation context.\n".repeat(6)}`));
  lines.push(message("assistant", "대화검색으로 로그인 연결을 확인했습니다. foo_bar OR \"(literal)*\" 도 그대로 찾습니다."));
  for (let i = 0; i < 30; i++) lines.push(message("assistant", `Later answer ${i}\n${"Keep reading after the match.\n".repeat(6)}`));
  const file = path.join(directory, `rollout-${id}.jsonl`);
  fs.writeFileSync(file, lines.map((line) => JSON.stringify(line)).join("\n") + "\n");
  return file;
}
export async function startServer(cwd: string, host = "127.0.0.1", requestedPort = 0): Promise<{ child: ChildProcess; port: number }> {
  const child = spawn(process.execPath, ["-e", "const http=require('node:http'); const s=http.createServer((q,r)=>{r.writeHead(200,{'content-type':'text/html; charset=utf-8'});r.end('<title>Workspace preview</title><h1>Workspace preview</h1>');});s.listen(Number(process.argv[1]),process.argv[2],()=>console.log(s.address().port));", String(requestedPort), host], { cwd, stdio: ["ignore", "pipe", "pipe"] });
  const port = await new Promise<number>((resolve, reject) => {
    const timeout = setTimeout(() => { child.kill(); reject(new Error("fixture listener did not start")); }, 5000);
    let buffer = "";
    child.stdout?.on("data", (data) => { buffer += stripVTControlCharacters(String(data)); const line = /^([0-9]{1,5})$/m.exec(buffer); if (line) { clearTimeout(timeout); resolve(Number(line[1])); } });
    child.once("error", (error) => { clearTimeout(timeout); reject(error); });
    child.once("exit", (code) => { clearTimeout(timeout); reject(new Error(`server exited ${code}`)); });
  });
  return { child, port };
}
export async function openSessions(page: Page, overview: Locator = page.locator("[data-go-overview]")): Promise<void> {
  await expect(overview).toHaveCount(1);
  await expect(overview).toBeVisible();
  const projectId = await overview.getAttribute("data-project-overview") ?? await overview.getAttribute("data-go-overview");
  expect(projectId).toBeTruthy();
  await overview.click();
  await expect(page.locator("[data-overview-screen]")).toHaveAttribute("data-overview-screen", projectId!);
  await page.locator('[data-lens-tile-button="sessions"]').click();
  await expect(page.locator("[data-sessions-screen]")).toBeVisible();
}
