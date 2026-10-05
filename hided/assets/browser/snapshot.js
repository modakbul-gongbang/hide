/*
 * Ported from chromux (https://github.com/modakbul-gongbang/chromux, commit
 * 93f770f, chromux.mjs SNAPSHOT_JS) for `hide browser snapshot`.
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
 * One function expression: (FILTER, CLICKABLE, REDACT_FIELDS) => snapshot text.
 * FILTER 'interactive' keeps only refs; CLICKABLE 'auto' | 'on' | 'off' |
 * 'stable'; REDACT_FIELDS hides values and paths (cross-origin frames).
 * Refs live in the page as data-ct-ref, so a stateless client keeps them.
 */
((FILTER, CLICKABLE, REDACT_FIELDS) => {
  const INTERACTIVE_ONLY = FILTER === 'interactive';
  // Behavior-based clickable detection ('auto' | 'on' | 'off'). Many SPAs and
  // micro-UIs build their controls from bare divs with click handlers - no
  // roles, no semantic tags - which makes an accessibility snapshot blind.
  // 'auto' turns detection on when the page is nearly dead (almost no
  // standard interactive elements) OR when behaviorally-clickable candidates
  // are dense relative to standard controls (div-heavy content behind a
  // standard nav). Ordinary link/button pages pay zero extra payload.
  const STANDARD_SEL = 'a[href],button,input,select,textarea,[role="button"],[role="link"],[role="tab"],[role="menuitem"]';
  const standardCount = [...document.querySelectorAll(STANDARD_SEL)]
    .filter(el => { const r = el.getBoundingClientRect(); return r.width > 0 && r.height > 0; }).length;
  function inViewport(rect) {
    return rect.bottom > 0 && rect.top < window.innerHeight && rect.right > 0 && rect.left < window.innerWidth;
  }
  function sameGeometry(a, b) {
    return Math.abs(a.left - b.left) < 2 && Math.abs(a.top - b.top) < 2
      && Math.abs(a.width - b.width) < 2 && Math.abs(a.height - b.height) < 2;
  }
  function isClickableBoundary(el, style) {
    // An inline onclick handler is the one behavioral signal readable from
    // the page itself; listener-only controls are out of reach (the scoped
    // gateway has no DOMDebugger).
    const hasHandler = el.hasAttribute('onclick');
    if (!hasHandler) {
      if (style.cursor !== 'pointer') return false;
      const parent = el.parentElement;
      if (parent && parent !== document.body) {
        try { if (getComputedStyle(parent).cursor === 'pointer') return false; } catch {}
      }
    } else {
      // A handler-bearing element whose clickable parent (handler or pointer
      // cursor) has the same geometry is the same control bound twice
      // (delegation patterns); keep only the outer one. Distinct nested
      // controls (a star icon inside a clickable row) differ in geometry
      // and stay visible.
      const p = el.parentElement;
      if (p) {
        let parentClickable = p.hasAttribute('onclick');
        if (!parentClickable) {
          try { parentClickable = getComputedStyle(p).cursor === 'pointer'; } catch {}
        }
        if (parentClickable && sameGeometry(el.getBoundingClientRect(), p.getBoundingClientRect())) return false;
      }
    }
    // A clickable wrapper is redundant only when a standard control inside it
    // covers roughly the same area - a product card with a small wishlist
    // button is still its own control and must keep its ref.
    const inner = el.querySelector(STANDARD_SEL);
    if (inner) {
      const r = el.getBoundingClientRect();
      const ir = inner.getBoundingClientRect();
      if (ir.width * ir.height >= r.width * r.height * 0.5) return false;
    }
    return true;
  }
  // Scroll-invariant capping for verify diff baselines ('stable'): a viewport
  // dependent candidate set would make every scroll look like a page change.
  const CLICK_STABLE = CLICKABLE === 'stable';
  let CLICK_ON = CLICKABLE === 'on' || CLICK_STABLE;
  if (!CLICK_ON && CLICKABLE !== 'off') {
    if (standardCount < 3) {
      CLICK_ON = true;
    } else {
      // Ratio gate, viewport-scoped: probe visible-in-viewport container-ish
      // elements for non-redundant clickable candidates and compare against
      // the standard controls currently in the viewport. Offscreen content
      // does not vote - scrolling re-evaluates the gate for what is now
      // visible. Bounded so mega-DOM pages pay a fixed cost.
      let inViewStandard = 0;
      for (const el of document.querySelectorAll(STANDARD_SEL)) {
        const r = el.getBoundingClientRect();
        if (r.width > 0 && r.height > 0 && inViewport(r)) inViewStandard++;
      }
      let candidates = 0;
      let styleChecked = 0;
      let iterated = 0;
      const enough = () => candidates >= 4 && candidates * 8 >= inViewStandard;
      for (const el of document.querySelectorAll('div,span,li,td,img,i,b,p,label,section,article')) {
        if (iterated++ >= 5000 || styleChecked >= 400 || enough()) break;
        const r = el.getBoundingClientRect();
        if (r.width < 8 || r.height < 8 || !inViewport(r)) continue;
        styleChecked++;
        try { if (isClickableBoundary(el, getComputedStyle(el))) candidates++; } catch {}
      }
      CLICK_ON = enough();
    }
  }
  // Snapshot caps are viewport-first: what the user can see must never be
  // starved of clickable refs by document-order-earlier offscreen candidates.
  // Verify baselines instead use a document-order cap so the set does not
  // flap with scroll position.
  const CLICK_CAP_VIEWPORT = 40;
  const CLICK_CAP_OFFSCREEN = 10;
  const CLICK_CAP_STABLE = 50;
  let clickInView = 0;
  let clickOffscreen = 0;
  let clickStable = 0;
  function takeClickSlot(el) {
    if (CLICK_STABLE) {
      if (clickStable >= CLICK_CAP_STABLE) return false;
      clickStable++;
      return true;
    }
    if (inViewport(el.getBoundingClientRect())) {
      if (clickInView >= CLICK_CAP_VIEWPORT) return false;
      clickInView++;
      return true;
    }
    if (clickOffscreen >= CLICK_CAP_OFFSCREEN) return false;
    clickOffscreen++;
    return true;
  }
  // Occlusion probe: if one element sits on top of most of the page's
  // standard controls (sync covers, cookie walls, modals, loading scrims),
  // it is the thing the agent must deal with first - surface it prominently
  // even on pages where clickable detection stays off.
  let OCCLUDER = null;
  (() => {
    // Probe standard controls whose center sits inside the viewport
    // (offscreen controls are excluded, not clamped), sampled across
    // top/middle/bottom bands so a header-sparing modal is caught and a
    // bottom-only consent bar is not mistaken for a page-wide overlay.
    const probes = [];
    for (const el of document.querySelectorAll('a[href],button,input,select,textarea')) {
      const r = el.getBoundingClientRect();
      if (r.width < 2 || r.height < 2) continue;
      const x = r.left + r.width / 2;
      const y = r.top + r.height / 2;
      if (x < 0 || y < 0 || x >= window.innerWidth || y >= window.innerHeight) continue;
      probes.push({ el, x, y });
      if (probes.length >= 60) break;
    }
    if (probes.length < 2) return;
    const bands = [[], [], []];
    for (const p of probes) {
      const bandIndex = Math.min(2, Math.floor(p.y * 3 / window.innerHeight));
      p.band = bandIndex;
      bands[bandIndex].push(p);
    }
    const sample = [];
    for (const band of bands) {
      const step = Math.max(1, Math.floor(band.length / 4));
      for (let i = 0, taken = 0; i < band.length && taken < 4; i += step, taken++) sample.push(band[i]);
    }
    const hits = new Map();
    for (const p of sample) {
      const top = document.elementFromPoint(p.x, p.y);
      if (!top || top === p.el || p.el.contains(top) || top.contains(p.el)) continue;
      // Attribute the hit to the outermost covering ancestor that still does
      // not contain the probed control, so one dialog's many children tally
      // as a single occluder instead of splitting the count.
      let node = top;
      while (node.parentElement && !node.parentElement.contains(p.el)) node = node.parentElement;
      const entry = hits.get(node) || { count: 0, bands: new Set() };
      entry.count += 1;
      entry.bands.add(p.band);
      hits.set(node, entry);
    }
    let best = null;
    let bestEntry = null;
    for (const [el, entry] of hits) {
      if (!bestEntry || entry.count > bestEntry.count) { best = el; bestEntry = entry; }
    }
    if (!best || bestEntry.count < Math.max(2, Math.ceil(sample.length * 0.5))) return;
    // "Covers page" is a strong directive: when all probes live in one band
    // (header-only pages), demand that the covering element itself is
    // page-sized before promoting a local strip/ribbon to a page-wide
    // overlay.
    if (bestEntry.bands.size < 2) {
      let area = 0;
      try {
        const r = best.getBoundingClientRect();
        area = r.width * r.height;
      } catch {}
      if (area < window.innerWidth * window.innerHeight * 0.4) return;
    }
    OCCLUDER = best;
  })();
  // Refs are stable within a document: an element keeps its data-ct-ref across
  // re-snapshots, and new elements continue from the persisted counter. A
  // navigation replaces the document, so refs naturally reset to @1.
  let refMax = Number(document.documentElement.getAttribute('data-ct-ref-max')) || 0;
  const ROLES = {
    a:'link', button:'button', input:'textbox', select:'combobox',
    textarea:'textbox', img:'img', nav:'navigation', main:'main',
    header:'banner', footer:'contentinfo', form:'form',
    h1:'heading', h2:'heading', h3:'heading',
    h4:'heading', h5:'heading', h6:'heading',
    ul:'list', ol:'list', li:'listitem',
    table:'table', tr:'row', td:'cell', th:'columnheader',
    dialog:'dialog', section:'region', aside:'complementary',
  };
  const INTERACTIVE = new Set(['a','button','input','select','textarea']);
  function isEditableRoot(el) {
    const value = el.getAttribute('contenteditable');
    return value !== null && value.toLowerCase() !== 'false';
  }
  function getRole(el) {
    if (isEditableRoot(el)) return el.getAttribute('role') || 'textbox';
    return el.getAttribute('role') || ROLES[el.tagName.toLowerCase()] || null;
  }
  function isInteractive(el) {
    const tag = el.tagName.toLowerCase();
    if (isEditableRoot(el)) return true;
    if (INTERACTIVE.has(tag)) return true;
    const role = el.getAttribute('role');
    if (role === 'button' || role === 'link' || role === 'tab' || role === 'menuitem') return true;
    if (el.getAttribute('tabindex') !== null && el.getAttribute('tabindex') !== '-1') return true;
    return false;
  }
  // Never leak typed secrets into snapshot text. Password inputs are always
  // masked; card numbers, CVCs, OTPs, and national IDs usually arrive as
  // type=text|tel, so mask by autocomplete/name/id heuristics too.
  function isSensitiveInput(el) {
    if (el.type === 'password') return true;
    // Normalize separators so otp_code / otpCode-ish spellings hit the same
    // word boundaries as otp-code.
    const hints = ((el.getAttribute('autocomplete') || '') + ' ' + (el.name || '') + ' ' + (el.id || ''))
      .toLowerCase().replace(/[_\s]+/g, '-');
    return /cc-number|cc-csc|cc-exp|card-?(number|no)|cardnumber|cvv|cvc|one-?time-?code|\botp\b|otpcode|verification-?code|ssn|social-?security|\bpin\b|pincode|passport|routing-?number|iban|passw|secret|token|api-?key|\brrn\b|resident/.test(hints);
  }
  function getLabel(el, clickable) {
    const tag = el.tagName.toLowerCase();
    const aria = el.getAttribute('aria-label');
    if (REDACT_FIELDS && isEditableRoot(el)) return aria || '';
    if (REDACT_FIELDS && (tag === 'input' || tag === 'textarea' || tag === 'select')) {
      return aria || el.placeholder || '';
    }
    if ((tag === 'input' || tag === 'textarea' || tag === 'select') && isSensitiveInput(el)) {
      return aria || el.placeholder || '';
    }
    if (aria) return aria;
    if (tag === 'input' || tag === 'textarea') return el.value || el.placeholder || '';
    if (tag === 'select') return el.selectedOptions && el.selectedOptions[0] ? el.selectedOptions[0].textContent.trim() : '';
    if (tag === 'img') return el.alt || '';
    if (tag === 'iframe' || tag === 'frame') return el.title || '';
    let text = '';
    for (const n of el.childNodes) { if (n.nodeType === 3) text += n.textContent; }
    text = text.trim();
    // Links/buttons (and behaviorally-clickable containers) often wrap their
    // text in child elements; fall back to rendered innerText so snapshot
    // lines stay identifiable without a follow-up js().
    if (!text && (tag === 'a' || tag === 'button' || clickable)) {
      text = (el.innerText || '').trim().split('\n').map(s => s.trim()).filter(Boolean).join(' / ');
    }
    return text.substring(0, 100);
  }
  function walk(el, depth) {
    if (!el || el.nodeType !== 1) return '';
    let style;
    try {
      // Elements inside same-origin iframes must be styled by their own
      // window, not the top one.
      style = (el.ownerDocument.defaultView || window).getComputedStyle(el);
      if (style.display === 'none' || style.visibility === 'hidden' || el.hidden) return '';
    } catch { return ''; }
    if (el.getAttribute('aria-hidden') === 'true') return '';
    const tag = el.tagName.toLowerCase();
    if (['script','style','noscript','br','hr','svg','path'].includes(tag)) return '';
    const role = getRole(el);
    let interactive = isInteractive(el);
    let clickable = false;
    let overlay = false;
    let innerFrameDoc = null;
    let opaqueFrame = null;
    if (tag === 'iframe' || tag === 'frame') {
      try { innerFrameDoc = el.contentDocument; } catch {}
      if (!innerFrameDoc || !innerFrameDoc.body) {
        interactive = true;
        let origin = 'opaque';
        try {
          const parsed = new URL(el.getAttribute('src') || '', el.ownerDocument.location.href);
          if (parsed.origin && parsed.origin !== 'null') origin = parsed.origin;
        } catch {}
        const rect = el.getBoundingClientRect();
        let x = rect.left;
        let y = rect.top;
        let view = el.ownerDocument.defaultView || window;
        while (view !== view.parent && view.frameElement) {
          const frameRect = view.frameElement.getBoundingClientRect();
          x += frameRect.left + view.frameElement.clientLeft;
          y += frameRect.top + view.frameElement.clientTop;
          view = view.parent;
        }
        opaqueFrame = {
          origin,
          rect: [x, y, rect.width, rect.height].map(value => Math.round(value * 100) / 100),
        };
      }
    }
    if (!interactive && el === OCCLUDER) {
      interactive = true;
      clickable = true;
      overlay = true;
    }
    if (!interactive && CLICK_ON && isClickableBoundary(el, style) && takeClickSlot(el)) {
      interactive = true;
      clickable = true;
    }
    const label = getLabel(el, clickable);
    const has = role || interactive || label;
    const keep = INTERACTIVE_ONLY ? interactive : has;
    const cd = keep ? depth + 1 : depth;
    let children = '';
    if (tag === 'iframe' || tag === 'frame') {
      // Same-origin frames are walked like page content; cross-origin frames
      // expose only origin and geometry. Paths, queries, and child field values
      // stay behind the origin boundary.
      if (innerFrameDoc && innerFrameDoc.body) children = walk(innerFrameDoc.body, cd);
    } else if (el.shadowRoot) {
      // Flattened-tree walk: an open shadow root replaces the host's light
      // children; slotted light content re-enters through <slot> below.
      // Closed shadow roots stay invisible (no API to reach them).
      for (const c of el.shadowRoot.children) children += walk(c, cd);
    } else if (tag === 'slot') {
      let assigned = [];
      try { assigned = el.assignedElements(); } catch {}
      for (const c of assigned) children += walk(c, cd);
    } else {
      for (const c of el.children) children += walk(c, cd);
    }
    if (!keep && !children) return '';
    if (!keep) return children;
    const indent = '  '.repeat(depth);
    let line = indent;
    if (interactive) {
      let ref = Number(el.getAttribute('data-ct-ref')) || 0;
      if (!ref) {
        ref = ++refMax;
        el.setAttribute('data-ct-ref', String(ref));
      } else if (ref > refMax) {
        refMax = ref;
      }
      line += '@' + ref + ' ';
    }
    line += opaqueFrame
      ? 'iframe (cross-origin opaque)'
      : overlay ? 'overlay (covers page; interact or dismiss first)'
      : clickable ? (role ? role + ' (clickable)' : 'clickable') : (role || tag);
    if (label) line += ' "' + label.replace(/\s+/g, ' ').trim().replace(/"/g, '\\"') + '"';
    else if (clickable) {
      // Icon-only clickables: developer-facing id/class names are the best
      // available handle ("#close-email", ".star.clicked" - state included).
      if (el.id) line += ' #' + el.id;
      else if (typeof el.className === 'string' && el.className.trim()) {
        line += ' ' + el.className.trim().split(/\s+/).slice(0, 2).map(c => '.' + c).join('');
      }
    }
    if (tag === 'input') {
      const checkish = el.type === 'checkbox' || el.type === 'radio';
      line += ' [' + (el.type || 'text') + (checkish && el.checked ? ' checked' : '') + ']';
    }
    if (!REDACT_FIELDS && tag === 'select' && el.selectedOptions && el.selectedOptions[0] && !isSensitiveInput(el)) {
      const sel = el.selectedOptions[0].textContent.replace(/\s+/g, ' ').trim().substring(0, 40);
      if (sel && sel !== label) line += ' = "' + sel.replace(/"/g, '') + '"';
    }
    if (el.disabled) line += ' (disabled)';
    if (opaqueFrame) {
      line += ' origin=' + opaqueFrame.origin + ' rect=[' + opaqueFrame.rect.join(',') + '] CSS';
    }
    if (tag === 'a' && el.href) {
      const href = el.getAttribute('href');
      if (href && !href.startsWith('javascript:') && !href.startsWith('#')) {
        let shownHref = href;
        if (REDACT_FIELDS) {
          try {
            const parsed = new URL(href, el.ownerDocument.location.href);
            shownHref = parsed.origin && parsed.origin !== 'null' ? parsed.origin : 'opaque';
          } catch {
            shownHref = 'opaque';
          }
        }
        line += ' -> ' + shownHref.replace(/\s+/g, ' ').trim().substring(0, 80);
      }
    }
    // One element is one line, whatever its id, role or text holds: a line
    // break would let page text forge lines of its own.
    return line.replace(/[\r\n\u2028\u2029]+/g, ' ') + '\n' + children;
  }
  // Cap the URL line: data:/blob: URLs can be tens of KB and would drown the
  // snapshot (and every diff computed from it) in address noise. A short hash
  // of the FULL URL keeps diff navigation detection exact for long URLs that
  // share a 300-char prefix.
  let urlLine = location.href;
  if (urlLine.length > 300) {
    let hash = 0x811c9dc5;
    for (let i = 0; i < location.href.length; i++) {
      hash ^= location.href.charCodeAt(i);
      hash = Math.imul(hash, 0x01000193) >>> 0;
    }
    urlLine = urlLine.substring(0, 300) + '…#' + hash.toString(16);
  }
  const out = '# ' + document.title + '\n# ' + urlLine + '\n\n' + walk(document.body, 0);
  document.documentElement.setAttribute('data-ct-ref-max', String(refMax));
  return out;
})
