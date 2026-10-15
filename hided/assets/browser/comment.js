/*
 * The operator's Comment mode on a page (PRD view-opener-feedback D-07,
 * B9-B11). Installed by hided in an isolated world named `hide-comment`, so
 * page scripts can neither see its state nor call the binding it reports
 * through; every handler reads only trusted (operator) input events.
 *
 * The overlay only points: it outlines, takes a click or a drag, and draws
 * the page's pins. The note is written in the operator's window, over the
 * page's still, never in the page's document, where the page's scripts
 * could read or change what the operator types (D-17).
 *
 * One statement that sets `globalThis.__hideComment`, the API hided calls
 * with Runtime.evaluate in this world:
 *   start(strings)  draw the hint, outline elements, capture click and drag
 *   pins(list)      the page's comments: [{id, number, rect, note, late, area}]
 *   shot(on)        hide the overlay while hided takes a screenshot, or show it
 *   stop()          remove everything; the page takes clicks again
 * and reports through `__hideCommentSend(json)`:
 *   {type: "pick", anchor, at}   an element or an area to comment on
 *   {type: "open", id, at}       a pin pressed: that comment
 *   {type: "off"}                Esc
 * Anchor rectangles are document CSS pixels, which a clipped screenshot
 * reads; `at` is the same place as fractions of the page's viewport, which
 * the window scales to the page's slot whatever either one's zoom.
 */
