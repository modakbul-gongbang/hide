/* global document, getComputedStyle */
// Measure the records that the production table actually draws.
export async function measure(page) {
  return page.evaluate(() => {
    const root = document.querySelector('[data-usage-history="year"]');
    if (!root) throw new Error('Usage history did not render');
    const table = root.querySelector('[data-usage-table]');
    const scroll = root.querySelector('.usage-table-scroll');
    if (!table || !scroll || table.querySelectorAll('thead th').length !== 8) throw new Error('Usage record columns did not render');
    const partProblems = [], rowsFit = [], overflow = [];
    if (root.scrollWidth > root.clientWidth + 1) overflow.push('Usage records overflow sideways');
    if (scroll.getBoundingClientRect().right > root.getBoundingClientRect().right + 1) overflow.push('Table scroll region escapes the records');
    for (const row of [root.firstElementChild, root.querySelector('.usage-summary'), ...table.querySelectorAll('tr')]) {
      const edge = row.getBoundingClientRect();
      const parts = [...row.children].filter(child => getComputedStyle(child).visibility !== 'hidden');
      for (const [i, part] of parts.entries()) {
        const a = part.getBoundingClientRect();
        if (a.left < edge.left - .5 || a.right > edge.right + .5) rowsFit.push('A usage value escapes its row');
        for (const other of parts.slice(i + 1)) {
          const b = other.getBoundingClientRect();
          if (Math.min(a.right, b.right) - Math.max(a.left, b.left) > .5 && Math.min(a.bottom, b.bottom) - Math.max(a.top, b.top) > .5) partProblems.push('Usage controls or values overlap');
        }
      }
    }
    return { partProblems, rowsFit, overflow };
  });
}
