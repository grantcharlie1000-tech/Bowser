// --- spoiler guard ---
// Spoiler Firewall in Bowser's own chrome: the shared engine masks risky suggestion, history, palette and
// tab-title text and URL slugs; clean rows are untouched; protection off masks nothing; "Show" reveals one
// row; the live setting arrives as a host message. The engine here is the generated copy of
// extensions/spoiler-firewall/engine (spoiler-engine.gen.ts), the same code the page injection runs.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { existsSync, readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { createRequire } from 'node:module';
import * as esbuild from 'esbuild';

const here = dirname(fileURLToPath(import.meta.url));
const src = join(here, '..', 'src');
const bundleSource = (contents, resolveDir = src) => {
  const out = esbuild.buildSync({
    stdin: { contents, resolveDir, loader: 'ts' },
    bundle: true,
    format: 'esm',
    platform: 'neutral',
    write: false,
    logLevel: 'silent',
  });
  return import(`data:text/javascript;base64,${Buffer.from(out.outputFiles[0].contents).toString('base64')}`);
};
const bundle = (entry) => bundleSource(`export * from './${entry}';`);

const listeners = [];
globalThis.window = { chrome: { webview: { postMessage() {}, addEventListener: (type, cb) => { if (type === 'message') listeners.push(cb); } } } };

// One instance of guard + config, so the setting the tests change is the one the guard reads.
const guard = await bundleSource("export * from './spoiler-guard.ts'; export * from './spoiler-config.ts';");
const { riskyText, riskyUrl, rowRisky, guardRow, guardTitle, hostOnly, RevealedRows, rowKey, MASK_LABEL, TITLE_MASK,
  applySpoilerMessage, setSpoilerConfig, activeSpoilerMode } = guard;
const reset = () => setSpoilerConfig(true, 'strict');

const SLUG = 'https://www.reddit.com/r/GTA6/comments/abc/lucia_dies_in_the_ending/';

test('the UI engine is byte-identical to the shipped engine fragments (one classifier)', () => {
  const generated = readFileSync(join(src, 'spoiler-engine.gen.ts'), 'utf8');
  const root = join(here, '..', '..', 'extensions', 'spoiler-firewall');
  // Non-ASCII escaping aside, each engine fragment must appear verbatim, in the shipped order.
  const escape = (text) => text.split('').map((ch) => (ch.charCodeAt(0) < 128 ? ch : `\\u${ch.charCodeAt(0).toString(16).padStart(4, '0')}`)).join('');
  let at = 0;
  for (const name of ['normalize', 'lexicon', 'classify', 'policy']) {
    const body = escape(readFileSync(join(root, 'engine', `${name}.js`), 'utf8').replace(/\r\n/g, '\n'));
    const found = generated.indexOf(`// ---- engine/${name}.js ----\n${body}`, at);
    assert.ok(found >= at, `${name}.js is embedded unchanged and in order`);
    at = found + body.length;
  }
  // And it behaves like the Node build the engine's own tests exercise.
  // dist/node is a build output the extension does not commit (a fresh checkout, CI): build it first.
  const nodeBuild = join(root, 'dist', 'node', 'engine.cjs');
  if (!existsSync(nodeBuild)) execFileSync(process.execPath, [join(root, 'build', 'build.mjs')], { cwd: root, stdio: 'pipe' });
  const shipped = createRequire(import.meta.url)(nodeBuild);
  for (const text of ['gta 6 ending explained', 'weather today', 'GTA VI release date']) {
    const here1 = guard.riskyText(text, 'strict');
    const there = shipped.policy.decide(shipped.classify.text(text, { hot: 0, source: 'title' }), { mode: 'strict', hot: 0, kind: 'title' }) !== 'clear';
    assert.equal(here1, there, text);
  }
});

test('a bundle loaded after the host message starts from the setting an earlier bundle holds', async () => {
  try {
    setSpoilerConfig(false, 'balanced');
    const late = await bundleSource("export * from './spoiler-config.ts'; export const tag = 1;");
    assert.equal(late.activeSpoilerMode(), 'off', 'protection off carries over to a late bundle');
    setSpoilerConfig(true, 'lockdown');
    const later = await bundleSource("export * from './spoiler-config.ts'; export const tag = 2;");
    assert.equal(later.activeSpoilerMode(), 'lockdown');
  } finally {
    reset();
  }
  const fresh = await bundleSource("export * from './spoiler-config.ts'; export const tag = 3;");
  assert.equal(fresh.activeSpoilerMode(), 'strict');
});

