import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import * as esbuild from 'esbuild';

const here = dirname(fileURLToPath(import.meta.url));
const source = readFileSync(join(here, '..', 'src', 'menu-logic.ts'), 'utf8');
const { code } = esbuild.transformSync(source, { loader: 'ts', format: 'esm' });
const logic = await import(`data:text/javascript;base64,${Buffer.from(code).toString('base64')}`);

test('menu navigation wraps and skips disabled rows and separators', () => {
  const entries = [{ id: 1, label: 'A', enabled: true, checked: false }, { separator: true }, { id: 2, label: 'B', enabled: false, checked: false }, { id: 3, label: 'C', enabled: true, checked: false }];
  assert.equal(logic.stepMenu(entries, 0, 1), 3);
  assert.equal(logic.stepMenu(entries, 3, 1), 0);
  assert.equal(logic.stepMenu(entries, 0, -1), 3);
  assert.equal(logic.stepMenu(entries, -1, -1), 3);
});

test('Home and End find the first and last enabled item', () => {
  const entries = [{ separator: true }, { id: 1, label: 'A', enabled: false, checked: false }, { id: 2, label: 'B', enabled: true, checked: false }];
  assert.equal(logic.edgeMenu(entries, false), 2);
  assert.equal(logic.edgeMenu(entries, true), 2);
  assert.equal(logic.edgeMenu([{ separator: true }], true), -1);
});

test('menu payload caps nested entries and validates selectable row fields', () => {
  const msg = { v: 1, t: 'menu', tok: 4, kind: 'sample', entries: [{ id: 1, label: 'One', enabled: true, checked: false }] };
  assert.equal(logic.validMenuPayload(msg), true);
  assert.equal(logic.validMenuPayload({ ...msg, entries: Array.from({ length: 129 }, (_, id) => ({ id, label: 'x', enabled: true, checked: false })) }), false);
  assert.equal(logic.validMenuPayload({ ...msg, entries: [{ ...msg.entries[0], label: 'x'.repeat(129) }] }), false);
  assert.equal(logic.validMenuPayload({ ...msg, entries: [{ ...msg.entries[0], enabled: 'yes' }] }), false);
});

const item = (id, label, extra = {}) => ({ id, label, enabled: true, checked: false, ...extra });

test('arrow keys, Home and End move the selection on the current page', () => {
  const nav = logic.createNav([item(1, 'Alpha'), { separator: true }, item(2, 'Beta', { enabled: false }), item(3, 'Gamma')]);
  assert.equal(logic.currentPage(nav).selected, 0);
  assert.deepEqual(logic.menuKey(nav, 'ArrowDown', {}, 0), { kind: 'select' });
  assert.equal(logic.currentPage(nav).selected, 3);
  assert.deepEqual(logic.menuKey(nav, 'ArrowDown', {}, 0), { kind: 'select' });
  assert.equal(logic.currentPage(nav).selected, 0);
  logic.menuKey(nav, 'End', {}, 0);
  assert.equal(logic.currentPage(nav).selected, 3);
  assert.deepEqual(logic.menuKey(nav, 'End', {}, 0), { kind: 'none' });
  logic.menuKey(nav, 'Home', {}, 0);
  assert.equal(logic.currentPage(nav).selected, 0);
  logic.menuKey(nav, 'Tab', { shift: true }, 0);
  assert.equal(logic.currentPage(nav).selected, 3);
});

test('Enter and Space pick commands; Escape dismisses', () => {
  const nav = logic.createNav([item(7, 'Open'), item(8, 'Close')]);
  assert.deepEqual(logic.menuKey(nav, 'Enter', {}, 0), { kind: 'pick', id: 7 });
  logic.menuKey(nav, 'ArrowDown', {}, 0);
  assert.deepEqual(logic.menuKey(nav, ' ', {}, 0), { kind: 'pick', id: 8 });
  assert.deepEqual(logic.menuKey(nav, 'Escape', {}, 0), { kind: 'dismiss' });
  assert.deepEqual(logic.menuKey(logic.createNav([item(1, 'Off', { enabled: false })]), 'Enter', {}, 0), { kind: 'none' });
});

