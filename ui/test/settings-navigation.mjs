// Navigation integration with mutable layout, queued animation frames, ResizeObserver delivery,
// browser-like scroll clamping and delayed real Media rendering. This is not a native UI proof.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import * as esbuild from 'esbuild';
import { Node, FakeDocument } from './fake-dom.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const fixture = JSON.parse(readFileSync(join(here, 'fixtures/settings-messages.json'), 'utf8'));
const extensions = JSON.parse(readFileSync(join(here, 'fixtures/extensions-messages.json'), 'utf8'));
const out = esbuild.buildSync({ entryPoints: [join(here, '../src/settings-page.ts')], bundle: true, format: 'esm', platform: 'neutral', write: false, logLevel: 'silent' });
const { startSettingsPage } = await import(`data:text/javascript;base64,${Buffer.from(out.outputFiles[0].contents).toString('base64')}`);

class LayoutNode extends Node {
  get lastElementChild() { return [...this.children].reverse().find((node) => node.tagName !== '#text') ?? null; }
  after(node) { const parent = this.parent; parent.detach(node); parent.children.splice(parent.children.indexOf(this) + 1, 0, node); }
  replaceWith(node) { const parent = this.parent; const at = parent.children.indexOf(this); parent.detach(node); parent.children.splice(at, 1, node); this.parent = null; }
  canPlayType() { return 'probably'; }
  scrollTo({ top = 0, behavior = 'auto' }) {
    const destination = Math.max(0, Math.min(top, Math.max(0, this.scrollHeight - this.clientHeight)));
    (this.scrollCalls ??= []).push({ top: destination, behavior });
    if (behavior === 'smooth') this.smoothDestination = destination;
    else { this.scrollTop = destination; this.smoothDestination = null; }
  }
}

