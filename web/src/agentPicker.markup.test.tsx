import { renderToStaticMarkup } from "react-dom/server";
import { beforeEach, describe, expect, it } from "vitest";
import { catalogFor, forgetCatalogs } from "./agentPicker";
import { AgentPickerView } from "./components/agent-picker";
import { TooltipProvider } from "./components/ui/tooltip";
import type { BackgroundAi } from "./snapshot";

let ai: BackgroundAi | undefined;

function draw(kind: "claude" | "codex" | "terminal", model: string | null, withTerminal = false) {
  return renderToStaticMarkup(
    <TooltipProvider>
      <AgentPickerView value={{ kind, model }} catalog={kind === "terminal" ? null : catalogFor(ai, kind)} onKind={() => undefined} onModel={() => undefined} withTerminal={withTerminal} />
    </TooltipProvider>,
  );
}

const withCatalog = (next: Partial<BackgroundAi>) => {
  ai = next as BackgroundAi;
};

beforeEach(() => {
  forgetCatalogs();
  ai = undefined;
});

describe("the agent picker (B27, B28)", () => {
  it("shows the remembered model, disabled with a reason, while the catalog is not read", () => {
    const html = draw("claude", "opus");
    expect(html).toContain('data-agent-kind="claude"');
    expect(html).toContain('data-agent-model="opus"');
    expect(html).toContain("data-agent-model-reason");
    expect(html).toMatch(/data-agent-model="opus"[^>]*disabled|disabled[^>]*data-agent-model="opus"/);
  });

  it("names the CLI default when nothing is remembered and the catalog failed, still with the reason", () => {
    withCatalog({ providers: [{ id: "codex", label: "Codex", state: "needs_login", headline: "", message: null, model: "", models: [], models_unavailable_reason: "Codex is not signed in" }] } as never);
    const html = draw("codex", null);
    expect(html).toContain('data-agent-model=""');
    expect(html).toContain("data-agent-model-reason");
  });

  it("enables the model menu once the catalog lists the kind", () => {
    withCatalog({ providers: [{ id: "claude", label: "Claude", state: "ready", headline: "", message: null, model: "", models: ["haiku", "opus"], models_unavailable_reason: null }] } as never);
    const html = draw("claude", "opus");
    expect(html).not.toContain("data-agent-model-reason");
    expect(html).toContain('data-agent-model="opus"');
  });

  it("has no model menu for a terminal, which only New worktree offers", () => {
    const html = draw("terminal", null, true);
    expect(html).toContain('data-agent-kind="terminal"');
    expect(html).not.toContain("data-agent-model-kind");
  });
});
