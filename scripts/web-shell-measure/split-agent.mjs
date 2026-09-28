// Prepare a measured Agent area through the shell's ordinary tab menu.
// The pointer interaction itself is covered separately by web/desktop e2e.
import { connectPage } from "./cdp.mjs";
const [port, tab] = process.argv.slice(2);
if (!port || !tab) throw new Error("usage: split-agent.mjs <cdp-port> <tab-id>");
const page = await connectPage(port);
try {
  const point = await page.evaluate(`(() => {
    const rect = document.querySelector('[data-agent-tab-bar] [data-tab="${tab}"]')?.getBoundingClientRect();
    if (!rect) throw new Error('missing Agent tab');
    return {x: rect.x + rect.width / 2, y: rect.y + rect.height / 2};
  })()`);
  await page.send("Input.dispatchMouseEvent", { type: "mousePressed", button: "right", clickCount: 1, ...point });
  await page.send("Input.dispatchMouseEvent", { type: "mouseReleased", button: "right", clickCount: 1, ...point });
  // Down splits preserve adequate width for three simultaneously shown areas.
  const deadline = Date.now() + 5000;
  let split = false;
  while (Date.now() < deadline && !split) {
    split = await page.evaluate(`(() => {
      const item = document.querySelector('[data-menu-item="split_down"]');
      if (!item) return false;
      if (item.getAttribute('data-disabled') !== null) throw new Error('measured area cannot split');
      item.click(); return true;
    })()`);
    if (!split) await new Promise((resolve) => setTimeout(resolve, 50));
  }
  if (!split) throw new Error("Agent menu did not open");
} finally { page.close(); }
