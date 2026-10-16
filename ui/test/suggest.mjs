// --- omnibox suggest ---
// Omnibox dropdown contract: the Rust fixture (written by protocol.rs suggest_messages_write_the_ui_fixture)
// parses, malformed messages are rejected, outbound commands are validated, and the selection rules hold.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import * as esbuild from 'esbuild';

const here = dirname(fileURLToPath(import.meta.url));
const bundle = (entry) => {
  const out = esbuild.buildSync({
    entryPoints: [join(here, '..', 'src', entry)],
    bundle: true,
    format: 'esm',
    platform: 'neutral',
    write: false,
    logLevel: 'silent',
  });
  return import(`data:text/javascript;base64,${Buffer.from(out.outputFiles[0].contents).toString('base64')}`);
};

const posted = [];
globalThis.window = { chrome: { webview: { postMessage: (m) => posted.push(JSON.parse(m)), addEventListener() {} } } };
const { parseInbound, sendSuggestPick, sendSuggestPreview, sendSuggestEnd } = await bundle('protocol.ts');
const { moveSuggestSelection, suggestPreviewText, suggestGlyph, suggestAction, suggestBadge, suggestNeedle, suggestMatch, suggestEmphasis } = await bundle('suggest-logic.ts');
const { ICON_NAMES } = await bundle('icons.gen.ts');
const fixture = JSON.parse(readFileSync(join(here, 'fixtures', 'suggest-messages.json'), 'utf8'));
const take = () => posted.splice(0, posted.length);

test('Rust suggest fixture parses as list, toolbar copy, selection and close', () => {
  const list = parseInbound(fixture.suggest);
  assert.equal(list?.t, 'suggest');
  assert.equal(list.tok, 3);
  assert.equal(list.seq, 12);
  assert.equal(list.w, 560);
  assert.equal(list.sel, 1);
  assert.equal(list.q, 'Exa', 'the hover copy carries the typed text');
  assert.equal(parseInbound(fixture.toolbar).q, '');
  assert.deepEqual(list.items.map((i) => i.k), ['url', 'tab', 'bm', 'hist', 'search']);
  assert.equal(list.items[1].title, 'Guide <b>', 'markup in a title stays text');
  assert.equal(list.items[4].url, '');
  assert.equal(parseInbound(fixture.toolbar).tok, 0);
  assert.deepEqual(parseInbound(fixture.sel), { t: 'suggestSel', seq: 12, sel: 2 });
  const close = parseInbound(fixture.close);
  assert.equal(close.items.length, 0);
  assert.deepEqual(parseInbound(JSON.stringify(fixture.suggest)), list);
});

test('Rust suggestOpen fixture parses as the pill flag; anything else is rejected', () => {
  assert.deepEqual(parseInbound(fixture.open), { t: 'suggestOpen', on: true });
  assert.deepEqual(parseInbound(fixture.shut), { t: 'suggestOpen', on: false });
  assert.deepEqual(parseInbound(JSON.stringify(fixture.open)), { t: 'suggestOpen', on: true });
  for (const bad of [
    { ...fixture.open, on: 1 },
    { ...fixture.open, on: 'true' },
    { ...fixture.open, v: 2 },
    { ...fixture.open, seq: 3 },
    { v: 1, t: 'suggestOpen' },
    (({ v, ...rest }) => rest)(fixture.open),
  ]) {
    assert.equal(parseInbound(bad), null, JSON.stringify(bad));
  }
});

