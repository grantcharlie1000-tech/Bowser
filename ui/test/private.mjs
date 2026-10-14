// Private tabs in the chrome: the strip tab, the sidebar row and the toolbar pill carry
// data-private (set and cleared), name themselves as private, and the private badge never takes
// room in the row. Runs the real view code against a small fake DOM (no browser needed).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import * as esbuild from 'esbuild';

const here = dirname(fileURLToPath(import.meta.url));
const srcDir = join(here, '..', 'src');

class FakeClassList {
  constructor() { this.set = new Set(); }
  add(...names) { for (const n of names) this.set.add(n); }
  remove(...names) { for (const n of names) this.set.delete(n); }
  contains(name) { return this.set.has(name); }
  toggle(name, force) {
    const on = force === undefined ? !this.set.has(name) : force;
    if (on) this.set.add(name);
    else this.set.delete(name);
    return on;
  }
}

class FakeElement {
  constructor(tag) {
    this.tagName = tag.toUpperCase();
    this.attrs = new Map();
    this.children = [];
    this.dataset = {};
    this.classList = new FakeClassList();
    this.hidden = false;
    this.textContent = '';
    const props = new Map();
    this.style = {
      setProperty: (k, v) => props.set(k, v),
      getPropertyValue: (k) => props.get(k) ?? '',
      removeProperty: (k) => props.delete(k),
    };
  }
  set className(value) { this.classList = new FakeClassList(); this.classList.add(...value.split(/\s+/).filter(Boolean)); }
  get className() { return [...this.classList.set].join(' '); }
  setAttribute(k, v) { this.attrs.set(k, String(v)); }
  getAttribute(k) { return this.attrs.has(k) ? this.attrs.get(k) : null; }
  hasAttribute(k) { return this.attrs.has(k); }
  removeAttribute(k) { this.attrs.delete(k); }
  addEventListener() {}
  focus() {}
  contains() { return false; }
  querySelector() { return null; }
  querySelectorAll() { return []; }
  get offsetWidth() { return 0; }
  get offsetHeight() { return 0; }
  prepend(...nodes) { this.children.unshift(...nodes); }
  append(...nodes) { this.children.push(...nodes); }
  replaceChildren(...nodes) { this.children = nodes; }
  /** First descendant carrying class `name` (depth first). */
  find(name) {
    for (const child of this.children) {
      if (child.classList.contains(name)) return child;
      const deep = child.find(name);
      if (deep) return deep;
    }
    return null;
  }
}

class FakeText {
  constructor(text) { this.textContent = text; this.classList = new FakeClassList(); this.children = []; }
  find() { return null; }
}

globalThis.HTMLElement = FakeElement;
globalThis.requestAnimationFrame = () => 0;
globalThis.CSS = { escape: (s) => s };

globalThis.document = {
  activeElement: null,
  createTextNode: (text) => new FakeText(text),
  createElement: (tag) => new FakeElement(tag),
  createElementNS: (_ns, tag) => new FakeElement(tag),
  getElementById: () => null,
};

const out = esbuild.buildSync({
  stdin: {
    contents: [
      "export { createTab, updateTab } from './tabs';",
      "export { makeTab, syncTab } from './sidebar';",
      "export { applyState, buildView, PRIVATE_TIP } from './view';",
      "export { startPrivacyPanel } from './panels/privacy';",
      "export { PRIVATE_TABS_NOTE } from './panels/privacy-logic';",
      "export { createSiteInfoView } from './siteinfo';",
      "export { PRIVATE_SITE_NOTE } from './siteinfo-logic';",
    ].join('\n'),
    resolveDir: srcDir,
    loader: 'ts',
  },
  bundle: true,
  format: 'esm',
  platform: 'neutral',
  write: false,
  logLevel: 'silent',
});
const ui = await import(`data:text/javascript;base64,${Buffer.from(out.outputFiles[0].contents).toString('base64')}`);

const item = (id, extra = {}) => ({
  id, title: `Tab ${id}`, loading: false, pinned: false, muted: false, private: false,
  marker: -1, g: 0, gl: false, fav: false, ...extra,
});