test('submenus open beside the parent with Right/Enter and close with Left/Backspace', () => {
  const nav = logic.createNav([item(1, 'Plain'), item(2, 'More', { submenu: [item(3, 'Nested'), item(4, 'Other')] })]);
  assert.deepEqual(logic.menuKey(nav, 'ArrowRight', {}, 0), { kind: 'none' });
  logic.menuKey(nav, 'ArrowDown', {}, 0);
  assert.deepEqual(logic.menuKey(nav, 'ArrowRight', {}, 0), { kind: 'page' });
  assert.equal(nav.pages.length, 2);
  assert.equal(logic.currentPage(nav).label, 'More');
  assert.equal(logic.currentPage(nav).selected, 0);
  logic.menuKey(nav, 'ArrowDown', {}, 0);
  assert.deepEqual(logic.menuKey(nav, 'Enter', {}, 0), { kind: 'pick', id: 4 });
  assert.deepEqual(logic.menuKey(nav, 'ArrowLeft', {}, 0), { kind: 'page' });
  assert.equal(nav.pages.length, 1);
  assert.equal(logic.currentPage(nav).selected, 1, 'the parent row stays selected');
  assert.deepEqual(logic.menuKey(nav, 'ArrowLeft', {}, 0), { kind: 'none' });
  assert.deepEqual(logic.menuKey(nav, 'Enter', {}, 0), { kind: 'page' });
  assert.deepEqual(logic.menuKey(nav, 'Backspace', {}, 0), { kind: 'page' });
});

test('at most the open page and its parent are on screen', () => {
  const deep = item(9, 'Deep', { submenu: [item(10, 'Leaf')] });
  const nav = logic.createNav([item(1, 'Top', { submenu: [item(2, 'Middle', { submenu: [deep] })] })]);
  assert.equal(logic.firstVisible(nav), 0);
  logic.menuKey(nav, 'Enter', {}, 0);
  assert.equal(nav.pages.length, 2);
  assert.equal(logic.firstVisible(nav), 0, 'parent and submenu are both on screen');
  logic.menuKey(nav, 'Enter', {}, 0);
  logic.menuKey(nav, 'Enter', {}, 0);
  assert.equal(nav.pages.length, 4);
  assert.equal(logic.firstVisible(nav), 2, 'older pages slide out of view');
});

test('a row of the parent page works while its submenu is open', () => {
  const nav = logic.createNav([
    item(1, 'Plain'),
    item(2, 'First', { submenu: [item(3, 'One')] }),
    item(4, 'Second', { submenu: [item(5, 'Two')] }),
  ]);
  logic.menuKey(nav, 'ArrowDown', {}, 0);
  assert.deepEqual(logic.menuKey(nav, 'Enter', {}, 0), { kind: 'page' });
  assert.equal(nav.pages.length, 2);
  // The row whose submenu is open stays marked on the parent page.
  assert.equal(nav.pages[0].selected, 1);
  // A plain command on the parent is picked and leaves the stack alone.
  assert.deepEqual(logic.activateAt(nav, 0, 0), { kind: 'pick', id: 1 });
  assert.equal(nav.pages.length, 2);
  // Another submenu row swaps the child; the open row only closes it.
  assert.deepEqual(logic.activateAt(nav, 0, 2), { kind: 'page' });
  assert.equal(nav.pages.length, 2);
  assert.equal(logic.currentPage(nav).label, 'Second');
  assert.deepEqual(logic.activateAt(nav, 0, 2), { kind: 'page' });
  assert.equal(nav.pages.length, 1);
  assert.equal(logic.currentPage(nav).selected, 2, 'the parent row stays selected');
  // On the open page it is the ordinary activation; stale levels and disabled rows do nothing.
  assert.deepEqual(logic.activateAt(nav, 0, 1), { kind: 'page' });
  assert.deepEqual(logic.activateAt(nav, 1, 0), { kind: 'pick', id: 3 });
  assert.deepEqual(logic.activateAt(nav, 5, 0), { kind: 'none' });
  assert.deepEqual(logic.activateAt(nav, 0, 9), { kind: 'none' });
});