function harness({ ready = true, snapshot = true, reduced = false } = {}) {
  const frames = new Map();
  const observers = [];
  const intersections = [];
  const inbound = [];
  const sent = [];
  let frameId = 0;
  let resolveMedia;
  const media = new Promise((resolve) => { resolveMedia = resolve; });
  const document = new FakeDocument();
  document.createElement = (tag) => new LayoutNode(tag);
  document.createElementNS = (_namespace, tag) => new LayoutNode(tag);
  document.createTextNode = (text) => new LayoutNode('#text', text);
  Object.assign(globalThis, {
    document, window: globalThis, Element: Node, HTMLElement: Node, HTMLInputElement: class {},
    innerWidth: 1280, innerHeight: 800,
    matchMedia: () => ({ matches: reduced }),
    requestAnimationFrame: (callback) => { const id = ++frameId; frames.set(id, callback); return id; },
    cancelAnimationFrame: (id) => frames.delete(id),
    addEventListener() {}, removeEventListener() {},
    localStorage: { getItem: (key) => key === 'bowser.settings.view' ? 'all' : null, setItem() {} },
    ResizeObserver: class {
      constructor(callback) { this.callback = callback; this.targets = new Map(); observers.push(this); }
      observe(node) { this.targets.set(node, null); }
      disconnect() { this.targets.clear(); }
    },
    IntersectionObserver: class {
      constructor(callback) { this.callback = callback; intersections.push(this); }
      observe(node) { this.node = node; }
      disconnect() { this.node = null; }
    },
    chrome: { webview: { postMessage: (wire) => sent.push(JSON.parse(wire)), addEventListener: (_type, callback) => inbound.push(callback) } },
  });
  Object.defineProperty(globalThis, 'navigator', { configurable: true, value: {
    mediaCapabilities: { decodingInfo: () => media }, requestMediaKeySystemAccess: async () => { throw new Error('not available'); },
  } });
  const app = new LayoutNode('div');
  startSettingsPage(app);
  const main = app.querySelector('main');
  const column = app.querySelector('.sp-col');
  const areas = app.querySelectorAll('section[data-area]');
  const area = (key) => areas.find((node) => node.dataset.area === key);
  const extra = new Map();
  const partExtra = new Map();
  const height = (node) => {
    if (node.hidden) return 0;
    if (node.dataset.area === 'media') {
      const results = app.querySelector('.sp-media');
      return 160 + results.querySelectorAll('.sp-media-item').length * 80 + (results.querySelector('.sp-hints')?.children.length ?? 0) * 70;
    }
    const partGrowth = node.querySelectorAll('section.sp-part').reduce((sum, part) => sum + (partExtra.get(part.dataset.section) ?? 0), 0);
    return 300 + (extra.get(node.dataset.area) ?? 0) + partGrowth;
  };
  const top = (node) => 40 + areas.slice(0, areas.indexOf(node)).reduce((sum, prior) => sum + height(prior), 0);
  for (const node of areas) {
    Object.defineProperty(node, 'offsetTop', { configurable: true, get: () => ready ? top(node) : 0 });
    Object.defineProperty(node, 'offsetHeight', { configurable: true, get: () => ready ? height(node) : 0 });
    const parts = node.querySelectorAll('section.sp-part');
    for (const [index, part] of parts.entries()) {
      Object.defineProperty(part, 'offsetTop', { configurable: true, get: () => ready ? top(node) + 60 + parts.slice(0, index).reduce((sum, prior) => sum + 70 + (partExtra.get(prior.dataset.section) ?? 0), 0) : 0 });
      Object.defineProperty(part, 'offsetHeight', { configurable: true, get: () => ready ? 70 + (partExtra.get(part.dataset.section) ?? 0) : 0 });
    }
  }
  Object.defineProperty(main, 'scrollHeight', { get: () => ready ? 160 + areas.reduce((sum, node) => sum + height(node), 0) : 0 });
  main.clientHeight = ready ? 640 : 0;
  Object.defineProperty(main, 'offsetHeight', { get: () => main.clientHeight });
  Object.defineProperty(column, 'offsetHeight', { get: () => main.scrollHeight });
  const deliver = (message) => { for (const callback of inbound) callback({ data: JSON.stringify(message) }); };
  const flush = () => { const batch = [...frames]; frames.clear(); for (const [, callback] of batch) callback(0); };
  const resize = () => {
    for (const observer of observers) {
      const changed = [];
      for (const [node, previous] of observer.targets) {
        const size = `${node.offsetWidth}:${node.offsetHeight}`;
        if (size !== previous) { observer.targets.set(node, size); changed.push({ target: node }); }
      }
      if (changed.length) observer.callback(changed, observer);
    }
  };
  if (snapshot) deliver(fixture.snap);
  deliver(extensions.snap);
  const popup = app.querySelectorAll('button').find((button) => button.getAttribute('aria-label')?.startsWith('Open Reader'));
  assert.ok(popup, 'the real Extensions renderer supplies the popup control');
  popup.getBoundingClientRect = () => {
    const y = area('extensions').offsetTop + 120 - main.scrollTop;
    return { top: y, bottom: y + 30, height: 30, left: 0, right: 80, width: 80 };
  };
  resize(); flush();
  return {
    app, main, area, popup, sent, deliver, flush, resize, frames,
    go: (key) => deliver({ v: 1, t: 'settingsgo', s: key }),
    current: () => app.querySelector('.sp-nav-item[aria-current="true"]').dataset.section,
    target: (key) => Math.max(0, Math.min(area(key).offsetTop - 8, main.scrollHeight - main.clientHeight)),
    visible: () => popup.getBoundingClientRect().top >= 0 && popup.getBoundingClientRect().bottom <= main.clientHeight,
    grow: (key, delta) => extra.set(key, (extra.get(key) ?? 0) + delta),
    growPart: (key, delta) => partExtra.set(key, (partExtra.get(key) ?? 0) + delta),
    showHost: () => { ready = true; main.clientHeight = 640; },
    startMedia: () => {
      const observer = intersections.find((entry) => entry.node?.querySelector('[data-row="media"]'));
      assert.ok(observer, 'the Media card has its real intersection observer');
      observer.callback([{ target: observer.node, isIntersecting: true }], observer);
    },
    finishMedia: async () => {
      resolveMedia({ supported: true, smooth: true, powerEfficient: false });
      // Flush Promise.all/settle/runMediaCheck and the real paintMedia continuation.
      await new Promise((resolve) => setImmediate(resolve));
      assert.equal(app.querySelector('.sp-media-status').textContent, 'Check complete');
      assert.equal(app.querySelectorAll('.sp-media-item').length, 6);
    },
  };
}

const storeSnapshot = { v: 1, t: 'snap', p: 'store', d: { shown: true, catalogue: [], phase: 'idle', requestId: '', extensionId: '', name: '', version: '', error: '', restart: false, permissions: [], hostPermissions: [], contentScripts: [], contentScriptDeclarations: [], optionalPermissions: [], optionalHostPermissions: [] } };

