#!/usr/bin/env node
// Flatten a class-based canvas artboard into the inline-styled HTML Paper accepts.
//
// Paper's write_html reads inline styles only: a <style> block is discarded and the
// elements that depended on it are dropped entirely, and <use href="#id"> renders
// nothing because the <defs> never survive. This resolves both ahead of time.
//
//   node scripts/paper-inline.mjs <body.html> [--css _shared.css] [--icons _icons.html]
//
// Writes the converted markup to stdout and every unconvertible construct to stderr.

import fs from 'node:fs';
import path from 'node:path';

const args = process.argv.slice(2);
const bodyPath = args.find((a) => !a.startsWith('--'));
if (!bodyPath) {
  console.error('usage: paper-inline.mjs <body.html> [--css <file>] [--icons <file>]');
  process.exit(1);
}
const flag = (name, fallback) => {
  const i = args.indexOf(`--${name}`);
  return i === -1 ? fallback : args[i + 1];
};
const dir = path.dirname(bodyPath);
const cssPath = flag('css', path.join(dir, '_shared.css'));
const iconsPath = flag('icons', path.join(dir, '_icons.html'));

const warnings = [];

// --- Paper token names -------------------------------------------------------
// The canvas files carry their own short custom-property names. The Paper file
// carries the token set generated from HideTheme.swift, so rename rather than
// re-declare, and a value with no token stays a literal.
const TOKEN = {
  background: 'color-background', sidebar: 'color-sidebar', panel: 'color-panel',
  elevated: 'color-elevated', balloon: 'color-balloon', divider: 'color-divider',
  primary: 'color-primary', secondary: 'color-secondary', muted: 'color-muted',
  accent: 'color-accent', danger: 'color-danger', warning: 'color-warning',
  working: 'color-agent-working', success: 'color-success',
};

// --- properties Paper drops or mis-handles -----------------------------------
const DROP = new Set([
  'box-sizing', 'transition', 'cursor', 'outline', 'appearance', '-webkit-appearance',
  '-webkit-font-smoothing', 'font-feature-settings', 'user-select', '-webkit-user-select',
  'list-style', 'text-decoration-skip-ink', 'scrollbar-width',
]);
const REJECTED = new Set(['margin', 'margin-top', 'margin-right', 'margin-bottom', 'margin-left']);

// --- CSS ---------------------------------------------------------------------
function parseCss(src) {
  const rules = [];
  const stripped = src.replace(/\/\*[\s\S]*?\*\//g, '');
  const re = /([^{}]+)\{([^{}]*)\}/g;
  let m;
  while ((m = re.exec(stripped))) {
    const selectors = m[1].split(',').map((s) => s.trim()).filter(Boolean);
    const decls = {};
    for (const part of m[2].split(';')) {
      const idx = part.indexOf(':');
      if (idx === -1) continue;
      decls[part.slice(0, idx).trim()] = part.slice(idx + 1).trim();
    }
    for (const sel of selectors) rules.push({ sel, decls });
  }
  return rules;
}

// Selector forms the canvas files actually use: .a / .a.b / .a tag / .a .b / tag.
// Anything else is reported rather than silently mismatched.
function compile(sel) {
  if (sel === ':root' || sel === '*' || sel === 'body' || sel === 'a') return null;
  if (/::?(after|before|hover|focus|active)/.test(sel)) {
    warnings.push(`pseudo selector not portable, skipped: ${sel}`);
    return null;
  }
  const parts = sel.split(/\s+/).filter(Boolean);
  const seg = (s) => {
    const classes = [...s.matchAll(/\.([A-Za-z0-9_-]+)/g)].map((x) => x[1]);
    const tag = s.startsWith('.') ? null : s.split('.')[0].toLowerCase();
    return { tag, classes };
  };
  return { key: parts.map(seg), depth: parts.length };
}

function segMatches(seg, el) {
  if (seg.tag && el.tag !== seg.tag) return false;
  return seg.classes.every((c) => el.classes.has(c));
}

// --- icons -------------------------------------------------------------------
function parseIcons(src) {
  const map = new Map();
  // the canvas files keep icons as <g id="..."> inside one <defs>, not <symbol>
  const re = /<(symbol|g)\b([^>]*)>([\s\S]*?)<\/\1>/g;
  let m;
  while ((m = re.exec(src))) {
    const id = /id="([^"]+)"/.exec(m[2]);
    const viewBox = /viewBox="([^"]+)"/.exec(m[2]);
    if (id) map.set(id[1], { inner: m[3].trim(), viewBox: viewBox ? viewBox[1] : '0 0 16 16' });
  }
  return map;
}