test('search suggestion text that reads as a spoiler is risky; ordinary queries are not', () => {
  reset();
  assert.equal(riskyText('gta 6 ending explained'), true);
  assert.equal(riskyText('lucia dies at the end of gta 6'), true);
  assert.equal(riskyText('weather today'), false);
  assert.equal(riskyText('how to bake bread'), false);
  assert.equal(riskyText(''), false);
});

test('URL slugs are checked, not just titles', () => {
  reset();
  assert.equal(riskyUrl(SLUG), true);
  assert.equal(riskyUrl('www.reddit.com/r/GTA6/comments/abc/lucia_dies_in_the_ending/'), true, 'the display form without a scheme');
  assert.equal(riskyUrl('https://example.com/recipes/bread'), false);
  assert.equal(riskyUrl('example.com/'), false);
});

test('a masked row shows the label and the host only; a clean row is untouched', () => {
  reset();
  const masked = guardRow({ title: 'Lucia dies at the end of GTA 6', url: 'reddit.com/r/GTA6/comments/abc/lucia_dies_in_the_ending/' }, false);
  assert.deepEqual(masked, { masked: true, title: MASK_LABEL, url: 'reddit.com' });
  assert.equal(MASK_LABEL, 'Spoiler hidden · GTA VI');
  // A clean title with a spoiler slug is masked through the URL alone.
  const slugOnly = guardRow({ title: 'Reddit', url: SLUG }, false);
  assert.equal(slugOnly.masked, true);
  assert.equal(slugOnly.url, 'www.reddit.com', 'no path, query or hash');
  const clean = guardRow({ title: 'Example', url: 'example.com/page' }, false);
  assert.deepEqual(clean, { masked: false, title: 'Example', url: 'example.com/page' });
});

test('the history panel and palette shapes: title plus host or address', () => {
  reset();
  assert.equal(rowRisky({ title: 'GTA 6 ending leaked', url: '' }), true);
  assert.equal(rowRisky({ title: 'Docs', url: 'docs.rs/serde' }), false);
});

test('text the user typed is never hidden from them', () => {
  reset();
  assert.equal(rowRisky({ title: 'gta 6 ending explained', text: 'gta 6 ending explained' }, undefined, 'gta 6 ending explained'), false);
  assert.equal(rowRisky({ title: 'gta 6 ending explained', text: 'gta 6 ending explained' }, undefined, 'gta 6 end'), true, 'only the exact text typed');
  assert.equal(rowRisky({ title: SLUG, url: SLUG }, undefined, SLUG), false, 'a pasted address is theirs');
});

test('protection off masks nothing, in any text or address', () => {
  assert.equal(riskyText('gta 6 ending explained', 'off'), false);
  assert.equal(riskyUrl(SLUG, 'off'), false);
  assert.deepEqual(guardRow({ title: 'gta 6 ending explained', url: SLUG }, false, 'off'), { masked: false, title: 'gta 6 ending explained', url: SLUG });
  assert.equal(guardTitle('gta 6 ending explained', 'off'), 'gta 6 ending explained');
  // Through the live setting as well.
  setSpoilerConfig(false, 'strict');
  assert.equal(activeSpoilerMode(), 'off');
  assert.equal(riskyText('gta 6 ending explained'), false);
  setSpoilerConfig(true, 'strict');
  assert.equal(riskyText('gta 6 ending explained'), true);
});

test('modes follow the shared policy: a bare mention hides in Strict and Lockdown, not Balanced', () => {
  const text = 'gta 6 map size comparison';
  assert.equal(riskyText(text, 'balanced'), false);
  assert.equal(riskyText(text, 'strict'), true);
  assert.equal(riskyText(text, 'lockdown'), true);
  assert.equal(riskyText('gta 6 ending explained', 'balanced'), true, 'a clear spoiler hides in every mode');
});

test('a tab title that reads as a spoiler shows the engine guard wording', () => {
  reset();
  assert.equal(guardTitle('GTA 6 ending explained - YouTube'), TITLE_MASK);
  assert.equal(TITLE_MASK, 'Spoiler-protected page');
  assert.equal(guardTitle('Weather today'), 'Weather today');
  assert.equal(guardTitle(TITLE_MASK), TITLE_MASK, 'already-guarded titles stay as they are');
});

