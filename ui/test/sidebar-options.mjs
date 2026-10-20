// Settings > Tabs in the vertical sidebar: the close button on the left or always shown, the page load
// indicator and the pinned separator do what they say there too, as they do in the horizontal strip.
// Runs the real sidebar renderer against the shared fake document (no browser needed).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import * as esbuild from 'esbuild';
import { FakeDocument, Node } from './fake-dom.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const srcDir = join(here, '..', 'src');
const css = (name) => readFileSync(join(srcDir, 'styles', name), 'utf8').replace(/\r\n/g, '\n');

// What the renderer needs beyond the shared fake: sibling links, insertBefore and style.removeProperty.
Object.defineProperty(Node.prototype, 'parentNode', { get() { return this.parent; } });
Object.defineProperty(Node.prototype, 'firstChild', { get() { return this.children[0] ?? null; } });
Object.defineProperty(Node.prototype, 'nextSibling', { get() { const p = this.parent; return p ? p.children[p.children.indexOf(this) + 1] ?? null : null; } });
Object.defineProperty(Node.prototype, 'previousElementSibling', { get() { const p = this.parent; return p ? p.children[p.children.indexOf(this) - 1] ?? null : null; } });
Object.defineProperty(Node.prototype, 'isConnected', { get() { return true; } });
Node.prototype.insertBefore = function insertBefore(node, ref) {
  this.detach(node);
  const at = ref ? this.children.indexOf(ref) : -1;
  if (at < 0) this.children.push(node); else this.children.splice(at, 0, node);
  return node;
};
class StyledDocument extends FakeDocument {
  createElement(tag) { return styled(super.createElement(tag)); }
  createElementNS(ns, tag) { return styled(super.createElementNS(ns, tag)); }
}
function styled(node) {
  const props = new Map();
  node.style = { setProperty: (k, v) => props.set(k, String(v)), getPropertyValue: (k) => props.get(k) ?? '', removeProperty: (k) => props.delete(k) };
  return node;
}
globalThis.document = new StyledDocument();
globalThis.MutationObserver = class { observe() {} disconnect() {} };
globalThis.HTMLElement = Node;
globalThis.SVGElement = Node;
globalThis.requestAnimationFrame = () => 0;
globalThis.cancelAnimationFrame = () => {};
globalThis.CSS = { escape: (s) => s };
globalThis.ResizeObserver = class { observe() {} disconnect() {} };
globalThis.matchMedia = () => ({ matches: false, addEventListener() {} });
globalThis.getComputedStyle = () => ({ getPropertyValue: () => '' });
globalThis.window = globalThis;
globalThis.window.chrome = { webview: { postMessage() {}, addEventListener() {} } };
globalThis.addEventListener = () => {};

const out = esbuild.buildSync({
  stdin: { contents: "export { createSidebarRenderer, makeTab, syncTab } from './sidebar';", resolveDir: srcDir, loader: 'ts' },
  bundle: true, format: 'esm', platform: 'neutral', write: false, logLevel: 'silent',
});
const ui = await import(`data:text/javascript;base64,${Buffer.from(out.outputFiles[0].contents).toString('base64')}`);

const item = (id, extra = {}) => ({
  id, title: `Tab ${id}`, loading: false, pinned: false, muted: false, private: false,
  marker: -1, g: 0, gl: false, fav: false, ...extra,
});
const OPTS = {
  t: 'tabopts', closeLeft: false, closeAlways: false, scroll: false, minWidth: 0, indicator: 'spinner', wheel: false, separator: false,
  alignClose: false, selection: true, selectionActive: true, removeSpacing: false, maximised: false, closed: 0, stackDrag: false,
  stackDelay: 0, stackRow: false, closedButton: false, tools: [],
};
const withStyle = (node) => {
  if (!node.style.removeProperty) node.style.removeProperty = () => {};
  for (const child of node.children) withStyle(child);
  return node;
};

test('tabs_load_indicator: the sidebar row shows the spinner, a bar, a ring or nothing, as the strip does', () => {
  const favs = new Map([[1, 'data:image/png;base64,AA==']]);
  const loading = item(1, { loading: true, pct: 40, fav: true });
  const node = ui.makeTab(loading, false, favs);
  assert.equal(node.face.dataset.state, 'spin', 'the default is the favicon spinner');
  assert.equal(node.load, undefined);

  ui.syncTab(node, loading, false, favs, false, 'bar');
  assert.equal(node.face.dataset.state, 'img', 'the favicon stays while the bar shows the progress');
  assert.equal(node.load.className, 'side-loadbar');
  assert.equal(node.load.parent, node.root);
  assert.equal(node.load.style.getPropertyValue('--pct'), '0.4');

  ui.syncTab(node, loading, false, favs, false, 'favicon');
  assert.equal(node.load.className, 'side-ring');
  assert.equal(node.load.parent, node.face);
  assert.equal(node.root.children.some((c) => c.className === 'side-loadbar'), false, 'the bar leaves when the ring comes');

  ui.syncTab(node, loading, false, favs, false, 'none');
  assert.equal(node.face.dataset.state, 'img');
  assert.equal(node.load, undefined);

  ui.syncTab(node, item(1, { fav: true }), false, favs, false, 'bar');
  assert.equal(node.load, undefined, 'a finished load carries no mark');
});

