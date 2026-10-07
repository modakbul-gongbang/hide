/* global document, getComputedStyle */
// Where the Factory screen's rows, cards and header parts are on screen,
// measured in the page for the design review command (scripts/design-review.mjs),
// which judges them against the rules a baseline states. Expected values come
// from the baseline, never from these measurements. Plain JavaScript because
// that command is a Node script, not a Playwright spec.

/** The boxes whose direct children share one line: a part of one must not overlap another or pass the box's edge. */
const LINES = "[data-factory-header], [data-factory-flow], [data-factory-tabs], [data-factory-item-open='false'], [data-factory-card], [data-factory-chain-card]";

/** Parts of one line that overlap each other or run past the line's right edge. */
async function partProblems(page) {
  return page.evaluate(
    (lines) => {
      const describe = (element) => {
        const own = [...element.attributes].find((attribute) => attribute.name.startsWith("data-factory") || attribute.name === "aria-label");
        return own ? `${own.name}${own.value && own.value !== "true" ? `=${own.value}` : ""}` : element.tagName.toLowerCase();
      };
      const problems = [];
      for (const line of document.querySelectorAll(`[data-factory-screen] :is(${lines})`)) {
        if (line.getClientRects().length === 0) continue;
        const edge = line.getBoundingClientRect().right;
        const parts = [...line.children].filter((part) => part.getClientRects().length > 0);
        for (const [i, a] of parts.entries()) {
          const ra = a.getBoundingClientRect();
          if (ra.right > edge + 0.5) problems.push(`${describe(line)}: ${describe(a)} runs ${Math.round(ra.right - edge)}px past the line`);
          for (const b of parts.slice(i + 1)) {
            const rb = b.getBoundingClientRect();
            const across = Math.min(ra.right, rb.right) - Math.max(ra.left, rb.left);
            const down = Math.min(ra.bottom, rb.bottom) - Math.max(ra.top, rb.top);
            if (across > 0.5 && down > 0.5) problems.push(`${describe(line)}: ${describe(a)} overlaps ${describe(b)} by ${Math.round(across * 10) / 10}px`);
          }
        }
      }
      return problems;
    },
    LINES,
  );
}

/** Text that is cut off without the ellipsis a truncating box draws, so it is lost rather than shortened. */
async function textSpills(page) {
  return page.evaluate(() => {
    const problems = [];
    for (const part of document.querySelectorAll("[data-factory-screen] *")) {
      if (part.getClientRects().length === 0 || part.children.length > 0) continue;
      if (!part.textContent?.trim()) continue;
      const style = getComputedStyle(part);
      const truncates = style.textOverflow === "ellipsis" || style.webkitLineClamp !== "none";
      if (part.scrollWidth > part.clientWidth + 1 && !truncates && style.overflowX !== "visible") problems.push(`${part.textContent.trim().slice(0, 40)} is cut off`);
    }
    return problems;
  });
}

/** Whether the screen scrolls sideways, or a part sits past the screen's right edge. */
async function overflow(page) {
  return page.evaluate(() => {
    const screen = document.querySelector("[data-factory-screen]");
    if (!screen) return ["no Factory screen"];
    const problems = [];
    if (screen.scrollWidth > screen.clientWidth + 1) problems.push(`screen scrolls sideways ${screen.scrollWidth} > ${screen.clientWidth}`);
    const body = screen.querySelector("[data-factory-body]");
    if (body && body.scrollWidth > body.clientWidth + 1) problems.push(`body scrolls sideways ${body.scrollWidth} > ${body.clientWidth}`);
    const edge = screen.getBoundingClientRect().right;
    for (const part of screen.querySelectorAll("button, [data-factory-card], [data-factory-item]")) {
      if (part.getClientRects().length > 0 && part.getBoundingClientRect().right > edge + 0.5) problems.push(`${part.textContent?.trim().slice(0, 30) || part.tagName} passes the screen's edge`);
    }
    return problems;
  });
}

export async function measure(page) {
  return { partProblems: await partProblems(page), rowsFit: await textSpills(page), overflow: await overflow(page) };
}
