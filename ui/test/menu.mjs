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

test('a submenu slides the parent aside with one transform, ease-out, and snaps under reduced motion', () => {
  const css = readFileSync(join(here, '..', 'src', 'styles', 'menu.css'), 'utf8').replace(/\r\n/g, '\n');
  const tokens = readFileSync(join(here, '..', 'src', 'styles', 'tokens.css'), 'utf8');
  const slide = /\.html-menu-slide \{([^}]*)\}/.exec(css)?.[1] ?? '';
  const host = /\.html-menu-pane\.is-host \{([^}]*)\}/.exec(css)?.[1] ?? '';
  const child = /\.html-menu-pane\.is-child \{([^}]*)\}/.exec(css)?.[1] ?? '';
  // Only the parent card travels, and only by a transform. The child fades and nudges in place.
  assert.ok(!/transform:/.test(slide), 'the strip itself does not travel');
  assert.match(host, /transform: translateX\(var\(--menu-slide, 0px\)\);/);
  assert.match(host, /transition: transform var\(--bw-dur-base\) var\(--bw-ease-out\);/);
  assert.match(child, /opacity: var\(--menu-child-in, 1\);/);
  assert.match(child, /transform: translateX\(var\(--menu-child-x, 0px\)\);/);
  assert.ok(!/transition:[^;]*(width|left|margin|padding|height)/.test(host));
  assert.ok(!/transition:[^;]*(width|left|margin|padding|height)/.test(child));
  assert.match(css, /\.html-menu-slide\.is-snap \.html-menu-pane \{\s*transition: none;\s*\}/);
  // --bw-dur-base is the ~160ms token (167ms) and is zeroed under reduced motion, where base.css also
  // keeps only opacity transitions, so the parent snaps with no travel.
  assert.match(tokens, /--bw-dur-base: 167ms;/);
  assert.match(tokens, /@media \(prefers-reduced-motion: reduce\)[^]*?--bw-dur-base: 0ms;/);
  assert.match(tokens, /--bw-ease-out: cubic-bezier\(0, 0, 0, 1\);/);
  // The parent stays on screen. The child is its own card, not a stretched column of the parent:
  // panes start at the top of their own content, and the child is offset to the row that opened it.
  assert.match(slide, /align-items: flex-start;/);
  assert.match(css, /\.html-menu-pane \{[^}]*border-radius: var\(--bw-r-overlay\);/);
  assert.match(css, /#app\.html-menu\.is-paned \{[^}]*background: transparent;/);
  assert.match(css, /\.html-menu-pane \+ \.html-menu-pane \{[^}]*margin-top: var\(--menu-child-y, 0px\);/);
  assert.ok(!css.includes('border-inline-start: 1px solid var(--bw-hairline)'), 'the child is not a hairline inside the parent');
  assert.match(css, /\.html-menu-row\.selected:not\(:disabled\),\s*\.html-menu-row\.is-open \{/);
  assert.match(css, /\.html-menu-row\[aria-haspopup="menu"\]\.is-open \.html-menu-trail \{\s*transform: translateX\(2px\);/);
  // No back row and no page-replacing drill animation are left.
  assert.ok(!css.includes('.html-menu-back'));
  assert.ok(!/drill/.test(css));
});

test('the menu view keeps the parent mounted and drives the slide through one custom property', () => {
  const view = readFileSync(join(here, '..', 'src', 'menu-view.ts'), 'utf8');
  assert.ok(!/html-menu-back|chevron-left|drill/.test(view), 'no back row, no replacing page');
  assert.match(view, /root\.className = 'html-menu is-paned bw-menu-surface'/);
  assert.match(view, /style\.setProperty\('--menu-slide'/);
  assert.match(view, /--menu-child-in/);
  assert.match(view, /--menu-child-x/);
  assert.match(view, /--menu-child-y/);
  assert.match(view, /tagPanes/);
  assert.match(view, /flyoutCards/);
  assert.ok(!view.includes('watchMotion'), 'the clip is not updated every frame');
  assert.match(view, /activateAt\(nav, level, index\)/, 'the parent page stays clickable');
  assert.ok(!/\.style\.(left|right|width|top)\s*=/.test(view), 'the slide moves no layout property');
  assert.ok(!/scrollIntoView/.test(view), 'scrolling is done on the list, never on the clip');
});

test('the track is painted as one card while the cards move, so no bare pixel shows white', () => {
  const css = readFileSync(join(here, '..', 'src', 'styles', 'menu.css'), 'utf8');
  const view = readFileSync(join(here, '..', 'src', 'menu-view.ts'), 'utf8');
  const backing = /\.html-menu-track\.is-moving \{([^}]*)\}/.exec(css)?.[1] ?? '';
  assert.match(backing, /background: linear-gradient\(var\(--bw-overlay\), var\(--bw-overlay\)\), var\(--bw-window-opaque\);/);
  assert.match(backing, /border-radius: var\(--bw-r-overlay\);/);
  // On for the whole open and close motion, and off only after the resting clip is reported (or the
  // motion is settled), a few frames later so the clip cuts the window before the backing goes.
  assert.equal((view.match(/backingOn\(\);/g) ?? []).length, 2, 'open and close');
  assert.match(view, /publish\(tok, size\.width, size\.height, true\);\s*backingOff\(\);/);
  assert.match(view, /measure\(false\);\s*backingOff\(\);/);
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
