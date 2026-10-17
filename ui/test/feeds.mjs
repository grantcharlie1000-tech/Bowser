// The Feeds panel (thread T13): the Rust-written snapshots against the strict contract, the commands the page may
// send (never an address to open), and the panel's DOM in a fake document: subscriptions, items, preview,
// read/unread, and that feed text is only ever set as text.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import * as esbuild from 'esbuild';

const here = dirname(fileURLToPath(import.meta.url));
const srcDir = join(here, '..', 'src');
const fixture = JSON.parse(readFileSync(join(here, 'fixtures', 'feeds-messages.json'), 'utf8'));

async function bundle(file) {
  const out = esbuild.buildSync({ entryPoints: [join(srcDir, file)], bundle: true, format: 'esm', platform: 'neutral', write: false, logLevel: 'silent' });
  return import(`data:text/javascript;base64,${Buffer.from(out.outputFiles[0].contents).toString('base64')}`);
}

// ---- a small fake DOM ----
function matchOne(node, s) {
  const tag = /^[a-z][a-z0-9]*/.exec(s)?.[0];
  if (tag && node.tagName !== tag) return false;
  for (const cls of s.matchAll(/\.([\w-]+)/g)) if (!node.classList.contains(cls[1])) return false;
  return true;
}
class ClassList {
  constructor(owner) { this.owner = owner; }
  get set() { return new Set(this.owner.className.split(/\s+/).filter(Boolean)); }
  add(...n) { const s = this.set; n.forEach((x) => s.add(x)); this.owner.className = [...s].join(' '); }
  remove(...n) { const s = this.set; n.forEach((x) => s.delete(x)); this.owner.className = [...s].join(' '); }
  contains(n) { return this.set.has(n); }
  toggle(n, force) { const on = force ?? !this.contains(n); if (on) this.add(n); else this.remove(n); return on; }
}
class Node {
  constructor(tag, text = null) {
    this.tagName = tag; this.className = ''; this.attrs = new Map(); this.childList = []; this.parent = null; this.listeners = {};
    this.classList = new ClassList(this); this.dataset = {}; this.hidden = false; this.disabled = false; this.ownText = text; this.id = '';
    this.value = ''; this.tabIndex = 0;
    this.style = { setProperty() {}, getPropertyValue: () => '' };
  }
  set innerHTML(_v) { throw new TypeError('Trusted Types'); }
  get children() { return this.childList.filter((c) => c.tagName !== '#text'); }
  get textContent() { return this.ownText ?? this.childList.map((c) => c.textContent).join(''); }
  set textContent(v) { this.childList = []; this.ownText = null; if (v !== '') this.append(new Node('#text', String(v))); }
  setAttribute(k, v) { this.attrs.set(k, String(v)); if (k === 'id') this.id = String(v); }
  getAttribute(k) { return this.attrs.has(k) ? this.attrs.get(k) : null; }
  hasAttribute(k) { return this.attrs.has(k); }
  removeAttribute(k) { this.attrs.delete(k); }
  toggleAttribute(k, on) { if (on ?? !this.attrs.has(k)) this.attrs.set(k, ''); else this.attrs.delete(k); }
  addEventListener(t, cb) { (this.listeners[t] ??= []).push(cb); }
  removeEventListener() {}
  dispatch(type, extra = {}) { for (const cb of this.listeners[type] ?? []) cb({ type, target: this, preventDefault() {}, stopPropagation() {}, key: '', ...extra }); }
  click() { if (!this.disabled) this.dispatch('click'); }
  append(...nodes) { for (const n of nodes) { const node = typeof n === 'string' ? new Node('#text', n) : n; this.ownText = null; node.parent = this; this.childList.push(node); } }
  replaceChildren(...nodes) { this.childList = []; this.ownText = null; this.append(...nodes); }
  all() { return this.childList.flatMap((c) => [c, ...c.all()]); }
  matches(s) { return s.split(',').some((x) => matchOne(this, x.trim())); }
  querySelectorAll(s) { return this.all().filter((n) => n.tagName !== '#text' && n.matches(s)); }
  querySelector(s) { return this.querySelectorAll(s)[0] ?? null; }
  focus() { document.activeElement = this; }
  contains(n) { return n === this || this.all().includes(n); }
}
const sent = [];
globalThis.document = { activeElement: null, createElement: (t) => new Node(t), createElementNS: (_n, t) => new Node(t), createTextNode: (x) => new Node('#text', x) };
Object.assign(globalThis, {
  window: globalThis, setTimeout: () => 1, clearTimeout() {}, requestAnimationFrame: (cb) => { cb(); return 1; },
  chrome: { webview: { postMessage: (w) => sent.push(JSON.parse(w)), addEventListener() {} } },
});
globalThis.Element = Node; globalThis.HTMLElement = Node; globalThis.HTMLInputElement = Node;

