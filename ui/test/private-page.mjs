// Private tab landing and the private mark (docs/design/private-tabs.md): the title, the mark's
// animated parts against the generated icon data, the motion rules (finite, transform/opacity only,
// off under reduced motion), the private tokens' contrast, and the landing's DOM in a fake document.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import * as esbuild from 'esbuild';

const here = dirname(fileURLToPath(import.meta.url));
const srcDir = join(here, '..', 'src');
const css = (name) => readFileSync(join(srcDir, 'styles', name), 'utf8').replace(/\r\n/g, '\n');

class FakeClassList {
  constructor(owner) { this.owner = owner; }
  get set() { return new Set((this.owner.attrs.get('class') ?? '').split(/\s+/).filter(Boolean)); }
  write(set) { this.owner.attrs.set('class', [...set].join(' ')); }
  add(...names) { const s = this.set; for (const n of names) s.add(n); this.write(s); }
  remove(...names) { const s = this.set; for (const n of names) s.delete(n); this.write(s); }
  contains(name) { return this.set.has(name); }
  toggle(name, force) { const on = force ?? !this.contains(name); if (on) this.add(name); else this.remove(name); return on; }
}

class FakeNode {
  constructor(tag) {
    this.tagName = tag;
    this.attrs = new Map();
    this.children = [];
    this.parent = null;
    this.listeners = {};
    this.classList = new FakeClassList(this);
    this.textContent = '';
    this.type = '';
    this.id = '';
    const props = new Map();
    this.style = { setProperty: (k, v) => props.set(k, v), getPropertyValue: (k) => props.get(k) ?? '' };
  }
  set className(value) { this.attrs.set('class', value); }
  get className() { return this.attrs.get('class') ?? ''; }
  setAttribute(k, v) { this.attrs.set(k, String(v)); }
  getAttribute(k) { return this.attrs.has(k) ? this.attrs.get(k) : null; }
  addEventListener(type, cb) { (this.listeners[type] ??= []).push(cb); }
  click() { for (const cb of this.listeners.click ?? []) cb({}); }
  get offsetWidth() { return 0; }
  append(...nodes) {
    for (const node of nodes) {
      if (node.parent) node.parent.children = node.parent.children.filter((c) => c !== node);
      node.parent = this;
      this.children.push(node);
    }
  }
  replaceChildren(...nodes) { this.children = []; this.append(...nodes); }
  cloneNode() { const copy = new FakeNode(this.tagName); for (const [k, v] of this.attrs) copy.attrs.set(k, v); return copy; }
  all() { return this.children.flatMap((c) => [c, ...(c.all ? c.all() : [])]); }
  find(cls) { return this.all().find((n) => n.classList && n.classList.contains(cls)) ?? null; }
  text() { return [this.textContent, ...this.children.map((c) => (c.text ? c.text() : c.textContent))].join(''); }
}

globalThis.document = {
  createElement: (tag) => new FakeNode(tag),
  createElementNS: (_ns, tag) => new FakeNode(tag),
  createTextNode: (text) => ({ textContent: text, text: () => text }),
};

