/*
 * Based on chromux's record overlay (https://github.com/modakbul-gongbang/chromux,
 * commit 93f770f, chromux.mjs RECORD_OVERLAY_BOOTSTRAP_JS) for the operator's
 * view of `hide browser` actions: an accent arrow cursor that glides between
 * actions, a click ripple at its tip, a drag line, a field flash, a key label
 * and a scroll arrow, all gone about two seconds after the last action.
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
 * One function expression: (op, args) => result, evaluated in the top frame.
 * The host sits outside <body> with pointer-events off and aria-hidden, in a
 * closed shadow root: clicks pass through to the page (the operator can take
 * over at any time), the snapshot walk never reaches it, and page styles
 * cannot restyle it. Coordinates are top-viewport CSS pixels.
 */
((op, args) => {
  const HOST = 'hide-agent-overlay';
  const FADE_AFTER_MS = 2000;
  const ACCENT = '#84cc16';
  const INK = '#1c1f12';
  let host = document.documentElement.querySelector(':scope > ' + HOST);
  let overlay = host && Object.getOwnPropertyDescriptor(host, '__hideOverlay')?.value;
  if (!overlay) {
    host?.remove();
    host = document.createElement(HOST);
    host.setAttribute('aria-hidden', 'true');
    for (const [name, value] of [['position', 'fixed'], ['inset', '0'], ['pointer-events', 'none'], ['z-index', '2147483647'], ['display', 'block'], ['contain', 'strict'], ['margin', '0'], ['padding', '0'], ['border', '0'], ['background', 'transparent']]) {
      host.style.setProperty(name, value, 'important');
    }
    const root = host.attachShadow({ mode: 'closed' });
    root.innerHTML = `<style>
      :host { all: initial; }
      .layer { position: absolute; inset: 0; transition: opacity 240ms ease-out; }
      .layer.gone { opacity: 0; }
      .cursor { position: absolute; left: 0; top: 0; width: 22px; height: 28px; opacity: 0;
        transition: transform var(--glide, 0ms) cubic-bezier(.3,.7,.3,1), opacity 160ms ease-out; filter: drop-shadow(0 1px 2px rgba(0,0,0,.45)); }
      .cursor.shown { opacity: 1; }
      .cursor.pressed svg { transform: scale(.86); transform-origin: 2px 2px; }
      .ripple { position: absolute; width: 10px; height: 10px; margin: -5px 0 0 -5px; border-radius: 50%;
        border: 2px solid ${ACCENT}; animation: ripple 520ms ease-out forwards; }
      .flash { position: absolute; border: 2px solid ${ACCENT}; border-radius: 5px; box-shadow: 0 0 0 3px rgba(132,204,22,.25);
        animation: flash 700ms ease-out forwards; }
      .line { position: absolute; height: 3px; margin-top: -1.5px; border-radius: 2px; background: ${ACCENT}; transform-origin: 0 50%;
        box-shadow: 0 0 0 1px rgba(0,0,0,.25); transition: width var(--glide, 0ms) linear; }
      .key { position: absolute; padding: 3px 8px; border-radius: 6px; background: ${INK}; color: ${ACCENT};
        font: 600 12px/16px -apple-system, system-ui, sans-serif; border: 1px solid ${ACCENT}; white-space: nowrap; animation: pop 900ms ease-out forwards; }
      .arrow { position: absolute; right: 24px; width: 36px; height: 36px; margin-top: -18px; border-radius: 50%; background: ${INK};
        color: ${ACCENT}; font: 700 22px/36px system-ui, sans-serif; text-align: center; animation: pop 900ms ease-out forwards; }
      @keyframes ripple { from { transform: scale(1); opacity: 1; } to { transform: scale(4.2); opacity: 0; } }
      @keyframes flash { from { opacity: 1; } to { opacity: 0; } }
      @keyframes pop { 0% { opacity: 0; transform: translateY(4px); } 15% { opacity: 1; transform: none; } 75% { opacity: 1; } 100% { opacity: 0; } }
    </style><div class="layer"><div class="cursor"><svg width="22" height="28" viewBox="0 0 22 28" aria-hidden="true">
      <path d="M2 2 L2 22 L7.5 17 L11 25.5 L14.6 24 L11.2 15.8 L18.5 15.8 Z" fill="${ACCENT}" stroke="${INK}" stroke-width="1.6" stroke-linejoin="round"/>
    </svg></div></div>`;
    document.documentElement.appendChild(host);
    const layer = root.querySelector('.layer');
    overlay = { root, layer, cursor: root.querySelector('.cursor'), at: null, timer: 0 };
    Object.defineProperty(host, '__hideOverlay', { value: overlay, enumerable: false });
  }
  const { layer, cursor } = overlay;
  layer.classList.remove('gone');
  clearTimeout(overlay.timer);
  overlay.timer = setTimeout(() => {
    layer.classList.add('gone');
    overlay.timer = setTimeout(() => host.remove(), 260);
  }, FADE_AFTER_MS);
  const add = (cls, style, text, lifetime) => {
    const node = document.createElement('div');
    node.className = cls;
    node.style.cssText = style;
    if (text) node.textContent = text;
    layer.appendChild(node);
    if (lifetime) setTimeout(() => node.remove(), lifetime);
    return node;
  };
  const place = (x, y, ms) => {
    cursor.style.setProperty('--glide', ms + 'ms');
    cursor.style.transform = `translate(${x - 2}px, ${y - 2}px)`;
    overlay.at = { x, y };
  };
  switch (op) {
    case 'move': {
      // Glide from the last position, or arrive from a short distance the
      // first time, so the operator sees where the agent went.
      const from = overlay.at || { x: args.x + 48, y: args.y + 48 };
      place(from.x, from.y, 0);
      cursor.getBoundingClientRect();
      cursor.classList.add('shown');
      const distance = Math.hypot(args.x - from.x, args.y - from.y);
      const ms = Math.round(Math.min(420, Math.max(160, 140 + distance * 0.35)));
      place(args.x, args.y, ms);
      return { ms };
    }
    case 'click': add('ripple', `left:${args.x}px;top:${args.y}px`, '', 600); return {};
    case 'drag': {
      const line = add('line', `left:${args.x1}px;top:${args.y1}px;width:0;--glide:${args.ms}ms;transform:rotate(${Math.atan2(args.y2 - args.y1, args.x2 - args.x1)}rad)`);
      line.getBoundingClientRect();
      line.style.width = Math.hypot(args.x2 - args.x1, args.y2 - args.y1) + 'px';
      cursor.classList.add('pressed');
      place(args.x2, args.y2, args.ms);
      setTimeout(() => cursor.classList.remove('pressed'), args.ms);
      return {};
    }
    case 'flash': add('flash', `left:${args.x - 3}px;top:${args.y - 3}px;width:${args.width + 2}px;height:${args.height + 2}px`, '', 760); return {};
    case 'key': {
      const x = args.rect ? args.rect.x + args.rect.width + 8 : (overlay.at?.x ?? window.innerWidth / 2) + 18;
      const y = args.rect ? args.rect.y + Math.max(0, args.rect.height / 2 - 11) : (overlay.at?.y ?? window.innerHeight / 2) + 18;
      add('key', `left:${Math.min(x, window.innerWidth - 120)}px;top:${y}px`, args.name, 950);
      return {};
    }
    case 'scroll': add('arrow', `top:${window.innerHeight / 2}px`, args.direction === 'up' ? '↑' : '↓', 950); return {};
    default: throw new Error('unknown overlay op ' + op);
  }
})