test('malformed suggest messages are rejected', () => {
  const row = fixture.suggest.items[0];
  for (const bad of [
    { ...fixture.suggest, extra: 1 },
    { ...fixture.suggest, v: 2 },
    { ...fixture.suggest, w: 4001 },
    { ...fixture.suggest, seq: -1 },
    { ...fixture.suggest, sel: 20 },
    { ...fixture.suggest, sel: 5 },
    { ...fixture.suggest, items: Array.from({ length: 21 }, () => row) },
    { ...fixture.suggest, items: [{ ...row, k: 'weird' }] },
    { ...fixture.suggest, items: [{ ...row, extra: 1 }] },
    { ...fixture.suggest, items: [{ k: 'url', text: 'a', title: 'a' }] },
    { ...fixture.suggest, items: [{ ...row, text: 'x'.repeat(257) }] },
    { ...fixture.suggest, items: [{ ...row, fav: 'https://example.com/x.png' }] },
    { ...fixture.suggest, items: [{ ...row, fav: 'data:image/gif;base64,AAAA' }] },
    { ...fixture.suggest, q: 1 },
    { ...fixture.suggest, q: 'q'.repeat(513) },
    (({ q, ...rest }) => rest)(fixture.suggest),
    { ...fixture.sel, sel: 20 },
    { ...fixture.sel, extra: true },
  ]) {
    assert.equal(parseInbound(bad), null, JSON.stringify(bad).slice(0, 120));
  }
});

test('emoji, CJK and RTL rows from Rust survive validation; no row is dropped', () => {
  const list = parseInbound(fixture.heavy);
  assert.equal(list?.t, 'suggest', 'a list of over-long non-Latin rows must still parse');
  assert.equal(list.items.length, 4);
  assert.equal(list.sel, 2);
  for (const item of list.items) {
    assert.ok(item.text.length <= 256 && item.url.length <= 256, 'cut to the page limit in UTF-16 units');
    assert.ok(item.text.length > 200, 'and not cut more than needed');
    assert.ok(!/[\uD800-\uDBFF]$/.test(item.text) && !/^[\uDC00-\uDFFF]/.test(item.text), 'never mid-surrogate');
  }
  assert.ok(list.q.length <= 512);
});

test('one invalid row is dropped, not the whole list, and the selection follows its row', () => {
  const rows = fixture.suggest.items;
  const bad = { ...rows[0], text: 'x'.repeat(257) };
  const mixed = (items, sel) => parseInbound({ ...fixture.suggest, items, sel });
  let list = mixed([rows[0], bad, rows[2], rows[3]], 2);
  assert.deepEqual(list.items.map((i) => i.k), ['url', 'bm', 'hist']);
  assert.equal(list.sel, 1, 'row 2 (bm) is now at 1');
  list = mixed([bad, rows[1], rows[2]], 0);
  assert.deepEqual(list.items.map((i) => i.k), ['tab', 'bm']);
  assert.equal(list.sel, 0, 'a dropped selected row falls back to the first');
  list = mixed([rows[0], rows[1], { ...rows[2], extra: 1 }], 1);
  assert.deepEqual(list.items.map((i) => i.k), ['url', 'tab']);
  assert.equal(list.sel, 1);
  assert.equal(mixed([bad, bad], 0), null, 'a list with no valid row at all is rejected');
  assert.equal(mixed([rows[0], bad], 2), null, 'the selection must still point inside the message');
  assert.equal(mixed([], 0).items.length, 0, 'an empty list still closes the dropdown');
});

test('state accepts the split and bookmarked flags and rejects other types', () => {
  const state = { t: 'state', scale: 1, solid: true, dark: false, back: false, forward: false, loading: false, progress: 0, security: 'secure', label: 'x', theme: {} };
  assert.equal(parseInbound(state).split, undefined);
  const flags = parseInbound({ ...state, split: true, bookmarked: false });
  assert.equal(flags.split, true);
  assert.equal(flags.bookmarked, false);
  assert.equal(parseInbound({ ...state, split: 1 }), null);
});

test('outbound suggest commands are validated', () => {
  take();
  assert.equal(sendSuggestPick(12, 7), true);
  assert.equal(sendSuggestPreview(0, 0), true);
  assert.equal(sendSuggestEnd(), true);
  assert.deepEqual(take(), [
    { v: 1, t: 'suggestPick', seq: 12, i: 7 },
    { v: 1, t: 'suggestPreview', seq: 0, i: 0 },
    { v: 1, t: 'suggestEnd' },
  ]);
  for (const [seq, i] of [[1, 20], [1, -1], [1, 1.5], [-1, 0], [0x80000000, 0]]) {
    assert.equal(sendSuggestPick(seq, i), false);
    assert.equal(sendSuggestPreview(seq, i), false);
  }
  assert.deepEqual(take(), []);
});