const logic = await bundle('panels/feeds-logic.ts');
const { startFeedsPanel } = await bundle('panels/feeds.ts');

test('the Rust snapshots pass the strict contract and anything extra or missing is refused', () => {
  for (const name of ['snap', 'preview', 'busy', 'empty', 'guest']) {
    const parsed = logic.parseFeedsSnapshot(fixture[name].d);
    assert.ok(parsed, name);
  }
  const snap = logic.parseFeedsSnapshot(fixture.snap.d);
  assert.equal(snap.unread, 3);
  assert.deepEqual(snap.feeds.map((f) => f.kind), ['article', 'video', 'podcast']);
  assert.equal(snap.items[0].title, 'Two');
  assert.equal(snap.items.find((i) => i.media === 'audio').duration, '45:12');
  assert.equal(logic.parseFeedsSnapshot(fixture.guest.d).allowed, false);
  assert.equal(logic.parseFeedsSnapshot(fixture.preview.d).preview.subscribed, false);
  const d = fixture.snap.d;
  for (const bad of [
    { ...d, extra: 1 }, { ...d, feeds: [{ ...d.feeds[0], kind: 'rss' }] }, { ...d, items: [{ ...d.items[0], feed: 99 }] },
    { ...d, items: [{ ...d.items[0], h: 0 }] }, { ...d, selected: -1 }, { ...d, refresh: 'x' }, { ...d, page: [{ url: 1, title: '', subscribed: false }] },
    { ...d, feeds: [d.feeds[0], d.feeds[0]] }, { ...d, status: 1 },
  ]) assert.equal(logic.parseFeedsSnapshot(bad), null, JSON.stringify(bad).slice(0, 80));
  const { allowed, ...partial } = d;
  assert.equal(logic.parseFeedsSnapshot(partial), null);
  assert.ok(allowed);
});

test('commands carry ids and hashes, and an address only to preview or subscribe', () => {
  assert.deepEqual(logic.openCommand(2, 99), { a: 'open', id: 2, parent: 99 });
  assert.deepEqual(logic.playCommand(2, 99), { a: 'play', id: 2, parent: 99 });
  assert.deepEqual(logic.readCommand(2, 99, false), { a: 'read', id: 2, parent: 99, on: false });
  assert.deepEqual(logic.previewCommand('  https://a.example/feed '), { a: 'preview', text: 'https://a.example/feed' });
  assert.deepEqual(logic.refreshCommand(), { a: 'refresh' });
  assert.deepEqual(logic.readAllCommand(0), { a: 'readall' });
  assert.deepEqual(logic.readAllCommand(3), { a: 'readall', id: 3 });
  for (const bad of [logic.openCommand(0, 1), logic.openCommand(1, 0), logic.openCommand(1.5, 1), logic.unsubscribeCommand(0), logic.previewCommand(''),
    logic.previewCommand('a b'), logic.previewCommand('x'.repeat(2049)), logic.subscribeCommand('a\nb'), logic.refreshCommand(0), logic.selectCommand(-1)]) assert.equal(bad, null);
  assert.equal(logic.ageText(0, 100), '');
  assert.equal(logic.ageText(70, 100), 'now');
  assert.equal(logic.ageText(100000 - 3 * 3600, 100000), '3h');
  assert.equal(logic.ageText(1, 40 * 86400), '1970-01-01');
  assert.match(logic.refreshNote(0), /only when you press Refresh/);
  assert.match(logic.refreshNote(60), /older than an hour/);
});

const root = new Node('div');
const receive = startFeedsPanel(root);
const deliver = (msg) => receive({ t: 'snap', p: 'feeds', d: msg.d });
const rows = (cls) => root.querySelectorAll(cls);

test('the panel lists subscriptions and items, marks unread, and opens by naming the item', () => {
  deliver(fixture.snap);
  assert.deepEqual(rows('.pf-feed').map((r) => r.querySelector('.bw-row-title').textContent), ['All feeds', 'A "News"', 'Channel', 'Pod']);
  assert.equal(rows('.pf-item').length, 4);
  assert.equal(rows('.pf-item.is-unread').length, 3);
  assert.equal(root.querySelector('.pn-title').textContent, 'Feeds · 3 unread');
  const first = rows('.pf-item')[0];
  sent.length = 0;
  first.click();
  assert.deepEqual(sent.map((m) => ({ ...m, v: undefined })), [{ v: undefined, t: 'feed', a: 'open', id: 1, parent: logic.parseFeedsSnapshot(fixture.snap.d).items[0].h }]);
  sent.length = 0;
  rows('.pf-feed')[2].click();
  assert.deepEqual(sent, [{ v: 1, t: 'feed', a: 'select', id: 2 }]);
  const podcast = rows('.pf-item').find((r) => r.textContent.includes('Episode'));
  const play = podcast.querySelectorAll('button').find((b) => b.getAttribute('aria-label') === 'Play the episode');
  assert.ok(play, 'an episode has a Play button');
  sent.length = 0;
  play.click();
  assert.equal(sent[0].a, 'play');
});