test('typeahead matches prefixes, cycles on a repeated letter and resets after a pause', () => {
  const nav = logic.createNav([item(1, 'New tab'), item(2, 'Notes'), item(3, 'Settings'), item(4, 'Saved sessions'), item(5, 'Sleep', { enabled: false })]);
  assert.deepEqual(logic.menuKey(nav, 's', {}, 0), { kind: 'select' });
  assert.equal(logic.currentPage(nav).selected, 2);
  logic.menuKey(nav, 's', {}, 100);
  assert.equal(logic.currentPage(nav).selected, 3, 'same letter cycles');
  logic.menuKey(nav, 's', {}, 200);
  assert.equal(logic.currentPage(nav).selected, 2, 'disabled rows are skipped and the cycle wraps');
  logic.menuKey(nav, 'n', {}, 2000);
  assert.equal(logic.currentPage(nav).selected, 0);
  logic.menuKey(nav, 'o', {}, 2100);
  assert.equal(logic.currentPage(nav).selected, 1, 'two letters refine to "no"');
  assert.deepEqual(logic.menuKey(nav, 'x', { ctrl: true }, 2200), { kind: 'none' });
  assert.equal(logic.typeahead([item(1, '  Padded')], -1, 'pa', false), 0);
  assert.equal(logic.typeahead([], -1, 'a', false), -1);
});

test('payload accepts the main menu header and footer and rejects bad ones', () => {
  const msg = { v: 1, t: 'menu', tok: 4, kind: 'main', header: { name: 'Default', caption: 'Current profile', avatar: 'D', accent: '#3366ff' }, footer: 'Bowser 1.0', entries: [item(1, 'One')] };
  assert.equal(logic.validMenuPayload(msg), true);
  assert.equal(logic.validMenuPayload({ ...msg, header: { ...msg.header, accent: 'red' } }), false);
  assert.equal(logic.validMenuPayload({ ...msg, header: { ...msg.header, extra: 1 } }), false);
  assert.equal(logic.validMenuPayload({ ...msg, footer: 'x'.repeat(257) }), false);
  assert.equal(logic.validMenuPayload({ ...msg, entries: [item(1, 'x', { icon: 'javascript' })] }), false);
});

const repo = join(here, '..', '..');

test('MENU_ICONS is exactly the icon family and every Rust glyph is in it', () => {
  const gen = readFileSync(join(here, '..', 'src', 'icons.gen.ts'), 'utf8');
  const family = [...gen.match(/ICON_NAMES[^=]*=\s*\[([^\]]*)\]/s)[1].matchAll(/"([^"]+)"/g)].map(m => m[1]).sort();
  assert.deepEqual([...logic.MENU_ICONS].sort(), family);
  for (const file of ['window.rs', 'site_info.rs']) {
    const rust = readFileSync(join(repo, 'src', 'win32', file), 'utf8');
    for (const m of rust.matchAll(/\.glyph\("([^"]+)"\)/g)) assert.ok(family.includes(m[1]), `${file}: ${m[1]}`);
    for (const m of rust.matchAll(/=> "(workspace-[a-z]+)"/g)) assert.ok(family.includes(m[1]), `${file}: ${m[1]}`);
  }
});

test('column helpers reserve the icon and trailing columns only when a page needs them', () => {
  assert.equal(logic.hasLeadColumn([item(1, 'A'), item(2, 'B')]), false);
  assert.equal(logic.hasLeadColumn([item(1, 'A'), item(2, 'B', { icon: 'plus' })]), true);
  assert.equal(logic.hasLeadColumn([item(1, 'A', { badge: { avatar: 'D', accent: '#3366ff' } })]), true);
  assert.equal(logic.hasTrailColumn([item(1, 'A')]), false);
  assert.equal(logic.hasTrailColumn([item(1, 'A', { checked: true })]), true);
  assert.equal(logic.hasTrailColumn([item(1, 'A', { submenu: [item(2, 'B')] })]), true);
  assert.equal(logic.footerVisible('main'), true);
  assert.equal(logic.footerVisible('tab:12'), false);
});