test('arrow keys wrap and the default row shows what was typed', () => {
  assert.equal(moveSuggestSelection(0, 5, 'ArrowDown'), 1);
  assert.equal(moveSuggestSelection(4, 5, 'ArrowDown'), 0);
  assert.equal(moveSuggestSelection(0, 5, 'ArrowUp'), 4);
  assert.equal(moveSuggestSelection(3, 5, 'ArrowUp'), 2);
  assert.equal(moveSuggestSelection(9, 3, 'ArrowDown'), 0, 'an out-of-range selection is clamped first');
  assert.equal(moveSuggestSelection(0, 0, 'ArrowDown'), 0);
  assert.equal(suggestPreviewText('exa', 0, 'example.com/'), 'exa');
  assert.equal(suggestPreviewText('exa', 2, 'example.org/'), 'example.org/');
});

test('rows use family glyphs that say what picking them does', () => {
  const glyphs = { search: 'search', url: 'globe', hist: 'history', bm: 'bookmark', tab: 'tab-switch' };
  for (const [kind, name] of Object.entries(glyphs)) {
    assert.equal(suggestGlyph(kind), name);
    assert.ok(ICON_NAMES.includes(name), `${name} is in the icon family`);
  }
  assert.equal(suggestAction('tab'), 'Switch to tab');
  for (const kind of ['search', 'url', 'hist', 'bm']) assert.equal(suggestAction(kind), '');
  // A favicon in the lead slot hides the kind glyph: a bookmark then carries its own small mark.
  assert.deepEqual(suggestBadge('bm', true), { icon: 'bookmark', label: 'Bookmark' });
  for (const kind of ['search', 'url', 'hist', 'bm', 'tab']) if (kind !== 'bm') assert.equal(suggestBadge(kind, true), null);
  assert.equal(suggestBadge('bm', false), null, 'the lead glyph already is the bookmark');
  assert.ok(ICON_NAMES.includes('enter'), 'the keycap glyph is in the family');
});

test('the typed text is found in rows the way Rust matches it', () => {
  assert.equal(suggestNeedle('  wiki '), 'wiki');
  assert.equal(suggestNeedle('HTTPS://www.Example.com/a'), 'Example.com/a');
  assert.equal(suggestNeedle('http://www.'), '');
  assert.equal(suggestNeedle('www.rust'), 'rust');
  assert.equal(suggestNeedle('wwwx'), 'wwwx');
  assert.deepEqual(suggestMatch('en.wikipedia.org/wiki/Web', 'WIKI'), [3, 7]);
  assert.deepEqual(suggestMatch('Rust Programming', 'rust'), [0, 4]);
  assert.equal(suggestMatch('kiwi', 'wiki'), null);
  assert.equal(suggestMatch('abc', ''), null);
  assert.equal(suggestMatch('ab', 'abc'), null);
  // Only ASCII folds, so offsets stay valid for text whose case maps change length.
  assert.deepEqual(suggestMatch('İstanbul wiki', 'wiki'), [9, 13]);
  assert.equal(suggestMatch('É', 'é'), null);
  // A row that is nothing but the typed text (the search row) has nothing to single out.
  assert.equal(suggestEmphasis('wiki', 'WIKI'), null);
  assert.deepEqual(suggestEmphasis('wikihow.com/', 'wiki'), [0, 4]);
  assert.equal(suggestEmphasis('example.org/', 'wiki'), null);
});