test('strip tab: data-private and the private name are set, then cleared', () => {
  const node = ui.createTab(4);
  // Both private marks live inside the 16px icon slot: the full glyph (no favicon) and the badge.
  const slot = node.root.find('ico').children.map((c) => c.className);
  assert.deepEqual(slot, ['fav', 'glyph', 'priv-glyph', 'spin bw-spinner', 'priv']);
  assert.equal(node.root.find('priv-glyph').children[0].getAttribute('class').split(' ')[0], 'bw-icon');
  assert.equal(node.root.find('priv').children[0].style.getPropertyValue('--bw-icon-size'), '8px');

  ui.updateTab(node, item(4, { private: true }), true, '');
  assert.equal(node.root.dataset.private, 'true');
  assert.equal(node.root.getAttribute('aria-label'), 'Tab 4 (Private)');
  assert.equal(node.root.getAttribute('data-tip'), null, 'the hover card shows the title; a tooltip would flash first');

  ui.updateTab(node, item(4, { private: false }), true, '');
  assert.equal('private' in node.root.dataset, false);
  assert.equal(node.root.getAttribute('aria-label'), 'Tab 4');
  assert.equal(node.root.getAttribute('data-tip'), null);
});

test('pinned private strip tab keeps the pinned tile and the private mark', () => {
  const node = ui.createTab(5);
  ui.updateTab(node, item(5, { private: true, pinned: true, title: '' }), false, '');
  assert.equal(node.root.classList.contains('pinned'), true);
  assert.equal(node.root.dataset.private, 'true');
  assert.equal(node.root.getAttribute('aria-label'), 'Tab (Private)');
});

test('sidebar row: data-private and the private name are set, then cleared', () => {
  const favs = new Map();
  const node = ui.makeTab(item(6, { private: true }), false, favs);
  assert.deepEqual(node.face.children.map((c) => c.className), ['side-fav', 'side-glyph', 'side-priv-glyph', 'side-spin', 'side-priv']);
  assert.equal(node.face.dataset.state, 'glyph', 'no favicon: CSS swaps the globe tile for the private glyph');
  assert.equal(node.root.dataset.private, 'true');
  assert.equal(node.root.getAttribute('aria-label'), 'Tab 6 (Private)');
  assert.equal(node.root.getAttribute('data-tip'), null, 'the hover card shows the title; a tooltip would flash first');

  ui.syncTab(node, item(6), false, favs);
  assert.equal('private' in node.root.dataset, false);
  assert.equal(node.root.getAttribute('aria-label'), null, 'a normal row keeps its content name');
  assert.equal(node.root.getAttribute('data-tip'), null);
});

const STATE = {
  t: 'state', scale: 1, solid: false, dark: true, back: false, forward: false, loading: false,
  progress: 0, security: 'secure', label: 'Secure', theme: {},
};

test('toolbar pill: data-private and the private chip follow state.private', () => {
  const root = new FakeElement('div');
  const view = ui.buildView(root);
  assert.equal(view.privateChip.hidden, true);
  assert.equal(view.privateChip.getAttribute('aria-label'), ui.PRIVATE_TIP);
  assert.equal(view.privateChip.getAttribute('data-tip'), 'Private tab: browsing data is not saved');
  assert.equal(view.privateChip.find('tb-private-label').textContent, 'Private');
  // The chips and the reader button sit between the address field and the split button, never next to the site button.
  const at = (name) => view.pill.children.findIndex((c) => c.classList.contains(name));
  assert.deepEqual(['tb-site', 'omni-field', 'tb-private', 'tb-zoom', 'tb-reader', 'tb-split'].map(at), [0, 1, 2, 3, 4, 5]);

  ui.applyState(view, { ...STATE, private: true });
  assert.equal(view.pill.dataset.private, 'true');
  assert.equal(view.privateChip.hidden, false);
  assert.equal(view.input.getAttribute('aria-label'), 'Address and search bar, private tab');

  ui.applyState(view, { ...STATE, private: false });
  assert.equal('private' in view.pill.dataset, false);
  assert.equal(view.privateChip.hidden, true);
  assert.equal(view.input.getAttribute('aria-label'), 'Address and search bar');

  ui.applyState(view, { ...STATE, private: true });
  ui.applyState(view, STATE); // absent (older host) means not private
  assert.equal('private' in view.pill.dataset, false);
  assert.equal(view.privateChip.hidden, true);
});

test('toolbar pill: the private chip, zoom chip and reader button show together and hide independently', () => {
  const view = ui.buildView(new FakeElement('div'));
  ui.applyState(view, { ...STATE, private: true, zoom: 125, reader: 'available' });
  assert.equal(view.privateChip.hidden, false);
  assert.equal(view.zoom.hidden, false);
  assert.equal(view.reader.hidden, false);
  assert.equal(view.reader.getAttribute('aria-pressed'), 'false');
  ui.applyState(view, { ...STATE, private: true, zoom: 125, reader: 'on' });
  assert.equal(view.reader.getAttribute('aria-pressed'), 'true');
  assert.equal(view.privateChip.hidden, false);
  assert.equal(view.zoom.hidden, false);
  ui.applyState(view, { ...STATE, private: true, zoom: 100 });
  assert.equal(view.reader.hidden, true, 'absent reader (older host) means hidden');
  assert.equal(view.zoom.hidden, true);
  assert.equal(view.privateChip.hidden, false);
});