test('avatar text colour is the better of black and white on the accent', () => {
  assert.equal(logic.onFill('#ffffff'), '#000000');
  assert.equal(logic.onFill('#2b5fa8'), '#ffffff');
  assert.equal(logic.onFill('#d49a32'), '#000000');
});

test('payload accepts favicons by index and badges, and rejects bad ones', () => {
  const png = 'data:image/png;base64,iVBORw0KGgo=';
  const msg = { v: 1, t: 'menu', tok: 1, kind: 'open-tabs', favs: [png], entries: [item(1, 'Tab', { fav: 0, icon: 'workspace-globe' }), item(2, 'Swatch', { badge: { avatar: '', accent: '#4385d1' } })] };
  assert.equal(logic.validMenuPayload(msg), true);
  assert.equal(logic.validMenuPayload({ ...msg, entries: [item(1, 'Tab', { fav: 1 })] }), false, 'index out of range');
  assert.equal(logic.validMenuPayload({ ...msg, favs: ['https://example.com/a.png'] }), false, 'only PNG data URIs');
  assert.equal(logic.validMenuPayload({ ...msg, favs: [png.replace('png', 'svg+xml')] }), false);
  assert.equal(logic.validMenuPayload({ ...msg, favs: Array(49).fill(png) }), false);
  assert.equal(logic.validMenuPayload({ ...msg, entries: [item(2, 'S', { badge: { avatar: 'D', accent: 'red' } })] }), false);
});

test('menu rows press to 0.98 and submenu chevrons ease, transform only and snapped under reduced motion', () => {
  const css = readFileSync(join(here, '..', 'src', 'styles', 'menu.css'), 'utf8');
  const collapse = (s) => s.replace(/\s+/g, ' ');
  // Row press: a transform transition in the 80-120ms band (--bw-dur-fast is 83ms) and a 0.98 dip.
  assert.match(collapse(css), /\.html-menu-row \{ transition: transform var\(--bw-dur-fast\)/);
  assert.match(collapse(css), /\.html-menu-row:active:not\(:disabled\) \{ transform: scale\(0\.98\); \}/);
  // Submenu chevron: a transform nudge on the selected row, never a width animation.
  assert.match(collapse(css), /\.html-menu-row\[aria-haspopup="menu"\]\.selected:not\(:disabled\) \.html-menu-trail \{ transform: translateX\(2px\); \}/);
  assert.ok(!/transition:[^;]*width/.test(css), 'no layout width is animated');
  // Reduced motion: the press snaps (transition dropped) and the chevron hint is dropped entirely.
  const flat = collapse(css);
  assert.ok(
    flat.includes('@media (prefers-reduced-motion: reduce) { .html-menu-row { transition: none; } }'),
    'press snaps under reduced motion',
  );
  assert.ok(
    flat.includes('@media (prefers-reduced-motion: reduce) { .html-menu-row[aria-haspopup="menu"].selected:not(:disabled) .html-menu-trail { transform: none; } }'),
    'chevron dropped under reduced motion',
  );
});

test('New private window has its own glyph, and its shades settle once on the selected row only', () => {
  const rust = readFileSync(join(repo, 'src', 'win32', 'window.rs'), 'utf8');
  assert.match(rust, /"New private window", Icon::Shield\)\s*\.glyph\("private-window"\)/);
  // The other private rows keep their glyphs.
  assert.match(rust, /"Close all private tabs", Icon::Close\)\s*\.glyph\("glasses"\)/);
  assert.match(rust, /"New private tab", Icon::Shield\)\s*\.glyph\("private"\)/);
  // The CSS moves shapes 3-5: they must be the two filled lenses and the bridge, after frame and bar.
  const gen = readFileSync(join(here, '..', 'src', 'icons.gen.ts'), 'utf8');
  const shapes = JSON.parse(gen.match(/^\s*"private-window": (\[.*\]),$/m)[1]);
  assert.deepEqual(shapes.map(([tag, attrs]) => `${tag}${attrs.fill ? ':' + attrs.fill : ''}`),
    ['rect', 'path', 'path:currentColor', 'path:currentColor', 'path']);
  const css = readFileSync(join(here, '..', 'src', 'styles', 'menu.css'), 'utf8').replace(/\s+/g, ' ');
  const rule = '.html-menu-row[data-icon="private-window"].selected:not(:disabled) .html-menu-lead svg > :nth-child(n + 3) { animation: html-menu-shades-settle 240ms linear; }';
  assert.ok(css.includes(`@media (prefers-reduced-motion: no-preference) { ${rule} }`), 'only under no-preference, only while selected');
  assert.equal(css.split('html-menu-shades-settle').length - 1, 2, 'one use and one definition');
  const frames = css.match(/@keyframes html-menu-shades-settle \{(.*?\}) \}/)[1];
  for (const decl of frames.matchAll(/([a-z-]+):/g)) assert.ok(['transform', 'animation-timing-function'].includes(decl[1]), decl[1]);
  assert.match(frames, /^ 0% \{ transform: translateY\(0\);/, 'starts on the resting glyph');
  assert.match(frames, /100% \{ transform: translateY\(0\); \}$/, 'ends on the resting glyph');
  assert.ok(!/infinite/.test(frames));
});