test('About stays selected AND a control in it stays visible after delayed Media expands', async () => {
  const h = harness();
  // Media sits above About, so a late media report pushes About down until layout repair scrolls it back.
  const probe = h.app.querySelector('section[data-area="about"] button');
  assert.ok(probe, 'About has a control below Media');
  probe.getBoundingClientRect = () => {
    const y = h.area('about').offsetTop + 120 - h.main.scrollTop;
    return { top: y, bottom: y + 30, height: 30, left: 0, right: 80, width: 80 };
  };
  const seen = () => {
    const box = probe.getBoundingClientRect();
    return box.top >= 0 && box.bottom <= h.main.clientHeight;
  };
  h.go('about'); h.flush();
  assert.equal(h.current(), 'about');
  assert.equal(h.main.scrollTop, h.target('about'));
  assert.ok(seen());
  h.startMedia();
  const oldTop = h.main.scrollTop;
  await h.finishMedia();
  assert.equal(h.main.scrollTop, oldTop, 'Promise completion does not itself fake a scroll');
  assert.ok(!seen(), 'late rendered content reproduces the selected-but-offscreen contract');
  h.resize(); h.resize();
  assert.equal(h.frames.size, 1, 'layout notifications coalesce');
  h.flush();
  assert.equal(h.current(), 'about');
  assert.equal(h.main.scrollTop, h.target('about'));
  assert.ok(seen());
  assert.deepEqual(h.sent, [{ v: 1, t: 'ready' }], 'layout repair sends no navigation or extension command');
});

test('every existing user-input unpin defeats a queued correction and restores scroll tracking', () => {
  for (const type of ['wheel', 'pointerdown', 'keydown', 'touchstart']) {
    const h = harness(); h.go('extensions'); h.flush();
    h.grow('address', 700); h.resize();
    h.main.dispatch(type, { key: 'PageUp' });
    h.main.scrollTop = h.area('address').offsetTop;
    h.main.dispatch('scroll');
    h.flush();
    assert.equal(h.main.scrollTop, h.area('address').offsetTop, type);
    assert.equal(h.current(), 'address', type);
  }
});

test('first-show host geometry and a later viewport clamp are remeasured', () => {
  const h = harness({ ready: false });
  h.go('extensions'); h.flush();
  assert.equal(h.main.scrollTop, 0);
  h.showHost(); h.resize(); h.flush();
  assert.equal(h.main.scrollTop, h.target('extensions'));
  assert.ok(h.visible());
  h.go('about'); h.flush();
  assert.equal(h.main.scrollTop, h.main.scrollHeight - h.main.clientHeight);
  h.main.clientHeight = 900; h.resize(); h.flush();
  assert.equal(h.main.scrollTop, h.main.scrollHeight - 900);
  assert.equal(h.current(), 'about');
  const calls = h.main.scrollCalls.length;
  h.resize(); h.flush();
  assert.equal(h.main.scrollCalls.length, calls, 'stable end clamp causes no correction loop');
});

test('latest navigation wins queued layout and pre-navigation scroll-spy callbacks', () => {
  const h = harness();
  h.main.clientHeight = 2000;
  h.main.dispatch('scroll'); // An old scroll spy is already queued.
  h.go('extensions'); h.flush();
  assert.equal(h.current(), 'extensions', 'a pre-navigation scroll spy must respect the new pin even at a clamped destination');
  h.grow('appearance', 500); h.resize();
  h.go('privacy'); h.flush();
  assert.equal(h.current(), 'privacy');
  assert.equal(h.main.scrollTop, h.target('privacy'));
  h.go(''); h.grow('profile', 200); h.resize(); h.flush();
  assert.equal(h.current(), 'profile');
  assert.equal(h.main.scrollTop, 0, 'the empty key is a pinned top navigation, not no target');
});

test('merged subsection geometry and opposing area size changes keep the precise target', () => {
  const h = harness(); h.go('search'); h.flush();
  const part = h.app.querySelector('section.sp-part[data-section="search"]');
  assert.equal(h.current(), 'profile', 'search lives in General');
  assert.equal(h.main.scrollTop, part.offsetTop - 8);
  const height = h.main.scrollHeight;
  h.grow('appearance', 120); h.grow('about', -120); h.resize(); h.flush();
  assert.equal(h.main.scrollHeight, height);
  assert.equal(h.main.scrollTop, part.offsetTop - 8, 'area observation catches an unchanged total column size');
});

