/* global document, getComputedStyle */
// Where the first-run wizard's parts are on screen, measured in the page for the
// design review command (scripts/design-review.mjs), which judges them against
// the rules a baseline states. Expected values come from the baseline, never
// from these measurements. Plain JavaScript because that command is a Node
// script, not a Playwright spec.

export async function measure(page) {
  return page.evaluate(() => {
    const dialog = document.querySelector("[data-first-run]");
    if (!dialog) throw new Error("The first-run wizard is not on the scene");
    const visible = (element) => element.getClientRects().length > 0;
    const describe = (element) => {
      const own = [...element.attributes].find((attribute) => attribute.name.startsWith("data-") || attribute.name === "aria-label");
      return own ? `${own.name}${own.value && own.value !== "true" ? `=${own.value}` : ""}` : `${element.tagName.toLowerCase()} "${element.textContent?.trim().slice(0, 30)}"`;
    };
    const partProblems = [], rowsFit = [], overflow = [];
    // Every line of the wizard: a part must not overlap another part of its line or pass the line's edge.
    for (const line of [dialog, ...dialog.querySelectorAll("*")]) {
      if (!visible(line)) continue;
      const style = getComputedStyle(line);
      if (!style.display.includes("flex") || style.flexDirection !== "row") continue;
      const edge = line.getBoundingClientRect();
      const parts = [...line.children].filter((part) => visible(part) && getComputedStyle(part).position !== "absolute");
      for (const [i, a] of parts.entries()) {
        const ra = a.getBoundingClientRect();
        if (ra.right > edge.right + 0.5) partProblems.push(`${describe(line)}: ${describe(a)} runs ${Math.round(ra.right - edge.right)}px past its line`);
        for (const b of parts.slice(i + 1)) {
          const rb = b.getBoundingClientRect();
          const across = Math.min(ra.right, rb.right) - Math.max(ra.left, rb.left);
          const down = Math.min(ra.bottom, rb.bottom) - Math.max(ra.top, rb.top);
          if (across > 0.5 && down > 0.5) partProblems.push(`${describe(line)}: ${describe(a)} overlaps ${describe(b)} by ${Math.round(across * 10) / 10}px`);
        }
      }
    }
    // Text cut off without the ellipsis a truncating box draws is lost rather than shortened.
    for (const part of dialog.querySelectorAll("*")) {
      if (!visible(part) || part.children.length > 0 || !part.textContent?.trim()) continue;
      const style = getComputedStyle(part);
      const truncates = style.textOverflow === "ellipsis" || style.webkitLineClamp !== "none";
      if (part.scrollWidth > part.clientWidth + 1 && !truncates && style.overflowX !== "visible") rowsFit.push(`${part.textContent.trim().slice(0, 40)} is cut off`);
    }
    if (dialog.scrollWidth > dialog.clientWidth + 1) overflow.push(`the wizard scrolls sideways ${dialog.scrollWidth} > ${dialog.clientWidth}`);
    const box = dialog.getBoundingClientRect();
    if (box.left < -0.5 || box.right > document.documentElement.clientWidth + 0.5) overflow.push("the wizard passes the window's edge");
    return { partProblems, rowsFit, overflow };
  });
}
