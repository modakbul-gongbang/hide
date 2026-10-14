import { WebglAddon } from "@xterm/addon-webgl";
import type { IDisposable, Terminal } from "@xterm/xterm";

type FallbackReason = "activation_failed" | "context_loss";

/** One attempt per terminal. Disposing the addon restores xterm's normal renderer
 * on the same buffer; its own onContextLoss waits for transient restoration first. */
export function loadTerminalRenderer(term: Terminal, fallback: (reason: FallbackReason) => void): IDisposable {
  let addon: WebglAddon | undefined;
  let contextLoss: IDisposable | undefined;
  const dispose = () => {
    const owned = addon;
    addon = undefined;
    contextLoss?.dispose();
    contextLoss = undefined;
    owned?.dispose();
  };
  try {
    addon = new WebglAddon();
    contextLoss = addon.onContextLoss(() => {
      dispose();
      fallback("context_loss");
    });
    term.loadAddon(addon);
  } catch {
    // xterm registers the addon before activate(), even when activation throws.
    // Its public dispose removes that registration as well as owned resources.
    dispose();
    fallback("activation_failed");
  }
  return { dispose };
}
