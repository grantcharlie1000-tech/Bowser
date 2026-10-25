// Run the production marker script against disposable DOM fixtures without a browser process.
// Browser-backed computed-style coverage remains in x-promoted.mjs; this harness has no layout.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import vm from 'node:vm';

const SCRIPT = readFileSync(new URL('../../src/blocker/x_promoted.js', import.meta.url), 'utf8');
const RUST = readFileSync(new URL('../../src/blocker/x_promoted.rs', import.meta.url), 'utf8');
const STOP = /pub const STOP_SCRIPT: &str = "([^"]+)";/.exec(RUST)[1];
const MARK = 'data-bowser-ad';
class Element {
  constructor(tag, attrs = {}, ...children) { this.tag = tag; this.attrs = { ...attrs }; this.children = []; this.append(...children); }
  append(...children) { for (const child of children) { if (child instanceof Element) child.parentElement = this; this.children.push(child); } }
  get textContent() { return this.children.map((c) => c instanceof Element ? c.textContent : c).join(''); }
  set textContent(text) { this.children = [text]; }
  getAttribute(name) { return this.attrs[name] ?? null; }
  hasAttribute(name) { return name in this.attrs; }
  setAttribute(name, value) { this.attrs[name] = value; }
  removeAttribute(name) { delete this.attrs[name]; }
  contains(node) { return this === node || this.children.some((c) => c instanceof Element && c.contains(node)); }
  matches(selector) {
    return selector.split(',').some((part) => {
      const tag = /^[a-z]+/.exec(part)?.[0];
      if (tag && tag !== this.tag) return false;
      return [...part.matchAll(/\[([\w-]+)(?:="([^"]*)")?\]/g)].every(([, key, value]) => this.hasAttribute(key) && (value === undefined || this.getAttribute(key) === value));
    });
  }
  querySelectorAll(selector) { return this.children.flatMap((c) => c instanceof Element ? [...(c.matches(selector) ? [c] : []), ...c.querySelectorAll(selector)] : []); }
  querySelector(selector) { return this.querySelectorAll(selector)[0] ?? null; }
  closest(selector) { return this.matches(selector) ? this : this.parentElement?.closest(selector) ?? null; }
}
const el = (tag, attrs = {}, ...children) => new Element(tag, attrs, ...children);
const badge = (...children) => el('div', {}, ...children);
function post(label, { name = 'Someone', body = 'Normal content', caret = true, wrap = true, extra = [] } = {}) {
  const author = el('div', { 'data-testid': 'User-Name' }, el('a', { href: '/fixture', role: 'link' }, name));
  const header = el('div', {}, author, ...(label ? [label] : []), ...(caret ? [el('button', { 'data-testid': 'caret', 'aria-label': 'More' })] : []));
  const tweet = el('article', { 'data-testid': 'tweet', role: 'article' }, header, el('div', { 'data-testid': 'tweetText' }, body), ...extra);
  return { cell: wrap ? el('div', { 'data-testid': 'cellInnerDiv' }, tweet) : tweet, tweet, header, author, label };
}
function fixture(...posts) {
  const style = el('style', { id: 'bowser-cosmetic' });
  const timeline = el('main', {}, ...posts.map((p) => p.cell));
  const root = el('html', {}, style, el('body', {}, timeline));
  let observer;
  let timer = 0;
  const timers = new Map();
  const logs = [];
  const document = {
    documentElement: root, body: root.querySelector('body'),
    getElementById: (id) => root.querySelectorAll('[id]').find((n) => n.getAttribute('id') === id) ?? null,
    querySelectorAll: (selector) => root.querySelectorAll(selector),
    addEventListener() {}, removeEventListener() {},
  };
  const context = vm.createContext({
    document, console: { debug: (line) => logs.push(line) },
    setTimeout: (cb) => { timers.set(++timer, cb); return timer; }, clearTimeout: (id) => timers.delete(id),
    MutationObserver: class {
      constructor(callback) { this.callback = callback; this.active = false; observer = this; }
      observe(target, options) { this.target = target; this.options = options; this.active = true; }
      disconnect() { this.active = false; }
    },
  });
  context.window = context;
  const run = (script = SCRIPT) => vm.runInContext(script, context);
  run();
  function mutate(type = 'childList', attributeName) {
    if (observer.active && (type !== 'attributes' || observer.options.attributeFilter.includes(attributeName))) observer.callback([{ type, addedNodes: [], removedNodes: type === 'childList' ? [{}] : [] }]);
    for (const [id, callback] of [...timers]) { timers.delete(id); callback(); }
  }
  return { context, root, style, timeline, run, mutate, logs, get observer() { return observer; }, timers };
}
const marked = (...posts) => posts.map((p) => p.cell.hasAttribute(MARK));

test('split, nested, localized and attribute-only header badges work without a caret', () => {
  const posts = [
    post(badge(el('span', {}, 'Pro'), el('span', {}, 'moted'))),
    post(badge(el('span', {}, 'Ad'))),
    post(el('span', { 'aria-label': ' Sponsored ' }), { caret: false }),
    post(el('div', { title: 'Anzeige' }), { wrap: false }),
    post(badge('PUBLICIDAD')), post(badge('Ads')), post(badge('promocionado')),
  ];
  fixture(...posts);
  assert.deepEqual(marked(...posts), posts.map(() => true));
});

test('names, body, quotes, poll, media, cards, links and menu labels stay visible', () => {
  const posts = [
    post(null, { name: 'Ad', body: 'Promoted' }),
    post(badge('Ad hoc')),
    post(el('span', { 'aria-label': 'Ad' }, 'Unrelated text')),
    post(el('a', { href: '/ad' }, el('span', {}, 'Ad'))),
    post(el('button', { 'aria-label': 'Ad' })),
    ...['quoteTweet', 'cardPoll', 'tweetPhoto', 'card.wrapper'].map((id) => post(null, { extra: [el('div', { 'data-testid': id }, el('span', { 'aria-label': 'Ad' }, 'Ads'))] })),
    post(el('div', { 'data-testid': 'User-Name' }, el('span', { title: 'Ad' }))),
  ];
  fixture(...posts);
  assert.deepEqual(marked(...posts), posts.map(() => false));
});

for (const anchor of ['caret', 'User-Name']) {
  test(`wrapped links and controls stay visible under the ${anchor} anchor`, () => {
    const withAnchor = (label, name = 'Someone') => {
      const result = post(label, { name, caret: anchor === 'caret' });
      if (anchor === 'caret') result.author.removeAttribute('data-testid');
      return result;
    };
    const ordinary = [
      withAnchor(null, 'Ad'),
      withAnchor(badge(el('a', { href: '/fixture' }, el('span', {}, 'Ad')))),
      withAnchor(badge(badge(el('div', { role: 'link' }, el('span', {}, 'Sponsored'))))),
      withAnchor(badge(el('button', {}, el('span', {}, 'Ads')))),
      withAnchor(badge(el('div', { role: 'button' }, el('span', {}, 'Promoted')))),
      withAnchor(badge(el('time', {}, 'Ad'))),
      // Excluded text must not complete a partial badge into the exact label "Ad".
      withAnchor(badge(el('span', {}, 'A'), el('a', { href: '/fixture' }, 'd'))),
    ];
    const split = withAnchor(badge(el('span', {}, 'A'), el('span', {}, 'd')));
    const attribute = withAnchor(el('span', { 'aria-label': 'Sponsored' }));
    const f = fixture(...ordinary, split, attribute);
    assert.deepEqual(marked(...ordinary), ordinary.map(() => false));
    assert.deepEqual(marked(split, attribute), [true, true]);

    // Recycled markup may retain the same text while changing whether it is a badge.
    split.label.children = [];
    split.label.append(el('a', { href: '/fixture' }, el('span', {}, 'Ad')));
    f.mutate();
    assert.deepEqual(marked(split), [false]);
    split.label.children = [];
    split.label.append(el('span', {}, 'A'), el('span', {}, 'd'));
    f.mutate();
    assert.deepEqual(marked(split), [true]);
  });
}

test('attribute and text recycling and removal of the menu/name anchor reveal old cells', () => {
  const first = post(el('span', { 'aria-label': 'Ad' }));
  const second = post(el('span', { title: 'Normal' }), { caret: false });
  const text = post(badge(el('span', {}, 'Ad')));
  const f = fixture(first, second, text);
  assert.deepEqual(marked(first, second, text), [true, false, true]);
  first.label.setAttribute('aria-label', 'Normal'); second.label.setAttribute('title', 'Sponsored');
  f.mutate('attributes', 'aria-label');
  assert.deepEqual(marked(first, second), [false, true]);
  text.label.textContent = 'Normal'; f.mutate('characterData');
  assert.deepEqual(marked(text), [false]);
  second.author.setAttribute('data-testid', 'retired'); f.mutate('attributes', 'data-testid');
  assert.deepEqual(marked(second), [false]);
  second.author.setAttribute('data-testid', 'User-Name'); f.mutate('attributes', 'data-testid');
  assert.deepEqual(marked(second), [true]);
});

test('SPA insertion, replacement and empty recycled cells are rescanned without history hooks', () => {
  const old = post(badge('Ad'));
  const f = fixture(old);
  const next = post(badge('Sponsored'));
  f.timeline.append(next.cell); f.mutate();
  assert.deepEqual(marked(old, next), [true, true]);
  old.cell.children = [el('div', {}, 'Who to follow')]; f.mutate();
  assert.deepEqual(marked(old, next), [false, true]);
  const ordinary = post(null);
  next.cell.children = ordinary.cell.children;
  for (const child of next.cell.children) child.parentElement = next.cell;
  f.mutate();
  assert.deepEqual(marked(next), [false]);
});

test('explicit off stops observation and removes all markers while cookie CSS stays, then reenable works', () => {
  const promoted = post(badge('Ad'));
  const f = fixture(promoted);
  f.run(); // duplicate install is idempotent
  const active = f.observer;
  f.style.textContent = '.cookie-notice{display:none!important}';
  f.run(STOP);
  assert.equal(f.context.__bowserXPromoted, null);
  assert.equal(active.active, false);
  assert.equal(f.timers.size, 0);
  assert.deepEqual(marked(promoted), [false]);
  const later = post(el('span', { 'aria-label': 'Ad' }));
  f.timeline.append(later.cell); f.mutate();
  assert.deepEqual(marked(promoted, later), [false, false]);
  f.run();
  assert.deepEqual(marked(promoted, later), [true, true]);
  assert.notEqual(f.observer, active);
  assert.ok(f.logs.every((line) => /^\[bowser\] x promoted posts hidden: \d+ of \d+$/.test(line)));
});

test('style removal alone stops and reveals cells without another body mutation', () => {
  const promoted = post(badge('Ad'));
  const f = fixture(promoted);
  f.root.children = f.root.children.filter((node) => node !== f.style);
  f.mutate();
  assert.equal(f.context.__bowserXPromoted, null);
  assert.deepEqual(marked(promoted), [false]);
});