test('tabs_close_left, tabs_close_always and tabs_pinned_separator reach the sidebar list', () => {
  const root = styled(new Node('div'));
  root.id = 'app';
  const side = ui.createSidebarRenderer(root);
  const msg = { t: 'tabs', active: 2, items: [item(1, { pinned: true }), item(2), item(3, { loading: true, pct: 50 })] };
  side.render(msg, new Map(), { name: 'Workspace', icon: 'g:briefcase' });
  const list = root.querySelector('.side-list');
  assert.ok(list, 'the renderer builds its list');
  assert.equal(root.querySelector('.side-sep'), null, 'no separator unless the option asks for it');

  side.options({ ...OPTS, closeLeft: true, closeAlways: true, separator: true, indicator: 'bar' }, true);
  assert.equal(list.classList.contains('side-close-left'), true);
  assert.equal(list.classList.contains('side-close-always'), true);
  assert.ok(root.querySelector('.side-sep'), 'the pinned separator shows under the pinned tiles');
  assert.ok(root.querySelector('.side-loadbar'), 'the loading row redraws with the chosen indicator');

  side.options(OPTS, true);
  assert.equal(list.classList.contains('side-close-left'), false);
  assert.equal(list.classList.contains('side-close-always'), false);
  assert.equal(root.querySelector('.side-sep'), null);
  assert.equal(root.querySelector('.side-loadbar'), null);
});

test('a closed row that had the keyboard hands it to the row beside it (U10)', () => {
  const root = styled(new Node('div'));
  root.id = 'app';
  const side = ui.createSidebarRenderer(root);
  const workspace = { name: 'Workspace', icon: 'g:briefcase' };
  side.render({ t: 'tabs', active: 2, items: [item(1, { pinned: true }), item(2), item(3), item(4)] }, new Map(), workspace);
  const row = (id) => root.querySelectorAll('.side-tab').find((node) => node.dataset.id === String(id));
  row(3).focus();
  side.render({ t: 'tabs', active: 2, items: [item(1, { pinned: true }), item(2), item(4)] }, new Map(), workspace);
  assert.equal(document.activeElement, row(4), 'the row after it');
  assert.equal(row(4).tabIndex, 0);
  side.render({ t: 'tabs', active: 2, items: [item(1, { pinned: true }), item(2)] }, new Map(), workspace);
  assert.equal(document.activeElement, row(2), 'the last row: the one before');
});

test('with the horizontal strip in use, a changed indicator or separator does not draw the old sidebar over it (U8)', () => {
  const root = styled(new Node('div'));
  root.id = 'app';
  const side = ui.createSidebarRenderer(root);
  side.render({ t: 'tabs', active: 2, items: [item(1, { pinned: true }), item(2)] }, new Map(), { name: 'Workspace', icon: 'g:briefcase' });
  // The layout switched to the strip, which took the shared root over.
  const stripTab = new Node('div');
  stripTab.className = 'tab';
  root.replaceChildren(stripTab);
  side.options({ ...OPTS, separator: true, indicator: 'bar' }, false);
  assert.equal(root.querySelector('.side-list'), null, 'the strip keeps the root');
  assert.deepEqual(root.children, [stripTab]);
  // Back in the sidebar, the next options message draws with the new look.
  side.options({ ...OPTS, separator: true, indicator: 'favicon' }, true);
  assert.ok(root.querySelector('.side-list'));
  assert.ok(root.querySelector('.side-sep'));
});

test('sidebar.css shows and moves the close button for those classes; narrow strip tabs keep it under close-always', () => {
  const side = css('sidebar.css');
  assert.match(side, /\.side-list\.side-close-always \.side-tab:not\(\.pinned\) \.side-close \{ display: grid; \}/);
  assert.match(side, /\.side-list\.side-close-left \.side-tab:not\(\.pinned\) \.side-close \{ grid-column: 1;/);
  assert.match(side, /\.side-loadbar \{/);
  assert.match(side, /\.side-face > \.side-ring \{/);
  const strip = css('tabs.css');
  const narrow = /@container \(max-width: 111\.9px\) \{([^}]*)\}/.exec(strip)?.[1] ?? '';
  assert.match(narrow, /\.strip:not\(\.close-always\) \.tab:not\(\.active\) \.close/);
  assert.doesNotMatch(narrow, /(^|,)\s*\.strip \.tab:not\(\.active\) \.close/);
});