test('Show reveals exactly one row, by identity, for the life of the set', () => {
  reset();
  const rows = [
    { title: 'Lucia dies at the end of GTA 6', url: 'a.example/lucia_dies_at_the_end' },
    { title: 'GTA 6 ending explained', url: 'b.example/ending' },
  ];
  const revealed = new RevealedRows();
  const key = (row) => rowKey('hist', row.title, row.url);
  assert.deepEqual(rows.map((row) => guardRow(row, revealed.has(key(row))).masked), [true, true]);
  revealed.add(key(rows[0]));
  const after = rows.map((row) => guardRow(row, revealed.has(key(row))));
  assert.equal(after[0].masked, false);
  assert.equal(after[0].title, rows[0].title);
  assert.equal(after[0].url, rows[0].url);
  assert.equal(after[1].masked, true, 'the neighbour stays hidden');
  assert.notEqual(key(rows[0]), key(rows[1]));
  assert.equal(new RevealedRows().has(key(rows[0])), false, 'a new view starts with nothing revealed');
});

test('hostOnly never returns a path, query, hash or credentials', () => {
  assert.equal(hostOnly('https://user:pw@reddit.com/r/GTA6/x?y=1#z'), 'reddit.com');
  assert.equal(hostOnly('reddit.com/r/GTA6'), 'reddit.com');
  assert.equal(hostOnly('reddit.com?x=lucia'), 'reddit.com');
  assert.equal(hostOnly('gta 6 ending'), '', 'a search phrase has no host');
  assert.equal(hostOnly(''), '');
});

test('the live setting arrives as {t:"spoiler"}; malformed messages change nothing', () => {
  reset();
  assert.equal(applySpoilerMessage({ v: 1, t: 'spoiler', on: false, mode: 'balanced' }), true);
  assert.equal(activeSpoilerMode(), 'off');
  assert.equal(applySpoilerMessage(JSON.stringify({ v: 1, t: 'spoiler', on: true, mode: 'balanced' })), true);
  assert.equal(activeSpoilerMode(), 'balanced');
  for (const bad of [
    { v: 1, t: 'spoiler', on: 'yes', mode: 'strict' },
    { v: 1, t: 'spoiler', on: true, mode: 'off' },
    { v: 1, t: 'spoiler', on: true, mode: 'Strict' },
    { v: 1, t: 'spoiler', on: true },
    { v: 2, t: 'spoiler', on: true, mode: 'strict' },
    { v: 1, t: 'spoiler', on: false, mode: 'strict', extra: 1 },
    { v: 1, t: 'state', on: false, mode: 'strict' },
    'not json',
    null,
    [],
  ]) {
    assert.equal(applySpoilerMessage(bad), false, JSON.stringify(bad));
  }
  assert.equal(activeSpoilerMode(), 'balanced', 'bad messages never loosen or change the setting');
  reset();
});

test('the default before any message is on and Strict (fail closed while the setting is in flight)', async () => {
  const fresh = await bundle('spoiler-config.ts');
  assert.equal(fresh.activeSpoilerMode(), 'strict');
});

// ----- the omnibox dropdown, rendered by the real hover view against a small fake DOM -----

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
    this.parent = null;
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
  append(...nodes) { for (const n of nodes) { if (n.parent !== undefined) n.parent = this; } this.children.push(...nodes); }
  replaceChildren(...nodes) { this.children = nodes; for (const n of nodes) if (n.parent !== undefined) n.parent = this; }
  replaceWith(node) { const at = this.parent.children.indexOf(this); this.parent.children[at] = node; node.parent = this.parent; this.parent = null; }
  getBoundingClientRect() { return { width: 600, height: 200 }; }
  all(name, out = []) { for (const c of this.children) { if (c.classList?.contains(name)) out.push(c); c.all?.(name, out); } return out; }
  querySelectorAll(sel) { return this.all(sel.slice(1)); }
  querySelector(sel) { return this.all(sel.slice(1))[0] ?? null; }
}
const textOf = (node) => (!node.children ? node.textContent : node.children.length === 0 ? node.textContent : node.children.map(textOf).join(''));