test('opposing subsection size changes inside one merged area retain the exact deep-link target', () => {
  const h = harness(); h.go('search'); h.flush();
  const target = h.app.querySelector('section.sp-part[data-section="search"]');
  const areaHeight = h.area('profile').offsetHeight;
  const columnHeight = h.main.scrollHeight;
  const oldTop = h.main.scrollTop;
  h.growPart('profile', 40); h.growPart('startpage', -40);
  assert.equal(h.area('profile').offsetHeight, areaHeight);
  assert.equal(h.main.scrollHeight, columnHeight);
  assert.equal(target.offsetTop - 8, oldTop + 40, 'only the retained subsection destination moves');
  h.resize();
  assert.equal(h.frames.size, 1, 'the changed section roots schedule one correction');
  h.flush();
  assert.equal(h.current(), 'profile', 'search lives in General');
  assert.equal(h.main.scrollTop, target.offsetTop - 8);
});

test('search, a subpage, and Store stay in control when delayed Media resolves', async () => {
  for (const surface of ['search', 'subpage', 'store']) {
    const h = harness(); h.go('extensions'); h.flush(); h.startMedia();
    // Queue old work first; cancellation must be checked when the callback actually runs.
    h.grow('appearance', 50); h.resize();
    if (surface === 'search') {
      const input = h.app.querySelector('.sp-search-input'); input.value = 'privacy'; input.dispatch('input');
    } else if (surface === 'subpage') h.app.querySelector('[data-opens="engines"]').querySelector('button').click();
    else h.deliver(storeSnapshot);
    const top = h.main.scrollTop;
    const calls = h.main.scrollCalls.length;
    await h.finishMedia(); h.resize(); h.flush();
    assert.equal(h.main.scrollTop, top, surface);
    assert.equal(h.main.scrollCalls.length, calls, surface);
    if (surface === 'subpage') assert.ok(!h.app.querySelector('section[data-page="engines"]').hidden);
    if (surface === 'store') assert.ok(h.app.querySelector('.sp').hidden);
    // Leaving the surface must not resurrect the old navigation pin either.
    if (surface === 'search') { const input = h.app.querySelector('.sp-search-input'); input.value = ''; input.dispatch('input'); }
    else if (surface === 'subpage') h.app.querySelector('section[data-page="engines"]').querySelector('.sp-back').click();
    else h.deliver({ ...storeSnapshot, d: { ...storeSnapshot.d, shown: false } });
    const afterReturn = h.main.scrollTop;
    const afterCalls = h.main.scrollCalls.length;
    h.grow('appearance', 40); h.resize(); h.flush();
    assert.equal(h.main.scrollTop, afterReturn, `${surface} return`);
    assert.equal(h.main.scrollCalls.length, afterCalls, `${surface} return`);
  }
});

test('unchanged observer delivery preserves smooth navigation and reduced motion stays instant', () => {
  for (const reduced of [false, true]) {
    const h = harness({ reduced });
    h.app.querySelector('.sp-nav-item[data-section="extensions"]').click();
    assert.equal(h.main.scrollCalls.at(-1).behavior, reduced ? 'auto' : 'smooth');
    const calls = h.main.scrollCalls.length;
    h.resize(); h.flush();
    assert.equal(h.main.scrollCalls.length, calls, 'a pending smooth scroll is not mistaken for drift');
    h.grow('appearance', 150); h.resize(); h.flush();
    assert.equal(h.main.scrollCalls.at(-1).behavior, 'auto', 'genuine reflow stabilizes without another animation');
    assert.equal(h.main.scrollTop, h.target('extensions'));
  }
});

test('a deep link received before the Settings snapshot anchors after it arrives', () => {
  const h = harness({ snapshot: false });
  h.go('extensions');
  assert.equal(h.main.scrollCalls, undefined);
  h.deliver(fixture.snap); h.flush();
  assert.equal(h.current(), 'extensions');
  assert.equal(h.main.scrollTop, h.target('extensions'));
  h.grow('appearance', 180); h.resize(); h.flush();
  assert.ok(h.visible());
});
