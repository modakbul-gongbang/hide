/* global document, Node */
// Compare visible row parts, including Korean text and fixed facts. Truncated
// text is clipped to its visible box; font/contrast remain a visual judgment.
export async function measure(page) {
  return page.evaluate(() => {
    const partProblems = [], rowsFit = [], overflow = [];
    const root = document.querySelector('[data-session-panel]');
    if (!root) throw new Error('Sessions panel is missing');
    for (const row of root.querySelectorAll('[data-session-row], [data-session-closed-pr]')) {
      const edge = row.getBoundingClientRect();
      const parts = [...row.querySelectorAll('button, [data-mark], [data-agent-mark], span')].filter(part => part.getClientRects().length && (part.matches('button, [data-mark], [data-agent-mark]') || [...part.childNodes].some(node => node.nodeType === Node.TEXT_NODE && node.textContent.trim())));
      for (const part of parts) {
        const box = part.getBoundingClientRect();
        if (box.right > edge.right + 0.5 || box.left < edge.left - 0.5) overflow.push(`${row.dataset.sessionRow}: ${part.textContent} passes its row`);
        if (box.bottom > edge.bottom + 0.5) rowsFit.push(`${row.dataset.sessionRow}: ${part.textContent} spills below its row`);
      }
      for (const [index, a] of parts.entries()) for (const b of parts.slice(index + 1)) {
        if (a.contains(b) || b.contains(a)) continue;
        const ra = a.getBoundingClientRect(), rb = b.getBoundingClientRect();
        if (Math.min(ra.right, rb.right) - Math.max(ra.left, rb.left) > 1 && Math.min(ra.bottom, rb.bottom) - Math.max(ra.top, rb.top) > 1) partProblems.push(`${row.dataset.sessionRow}: ${a.textContent} overlaps ${b.textContent}`);
      }
    }
    if (root.scrollWidth > root.clientWidth) overflow.push('Sessions scrolls sideways');
    return { partProblems, rowsFit, overflow };
  });
}