// The stylesheet contract for the omnibox (docs/design/omnibox.md). `rules(css, selector)` finds a
// rule by its exact selector list (comments stripped, whitespace collapsed).
const styles = (name) => readFileSync(join(here, '..', 'src', 'styles', name), 'utf8');
const rulesOf = (css) => {
  const out = [];
  const flat = css.replace(/\/\*[\s\S]*?\*\//g, '');
  for (const m of flat.matchAll(/([^{}]+)\{([^{}]*)\}/g)) out.push([m[1].replace(/\s+/g, ' ').trim(), m[2]]);
  return out;
};
const rule = (css, selector) => {
  const hit = rulesOf(css).find(([sel]) => sel === selector);
  assert.ok(hit, `rule ${selector} exists`);
  return hit[1];
};
const toolbar = styles('toolbar.css');
const hover = styles('hover.css');
const tokens = styles('tokens.css');

test('the open pill and the dropdown are one surface: one material, no outline, one curvature', () => {
  const list = rule(hover, '.bw-suggest');
  const open = rule(toolbar, '.pill.is-open, .pill.is-open:focus-within');
  // One material for the focused pill, the open pill, the list and the page behind the list (and
  // Rust's native backdrop, html_hover.rs suggest_surface).
  assert.match(tokens, /--bw-omni-material: linear-gradient\(var\(--bw-fill\), var\(--bw-fill\)\), var\(--bw-window-opaque\);/);
  assert.match(rule(toolbar, '.pill:focus-within, .pill.is-open'), /background: var\(--bw-omni-material\);/);
  assert.match(list, /background: var\(--bw-omni-material\);/);
  assert.match(rule(hover, 'body.bw-hover-body[data-hover-kind="suggest"]'), /background: var\(--bw-omni-material\);/);
  // The open state changes nothing in the top half: no other fill, no transition, no other radius.
  assert.doesNotMatch(open, /background|box-shadow|transition|border-top-(left|right)-radius|border-radius:/);
  assert.doesNotMatch(open, /border-bottom-color/, 'a different bottom colour would shift the top arc');
  // No outline while open: the border keeps its pixel (nothing moves) but is transparent, the ring is off.
  assert.match(open, /border-color: transparent;/);
  assert.match(open, /border-bottom-left-radius: 0;/);
  assert.match(open, /border-bottom-right-radius: 0;/);
  assert.doesNotMatch(toolbar, /\.pill(\.is-open)?(:focus-within)?::after/, 'there is no ring layer to switch off');
  assert.match(list, /border: 1px solid transparent;/);
  assert.match(list, /border-top: 0;/);
  assert.doesNotMatch(hover.slice(hover.indexOf('/* ----- omnibox suggestions ----- */')), /--bw-pill-edge|--bw-focus\b/, 'the list paints no edge or ring colour');
  // One curvature at both ends: the pill's corner is half its height, the list's bottom uses it.
  assert.match(tokens, /--bw-r-omnibox: calc\(var\(--bw-pill-h\) \/ 2\);/);
  assert.match(rule(toolbar, '.pill'), /border-radius: var\(--bw-r-omnibox\);/);
  assert.match(list, /border-radius: 0 0 var\(--bw-r-omnibox\) var\(--bw-r-omnibox\);/);
  // The container never moves (its edges must stay on the pill's).
  assert.doesNotMatch(list, /transform|animation|transition/);
});

test('the focused pill has no outline, just the raised material and its lift', () => {
  assert.match(rule(toolbar, '.pill:focus-within'), /border-color: transparent;/);
  assert.match(rule(toolbar, '.pill:focus-within, .pill.is-open'), /box-shadow: var\(--bw-sh-omni\);/);
  assert.doesNotMatch(toolbar, /--bw-omni-halo|\.pill::after/);
  // Keyboard focus on the pill's own buttons keeps the shared inset ring.
  for (const name of ['tb-site', 'tb-split', 'tb-reader', 'tb-zoom']) assert.match(readFileSync(join(here, '..', 'src', 'view.ts'), 'utf8'), new RegExp(`${name} bw-focus-inset`));
});

test('rows are concentric with the open surface and the selection is one sliding layer', () => {
  // Rows sit 5px in (1px border + 4px padding), so their radius is the surface's less 5px: every row,
  // not just the last, so one indicator shape fits wherever it goes.
  assert.match(tokens, /--bw-r-omni-row: calc\(var\(--bw-r-omnibox\) - var\(--bw-space-1\) - 1px\);/);
  assert.match(rule(hover, '.bw-suggest'), /padding: var\(--bw-space-1\);/);
  const row = rule(hover, '.bw-suggest-row');
  assert.match(row, /border-radius: var\(--bw-r-omni-row\);/);
  assert.match(row, /height: 32px;/);
  assert.doesNotMatch(hover, /\.bw-suggest-row:(last|first)-child/, 'no row needs its own corners');
  assert.doesNotMatch(hover, /\.bw-suggest-row\[aria-selected="true"\] \{[^}]*background/, 'the indicator, not the row, paints the selection');
  const sel = rule(hover, '.bw-suggest-sel');
  assert.match(sel, /border-radius: var\(--bw-r-omni-row\);/);
  assert.match(sel, /height: 32px;/);
  assert.match(sel, /top: var\(--bw-space-1\);\s*right: var\(--bw-space-1\);\s*left: var\(--bw-space-1\);/);
  assert.match(sel, /transform: translateY\(calc\(var\(--suggest-sel, 0\) \* 100%\)\);/, 'moved by transform only');
  assert.match(sel, /z-index: -1;/);
  assert.match(rule(hover, '.bw-suggest'), /isolation: isolate;/, 'under the rows, above the surface');
  // It slides only once the selection moves, 83ms front-loaded; never under reduced motion.
  assert.match(rule(hover, '.bw-suggest.is-tracking .bw-suggest-sel'), /^\s*transition: transform var\(--bw-dur-fast\) var\(--bw-ease-out\);\s*$/);
  assert.match(hover, /@media \(prefers-reduced-motion: reduce\) \{\s*\.bw-suggest\.is-tracking \.bw-suggest-sel \{\s*transition: none;/);
  // The keycap and the action chip keep their box on every row, so nothing in a row moves.
  assert.match(rule(hover, '.bw-suggest-key'), /visibility: hidden;/);
  assert.match(rule(hover, '.bw-suggest-row[aria-selected="true"] .bw-suggest-key'), /visibility: visible;/);
  assert.match(hover, /@media \(forced-colors: active\) \{\s*\.bw-suggest-row\[aria-selected="true"\] \{\s*outline: 1px solid Highlight;/);
});

test('the rows settle in once, staggered and finite; reduced motion only fades', () => {
  assert.match(rule(hover, '.bw-suggest.is-entering :is(.bw-suggest-row, .bw-suggest-sel)'), /animation: bw-suggest-rows-in var\(--bw-dur-base\) var\(--bw-ease-out\) both;/);
  const keyframes = /@keyframes bw-suggest-rows-in \{([\s\S]*?)\n\}/.exec(hover.replace(/\r/g, ''));
  assert.ok(keyframes, 'keyframes exist');
  assert.match(keyframes[1], /opacity: 0;/);
  assert.match(keyframes[1], /translate: 0 -4px;/);
  assert.doesNotMatch(keyframes[1], /transform|height|top|margin/, 'opacity and translate only');
  const delays = [...hover.matchAll(/\.bw-suggest\.is-entering \.bw-suggest-row:nth-child\((\d)\) \{ animation-delay: (\d+)ms; \}/g)].map((m) => [Number(m[1]), Number(m[2])]);
  assert.deepEqual(delays, [2, 3, 4, 5, 6, 7, 8].map((n) => [n, (n - 1) * 10]), '10ms apart, at most 70ms for the eighth row');
  assert.doesNotMatch(hover, /infinite/);
  assert.match(hover, /@media \(prefers-reduced-motion: reduce\) \{\s*\.bw-suggest\.is-entering :is\(\.bw-suggest-row, \.bw-suggest-sel\) \{\s*animation: bw-fade-in var\(--bw-dur-fast\) var\(--bw-ease-fade\) both;\s*animation-delay: 0ms;/);
});

test('the pill text and the row titles share one size and one column', () => {
  assert.match(tokens, /--bw-text-omni: 13px;/);
  assert.match(rule(toolbar, '.omni, .omni-display'), /font-size: var\(--bw-text-omni\);/);
  assert.match(rule(hover, '.bw-suggest'), /font: var\(--bw-text-omni\) \/ var\(--bw-leading-body\) var\(--bw-font\);/);
  // Site button 4px in (the corner) + 20px + 10px text inset == list inset 5px + row inset 6px + 16px lead + 8px gap.
  assert.match(rule(toolbar, '.tb-site'), /margin-left: var\(--bw-space-1\);/);
  assert.match(rule(toolbar, '.omni, .omni-display'), /padding: 0 var\(--bw-space-2\) 0 calc\(var\(--bw-space-1_5\) \+ var\(--bw-space-1\)\);/);
  assert.match(rule(hover, '.bw-suggest-row'), /padding: 0 var\(--bw-space-2\) 0 var\(--bw-space-1_5\);/);
  assert.match(rule(hover, '.bw-suggest-row'), /gap: var\(--bw-space-2\);/);
  assert.equal(1 + 4 + 20 + 10, 1 + 4 + 6 + 16 + 8, 'the typed text and the titles start at the same x');
});

// ----- the list's DOM, rendered by the real hover view against a small fake DOM -----

class FakeClassList {
  constructor() { this.set = new Set(); }
  add(...names) { for (const n of names) this.set.add(n); }
  remove(...names) { for (const n of names) this.set.delete(n); }
  contains(name) { return this.set.has(name); }
  toggle(name, force) { const on = force === undefined ? !this.set.has(name) : force; if (on) this.set.add(name); else this.set.delete(name); return on; }
}
class FakeNode {
  constructor(tag) {
    this.tagName = tag.toUpperCase();
    this.attrs = new Map();
    this.children = [];
    this.dataset = {};
    this.classList = new FakeClassList();
    this.textContent = '';
    this.listeners = {};
    const props = new Map();
    this.props = props;
    this.style = { setProperty: (k, v) => props.set(k, v), getPropertyValue: (k) => props.get(k) ?? '', removeProperty: (k) => props.delete(k) };
  }
  set className(value) { this.classList = new FakeClassList(); this.classList.add(...value.split(/\s+/).filter(Boolean)); }
  get className() { return [...this.classList.set].join(' '); }
  get firstElementChild() { return this.children[0] ?? null; }
  get childElementCount() { return this.children.length; }
  setAttribute(k, v) { this.attrs.set(k, String(v)); if (k === 'class') this.className = String(v); }
  getAttribute(k) { return this.attrs.has(k) ? this.attrs.get(k) : null; }
  addEventListener(type, fn) { this.listeners[type] = fn; }
  append(...nodes) { this.children.push(...nodes); }
  replaceChildren(...nodes) { this.children = nodes; }
  getBoundingClientRect() { return { width: 600, height: 200 }; }
  all(name, out = []) { for (const c of this.children) { if (c.classList?.contains(name)) out.push(c); c.all?.(name, out); } return out; }
  querySelectorAll(sel) { return this.all(sel.slice(1)); }
  querySelector(sel) { return this.all(sel.slice(1))[0] ?? null; }
}

test('the hover view renders kinds, the kind mark, the keycap and one selection indicator', async () => {
  const listeners = [];
  globalThis.window.chrome.webview.addEventListener = (type, cb) => { if (type === 'message') listeners.push(cb); };
  globalThis.document = {
    activeElement: null,
    body: new FakeNode('body'),
    documentElement: new FakeNode('html'),
    createElement: (tag) => new FakeNode(tag),
    createElementNS: (_ns, tag) => new FakeNode(tag),
    createTextNode: (text) => ({ textContent: text }),
  };
  const { startHover } = await bundle('hover.ts');
  const root = new FakeNode('div');
  startHover(root);
  const deliver = (msg) => { for (const cb of listeners) cb({ data: JSON.stringify(msg) }); };
  const fav = 'data:image/png;base64,iVBORw0KGgo=';
  const items = [
    { k: 'search', text: 'wiki', title: 'wiki', url: '' },
    { k: 'url', text: 'wikipedia.org/', title: 'wikipedia.org/', url: 'wikipedia.org/', fav },
    { k: 'tab', text: 'en.wikipedia.org/wiki/Wiki', title: 'Wiki', url: 'en.wikipedia.org/wiki/Wiki', fav },
    { k: 'bm', text: 'en.wikipedia.org/wiki/WebView', title: 'WebView', url: 'en.wikipedia.org/wiki/WebView', fav },
    { k: 'bm', text: 'wikihow.com/', title: 'wikiHow', url: 'wikihow.com/' },
    { k: 'hist', text: 'en.wikipedia.org/wiki/Edge', title: 'Edge', url: 'en.wikipedia.org/wiki/Edge' },
  ];
  deliver({ v: 1, t: 'suggest', tok: 4, seq: 7, w: 600, sel: 0, q: 'wiki', items });
  const list = root.children[0];
  assert.ok(list.classList.contains('bw-suggest'));
  assert.equal(list.getAttribute('role'), 'listbox');
  assert.ok(list.classList.contains('is-entering'), 'the first list settles in');
  const rows = list.children.filter((c) => c.classList.contains('bw-suggest-row'));
  assert.equal(rows.length, items.length);
  const indicator = list.children.at(-1);
  assert.ok(indicator.classList.contains('bw-suggest-sel'), 'one indicator, after the rows (nth-child delays count rows only)');
  assert.equal(indicator.getAttribute('aria-hidden'), 'true');
  assert.equal(list.all('bw-suggest-sel').length, 1);
  assert.equal(list.props.get('--suggest-sel'), '0');
  assert.deepEqual(rows.map((r) => r.getAttribute('aria-selected')), ['true', 'false', 'false', 'false', 'false', 'false']);
  assert.deepEqual(rows.map((r) => r.getAttribute('role')), Array(items.length).fill('option'));
  assert.deepEqual(rows.map((r) => r.dataset.kind), items.map((i) => i.k));
  // Every row carries the (decorative) Enter keycap; CSS shows it on the selected row only.
  for (const r of rows) {
    const key = r.all('bw-suggest-key')[0];
    assert.equal(key.getAttribute('aria-hidden'), 'true');
    assert.equal(key.children[0].tagName, 'SVG');
  }
  // Only the open tab says what picking it does; only a bookmark whose favicon hides its glyph gets the mark.
  assert.deepEqual(rows.map((r) => r.all('bw-suggest-action').map((a) => a.textContent)), [[], [], ['Switch to tab'], [], [], []]);
  const marks = rows.map((r) => r.all('bw-suggest-mark'));
  assert.deepEqual(marks.map((m) => m.length), [0, 0, 0, 1, 0, 0]);
  assert.equal(marks[3][0].getAttribute('role'), 'img');
  assert.equal(marks[3][0].getAttribute('aria-label'), 'Bookmark', "the row's name says it is a bookmark");
  // The matched text is marked in the title (and the address), never in the search row's own text.
  assert.equal(rows[0].all('bw-suggest-match').length, 0);
  assert.ok(rows[1].all('bw-suggest-match').length >= 1);
  // The keyboard moves the selection: the row state and the layer, which now slides.
  deliver({ v: 1, t: 'suggestSel', seq: 7, sel: 3 });
  assert.ok(list.classList.contains('is-tracking'));
  assert.equal(list.props.get('--suggest-sel'), '3');
  assert.deepEqual(rows.map((r) => r.getAttribute('aria-selected')), ['false', 'false', 'false', 'true', 'false', 'false']);
  deliver({ v: 1, t: 'suggestSel', seq: 6, sel: 1 });
  assert.equal(list.props.get('--suggest-sel'), '3', 'a stale move is ignored');
  // Typing replaces the rows in place: no entrance, and the indicator is placed, not slid.
  deliver({ v: 1, t: 'suggest', tok: 5, seq: 8, w: 600, sel: 0, q: 'wikip', items: items.slice(0, 2) });
  const next = root.children[0];
  assert.notEqual(next, list);
  assert.ok(!next.classList.contains('is-entering'));
  assert.ok(!next.classList.contains('is-tracking'));
  assert.equal(next.props.get('--suggest-sel'), '0');
});
// --- end omnibox suggest ---
