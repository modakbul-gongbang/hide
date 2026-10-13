/* global document, getComputedStyle */
// Measure the production column. The frozen baseline owns acceptance rules.
export async function measure(page) {
  return page.evaluate(() => {
    const root = document.querySelector('[data-usage-history="year"]');
    if (!root) throw new Error('Usage history did not render');
    const partProblems = [], rowsFit = [], overflow = [];
    if (root.scrollWidth > root.clientWidth + 1) overflow.push('Usage column overflows sideways');
    for (const row of root.querySelectorAll('[data-usage-filter]')) {
      const edge = row.getBoundingClientRect();
      const parts = [...row.children].filter(child => getComputedStyle(child).visibility !== 'hidden');
      for (const [i, part] of parts.entries()) {
        const a = part.getBoundingClientRect();
        if (a.left < edge.left - .5 || a.right > edge.right + .5) rowsFit.push('An agent value escapes its row');
        for (const other of parts.slice(i + 1)) {
          const b = other.getBoundingClientRect();
          if (Math.min(a.right, b.right) - Math.max(a.left, b.left) > .5 && Math.min(a.bottom, b.bottom) - Math.max(a.top, b.top) > .5) partProblems.push('Agent name, share or total overlap');
        }
      }
    }
    return { partProblems, rowsFit, overflow };
  });
}
