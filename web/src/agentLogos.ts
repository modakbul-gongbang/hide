// The logos the onboarding grid draws, by adapter id. Every file is listed in
// `assets/agents/manifest.json` with its source and licence note, and an
// agent with none draws a monogram (`scripts/check-agent-logos.mjs`).
import claudeMark from "./assets/agent-claude.png";
import codexMark from "./assets/agent-codex.png";

const bundled = import.meta.glob<string>("./assets/agents/*.svg", { eager: true, query: "?url", import: "default" });

const LOGOS: Record<string, string> = { "claude-code": claudeMark, codex: codexMark };
for (const [path, url] of Object.entries(bundled)) {
  const id = /\/([^/]+)\.svg$/.exec(path)?.[1];
  if (id) LOGOS[id] = url;
}

/** The bundled logo of an agent, or null when the tile draws a monogram. */
export function agentLogo(id: string): string | null {
  return LOGOS[id] ?? null;
}

/** Up to two letters of a name: the monogram of an agent with no logo. */
export function monogram(label: string): string {
  const words = label.split(/[\s-]+/).filter(Boolean);
  const letters = words.length > 1 ? words.slice(0, 2).map((word) => word[0]) : [...(words[0] ?? "?")].slice(0, 2);
  return letters.join("").toUpperCase();
}