test('a submenu card slides out from behind the parent, which never moves: one transform, ease-out, snapped under reduced motion', () => {
  const css = readFileSync(join(here, '..', 'src', 'styles', 'menu.css'), 'utf8').replace(/\r\n/g, '\n');
  const tokens = readFileSync(join(here, '..', 'src', 'styles', 'tokens.css'), 'utf8');
  const rule = (selector) => new RegExp(`(?:^|\\n)${selector.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')} \\{([^}]*)\\}`).exec(css)?.[1] ?? '';
  const child = rule('.html-menu-pane.is-child');
  const column = rule('.html-menu-flyout');
  const track = rule('.html-menu-track');
  const pane = rule('.html-menu-pane');
  // Only the child card travels, by one transform on the 167ms ease-out token; it goes back with ease-in.
  assert.match(child, /transform: translateX\(var\(--menu-child-x, 0px\)\);/);
  assert.match(child, /transition: transform var\(--bw-dur-base\) var\(--bw-ease-out\);/);
  assert.ok(!/opacity/.test(child), 'no fade: a half-transparent pixel shows white on Windows 10');
  assert.match(rule('.html-menu-flyout.is-leaving > .html-menu-pane.is-child'), /transition-timing-function: var\(--bw-ease-in\);/);
  assert.match(css, /\.html-menu-flyout\.is-snap > \.html-menu-pane \{\s*transition: none;\s*\}/);
  // The parent card, its box and the page never move: no transform or transition on them, and the
  // old parent slide (--menu-slide, the strip, the opaque backing) is gone.
  for (const body of [track, pane, rule('.html-menu-pane.is-host'), rule('#app.html-menu.is-paned'), rule('#app.html-menu.is-paned.has-flyouts')]) {
    assert.ok(!/transform|transition/.test(body), body);
  }
  assert.ok(!/--menu-slide|html-menu-slide|is-moving|--menu-child-in/.test(css));
  assert.ok(!/transition:[^;]*(width|left|right|margin|padding|height|top)/.test(css), 'no layout property is animated');
  // The child's column hangs off the parent's side with the gap, dropped to the row that opened it,
  // and does not size the page; it clips the card only while it moves, so the card shows only past
  // the parent's edge. On the left it mirrors; pulled over the parent only by --menu-child-overlap.
  assert.match(track, /position: relative;/);
  assert.match(column, /position: absolute;/);
  assert.match(column, /top: var\(--menu-child-y, 0px\);/);
  assert.match(column, /left: calc\(100% \+ var\(--bw-space-1\) - var\(--menu-child-overlap, 0px\)\);/);
  assert.match(column, /max-height: calc\(100vh - var\(--menu-child-y, 0px\)\);/);
  assert.match(rule('.html-menu-flyout.is-left'), /right: calc\(100% \+ var\(--bw-space-1\) - var\(--menu-child-overlap, 0px\)\);\s*left: auto;/);
  assert.match(rule('.html-menu-flyout.is-sliding'), /overflow: hidden;/);
  assert.ok(!/overflow/.test(column), 'the resting column does not clip the card');
  // The room kept on both sides fits the widest card plus the gap.
  const reserve = rule('#app.html-menu.is-paned.has-flyouts');
  assert.match(reserve, /--menu-reserve: calc\(350px \+ var\(--bw-space-1\)\);/);
  assert.match(reserve, /padding-inline: var\(--menu-reserve\);/);
  assert.match(reserve, /height: 100vh;/);
  assert.match(pane, /max-width: 350px;/);
  // Reduced motion: no slide.
  assert.match(css, /@media \(prefers-reduced-motion: reduce\) \{\s*\.html-menu-pane\.is-child \{\s*transition: none;\s*\}\s*\}/);
  assert.match(tokens, /--bw-dur-base: 167ms;/);
  assert.match(tokens, /@media \(prefers-reduced-motion: reduce\)[^]*?--bw-dur-base: 0ms;/);
  assert.match(tokens, /--bw-ease-out: cubic-bezier\(0, 0, 0, 1\);/);
  assert.match(tokens, /--bw-ease-in: cubic-bezier\(0\.4, 0, 1, 1\);/);
  // Each card hugs its own rows: transparent surface, rounded cards, the open row keeps its tint.
  assert.match(css, /\.html-menu-pane \{[^}]*border-radius: var\(--bw-r-overlay\);/);
  assert.match(css, /#app\.html-menu\.is-paned \{[^}]*background: transparent;/);
  assert.ok(!css.includes('border-inline-start: 1px solid var(--bw-hairline)'), 'the child is not a hairline inside the parent');
  assert.match(css, /\.html-menu-row\.selected:not\(:disabled\),\s*\.html-menu-row\.is-open \{/);
  assert.match(css, /\.html-menu-row\[aria-haspopup="menu"\]\.is-open \.html-menu-trail \{\s*transform: translateX\(2px\);/);
  assert.ok(!css.includes('.html-menu-back'));
  assert.ok(!/drill/.test(css));
});

test('a child card opens right when it fits, left when only the left has room, else over the parent edge', () => {
  // 280 px card + 4 px gap.
  assert.deepEqual(logic.flyoutSide({ l: 900, r: 284 }, 284), { side: 'right', overlap: 0 });
  assert.deepEqual(logic.flyoutSide({ l: 900, r: 283 }, 284), { side: 'left', overlap: 0 }, 'the main menu at the right edge');
  assert.deepEqual(logic.flyoutSide({ l: 0, r: 2000 }, 284), { side: 'right', overlap: 0 });
  assert.deepEqual(logic.flyoutSide(null, 284), { side: 'right', overlap: 0 }, 'no room reported yet');
  assert.deepEqual(logic.flyoutSide({ l: 200, r: 250 }, 284), { side: 'right', overlap: 34 });
  assert.deepEqual(logic.flyoutSide({ l: 250, r: 200 }, 284), { side: 'left', overlap: 34 });
  assert.deepEqual(logic.flyoutSide({ l: 0, r: 0 }, 284), { side: 'right', overlap: 284 });
  // Rust's room message.
  assert.equal(logic.validRoom({ v: 1, t: 'room', tok: 3, l: 880, r: 120 }), true);
  assert.equal(logic.validRoom({ v: 1, t: 'room', tok: 3, l: -1, r: 120 }), false);
  assert.equal(logic.validRoom({ v: 1, t: 'room', tok: 3, l: 1.5, r: 120 }), false);
  assert.equal(logic.validRoom({ v: 1, t: 'room', tok: 3, l: 1, r: 4001 }), false);
  assert.equal(logic.validRoom({ v: 1, t: 'room', tok: 3, l: 1, r: 1, x: 0 }), false);
  assert.equal(logic.validRoom({ v: 1, t: 'menu', tok: 3, l: 1, r: 1 }), false);
  // Only a menu with a submenu row keeps room beside it.
  const item = (id, extra = {}) => ({ id, label: `R${id}`, enabled: true, checked: false, ...extra });
  assert.equal(logic.hasFlyouts([item(1), { separator: true }, item(2, { submenu: [item(3)] })]), true);
  assert.equal(logic.hasFlyouts([item(1), { section: 'S' }, item(2)]), false);
});

test('the window clip only ever holds what the sliding child card paints', () => {
  const out = 'cubic-bezier(0, 0, 0, 1)';
  const back = 'cubic-bezier(0.4, 0, 1, 1)';
  // The timing functions match CSS: ends pinned, ease-out front-loaded, ease-in back-loaded, keywords.
  assert.equal(logic.easeAt(out, 0), 0);
  assert.equal(logic.easeAt(out, 1), 1);
  assert.equal(logic.easeAt(out, -0.2), 0);
  assert.equal(logic.easeAt(out, Number.POSITIVE_INFINITY), 1);
  assert.ok(Math.abs(logic.easeAt(out, 0.1) - 0.446) < 0.01, 'x = s^3, y = 3s^2 - 2s^3');
  assert.ok(logic.easeAt(back, 0.5) < 0.5);
  assert.ok(Math.abs(logic.easeAt('linear', 0.3) - 0.3) < 1e-9);
  assert.ok(Math.abs(logic.easeAt('ease-out', 0.5) - logic.easeAt('cubic-bezier(0, 0, 0.58, 1)', 0.5)) < 1e-9);
  // Shift: the whole travel behind the parent's edge at the start of a slide out, 0 at rest.
  assert.equal(logic.slideShift(284, 167, out, 0, true), 284);
  assert.equal(logic.slideShift(284, 167, out, 167, true), 0);
  assert.equal(logic.slideShift(284, 167, back, 0, false), 0);
  assert.equal(logic.slideShift(284, 167, back, 400, false), 284);
  assert.equal(logic.slideShift(284, 0, out, 0, true), 0, 'reduced motion: in place at once');
  // The part that shows starts at the parent's side of the column: from its left edge on the right,
  // up to its right edge on the left. Rounded inward; nothing while the card is behind the parent.
  assert.deepEqual(logic.childSlice(0, 280, 0, 'right'), { x: 0, w: 280 });
  assert.deepEqual(logic.childSlice(0, 280, 100, 'right'), { x: 0, w: 180 });
  assert.deepEqual(logic.childSlice(0, 280, 100, 'left'), { x: 100, w: 180 });
  assert.deepEqual(logic.childSlice(649.6, 258.6, 0, 'right'), { x: 650, w: 258 }, 'a column on a half pixel');
  assert.deepEqual(logic.innerPixels(354.4, 0, 292.3, 462.7), { x: 355, y: 0, w: 291, h: 462 });
  assert.deepEqual(logic.childSlice(10.5, 280.6, 10.2, 'left'), { x: 21, w: 270 });
  assert.equal(logic.childSlice(0, 280, 279.5, 'right'), null);
  assert.equal(logic.childSlice(0, 280, 284, 'left'), null);
  // The safety property. Coming out the clip uses the shift of 25ms earlier (SLIDE_LAG_MS), going back
  // that of 50ms later (SLIDE_LEAD_MS). Whether the card on screen is from 25ms before the clip's frame
  // (the clip landed early) or from 50ms after it (the clip landed late), the clip is never wider than
  // the card: it can trim the card, never hold a pixel the card has not painted.
  const width = (timing, at, entering) => logic.childSlice(0, 280, logic.slideShift(284, 167, timing, at, entering), 'right')?.w ?? 0;
  for (const [entering, timing, bias] of [[true, out, -25], [false, back, 50]]) {
    for (let t = 0; t <= 260; t += 1) {
      const clip = width(timing, t + bias, entering);
      for (let skew = -25; skew <= 50; skew += 1) {
        const card = width(timing, t + skew, entering);
        assert.ok(clip <= card, `${entering ? 'out' : 'back'} t=${t} skew=${skew}: clip ${clip} > card ${card}`);
      }
    }
  }
  // Without the bias the same skew would hold bare pixels: the bias is what makes it safe.
  assert.ok(width(out, 10, true) > width(out, 10 - 25, true));
  assert.ok(width(back, 100, false) > width(back, 100 + 50, false));
});

test('the menu view moves only the child card and the clip, never the window or the parent', () => {
  const view = readFileSync(join(here, '..', 'src', 'menu-view.ts'), 'utf8');
  assert.ok(!/html-menu-back|chevron-left|drill/.test(view), 'no back row, no replacing page');
  assert.match(view, /root\.className = 'html-menu is-paned bw-menu-surface';\s*root\.classList\.toggle\('has-flyouts', hasFlyouts\(raw\.entries\)\);/);
  // One custom property drives the child card; the parent is never given a transform.
  assert.match(view, /setProperty\('--menu-child-x'/);
  assert.match(view, /--menu-child-y/);
  assert.match(view, /--menu-child-overlap/);
  assert.ok(!/--menu-slide|--menu-child-in|is-moving|backingOn|RESIZE_WAIT_MS|awaitWidth|keep_right_edge/.test(view));
  assert.ok(!/host[^;\n]*\.style\.(transform|setProperty)/.test(view), 'the parent card is never moved');
  // The size never changes with a submenu: one natural measurement, reported with every clip.
  assert.equal((view.match(/measureNatural\(\);/g) ?? []).length, 2, 'when a menu is shown and when its parent changes');
  assert.match(view, /flyoutSide\(room, need\)/);
  assert.match(view, /const bias = entering \? -SLIDE_LAG_MS : SLIDE_LEAD_MS;/);
  assert.match(view, /if \(entering\) requestAnimationFrame\(frame\);\s*else frame\(\);/, 'clip after the card coming out, before it going back');
  // A card that leaves is cut out of the clip before it stops being painted.
  assert.match(view, /publish\(null\);\s*retire\(card\.wrap\);/);
  assert.match(view, /slide\(card, false, \(\) => retire\(card\.wrap\)\)/);
  assert.match(view, /const KEEP_FRAMES = 3;/);
  assert.match(view, /const SLIDE_LAG_MS = 25;/);
  assert.match(view, /const SLIDE_LEAD_MS = 50;/);
  assert.match(view, /activateAt\(nav, level, index\)/, 'the parent page stays clickable');
  assert.ok(!/\.style\.(left|right|width|top)\s*=/.test(view), 'the slide moves no layout property');
  assert.ok(!/scrollIntoView/.test(view), 'scrolling is done on the list, never on the clip');
  // The popup routes Rust's room to the menu view.
  const popup = readFileSync(join(here, '..', 'src', 'popup.ts'), 'utf8');
  assert.match(popup, /case 'room': menu\.room\(value\); break;/);
});

test('disabled rows keep >= 3:1 contrast on the solid dark and light overlays', () => {
  const css = readFileSync(join(here, '..', 'src', 'styles', 'menu.css'), 'utf8');
  const pct = Number(/color-mix\(in srgb, var\(--bw-text-2\) (\d+)%, transparent\)/.exec(css)[1]) / 100;
  const mix = (fg, bg) => fg.map((c, i) => c * pct + bg[i] * (1 - pct));
  const lum = (rgb) => { const f = (v) => { const c = v / 255; return c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4; }; return 0.2126 * f(rgb[0]) + 0.7152 * f(rgb[1]) + 0.0722 * f(rgb[2]); };
  const ratio = (a, b) => { const [hi, lo] = [lum(a), lum(b)].sort((x, y) => y - x); return (hi + 0.05) / (lo + 0.05); };
  // --bw-text-2 over the overlay: dark placeholder #dde2e9 on rgb(44,44,44); light #4b5159 on #f4f6f8.
  assert.ok(ratio(mix([0xdd, 0xe2, 0xe9], [44, 44, 44]), [44, 44, 44]) >= 3);
  assert.ok(ratio(mix([0x4b, 0x51, 0x59], [0xf4, 0xf6, 0xf8]), [0xf4, 0xf6, 0xf8]) >= 3);
});