test('the dropdown masks risky rows (keeping their place), leaves clean rows, reveals one on Show, and honours the setting', async () => {
  const sent = [];
  globalThis.window.chrome.webview.postMessage = (m) => sent.push(JSON.parse(m));
  const hostListeners = [];
  globalThis.window.chrome.webview.addEventListener = (type, cb) => { if (type === 'message') hostListeners.push(cb); };
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
  const deliver = (msg) => { for (const cb of hostListeners) cb({ data: JSON.stringify(msg) }); };
  const items = [
    { k: 'search', text: 'gta', title: 'gta', url: '' },
    { k: 'search', text: 'gta 6 ending explained', title: 'gta 6 ending explained', url: '' },
    { k: 'hist', text: 'reddit.com/r/GTA6/comments/abc/lucia_dies_in_the_ending/', title: 'Reddit', url: 'reddit.com/r/GTA6/comments/abc/lucia_dies_in_the_ending/' },
    { k: 'hist', text: 'example.com/recipes', title: 'Recipes', url: 'example.com/recipes' },
  ];
  const message = { v: 1, t: 'suggest', tok: 4, seq: 7, w: 600, sel: 0, q: 'gta', items };
  deliver(message);
  const list = root.children[0];
  let rows = list.children.filter((c) => c.classList.contains('bw-suggest-row'));
  assert.equal(rows.length, 4, 'no row is removed: layout and keyboard order stay predictable');
  assert.deepEqual(rows.map((r) => r.dataset.spoiler ?? null), [null, 'hidden', 'hidden', null]);
  const title = (r) => textOf(r.all('bw-suggest-title')[0]);
  const url = (r) => { const node = r.all('bw-suggest-url')[0]; return node && textOf(node); };
  assert.equal(title(rows[1]), MASK_LABEL);
  assert.equal(title(rows[2]), MASK_LABEL);
  assert.equal(url(rows[2]), 'reddit.com', 'host only, never the slug');
  assert.equal(title(rows[3]), 'Recipes');
  assert.equal(url(rows[3]), 'example.com/recipes');
  const everything = JSON.stringify(rows.map((r) => textOf(r)));
  assert.doesNotMatch(everything, /lucia|ending|dies/i, 'no spoiler text reaches the masked rows');
  assert.equal(rows[0].all('bw-suggest-show').length, 0);
  assert.equal(rows[3].all('bw-suggest-show').length, 0);
  // Show reveals this row only, and does not pick it.
  const show = rows[2].all('bw-suggest-show')[0];
  assert.equal(show.textContent, 'Show');
  let stopped = false;
  show.listeners.click({ stopPropagation: () => { stopped = true; } });
  assert.ok(stopped);
  assert.equal(sent.filter((m) => m.t === 'suggestPick').length, 0, 'revealing is not picking');
  rows = list.children.filter((c) => c.classList.contains('bw-suggest-row'));
  assert.equal(rows.length, 4, 'the row keeps its place');
  assert.equal(rows[2].dataset.spoiler, undefined);
  assert.equal(title(rows[2]), 'Reddit');
  assert.equal(url(rows[2]), 'reddit.com/r/GTA6/comments/abc/lucia_dies_in_the_ending/');
  assert.equal(rows[1].dataset.spoiler, 'hidden', 'the neighbour stays hidden');
  // A masked row is still selectable and Enter's pick goes through: clicking it picks that index.
  rows[1].listeners.click();
  assert.deepEqual(sent.filter((m) => m.t === 'suggestPick'), [{ v: 1, t: 'suggestPick', seq: 7, i: 1 }]);
  // The text the user typed is shown to them: the default row is not masked even when it is a mention.
  deliver({ ...message, seq: 8, q: 'gta 6 ending explained', items: [{ k: 'search', text: 'gta 6 ending explained', title: 'gta 6 ending explained', url: '' }] });
  rows = root.children[0].children.filter((c) => c.classList.contains('bw-suggest-row'));
  assert.equal(rows[0].dataset.spoiler, undefined);
  // Protection off (a live setting message): nothing is masked.
  deliver({ v: 1, t: 'spoiler', on: false, mode: 'strict' });
  deliver({ ...message, seq: 9, tok: 5 });
  rows = root.children[0].children.filter((c) => c.classList.contains('bw-suggest-row'));
  assert.deepEqual(rows.map((r) => r.dataset.spoiler ?? null), [null, null, null, null]);
  assert.equal(title(rows[1]), 'gta 6 ending explained');
  deliver({ v: 1, t: 'spoiler', on: true, mode: 'strict' });
});
// --- end spoiler guard ---
