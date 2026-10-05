/*
 * Ported from chromux (https://github.com/modakbul-gongbang/chromux, commit
 * 93f770f, chromux.mjs DEEP_QUERY_JS, resolveDeepElementRect, the click
 * --text finder, the fill page code and the wait probes) for the page-side
 * half of `hide browser` actions.
 *
 * MIT License
 *
 * Copyright (c) 2026 Hoyeon Lee
 *
 * Permission is hereby granted, free of charge, to any person obtaining a copy
 * of this software and associated documentation files (the "Software"), to deal
 * in the Software without restriction, including without limitation the rights
 * to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
 * copies of the Software, and to permit persons to whom the Software is
 * furnished to do so, subject to the following conditions:
 *
 * The above copyright notice and this permission notice shall be included in all
 * copies or substantial portions of the Software.
 *
 * THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
 * IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
 * FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
 * AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
 * LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
 * OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
 * SOFTWARE.
 *
 * One function expression: (op, args) => result, evaluated in the frame it
 * acts on. A refusal is returned as {error: <reason>, detail}, never thrown,
 * so the CLI can report the reason a caller branches on.
 */
((op, args) => {
  // Plain querySelector is the fast path; on a miss the search pierces open
  // shadow roots and same-origin iframes so snapshot @refs assigned inside
  // them stay actionable. Cross-origin frames and closed shadow roots remain
  // out of reach.
  function deepQuery(sel) {
    const search = (root) => {
      let el = null;
      try { el = root.querySelector(sel); } catch (e) { throw new Error('Bad selector: ' + sel + ' (' + e.message + ')'); }
      if (el) return el;
      for (const host of root.querySelectorAll('*')) {
        if (host.shadowRoot) {
          const found = search(host.shadowRoot);
          if (found) return found;
        }
      }
      for (const frame of root.querySelectorAll('iframe,frame')) {
        let innerDoc = null;
        try { innerDoc = frame.contentDocument; } catch {}
        if (innerDoc) {
          const found = search(innerDoc);
          if (found) return found;
        }
      }
      return null;
    };
    return search(document);
  }
  function deepVisible(el) {
    const view = el.ownerDocument.defaultView || window;
    const style = view.getComputedStyle(el);
    const rect = el.getBoundingClientRect();
    return rect.width > 0 && rect.height > 0 && style.visibility !== 'hidden' && style.display !== 'none';
  }
  // Rendered text of the page including same-origin frames (innerText already
  // covers rendered shadow DOM content).
  function deepText(doc) {
    let text = doc.body ? doc.body.innerText : '';
    for (const frame of doc.querySelectorAll('iframe,frame')) {
      try { if (frame.contentDocument) text += '\n' + deepText(frame.contentDocument); } catch {}
    }
    return text;
  }
  function describe(node) {
    if (!node || node.nodeType !== 1) return String(node);
    const id = node.id ? '#' + node.id : '';
    const cls = node.className && typeof node.className === 'string'
      ? '.' + node.className.trim().split(/\s+/).filter(Boolean).slice(0, 2).join('.')
      : '';
    return node.tagName.toLowerCase() + id + (cls === '.' ? '' : cls);
  }
  function composedContains(ancestor, node) {
    let current = node;
    while (current) {
      if (current === ancestor) return true;
      current = current.parentNode || current.host || null;
    }
    return false;
  }
  const refused = (error, detail) => ({ error, detail });
  const bySelector = (sel) => {
    try { return { el: deepQuery(sel) }; } catch (error) { return { refusal: refused('invalid_selector', String(error.message)) }; }
  };

  // State hide keeps in the document it describes, so it ends with the
  // document: the frame's tag for @<tag>:N refs, the --diff baseline and the
  // no-change streak of `changed`. Nothing is written outside the page.
  function state() {
    let held = Object.getOwnPropertyDescriptor(document, '__hideBrowser')?.value;
    if (!held) {
      held = {};
      Object.defineProperty(document, '__hideBrowser', { value: held, enumerable: false });
    }
    return held;
  }

  // Where an element is, after an optional scroll into view, and whether a
  // click at its center would reach it: own root, shadow hosts, and the top
  // document over an enclosing same-origin frame.
  function rect(sel, scroll) {
    const found = bySelector(sel);
    if (found.refusal) return found.refusal;
    const el = found.el;
    if (!el) return refused('ref_stale', sel);
    if (scroll) el.scrollIntoView?.({ block: 'center', inline: 'center' });
    const view = el.ownerDocument.defaultView || window;
    const box = el.getBoundingClientRect();
    if (!deepVisible(el)) return refused('target_hidden', sel);
    const lx = box.left + box.width / 2;
    const ly = box.top + box.height / 2;
    if (lx < 0 || ly < 0 || lx >= view.innerWidth || ly >= view.innerHeight) return refused('target_outside_viewport', sel);
    const rootNode = el.getRootNode();
    let shadowHost = rootNode.host || null;
    while (shadowHost) {
      const hostRoot = shadowHost.getRootNode();
      const hostHitBase = hostRoot.elementFromPoint ? hostRoot : shadowHost.ownerDocument;
      const hostHit = hostHitBase.elementFromPoint(lx, ly);
      if (!hostHit) return refused('target_hidden', sel);
      if (!composedContains(shadowHost, hostHit) && !composedContains(hostHit, shadowHost)) {
        return refused('target_covered', describe(hostHit));
      }
      shadowHost = hostRoot.host || null;
    }
    const hitBase = rootNode.elementFromPoint ? rootNode : el.ownerDocument;
    const hit = hitBase.elementFromPoint(lx, ly);
    if (!hit) return refused('target_hidden', sel);
    if (!composedContains(el, hit) && !composedContains(hit, el)) return refused('target_covered', describe(hit));
    let left = box.left;
    let top = box.top;
    let w = view;
    let outermostFrame = null;
    while (w !== w.parent && w.frameElement) {
      const frameEl = w.frameElement;
      const frameRect = frameEl.getBoundingClientRect();
      let padLeft = 0;
      let padTop = 0;
      try {
        const style = frameEl.ownerDocument.defaultView.getComputedStyle(frameEl);
        padLeft = parseFloat(style.paddingLeft) || 0;
        padTop = parseFloat(style.paddingTop) || 0;
      } catch {}
      left += frameRect.left + frameEl.clientLeft + padLeft;
      top += frameRect.top + frameEl.clientTop + padTop;
      outermostFrame = frameEl;
      w = w.parent;
    }
    const centerX = left + box.width / 2;
    const centerY = top + box.height / 2;
    if (centerX < 0 || centerY < 0 || centerX >= window.innerWidth || centerY >= window.innerHeight) {
      return refused('target_outside_viewport', sel);
    }
    if (outermostFrame) {
      const topHit = document.elementFromPoint(centerX, centerY);
      if (topHit && topHit !== outermostFrame && !outermostFrame.contains(topHit) && !topHit.contains(outermostFrame)) {
        return refused('target_covered', describe(topHit));
      }
    }
    let opaqueFrame = false;
    if (/^(IFRAME|FRAME)$/.test(el.tagName)) {
      try { opaqueFrame = !el.contentDocument; } catch { opaqueFrame = true; }
    }
    return {
      x: left, y: top, width: box.width, height: box.height, centerX, centerY,
      draggable: el.draggable === true, opaqueFrame, editable: el.isContentEditable || 'value' in el,
    };
  }

  // The one visible control a label names, given a ref so the rest of the
  // click is the ordinary ref path. Ambiguity is a refusal with candidates.
  function textTarget(needleText) {
    const needle = String(needleText).trim().toLowerCase();
    const matches = [];
    let scanned = 0;
    const labelOf = (node) => {
      const aria = node.getAttribute && node.getAttribute('aria-label');
      if (aria) return aria.trim().replace(/\s+/g, ' ');
      // input.value is a label only for button-shaped inputs; a text field
      // whose typed value matches must not become a click target.
      if (node.tagName === 'INPUT') return /^(button|submit|reset)$/.test(node.type) ? (node.value || '').trim() : '';
      return (node.innerText || '').trim().replace(/\s+/g, ' ');
    };
    const collect = (doc) => {
      for (const node of doc.querySelectorAll('a[href],button,input,select,textarea,[role="button"],[role="link"],[role="tab"],[role="menuitem"],[onclick]')) {
        if (scanned++ >= 3000 || matches.length > 12) return;
        if (!deepVisible(node)) continue;
        const label = labelOf(node).toLowerCase();
        if (!label) continue;
        if (label === needle) matches.push({ node, label: labelOf(node), exact: true });
        else if (label.includes(needle)) matches.push({ node, label: labelOf(node), exact: false });
      }
      for (const frame of doc.querySelectorAll('iframe,frame')) {
        try { if (frame.contentDocument) collect(frame.contentDocument); } catch {}
      }
    };
    collect(document);
    const exact = matches.filter(m => m.exact);
    // Substring matches on huge containers would center-click an arbitrary
    // child; keep only tightly labeled, innermost matches.
    let pool = exact.length ? exact : matches.filter(m => m.label.length <= 100);
    pool = pool.filter(m => !pool.some(o => o !== m && m.node.contains(o.node)));
    if (!pool.length) return refused('text_not_found', String(needleText));
    if (pool.length > 1) {
      return refused('text_ambiguous', pool.slice(0, 8).map(m => describe(m.node) + ' "' + m.label.slice(0, 60) + '"').join('; '));
    }
    const el = pool[0].node;
    let ref = Number(el.getAttribute('data-ct-ref')) || 0;
    if (!ref) {
      const max = Number(document.documentElement.getAttribute('data-ct-ref-max')) || 0;
      ref = max + 1;
      el.setAttribute('data-ct-ref', String(ref));
      document.documentElement.setAttribute('data-ct-ref-max', String(ref));
    }
    return { ref };
  }

  // Constructors and prototypes come from the element's own realm, or
  // elements inside same-origin iframes fail instanceof/setter paths. The
  // native setter plus an InputEvent is what a React-controlled input reads.
  function fill(sel, txt) {
    const found = bySelector(sel);
    if (found.refusal) return found.refusal;
    const el = found.el;
    if (!el) return refused('ref_stale', sel);
    el.focus();
    const view = el.ownerDocument.defaultView || window;
    if (el.isContentEditable) {
      const selection = view.getSelection();
      const range = el.ownerDocument.createRange();
      range.selectNodeContents(el);
      selection.removeAllRanges();
      selection.addRange(range);
      return { contenteditable: true };
    }
    if (!('value' in el) || el.tagName === 'BUTTON') return refused('target_not_fillable', describe(el));
    if (el.tagName === 'SELECT') {
      const opts = Array.from(el.options);
      const match = opts.find(o => o.value === txt)
        || opts.find(o => o.textContent.trim() === txt)
        || opts.find(o => o.textContent.trim().toLowerCase() === txt.toLowerCase());
      if (!match) {
        return refused('option_missing', opts.slice(0, 20).map(o => o.value + ' (' + o.textContent.trim() + ')').join(', '));
      }
      const selectSetter = Object.getOwnPropertyDescriptor(view.HTMLSelectElement.prototype, 'value')?.set;
      if (selectSetter) selectSetter.call(el, match.value);
      else el.value = match.value;
      el.dispatchEvent(new view.Event('input', { bubbles: true }));
      el.dispatchEvent(new view.Event('change', { bubbles: true }));
      return { value: el.value, selectedLabel: match.textContent.trim() };
    }
    const proto = el.tagName === 'TEXTAREA' ? view.HTMLTextAreaElement.prototype : view.HTMLInputElement.prototype;
    const setter = Object.getOwnPropertyDescriptor(proto, 'value')?.set
      || Object.getOwnPropertyDescriptor(Object.getPrototypeOf(el), 'value')?.set;
    if (setter) setter.call(el, txt);
    else el.value = txt;
    try {
      el.dispatchEvent(new view.InputEvent('input', { bubbles: true, cancelable: true, inputType: 'insertText', data: txt }));
    } catch {
      el.dispatchEvent(new view.Event('input', { bubbles: true, cancelable: true }));
    }
    el.dispatchEvent(new view.Event('change', { bubbles: true }));
    return { contenteditable: false };
  }

  function editableText(sel) {
    const found = bySelector(sel);
    if (found.refusal) return found.refusal;
    if (!found.el || !found.el.isContentEditable) return refused('ref_stale', sel);
    return { text: found.el.innerText };
  }

  // The focused element's box, for the overlay of type and press.
  function focusRect() {
    let el = document.activeElement;
    if (!el || el === document.body || el === document.documentElement) return null;
    const box = el.getBoundingClientRect();
    return { x: box.left, y: box.top, width: box.width, height: box.height };
  }

  function activeOpaqueFrame() {
    const el = document.activeElement;
    if (!el || !/^(IFRAME|FRAME)$/.test(el.tagName)) return false;
    try { return !el.contentDocument; } catch { return true; }
  }

  function network(limit) {
    const entries = [...performance.getEntriesByType('navigation'), ...performance.getEntriesByType('resource')];
    const rows = entries.map((entry) => ({
      url: String(entry.name).slice(0, 300),
      type: entry.initiatorType || entry.entryType,
      status: entry.responseStatus || 0,
      ms: Math.round(entry.duration),
      bytes: entry.transferSize || entry.encodedBodySize || 0,
      start: entry.startTime,
    }));
    return { total: rows.length, rows: rows.slice(-limit).reverse() };
  }

  switch (op) {
    case 'probe': {
      const vv = window.visualViewport;
      return {
        visibility: document.visibilityState,
        url: location.href,
        width: window.innerWidth,
        height: window.innerHeight,
        dpr: window.devicePixelRatio,
        vv: vv ? { left: vv.offsetLeft, top: vv.offsetTop, width: vv.width, height: vv.height, scale: vv.scale }
          : { left: 0, top: 0, width: window.innerWidth, height: window.innerHeight, scale: 1 },
        scroll: { x: window.scrollX, y: window.scrollY },
      };
    }
    case 'tag': {
      const held = state();
      if (!held.tag) {
        const bytes = new Uint8Array(4);
        crypto.getRandomValues(bytes);
        held.tag = Array.from(bytes, b => 'abcdefghijkmnpqrstuvwxyz23456789'[b % 32]).join('');
      }
      return { tag: held.tag, origin: location.origin };
    }
    case 'baseline': {
      // Swap in the new --diff baseline of one snapshot kind and hand back
      // the old one.
      const held = state();
      held.baselines ||= {};
      const previous = held.baselines[args.key] ?? null;
      held.baselines[args.key] = args.text;
      return { previous };
    }
    case 'streak': {
      const held = state();
      held.streak = args.changed ? 0 : (held.streak || 0) + 1;
      return { streak: held.streak };
    }
    case 'rect': return rect(args.selector, args.scroll !== false);
    case 'box': {
      // Where an element is, without the click hit test: fill and scroll
      // act on it directly, and the overlay only needs its place.
      const found = bySelector(args.selector);
      if (found.refusal) return found.refusal;
      if (!found.el) return refused('ref_stale', args.selector);
      const box = found.el.getBoundingClientRect();
      let left = box.left;
      let top = box.top;
      for (let w = found.el.ownerDocument.defaultView || window; w !== w.parent && w.frameElement; w = w.parent) {
        const frameRect = w.frameElement.getBoundingClientRect();
        left += frameRect.left + w.frameElement.clientLeft;
        top += frameRect.top + w.frameElement.clientTop;
      }
      return { x: left, y: top, width: box.width, height: box.height };
    }
    case 'textTarget': return textTarget(args.text);
    case 'fill': return fill(args.selector, args.text);
    case 'editableText': return editableText(args.selector);
    case 'focusRect': return { rect: focusRect() };
    case 'activeOpaqueFrame': return { opaque: activeOpaqueFrame() };
    case 'hasFocus': return { focus: document.hasFocus() && !activeOpaqueFrame() };
    case 'scrollIntoView': {
      const found = bySelector(args.selector);
      if (found.refusal) return found.refusal;
      if (!found.el) return refused('ref_stale', args.selector);
      const before = { x: window.scrollX, y: window.scrollY };
      found.el.scrollIntoView({ block: 'center', inline: 'nearest' });
      return { dy: window.scrollY - before.y, dx: window.scrollX - before.x };
    }
    case 'scrollY': return { y: window.scrollY, height: window.innerHeight };
    case 'waitText': return { found: deepText(document).includes(String(args.text)) };
    case 'waitSelector': {
      const found = bySelector(args.selector);
      if (found.refusal) return found.refusal;
      return { found: found.el ? deepVisible(found.el) : false };
    }
    case 'network': return network(args.limit);
    default: throw new Error('unknown dom op ' + op);
  }
})