(() => {
  if (globalThis.__hideComment) return;
  const HOST = 'hide-comment-overlay';
  const send = (message) => {
    try { globalThis.__hideCommentSend(JSON.stringify(message)); } catch (_) { /* binding gone: session ended */ }
  };
  let state = null;

  const docRect = (rect) => ({
    x: Math.max(0, rect.left + window.scrollX),
    y: Math.max(0, rect.top + window.scrollY),
    width: Math.max(1, rect.width),
    height: Math.max(1, rect.height),
  });
  const viewRect = (rect) => ({
    left: rect.x - window.scrollX, top: rect.y - window.scrollY, width: rect.width, height: rect.height,
  });
  const fractionOf = (view) => {
    const width = Math.max(1, window.innerWidth), height = Math.max(1, window.innerHeight);
    return { left: view.left / width, top: view.top / height, width: view.width / width, height: view.height / height };
  };

  /* A selector that finds this element again: an id when it is unique,
   * else tag and position up from the nearest ancestor with one. */
  const selectorOf = (element) => {
    const parts = [];
    let node = element;
    while (node && node.nodeType === 1 && node !== document.documentElement && parts.length < 6) {
      if (node.id && document.querySelectorAll('#' + CSS.escape(node.id)).length === 1) {
        parts.unshift('#' + CSS.escape(node.id));
        break;
      }
      let part = node.localName;
      const classes = [...node.classList].filter((name) => /^[a-zA-Z][\w-]{0,40}$/.test(name)).slice(0, 2);
      if (classes.length) part += '.' + classes.map((name) => CSS.escape(name)).join('.');
      const parent = node.parentElement;
      if (parent) {
        const same = [...parent.children].filter((child) => child.localName === node.localName);
        if (same.length > 1) part += `:nth-of-type(${same.indexOf(node) + 1})`;
      }
      parts.unshift(part);
      node = parent;
    }
    return parts.join(' > ').slice(0, 512);
  };

  /* Where the element's source is, when the page's dev tooling says. */
  const sourceOf = (element) => {
    for (let node = element; node && node.nodeType === 1; node = node.parentElement) {
      const read = (name) => node.getAttribute(name);
      const file = read('data-source-file') || read('data-inspector-relative-path') || read('data-insp-path');
      if (file) {
        const line = read('data-source-line') || read('data-inspector-line');
        return (line ? `${file}:${line}` : file).slice(0, 512);
      }
      const whole = read('data-source');
      if (whole) return whole.slice(0, 512);
    }
    return null;
  };

  const textOf = (element) => (element.innerText || element.textContent || element.getAttribute('aria-label') || '')
    .replace(/\s+/g, ' ').trim().slice(0, 200);

  const build = (strings) => {
    const host = document.createElement(HOST);
    for (const [name, value] of [['position', 'fixed'], ['inset', '0'], ['z-index', '2147483647'], ['display', 'block'], ['margin', '0'], ['padding', '0'], ['border', '0'], ['background', 'transparent'], ['cursor', 'crosshair']]) {
      host.style.setProperty(name, value, 'important');
    }
    const root = host.attachShadow({ mode: 'closed' });
    root.innerHTML = `<style>
      :host { all: initial; }
      * { box-sizing: border-box; font: 12px/1.4 -apple-system, BlinkMacSystemFont, "Segoe UI", system-ui, sans-serif; }
      .hint { position: fixed; top: 10px; left: 50%; transform: translateX(-50%); padding: 5px 10px; border-radius: 999px;
        background: rgba(28,31,18,.9); color: #f5f7ee; pointer-events: none; white-space: nowrap; }
      .outline { position: fixed; border: 2px solid #3b82f6; border-radius: 3px; background: rgba(59,130,246,.08); pointer-events: none; display: none; }
      .area { position: fixed; border: 1.5px dashed #3b82f6; background: rgba(59,130,246,.10); pointer-events: none; display: none; }
      .pin { position: absolute; width: 20px; height: 20px; margin: -10px 0 0 -10px; border-radius: 50%; background: #3b82f6; color: white;
        font-weight: 600; font-size: 11px; display: flex; align-items: center; justify-content: center; cursor: pointer; border: 1.5px solid white;
        box-shadow: 0 1px 3px rgba(0,0,0,.35); }
      .pin.late { background: #f97316; }
      .pin:focus-visible { outline: 2px solid #1d4ed8; outline-offset: 2px; }
      .box { position: absolute; border: 1.5px solid #3b82f6; background: rgba(59,130,246,.06); pointer-events: none; }
      .box.late { border-color: #f97316; background: rgba(249,115,22,.06); }
    </style>
    <div class="hint"></div><div class="outline"></div><div class="area"></div><div class="pins"></div>`;
    root.querySelector('.hint').textContent = strings.hint;
    document.documentElement.appendChild(host);
    return { host, root };
  };

  const drawPins = () => {
    const layer = state.root.querySelector('.pins');
    layer.replaceChildren();
    for (const pin of state.pins) {
      const view = viewRect(pin.rect);
      if (pin.area) {
        const box = document.createElement('div');
        box.className = 'box' + (pin.late ? ' late' : '');
        Object.assign(box.style, { left: `${view.left}px`, top: `${view.top}px`, width: `${view.width}px`, height: `${view.height}px` });
        layer.appendChild(box);
      }
      const dot = document.createElement('button');
      dot.className = 'pin' + (pin.late ? ' late' : '');
      dot.textContent = String(pin.number);
      dot.setAttribute('aria-label', `${pin.number}: ${pin.note}`);
      Object.assign(dot.style, { left: `${view.left + view.width}px`, top: `${view.top}px` });
      dot.addEventListener('mousedown', (event) => event.stopPropagation());
      dot.addEventListener('mouseup', (event) => event.stopPropagation());
      dot.addEventListener('click', (event) => {
        if (!event.isTrusted) return;
        event.stopPropagation();
        event.preventDefault();
        send({ type: 'open', id: pin.id, at: fractionOf(view) });
      });
      layer.appendChild(dot);
    }
  };

  const elementAt = (x, y) => {
    state.host.style.setProperty('pointer-events', 'none', 'important');
    const element = document.elementFromPoint(x, y);
    state.host.style.removeProperty('pointer-events');
    return element && element !== document.documentElement ? element : null;
  };

  const pick = (anchor) => {
    // While its screenshot is taken the overlay is drawn clear but still
    // takes every press, so a second click never reaches the page.
    if (!state.shooting) send({ type: 'pick', anchor, at: fractionOf(viewRect(anchor.rect)) });
  };

  const handlers = {
    mousemove(event) {
      if (!event.isTrusted) return;
      const outline = state.root.querySelector('.outline');
      if (state.drag) {
        const area = state.root.querySelector('.area');
        const left = Math.min(state.drag.x, event.clientX), top = Math.min(state.drag.y, event.clientY);
        Object.assign(area.style, { display: 'block', left: `${left}px`, top: `${top}px`,
          width: `${Math.abs(event.clientX - state.drag.x)}px`, height: `${Math.abs(event.clientY - state.drag.y)}px` });
        outline.style.display = 'none';
        return;
      }
      const element = elementAt(event.clientX, event.clientY);
      if (!element) { outline.style.display = 'none'; return; }
      const rect = element.getBoundingClientRect();
      Object.assign(outline.style, { display: 'block', left: `${rect.left}px`, top: `${rect.top}px`, width: `${rect.width}px`, height: `${rect.height}px` });
    },
    mousedown(event) {
      if (!event.isTrusted || event.button !== 0) return;
      event.preventDefault();
      event.stopPropagation();
      state.root.querySelector('.area').style.display = 'none';
      state.drag = { x: event.clientX, y: event.clientY };
    },
    mouseup(event) {
      if (!event.isTrusted || !state.drag) return;
      event.preventDefault();
      event.stopPropagation();
      const start = state.drag;
      state.drag = null;
      const moved = Math.abs(event.clientX - start.x) > 6 || Math.abs(event.clientY - start.y) > 6;
      if (moved) {
        // The dashed area stays drawn while its note is written.
        const rect = docRect({ left: Math.min(start.x, event.clientX), top: Math.min(start.y, event.clientY),
          width: Math.abs(event.clientX - start.x), height: Math.abs(event.clientY - start.y) });
        pick({ kind: 'area', rect });
        return;
      }
      const element = elementAt(event.clientX, event.clientY);
      if (!element) return;
      const rect = docRect(element.getBoundingClientRect());
      const selector = selectorOf(element);
      const text = textOf(element);
      const source = sourceOf(element);
      pick({ kind: 'element', selector, text, rect, ...(source ? { source } : {}) });
    },
    click(event) { if (event.isTrusted) { event.preventDefault(); event.stopPropagation(); } },
    /* The page never sees the operator's presses on the overlay, as pointer events either. */
    pointer(event) { if (event.isTrusted) event.stopPropagation(); },
    keydown(event) {
      if (!event.isTrusted || event.key !== 'Escape') return;
      event.preventDefault();
      event.stopPropagation();
      send({ type: 'off' });
    },
    scroll() { if (state) drawPins(); },
  };

  globalThis.__hideComment = {
    start(strings) {
      if (state) { state.strings = strings; return true; }
      const { host, root } = build(strings);
      state = { host, root, strings, pins: [], drag: null, shooting: false };
      // The pointer handlers listen as a press bubbles out of the overlay, so
      // the pins, which stop their own presses, keep them.
      for (const kind of ['mousemove', 'mousedown', 'mouseup', 'click']) host.addEventListener(kind, handlers[kind]);
      for (const kind of ['pointerdown', 'pointerup', 'pointermove', 'dblclick', 'contextmenu']) host.addEventListener(kind, handlers.pointer);
      window.addEventListener('scroll', handlers.scroll, { passive: true });
      window.addEventListener('keydown', handlers.keydown, true);
      return true;
    },
    pins(list) {
      if (!state) return false;
      state.pins = Array.isArray(list) ? list : [];
      // A new comment's own box replaces the area drawn while it was written.
      state.root.querySelector('.area').style.display = 'none';
      drawPins();
      return true;
    },
    /* Resolves once the page has drawn a frame without (or with) the overlay. */
    shot(on) {
      if (!state) return false;
      state.shooting = on;
      if (on) state.host.style.setProperty('opacity', '0', 'important');
      else state.host.style.removeProperty('opacity');
      return new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(() => resolve(true))));
    },
    stop() {
      if (!state) return true;
      window.removeEventListener('keydown', handlers.keydown, true);
      window.removeEventListener('scroll', handlers.scroll, { passive: true });
      state.host.remove();
      state = null;
      return true;
    },
  };
})();