// --- HTML --------------------------------------------------------------------
const VOID = new Set(['area', 'base', 'br', 'col', 'embed', 'hr', 'img', 'input',
  'link', 'meta', 'param', 'source', 'track', 'wbr', 'use', 'path', 'circle',
  'rect', 'line', 'polyline', 'polygon', 'ellipse', 'stop']);

function parseHtml(src) {
  const root = { tag: '#root', attrs: {}, children: [], parent: null };
  let cur = root;
  const re = /<!--[\s\S]*?-->|<\/([A-Za-z][\w:-]*)\s*>|<([A-Za-z][\w:-]*)((?:[^>"']|"[^"]*"|'[^']*')*?)(\/?)>|([^<]+)/g;
  let m;
  while ((m = re.exec(src))) {
    const [full, closeTag, openTag, rawAttrs, selfClose, text] = m;
    if (full.startsWith('<!--')) continue;
    if (closeTag) {
      if (cur.parent) cur = cur.parent;
      continue;
    }
    if (openTag) {
      const attrs = {};
      const ar = /([\w:-]+)(?:\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s"'>]+)))?/g;
      let a;
      while ((a = ar.exec(rawAttrs))) attrs[a[1]] = a[2] ?? a[3] ?? a[4] ?? '';
      const node = { tag: openTag.toLowerCase(), attrs, children: [], parent: cur };
      cur.children.push(node);
      if (!selfClose && !VOID.has(node.tag)) cur = node;
      continue;
    }
    if (text && text.trim()) cur.children.push({ tag: '#text', text });
  }
  return root;
}

function renameVars(value) {
  return value.replace(/var\(--([A-Za-z0-9_-]+)\)/g, (whole, name) =>
    TOKEN[name] ? `var(--${TOKEN[name]})` : whole);
}

function fixFont(value) {
  if (!/SF Mono|SFMono|ui-monospace|Menlo|monospace/.test(value)) return value;
  return "'JetBrains Mono', monospace";
}

function serializeStyle(decls) {
  const out = [];
  for (const [prop, raw] of Object.entries(decls)) {
    if (DROP.has(prop)) continue;
    if (REJECTED.has(prop)) {
      warnings.push(`margin is not supported by Paper, dropped: ${prop}: ${raw}`);
      continue;
    }
    // the `font` shorthand does not survive; expand it into the longhands Paper reads
    if (prop === 'font') {
      const sh = /^\s*(\d{3}|bold|normal)?\s*(\d+(?:\.\d+)?px)(?:\s*\/\s*([\d.]+\w*))?\s+(.+)$/.exec(raw);
      if (sh) {
        if (sh[1]) out.push(`font-weight:${sh[1]}`);
        out.push(`font-size:${sh[2]}`);
        if (sh[3]) out.push(`line-height:${sh[3]}`);
        out.push(`font-family:${fixFont(sh[4])}`);
      } else {
        out.push(`font-family:${fixFont(raw)}`);
      }
      continue;
    }
    let v = renameVars(raw);
    if (prop === 'font-family') v = fixFont(v);
    if (prop === 'border-radius' && v === '50%') v = '9999px';
    out.push(`${prop}:${v}`);
  }
  return out.join('; ');
}

function esc(s) {
  return s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');
}

// --- main --------------------------------------------------------------------
const rules = parseCss(fs.readFileSync(cssPath, 'utf8'))
  .map((r) => ({ ...r, m: compile(r.sel) }))
  .filter((r) => r.m);
const icons = fs.existsSync(iconsPath) ? parseIcons(fs.readFileSync(iconsPath, 'utf8')) : new Map();

function ancestry(node) {
  const chain = [];
  for (let p = node; p; p = p.parent) {
    if (p.tag === '#root' || p.tag === '#text') continue;
    chain.unshift({ tag: p.tag, classes: new Set((p.attrs.class || '').split(/\s+/).filter(Boolean)) });
  }
  return chain;
}

function matched(node) {
  const chain = ancestry(node);
  const self = chain[chain.length - 1];
  const decls = {};
  for (const { m, decls: d } of rules) {
    const segs = m.key;
    if (!segMatches(segs[segs.length - 1], self)) continue;
    // walk remaining segments upward through the ancestor chain, in order
    let ok = true;
    let i = chain.length - 2;
    for (let s = segs.length - 2; s >= 0; s--) {
      let found = false;
      while (i >= 0) {
        if (segMatches(segs[s], chain[i])) { found = true; i--; break; }
        i--;
      }
      if (!found) { ok = false; break; }
    }
    if (ok) Object.assign(decls, d);
  }
  return decls;
}

let usedIcons = 0;
let missingIcons = 0;

// class rules plus the element's own inline style, which wins
function resolved(node) {
  const decls = matched(node);
  for (const part of (node.attrs?.style || '').split(';')) {
    const i = part.indexOf(':');
    if (i > -1) decls[part.slice(0, i).trim()] = part.slice(i + 1).trim();
  }
  return decls;
}

// nearest `color` on the way up, class rules and inline style both counted
function resolveColor(node) {
  for (let p = node; p; p = p.parent) {
    if (p.tag === '#root' || p.tag === '#text') continue;
    const inline = /(?:^|;)\s*color\s*:\s*([^;]+)/.exec(p.attrs?.style || '');
    if (inline) return renameVars(inline[1].trim());
    const c = matched(p).color;
    if (c) return renameVars(c.trim());
  }
  return 'var(--color-secondary)';
}

function render(node, depth = 0, pre = false) {
  // collapsing whitespace inside white-space:pre would fold a terminal transcript onto one line
  if (node.tag === '#text') return esc(pre ? node.text : node.text.replace(/\s+/g, ' '));
  if (node.tag === '#root') return node.children.map((c) => render(c, depth, pre)).join('');
  if (node.tag === 'style' || node.tag === 'defs' || node.tag === 'symbol') return '';
  void depth;

  // <use href="#id"> carries no geometry once the defs are gone; splice the symbol in.
  if (node.tag === 'use') {
    const ref = (node.attrs.href || node.attrs['xlink:href'] || '').replace(/^#/, '');
    const sym = icons.get(ref);
    if (!sym) { missingIcons++; warnings.push(`no symbol for <use href="#${ref}">`); return ''; }
    usedIcons++;
    // currentColor resolves against an inherited `color` Paper will not carry down
    return sym.inner.replace(/currentColor/g, resolveColor(node));
  }

  const decls = matched(node);
  // the element's own inline style wins over anything a class contributed
  if (node.attrs.style) {
    for (const part of node.attrs.style.split(';')) {
      const i = part.indexOf(':');
      if (i > -1) decls[part.slice(0, i).trim()] = part.slice(i + 1).trim();
    }
  }
  // A browser sizes a bare text span to its content; Paper lets a flex row squeeze it
  // until the label breaks one letter per line. Pin short labels open.
  // Only in a row: a column's children are meant to wrap (descriptions, log lines).
  const isPre = pre || decls['white-space'] === 'pre' || decls['white-space'] === 'pre-wrap' || node.tag === 'pre';
  const textOnly = node.children.length > 0 && node.children.every((c) => c.tag === '#text');
  const sized = 'width' in decls || 'flex' in decls || 'flex-basis' in decls;
  const parentDecls = node.parent && node.parent.tag !== '#root' ? resolved(node.parent) : {};
  const inRow = parentDecls.display === 'flex' && parentDecls['flex-direction'] !== 'column';
  if (textOnly && inRow && !sized && !isPre && !('white-space' in decls)) {
    decls['white-space'] = 'nowrap';
  }

  const style = serializeStyle(decls);

  const attrs = [];
  const name = node.attrs.class ? node.attrs.class.split(/\s+/)[0] : null;
  if (name && node.tag === 'div') attrs.push(`layer-name="${esc(name)}"`);
  for (const [k, v] of Object.entries(node.attrs)) {
    if (k === 'class' || k === 'style') continue;
    if (k === 'viewBox' && node.tag !== 'svg') continue;
    attrs.push(v === '' ? k : `${k}="${esc(v)}"`);
  }
  // an svg that only referenced a symbol needs that symbol's viewBox
  if (node.tag === 'svg' && !node.attrs.viewBox) {
    const u = node.children.find((c) => c.tag === 'use');
    const ref = u && (u.attrs.href || u.attrs['xlink:href'] || '').replace(/^#/, '');
    const sym = ref && icons.get(ref);
    if (sym && sym.viewBox) attrs.push(`viewBox="${sym.viewBox}"`);
  }
  if (style) attrs.push(`style="${esc(style)}"`);

  // <button> and <i> are not Paper primitives; emit them as plain boxes
  const tag = node.tag === 'button' || node.tag === 'i' || node.tag === 'b'
    ? 'div' : node.tag;

  const inner = node.children.map((c) => render(c, depth + 1, isPre)).join('');
  if (VOID.has(node.tag) && !inner) return `<${tag} ${attrs.join(' ')} />`;
  return `<${tag}${attrs.length ? ' ' + attrs.join(' ') : ''}>${inner}</${tag}>`;
}

const html = render(parseHtml(fs.readFileSync(bodyPath, 'utf8')));
process.stdout.write(html + '\n');

const seen = new Set();
const unique = warnings.filter((w) => (seen.has(w) ? false : seen.add(w)));
if (unique.length) {
  process.stderr.write(`\n${unique.length} unconvertible construct(s):\n`);
  for (const w of unique) process.stderr.write(`  - ${w}\n`);
}
process.stderr.write(`\nicons inlined: ${usedIcons}, missing: ${missingIcons}\n`);