const out = esbuild.buildSync({
  stdin: {
    contents: [
      "export * from './private-logic';",
      "export { buildMark, buildPrivatePage } from './private-page';",
      "export { ICON_SHAPES } from './icons.gen';",
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

test('the mark parts the landing animates match the generated private icon', () => {
  const shapes = ui.ICON_SHAPES.private;
  assert.equal(shapes.length, ui.MARK_SHAPE_COUNT);
  const all = [...ui.MARK_PARTS.hat, ...ui.MARK_PARTS.shades].sort();
  assert.deepEqual(all, [0, 1, 2, 3, 4], 'every shape is in exactly one group');
  for (const i of ui.MARK_PARTS.lenses) {
    assert.ok(ui.MARK_PARTS.shades.includes(i));
    assert.equal(shapes[i][1].fill, 'currentColor', `lens ${i} is filled (the glint is clipped to it)`);
  }
  for (const i of ui.MARK_PARTS.hat) assert.equal(shapes[i][1].fill, undefined, `hat shape ${i} is a stroke`);
  // The chip and the strip address the shades as :nth-child(n + 3): the shades are the last shapes.
  const first = Math.min(...ui.MARK_PARTS.shades) + 1;
  assert.deepEqual(ui.MARK_PARTS.shades, [first - 1, first, first + 1]);
  assert.match(css('private.css'), new RegExp(`\\.tb-private > svg > :nth-child\\(n \\+ ${first}\\)`));
  assert.match(css('private.css'), new RegExp(`\\.priv-glyph svg > :nth-child\\(n \\+ ${first}\\)`));
  // The glint is two strokes (thick, then thin: private.css styles :first-child and :last-child).
  assert.equal(ui.ICON_SHAPES.glint.length, 2);
  // The favicon badge is the shades alone: two filled lenses and a bridge.
  assert.equal(ui.ICON_SHAPES.glasses.filter(([, a]) => a.fill === 'currentColor').length, 2);
});

test('the landing mark groups the bowler and the shades and clips the glint to the lenses', () => {
  const svg = ui.buildMark(80);
  const [defs, hat, shades] = svg.children;
  assert.equal(defs.tagName, 'defs');
  assert.equal(hat.getAttribute('class'), 'pm-hat');
  assert.equal(shades.getAttribute('class'), 'pm-shades');
  assert.equal(hat.children.length, 2);
  const clip = defs.children[0];
  assert.equal(clip.tagName, 'clipPath');
  assert.equal(clip.children.length, 2);
  const glintClip = shades.children.at(-1);
  assert.equal(glintClip.getAttribute('clip-path'), `url(#${clip.id})`);
  assert.equal(glintClip.children[0].getAttribute('class'), 'pm-glint');
  assert.equal(glintClip.children[0].children.length, 2);
  assert.equal(svg.getAttribute('aria-hidden'), 'true');
});

test('the landing is the mark and the title, centred, and nothing else', () => {
  const page = ui.buildPrivatePage();
  const text = page.root.text().replace(/\s+/g, ' ').trim();
  assert.equal(text, ui.PRIVATE_PAGE_TITLE);
  assert.doesNotMatch(text, /\b(anonymous|anonymously|untraceable|invisible|hides? your (ip|identity)|no one can see)\b/i);
  assert.equal(page.root.getAttribute('aria-labelledby'), 'pm-title');
  const title = page.root.find('pm-title');
  assert.equal(title.tagName, 'h1');
  assert.equal(title.id, 'pm-title');
  assert.equal(title.textContent, ui.PRIVATE_PAGE_TITLE);
  const mark = page.root.find('pm-mark');
  assert.equal(mark.getAttribute('width'), '80');
  assert.equal(mark.style.getPropertyValue('--bw-icon-size'), '80px');
  assert.equal(page.root.find('pm-halo').getAttribute('aria-hidden'), 'true');
  assert.equal(page.root.all().some((n) => n.tagName === 'button'), false);
  for (const gone of ['pm-eyebrow', 'pm-lead', 'pm-cards', 'pm-card', 'pm-note', 'pm-footer', 'pm-count', 'pm-close', 'pm-column']) {
    assert.equal(page.root.find(gone), null, gone);
  }
  const sheet = css('private.css');
  assert.match(sheet, /\.pm-page \{[^}]*align-items: center;[^}]*justify-content: center;/);
  for (const gone of ['.pm-eyebrow', '.pm-lead', '.pm-cards', '.pm-card', '.pm-note', '.pm-footer', '.pm-count', '.pm-close', '.pm-column']) {
    assert.equal(sheet.includes(gone), false, gone);
  }
});

test('the entrance plays once per tab, and a re-shown landing comes back settled', () => {
  const page = ui.buildPrivatePage();
  page.show({ t: 'privatepage', id: 3, n: 1 });
  assert.ok(page.root.classList.contains('is-entering'), 'first show plays');
  page.show({ t: 'privatepage', id: 3, n: 2 });
  assert.ok(page.root.classList.contains('is-entering'), 'a count update leaves the entrance running');
  page.hidden();
  page.show({ t: 'privatepage', id: 3, n: 2 });
  assert.ok(!page.root.classList.contains('is-entering'), 'same tab again: settled');
  page.hidden();
  page.show({ t: 'privatepage', id: 5, n: 2 });
  assert.ok(page.root.classList.contains('is-entering'), 'another blank private tab plays again');
});

test('private motion is finite, transform/opacity only, and off under reduced motion', () => {
  const text = css('private.css');
  assert.doesNotMatch(text, /infinite/);
  for (const m of text.matchAll(/@keyframes ([\w-]+) \{([\s\S]*?)\n\}/g)) {
    for (const prop of m[2].matchAll(/^\s*([a-z-]+):/gm)) assert.ok(['opacity', 'transform'].includes(prop[1]), `${m[1]}: ${prop[1]}`);
  }
  const reduce = text.slice(text.indexOf('@media (prefers-reduced-motion: reduce)'));
  assert.ok(reduce.length > 0);
  for (const sel of ['.tb-private', '.pm-hat', '.pm-shades', '.pm-glint', '.priv-glyph svg > :nth-child(n + 3)']) assert.ok(reduce.includes(sel), sel);
  assert.match(reduce, /animation: none;/);
  // The idle glint runs once more and stops; its fill mode lets neither run hide the other.
  assert.match(text, /pm-glint 900ms var\(--bw-ease-move\) 760ms,\n\s*pm-glint 900ms var\(--bw-ease-move\) 7200ms;/);
});

test('the private violet is text-grade on both schemes', () => {
  const tokens = css('tokens.css');
  const dark = /--bw-private: (#[0-9a-f]{6});/.exec(tokens)[1];
  const light = /\[data-scheme="light"\] \{[\s\S]*?--bw-private: (#[0-9a-f]{6});/.exec(tokens)[1];
  const lum = (hex) => {
    const c = [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16) / 255).map((v) => (v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4));
    return 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
  };
  const ratio = (a, b) => (Math.max(lum(a), lum(b)) + 0.05) / (Math.min(lum(a), lum(b)) + 0.05);
  assert.ok(ratio(dark, '#20242c') >= 4.5, `dark ${dark}`);
  assert.ok(ratio(light, '#f4f6f8') >= 4.5, `light ${light}`);
  // Normal tabs and the normal pill never use it.
  for (const name of ['tabs.css', 'sidebar.css', 'toolbar.css']) {
    for (const m of css(name).matchAll(/([^{}]*)\{([^}]*--bw-private[^}]*)\}/g)) {
      assert.match(m[1], /data-private|tb-private/, `${name}: ${m[1].trim()}`);
    }
  }
});
