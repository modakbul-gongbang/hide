/* global document, getComputedStyle */
// The terminal band is an intentional overlay. Judge identity and band rows
// separately, so it cannot make the terminal's deliberate overlap a failure.
export async function measure(page) {
  return page.evaluate(() => {
    const partProblems = [], rowsFit = [], overflow = [];
    const panes = [...document.querySelectorAll('[data-pane-view]')];
    if (!panes.length) throw new Error('Pane state board is missing');
    for (const pane of panes) for (const row of pane.querySelectorAll(':scope > header, [data-pane-header-band]')) {
      const edge = row.getBoundingClientRect();
      const parts = [...row.children].filter(part => {
        const box = part.getBoundingClientRect();
        // The menu's fixed, zero-size anchor is not a painted header part.
        return box.width > 0 && box.height > 0 && getComputedStyle(part).position !== 'absolute';
      });
      for (const part of parts) {
        const box = part.getBoundingClientRect();
        if (box.right > edge.right + 0.5 || box.left < edge.left - 0.5) overflow.push(`${pane.dataset.paneView}: ${part.textContent} passes its row`);
        if (box.bottom > edge.bottom + 0.5) rowsFit.push(`${pane.dataset.paneView}: ${part.textContent} spills below its row`);
      }
      for (const [index, a] of parts.entries()) for (const b of parts.slice(index + 1)) {
        const ra = a.getBoundingClientRect(), rb = b.getBoundingClientRect();
        if (Math.min(ra.right, rb.right) - Math.max(ra.left, rb.left) > 1 && Math.min(ra.bottom, rb.bottom) - Math.max(ra.top, rb.top) > 1) partProblems.push(`${pane.dataset.paneView}: identity parts overlap`);
      }
    }
    return { partProblems, rowsFit, overflow };
  });
}