test('mark read and unread, refresh, import and unsubscribe (two clicks) each send one command', () => {
  deliver(fixture.snap);
  const item = rows('.pf-item').find((r) => r.classList.contains('is-unread'));
  const toggle = item.querySelectorAll('button').find((b) => b.getAttribute('aria-label') === 'Mark read');
  sent.length = 0;
  toggle.click();
  assert.equal(sent[0].a, 'read');
  assert.equal(sent[0].on, true);
  const [refresh, imp, readAll] = root.querySelector('.pn-tools').querySelectorAll('button');
  sent.length = 0;
  refresh.click(); imp.click(); readAll.click();
  assert.deepEqual(sent.map((m) => m.a), ['refresh', 'import', 'readall']);
  const feedRow = rows('.pf-feed')[1];
  const trash = feedRow.querySelectorAll('button').find((b) => b.getAttribute('aria-label') === 'Unsubscribe');
  sent.length = 0;
  trash.click();
  assert.deepEqual(sent, [], 'the first click only asks to confirm');
  const confirm = rows('.pf-feed')[1].querySelectorAll('button').find((b) => b.getAttribute('aria-label')?.startsWith('Confirm'));
  confirm.click();
  assert.deepEqual(sent, [{ v: 1, t: 'feed', a: 'unsubscribe', id: 1 }]);
});

test('a preview shows its items with Subscribe, and a page’s feeds offer Preview', () => {
  deliver(fixture.preview);
  assert.equal(root.querySelector('.pf-preview').hidden, false);
  const subscribe = root.querySelector('.pf-preview').querySelectorAll('button').find((b) => b.textContent.includes('Subscribe'));
  sent.length = 0;
  subscribe.click();
  assert.deepEqual(sent, [{ v: 1, t: 'feed', a: 'subscribe', text: 'https://new.example/feed' }]);
  const page = logic.parseFeedsSnapshot(fixture.preview.d);
  assert.equal(page.page[0].subscribed, false);
  deliver({ d: { ...fixture.snap.d, page: [{ url: 'https://new.example/feed', title: 'New', subscribed: false }] } });
  assert.equal(root.querySelector('.pf-page').hidden, false);
  sent.length = 0;
  root.querySelector('.pf-page').querySelectorAll('button')[0].click();
  assert.deepEqual(sent, [{ v: 1, t: 'feed', a: 'preview', text: 'https://new.example/feed' }]);
});

test('typing an address previews it; a bad one is refused before anything is sent', () => {
  deliver(fixture.snap);
  const field = root.querySelector('input');
  sent.length = 0;
  field.value = '   ';
  root.querySelector('form').dispatch('submit');
  assert.deepEqual(sent, []);
  assert.match(root.querySelector('.pf-error').textContent, /Enter a feed or page address/);
  field.value = 'example.com/feed';
  root.querySelector('form').dispatch('submit');
  assert.deepEqual(sent, [{ v: 1, t: 'feed', a: 'preview', text: 'example.com/feed' }]);
});

test('feed text is set as text: markup in a title never becomes an element', () => {
  const hostile = structuredClone(fixture.snap.d);
  hostile.items[0].title = '<img src=x onerror=alert(1)>';
  hostile.items[0].summary = '<script>alert(1)</script>';
  deliver({ d: hostile });
  const item = rows('.pf-item')[0];
  assert.equal(item.querySelector('.pf-title').textContent, '<img src=x onerror=alert(1)>');
  assert.equal(root.querySelectorAll('img').length, 0);
  assert.equal(root.querySelectorAll('script').length, 0);
});

test('a guest window offers no feeds, and an empty profile says how to start', () => {
  deliver(fixture.guest);
  assert.match(root.querySelector('.pf-items').textContent, /guest window does not keep feeds/);
  assert.equal(root.querySelector('input').disabled, true);
  deliver(fixture.empty);
  assert.match(root.querySelector('.pf-items').textContent, /No feeds yet/);
  deliver(fixture.busy);
  assert.equal(root.querySelector('.pf').classList.contains('is-busy'), true);
  assert.equal(root.querySelector('.pf-status').textContent, 'Refreshing 2 feeds…');
});