const rule = (css, selector) => {
  const at = css.indexOf(`${selector} {`);
  assert.ok(at >= 0, `missing rule ${selector}`);
  return css.slice(at, css.indexOf('}', at));
};

const tabsCss = readFileSync(join(srcDir, 'styles', 'tabs.css'), 'utf8');
const sidebarCss = readFileSync(join(srcDir, 'styles', 'sidebar.css'), 'utf8');
const STRIP_BADGE = '.strip .tab[data-private="true"]:not([data-icon="glyph"]) .priv';
const SIDE_BADGE = '.side-tab[data-private="true"] .side-face:not([data-state="glyph"]) > .side-priv';

test('the private badge is out of flow, inside the icon box, and the private rules change no tab geometry', () => {
  for (const badge of [rule(tabsCss, STRIP_BADGE), rule(sidebarCss, SIDE_BADGE)]) {
    assert.match(badge, /position: absolute;/);
    // Anchored to the 16px icon box's own corner, never pushed out towards the title.
    assert.match(badge, /right: 0;/);
    assert.match(badge, /bottom: 0;/);
    assert.match(badge, /width: calc\(var\(--bw-space-2\) \+ var\(--bw-space-0_5\)\);/);
    assert.doesNotMatch(badge, /(right|bottom): calc\(-/);
  }
  const geometry = /(^|[\s;{])(flex|flex-grow|flex-basis|width|min-width|max-width|height|padding|margin|border-width|gap|grid-template-columns|inset|top|left)\s*:/;
  for (const [file, css] of [['tabs.css', tabsCss], ['sidebar.css', sidebarCss]]) {
    for (const m of css.matchAll(/([^{}]*data-private[^{}]*)\{([^}]*)\}/g)) {
      const selector = m[1].replace(/\/\*[\s\S]*?\*\//g, '').trim();
      if (/\.(priv|side-priv)$/.test(selector.split(',').pop().trim())) continue; // the badge itself (absolute)
      assert.doesNotMatch(m[2], geometry, `${file}: ${selector} must not change geometry`);
    }
  }
});

test('private marks use token pairs with real contrast and a geometry-free edge', () => {
  for (const badge of [rule(tabsCss, STRIP_BADGE), rule(sidebarCss, SIDE_BADGE)]) {
    assert.match(badge, /background: var\(--bw-window-opaque\);/);
    assert.match(badge, /color: var\(--bw-private\);/);
    assert.match(badge, /box-shadow: 0 0 0 1px var\(--bw-private\);/, 'contrasting ring');
    assert.doesNotMatch(badge, /--bw-on-accent|--bw-accent/);
  }
  // No favicon: the private mark replaces the globe, in the private violet.
  assert.match(rule(tabsCss, '.strip .tab[data-private="true"][data-icon="glyph"] .glyph'), /display: none;/);
  assert.match(rule(tabsCss, '.strip .tab[data-private="true"][data-icon="glyph"] .priv-glyph'), /display: block;[\s\S]*color: var\(--bw-private\);/);
  assert.match(rule(sidebarCss, '.side-tab[data-private="true"] .side-face[data-state="glyph"] > .side-glyph'), /display: none;/);
  assert.match(rule(sidebarCss, '.side-tab[data-private="true"] .side-face[data-state="glyph"] > .side-priv-glyph'), /color: var\(--bw-private\);/);
  // The 2px violet edge is an inset shadow (strip: underline on the island layer; sidebar: leading bar).
  assert.match(rule(tabsCss, '.strip .tab[data-private="true"]::before'), /box-shadow: inset 0 calc\(-1 \* var\(--bw-space-0_5\)\) 0 var\(--bw-private\);/);
  assert.match(rule(sidebarCss, '.side-tab[data-private="true"]:not(:focus-visible, .dragging)'), /box-shadow: inset var\(--bw-space-0_5\) 0 0 var\(--bw-private\);/);
  // Private marks never borrow the theme accent: in any accent they still read as private.
  for (const css of [tabsCss, sidebarCss]) {
    for (const m of css.matchAll(/([^{}]*data-private[^{}]*)\{([^}]*)\}/g)) assert.doesNotMatch(m[2], /--bw-accent/, m[1].trim());
  }
  assert.match(rule(sidebarCss, '.side-tab[data-private="true"].is-active:not(:focus-visible, .dragging)'), /var\(--bw-sh-tip\)/, 'the active row keeps its lift');
});

test('the toolbar chip never shrinks and the address truncates first', () => {
  const css = readFileSync(join(srcDir, 'styles', 'toolbar.css'), 'utf8');
  assert.match(rule(css, '.tb-private'), /flex: none;/);
  assert.match(rule(css, '.omni-field'), /flex: 1 1 auto;[\s\S]*min-width: 0;/);
  assert.doesNotMatch(rule(css, '.tb-private'), /background|border-radius/, 'no bubble behind the label');
});

test('the privacy panel renders the static Private tabs note as plain setting rows', () => {
  const root = new FakeElement('div');
  const onMessage = ui.startPrivacyPanel(root);
  const d = {
    profile: 'Default', guest: false,
    stats: { allTime: 0, session: 0, currentSite: 0, topSites: [], days: [] },
    site: null, security: 'none', blocking: null, tracking: null, trackingSetting: 'balanced', efficiencyActive: false, smartScreen: null,
    blockerSetting: true, sleeping: false, sleepAfter: 30, sites: [], denied: [], status: null,
  };
  onMessage({ t: 'snap', p: 'privacy', d });
  const section = root.find('pg-private');
  assert.ok(section, 'Private tabs section rendered');
  assert.equal(section.tagName, 'SECTION');
  const heading = section.find('pg-heading');
  assert.equal(heading.tagName, 'H2');
  assert.equal(heading.textContent, 'Private tabs');
  const rows = section.find('pg-rows').children;
  assert.equal(rows.length, ui.PRIVATE_TABS_NOTE.length);
  rows.forEach((row, i) => {
    assert.ok(row.classList.contains('bw-row') && row.classList.contains('is-setting'));
    assert.equal(row.find('pg-label').textContent, ui.PRIVATE_TABS_NOTE[i][0]);
    assert.equal(row.find('pg-desc').textContent, ui.PRIVATE_TABS_NOTE[i][1]);
    assert.equal(row.find('pg-control'), null, 'information only: no control');
  });
});

const SITE = {
  v: 1, t: 'siteinfo', tok: 7, kind: 'site-info', host: 'example.test', origin: 'https://example.test',
  security: 'secure', summary: 'Secure connection (HTTPS)',
  perms: [{ id: 2, k: 'camera', label: 'Camera', state: 'allow' }],
  cookies: { state: 'count', n: 2, clear: true }, blocker: { state: 'on', n: 3 }, tracking: 'balanced', zoom: 100,
  reset: true, blockedHere: true,
};
const rowOf = (page, key) => page.children.find((c) => c.classList.contains('bw-siteinfo-row') && c.dataset.k === key);

test('site info: a normal page keeps permission rows and the blocker switch', () => {
  const root = new FakeElement('div');
  assert.equal(ui.createSiteInfoView(root).show(SITE), true);
  const page = root.children[0];
  assert.equal(page.classList.contains('is-private'), false);
  assert.ok(rowOf(page, 'camera').find('bw-segmented'));
  assert.ok(rowOf(page, 'blocker').find('bw-toggle'));
  assert.equal(rowOf(page, 'blocker').find('bw-siteinfo-meta').textContent, 'On · 3 requests blocked here');
  assert.equal(page.find('bw-siteinfo-private'), null);
});

test('site info: a private page shows the note instead of permission rows and no blocker switch', () => {
  const root = new FakeElement('div');
  const priv = { ...SITE, perms: [], reset: false, private: true };
  assert.equal(ui.createSiteInfoView(root).show(priv), true);
  const page = root.children[0];
  assert.equal(page.classList.contains('is-private'), true);
  const note = page.find('bw-siteinfo-private');
  assert.ok(note && note.classList.contains('bw-siteinfo-empty'), 'quiet note reuses the empty-state style');
  assert.equal(note.textContent, ui.PRIVATE_SITE_NOTE);
  assert.equal(page.children.filter((c) => c.classList.contains('bw-siteinfo-row') && ['camera', 'mic', 'location', 'notifications'].includes(c.dataset.k)).length, 0);
  const blocker = rowOf(page, 'blocker');
  assert.equal(blocker.find('bw-toggle'), null, 'no switch: Rust declares no blocker act');
  assert.equal(blocker.find('bw-siteinfo-meta').textContent, 'Always on · 3 requests blocked here');
  assert.ok(rowOf(page, 'cookies').find('bw-button'), 'clearing the in-memory cookies stays available');
  assert.doesNotMatch(page.find('bw-siteinfo-notice').children[1].textContent, /Allow it below/);
  // Rows stay dropped even if a private page ever carried permission rows.
  const stray = new FakeElement('div');
  ui.createSiteInfoView(stray).show({ ...priv, perms: SITE.perms });
  assert.equal(rowOf(stray.children[0], 'camera'), undefined);
});
