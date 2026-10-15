import type { Page } from "@playwright/test";
import { spawn, type ChildProcess } from "node:child_process";
import { stripVTControlCharacters } from "node:util";

export function openServerButton(page: Page) {
  // Expanded panels retain the covered agent toolbar in an inert subtree.
  // Role queries can still include it; only the active control can be used.
  return page.locator('[data-open-server]:not([inert] *)');
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
