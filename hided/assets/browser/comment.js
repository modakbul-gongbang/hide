/*
 * The operator's Comment mode on a page (PRD view-opener-feedback D-07,
 * B9-B11). Installed by hided in an isolated world named `hide-comment`, so
 * page scripts can neither see its state nor call the binding it reports
 * through; every handler reads only trusted (operator) input events.
 *
 * One statement that sets `globalThis.__hideComment`, the API hided calls
 * with Runtime.evaluate in this world:
 *   start(strings)  draw the hint, outline elements, capture click and drag
 *   pins(list)      the page's comments: [{id, number, rect, note, late}]
 *   saved(ok)       the last Add was stored (or not); show the overlay again
 *   stop()          remove everything; the page takes clicks again
 * and reports through `__hideCommentSend(json)`:
 *   {type: "add", anchor, note} | {type: "edit", id, note}
 *   {type: "delete", id} | {type: "off"}
 * Rectangles are document CSS pixels, which a clipped screenshot reads.
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
      .form { position: fixed; width: 300px; padding: 10px; border-radius: 8px; background: #ffffff; color: #1c1f12;
        box-shadow: 0 8px 24px rgba(0,0,0,.25); cursor: auto; display: flex; flex-direction: column; gap: 8px; }
      .what { color: #5b6151; font-family: ui-monospace, SFMono-Regular, Menlo, monospace; font-size: 11px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
      textarea { width: 100%; min-height: 64px; resize: vertical; padding: 6px 8px; border: 1px solid #d4d8cc; border-radius: 6px; color: #1c1f12; background: #fff; }
      textarea:focus { outline: 2px solid #3b82f6; outline-offset: -1px; }
      .row { display: flex; align-items: center; gap: 6px; }
      .grow { flex: 1; color: #5b6151; font-size: 11px; }
      button { border: 1px solid #d4d8cc; background: #fff; color: #1c1f12; border-radius: 6px; padding: 3px 10px; cursor: pointer; }
      button.primary { background: #1c1f12; color: #f5f7ee; border-color: #1c1f12; }
      button.danger { color: #b91c1c; }
      button:disabled { opacity: .45; cursor: default; }
      button:focus-visible { outline: 2px solid #3b82f6; outline-offset: 1px; }
      .full { color: #b45309; font-size: 11px; }
    </style>
    <div class="hint"></div><div class="outline"></div><div class="area"></div><div class="pins"></div>`;
    root.querySelector('.hint').textContent = strings.hint;
    document.documentElement.appendChild(host);
    return { host, root };
  };

  const placeForm = (form, rect) => {
    const view = viewRect(rect);
    const left = Math.min(Math.max(8, view.left), window.innerWidth - 308);
    const below = view.top + view.height + 8;
    const top = below + 160 < window.innerHeight ? below : Math.max(8, view.top - 168);
    form.style.left = `${left}px`;
    form.style.top = `${top}px`;
  };

  /* The note form: a new comment's on `target`, or an existing one's
   * (edit and delete). */
  const openForm = (what, rect, existing, target) => {
    closeForm();
    state.target = target;
    const { root, strings } = state;
    const form = document.createElement('div');
    form.className = 'form';
    form.innerHTML = '<div class="what"></div><textarea></textarea><div class="row"><span class="grow"></span></div>';
    form.querySelector('.what').textContent = what;
    const note = form.querySelector('textarea');
    note.placeholder = strings.placeholder;
    note.maxLength = 4000;
    const row = form.querySelector('.row');
    row.querySelector('.grow').textContent = strings.shortcut;
    const button = (label, kind, run) => {
      const element = document.createElement('button');
      element.textContent = label;
      if (kind) element.className = kind;
      element.addEventListener('click', (event) => { if (event.isTrusted) run(); });
      row.appendChild(element);
      return element;
    };
    // The shortcut runs the primary action itself: a synthetic click is
    // untrusted, and the button reads only the operator's own clicks.
    let submit;
    let primary = () => {};
    if (existing) {
      note.value = existing.note;
      button(strings.remove, 'danger', () => { send({ type: 'delete', id: existing.id }); closeForm(); });
      button(strings.cancel, '', closeForm);
      primary = () => {
        const text = note.value.trim();
        if (!text) return;
        send({ type: 'edit', id: existing.id, note: text });
        closeForm();
      };
      submit = button(strings.save, 'primary', primary);
    } else if (state.full) {
      note.disabled = true;
      const full = document.createElement('div');
      full.className = 'full';
      full.textContent = strings.full;
      form.insertBefore(full, row);
      button(strings.cancel, '', closeForm);
      submit = button(strings.add, 'primary', () => {});
      submit.disabled = true;
    } else {
      button(strings.cancel, '', closeForm);
      primary = () => add(note.value);
      submit = button(strings.add, 'primary', primary);
    }
    const sync = () => { if (!state.full || existing) submit.disabled = !note.value.trim(); };
    note.addEventListener('input', sync);
    sync();
    note.addEventListener('keydown', (event) => {
      if (!event.isTrusted) return;
      event.stopPropagation();
      if (event.key === 'Enter' && (event.metaKey || event.ctrlKey)) { event.preventDefault(); if (!submit.disabled) primary(); }
      if (event.key === 'Escape') { event.preventDefault(); closeForm(); }
    });
    for (const kind of ['mousedown', 'mouseup', 'click', 'pointerdown', 'pointerup']) {
      form.addEventListener(kind, (event) => event.stopPropagation());
    }
    root.appendChild(form);
    placeForm(form, rect);
    note.focus();
    state.form = form;
  };

  const closeForm = () => {
    state?.form?.remove();
    if (state) {
      state.form = null;
      state.target = null;
      state.root.querySelector('.area').style.display = 'none';
    }
  };

  /* The overlay hides for two frames so the screenshot hided clips shows
   * the page alone, then reports. */
  const add = (text) => {
    const note = text.trim();
    if (!note || !state.target) return;
    const anchor = state.target;
    state.host.style.setProperty('visibility', 'hidden', 'important');
    requestAnimationFrame(() => requestAnimationFrame(() => send({ type: 'add', anchor, note })));
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
      dot.addEventListener('click', (event) => {
        if (!event.isTrusted) return;
        event.stopPropagation();
        event.preventDefault();
        openForm(`${pin.number}`, pin.rect, pin, null);
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

  const handlers = {
    mousemove(event) {
      if (!event.isTrusted || state.form) return;
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
      if (state.form) { closeForm(); return; }
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
        const rect = docRect({ left: Math.min(start.x, event.clientX), top: Math.min(start.y, event.clientY),
          width: Math.abs(event.clientX - start.x), height: Math.abs(event.clientY - start.y) });
        openForm(`${Math.round(rect.width)} × ${Math.round(rect.height)}`, rect, null, { kind: 'area', rect });
        return;
      }
      state.root.querySelector('.area').style.display = 'none';
      const element = elementAt(event.clientX, event.clientY);
      if (!element) return;
      const rect = docRect(element.getBoundingClientRect());
      const selector = selectorOf(element);
      const text = textOf(element);
      const source = sourceOf(element);
      openForm(text ? `${selector}  “${text.slice(0, 60)}”` : selector, rect, null,
        { kind: 'element', selector, text, rect, ...(source ? { source } : {}) });
    },
    click(event) { if (event.isTrusted) { event.preventDefault(); event.stopPropagation(); } },
    keydown(event) {
      if (!event.isTrusted || event.key !== 'Escape' || state.form) return;
      event.preventDefault();
      event.stopPropagation();
      send({ type: 'off' });
    },
    scroll() { if (state && !state.form) drawPins(); },
  };

  globalThis.__hideComment = {
    start(strings) {
      if (state) { state.strings = strings; return true; }
      const { host, root } = build(strings);
      state = { host, root, strings, pins: [], form: null, target: null, drag: null, full: false };
      // The pointer handlers listen as a press bubbles out of the overlay, so
      // the form and the pins, which stop their own presses, keep them.
      for (const [kind, handler] of Object.entries(handlers)) {
        if (kind === 'scroll') window.addEventListener(kind, handler, { passive: true });
        else if (kind !== 'keydown') host.addEventListener(kind, handler);
      }
      window.addEventListener('keydown', handlers.keydown, true);
      return true;
    },
    pins(list, full) {
      if (!state) return false;
      state.pins = Array.isArray(list) ? list : [];
      state.full = Boolean(full);
      drawPins();
      return true;
    },
    saved(ok) {
      if (!state) return false;
      state.host.style.removeProperty('visibility');
      if (ok) closeForm();
      return true;
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
