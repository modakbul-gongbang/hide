// Observed geometry and focus from the production area renderer. Expected
// values come from the chosen design bundle, never from these measurements.
export async function measure(page) {
  const read = () => page.locator('[data-view-area-id]').evaluateAll(areas => areas.map(area => {
    const box = area.getBoundingClientRect();
    const body = area.querySelector('[data-view-body]');
    return { id: area.dataset.viewAreaId, keyboard: area.dataset.keyboardArea === 'true', x: box.x, y: box.y, width: box.width, height: box.height, selected: area.querySelectorAll('[role=tab][aria-selected=true]').length, filter: globalThis.getComputedStyle(body).filter };
  }));
  const before = await read();
  await page.locator('[data-view-area-id=a2] textarea').focus();
  await page.waitForTimeout(30);
  const after = await read();
  return { areaFocus: { before, after } };
}
