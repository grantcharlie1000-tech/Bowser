// Toolbars and feeds (thread T13): the pure rules of the three bars' button lists (src/bars-logic.ts) against the
// Rust authority (src/toolbar_prefs.rs), the wire messages that carry them, and the feed options' choices.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import * as esbuild from 'esbuild';

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, '..', '..');
const srcDir = join(here, '..', 'src');
const read = (path) => readFileSync(path, 'utf8').replace(/\r\n/g, '\n');

async function bundle(file) {
  const out = esbuild.buildSync({ entryPoints: [join(srcDir, file)], bundle: true, format: 'esm', platform: 'neutral', write: false, logLevel: 'silent' });
  return import(`data:text/javascript;base64,${Buffer.from(out.outputFiles[0].contents).toString('base64')}`);
}
const bars = await bundle('bars-logic.ts');
const feedsLogic = await bundle('feeds-settings-logic.ts');
const { parseInbound } = await bundle('protocol.ts');

const rust = read(join(root, 'src', 'toolbar_prefs.rs'));
const rustList = (name) => [...new RegExp(`pub const ${name}: \\[&str; \\d+\\] =\\s*\\[([^\\]]*)\\]`, 's').exec(rust)[1].matchAll(/"([^"]+)"/g)].map((m) => m[1]);

test('the lists, defaults and limits match toolbar_prefs.rs', () => {
  assert.deepEqual([...bars.ADDRESS_ITEMS], rustList('ADDRESS_ITEMS'));
  assert.deepEqual([...bars.DEFAULT_ADDRESS], rustList('DEFAULT_ADDRESS'));
  assert.deepEqual([...bars.TAB_ITEMS], rustList('TAB_ITEMS'));
  assert.deepEqual([...bars.PANEL_ITEMS], rustList('PANEL_ITEMS'));
  assert.equal(bars.MAX_ITEMS, Number(/pub const MAX_ITEMS: usize = (\d+);/.exec(rust)[1]));
  assert.equal(bars.MAX_COMMAND_ITEMS, Number(/pub const MAX_COMMAND_ITEMS: usize = (\d+);/.exec(rust)[1]));
  assert.equal(bars.MAX_RIGHT_STATIC, Number(/pub const MAX_RIGHT_STATIC: usize = (\d+);/.exec(rust)[1]));
  assert.deepEqual(bars.BAR_KEYS, { address: 'toolbar_buttons', tabs: 'toolbar_tabs', panel: 'toolbar_panel' });
  for (const [bar, key] of Object.entries(bars.BAR_KEYS)) assert.match(rust, new RegExp(`Self::${bar[0].toUpperCase()}${bar.slice(1)} => "${key}"`), bar);
  // The slot of one always-there button: the extensions button's, 32 px.
  assert.equal(bars.SLOT_PX, Number(/pub const EXT_BUTTON_SLOT: i32 = (\d+);/.exec(read(join(root, 'src', 'chrome_host', 'mod.rs')))[1]));
  // Today's address bar is the default: bookmark and new tab are opt-in.
  assert.ok(!bars.DEFAULT_ADDRESS.includes('bookmark') && !bars.DEFAULT_ADDRESS.includes('newtab'));
});

test('the refresh choices of the Feeds panel match feeds::REFRESH_CHOICES', () => {
  const feeds = read(join(root, 'src', 'feeds', 'mod.rs'));
  const rustChoices = /REFRESH_CHOICES: \[u32; \d+\] = \[([^\]]*)\]/.exec(feeds)[1].split(',').map((n) => Number(n.trim()));
  assert.deepEqual([...feedsLogic.REFRESH_CHOICES], rustChoices);
  assert.equal(feedsLogic.refreshLabel(0), 'Never');
  assert.equal(feedsLogic.refreshLabel(60), '1 hour');
  assert.equal(feedsLogic.refreshLabel(15), '15 min');
  assert.equal(feedsLogic.refreshLabel(720), '12 hours');
  assert.equal(feedsLogic.feedAssignment('feeds_detect', false), 'feeds_detect=false');
  assert.equal(feedsLogic.feedAssignment('feeds_refresh_minutes', 30), 'feeds_refresh_minutes=30');
  for (const bad of [7, -1, 1.5, '30', null, true]) assert.equal(feedsLogic.feedAssignment('feeds_refresh_minutes', bad), null, String(bad));
  assert.equal(feedsLogic.feedAssignment('feeds_detect', 1), null);
});

test('a list is valid only with a bar’s own buttons or command buttons, no repeats, within the limits', () => {
  assert.equal(bars.validItems('address', ['back', 'reload']), true);
  assert.equal(bars.validItems('address', []), true);
  assert.equal(bars.validItems('tabs', ['closed-tabs', 'cmd:new-tab', 'chain:2']), true);
  assert.equal(bars.validItems('panel', ['feeds', 'cmd:history']), true);
  for (const [bar, items] of [
    ['address', ['back', 'back']], ['address', ['menu']], ['address', ['feeds']], ['tabs', ['back']], ['panel', ['closed-tabs']], ['address', ['cmd:']],
    ['address', ['cmd:UPPER']], ['address', ['chain:0']], ['address', ['chain:01']], ['address', ['chain:12345']], ['address', ['cmd:' + 'a'.repeat(33)]],
    ['tabs', ['cmd:a', 'cmd:b', 'cmd:c', 'cmd:d', 'cmd:e']],
    ['address', ['bookmark', 'newtab', 'cmd:a', 'cmd:b', 'chain:1', 'chain:2']],
  ]) assert.equal(bars.validItems(bar, items), false, `${bar} ${items}`);
  assert.equal(bars.validItems('address', ['bookmark', 'newtab', 'cmd:a', 'cmd:b', 'chain:1']), true, 'five always-there buttons are allowed');
  assert.equal(bars.validItems('address', ['bookmark', 'newtab', 'cmd:a', 'cmd:b', 'downloads', 'extensions']), true, 'downloads and extensions take room only when shown');
  assert.equal(bars.validItems('address', Array.from({ length: bars.MAX_ITEMS + 1 }, (_, i) => `cmd:x${i}`)), false);
});

test('buttons sit in a zone, and the editor moves them only among their zone’s', () => {
  assert.deepEqual(['back', 'forward', 'reload'].map(bars.zoneOf), ['left', 'left', 'left']);
  assert.deepEqual(['site-info', 'split', 'reader'].map(bars.zoneOf), ['pill', 'pill', 'pill']);
  assert.deepEqual(['bookmark', 'newtab', 'downloads', 'extensions', 'cmd:x', 'chain:1'].map(bars.zoneOf), Array(6).fill('right'));
  assert.deepEqual(bars.addressZones(['bookmark', 'back', 'split', 'reload']), { left: ['back', 'reload'], pill: ['split'], right: ['bookmark'] });
  assert.equal(bars.rightStatic(['back', 'bookmark', 'downloads', 'cmd:x']), 2);
  const list = ['back', 'forward', 'site-info', 'split', 'bookmark', 'cmd:x'];
  assert.deepEqual(bars.moveItem('address', list, 'forward', -1), ['forward', 'back', 'site-info', 'split', 'bookmark', 'cmd:x']);
  assert.equal(bars.moveItem('address', list, 'forward', 1), null, 'forward is the last before the address field');
  assert.equal(bars.moveItem('address', list, 'site-info', -1), null, 'it does not cross into the left zone');
  assert.deepEqual(bars.moveItem('address', list, 'bookmark', 1), ['back', 'forward', 'site-info', 'split', 'cmd:x', 'bookmark']);
  assert.deepEqual(bars.moveItem('tabs', ['closed-tabs', 'cmd:x'], 'cmd:x', -1), ['cmd:x', 'closed-tabs'], 'on the other bars every button is a peer');
  assert.equal(bars.moveItem('tabs', ['closed-tabs'], 'closed-tabs', 1), null);
  assert.equal(bars.moveItem('tabs', ['closed-tabs'], 'nope', 1), null);
  assert.equal(bars.canMove('panel', ['feeds', 'cmd:x'], 'feeds', 1), true);
});

test('adding puts a button at the end of its zone, and says why when it cannot', () => {
  assert.deepEqual(bars.addItem('address', ['back', 'site-info', 'bookmark'], 'reload'), ['back', 'reload', 'site-info', 'bookmark']);
  assert.deepEqual(bars.addItem('address', ['back', 'site-info', 'bookmark'], 'split'), ['back', 'site-info', 'split', 'bookmark']);
  assert.deepEqual(bars.addItem('address', ['back', 'site-info'], 'newtab'), ['back', 'site-info', 'newtab']);
  assert.deepEqual(bars.addItem('tabs', [], 'closed-tabs'), ['closed-tabs']);
  assert.deepEqual(bars.addItem('panel', ['feeds'], 'cmd:history'), ['feeds', 'cmd:history']);
  assert.equal(bars.addItem('address', ['back'], 'back'), null);
  assert.match(bars.whyNotAdd('address', ['back'], 'back'), /already/);
  assert.match(bars.whyNotAdd('tabs', ['cmd:a', 'cmd:b', 'cmd:c', 'cmd:d'], 'cmd:e'), /at most 4 command buttons/);
  assert.match(bars.whyNotAdd('address', ['bookmark', 'newtab', 'cmd:a', 'cmd:b', 'chain:1'], 'chain:2'), /at most 4 command buttons|at most 5 buttons/);
  assert.match(bars.whyNotAdd('address', ['bookmark', 'newtab', 'cmd:a', 'cmd:b'], 'newtab'), /already/);
  assert.equal(bars.whyNotAdd('address', ['bookmark', 'newtab', 'cmd:a', 'cmd:b'], 'downloads'), null);
  assert.deepEqual(bars.removeItem(['a', 'b', 'c'], 'b'), ['a', 'c']);
  assert.deepEqual(bars.addable('address', ['back', 'forward']), ['reload', 'site-info', 'split', 'reader', 'bookmark', 'newtab', 'downloads', 'extensions']);
  assert.equal(bars.itemsText(['a', 'b']), 'a,b');
  assert.deepEqual(bars.itemsOf(''), []);
  assert.deepEqual(bars.itemsOf('a,b'), ['a', 'b']);
  assert.equal(bars.isDefault('address', [...bars.DEFAULT_ADDRESS]), true);
  assert.equal(bars.isDefault('address', ['back']), false);
  assert.equal(bars.isDefault('panel', []), true);
});

test('every item has a name, a note slot and a glyph that is in the icon family', () => {
  const gen = read(join(srcDir, 'icons.gen.ts'));
  const family = new Set([...gen.match(/ICON_NAMES[^=]*=\s*\[([^\]]*)\]/s)[1].matchAll(/"([^"]+)"/g)].map((m) => m[1]));
  for (const id of [...bars.ADDRESS_ITEMS, ...bars.TAB_ITEMS, ...bars.PANEL_ITEMS]) {
    assert.ok(bars.ITEM_LABELS[id]?.label, `${id} has a name`);
    assert.ok(family.has(bars.ITEM_ICONS[id]), `${id}: ${bars.ITEM_ICONS[id]} is in the icon family`);
  }
  assert.equal(bars.itemLabel('cmd:new-tab', new Map([['cmd:new-tab', 'New tab']])), 'New tab');
  assert.equal(bars.itemLabel('chain:3', new Map()), 'Chain 3');
  assert.equal(bars.itemLabel('cmd:close-private-tabs', new Map()), 'close private tabs');
});

// ---- the wire ----

const stateBase = { t: 'state', scale: 1, solid: false, dark: false, back: false, forward: false, loading: false, progress: 0, security: 'secure', label: 'Secure', theme: {} };
const parse = (extra) => parseInbound(JSON.stringify({ ...stateBase, ...extra }));

test('the state message carries the address bar’s buttons, all four fields or none', () => {
  const ok = parse({ items: 'back,reload,cmd:new-tab', tools: [{ k: 'cmd:new-tab', l: 'New tab', i: 2 }], extra: 32, feeds: 1 });
  assert.deepEqual([ok.items, ok.tools, ok.extra, ok.feeds], ['back,reload,cmd:new-tab', [{ k: 'cmd:new-tab', l: 'New tab', i: 2 }], 32, 1]);
  const older = parse({});
  assert.equal(older.items, undefined);
  assert.equal(older.feeds, undefined);
  for (const bad of [
    { items: 'a' }, { items: 'a', tools: [], extra: 0 }, { items: 'a', tools: [], feeds: 0 }, { items: 1, tools: [], extra: 0, feeds: 0 },
    { items: 'a', tools: 'x', extra: 0, feeds: 0 }, { items: 'a', tools: [{ k: 'Bad Id', l: 'x', i: 0 }], extra: 0, feeds: 0 },
    { items: 'a', tools: [{ k: 'cmd:a', l: 'x', i: 14 }], extra: 0, feeds: 0 }, { items: 'a', tools: [{ k: 'cmd:a', l: 'x', i: 0, z: 1 }], extra: 0, feeds: 0 },
    { items: 'a', tools: Array(5).fill({ k: 'cmd:a', l: 'x', i: 0 }), extra: 0, feeds: 0 }, { items: 'a', tools: [], extra: 401, feeds: 0 },
    { items: 'a', tools: [], extra: 0, feeds: 10 }, { items: 'x'.repeat(513), tools: [], extra: 0, feeds: 0 },
  ]) assert.equal(parse(bad), null, JSON.stringify(bad));
});

test('the Rust state fixture still parses, and a tool command is validated before it is sent', () => {
  const fixture = JSON.parse(read(join(here, 'fixtures', 'tab-options-messages.json')));
  const opts = parseInbound(JSON.stringify(fixture.opts));
  assert.equal(opts.closedButton, true);
  assert.deepEqual(opts.tools, [{ k: 'cmd:new-tab', l: 'New tab', i: 1 }]);
  const defaults = parseInbound(JSON.stringify(fixture.defaults));
  assert.equal(defaults.closedButton, true);
  assert.deepEqual(defaults.tools, []);
  const sent = [];
  globalThis.window = { chrome: { webview: { postMessage: (wire) => sent.push(JSON.parse(wire)), addEventListener() {} } } };
  return bundle('protocol.ts').then(({ sendTool }) => {
    assert.equal(sendTool('address', 'cmd:new-tab', 2), true);
    assert.equal(sendTool('tabs', 'closed-tabs', 0), true);
    for (const [bar, item, index] of [['address', 'BAD', 0], ['address', 'cmd:a', -1], ['address', 'cmd:a', 14], ['address', '', 0], ['address', 'cmd:a', 1.5]]) {
      assert.equal(sendTool(bar, item, index), false, `${item} ${index}`);
    }
    assert.deepEqual(sent, [{ v: 1, t: 'tool', b: 'address', k: 'cmd:new-tab', i: 2 }, { v: 1, t: 'tool', b: 'tabs', k: 'closed-tabs', i: 0 }]);
    delete globalThis.window;
  });
});

test('the rail knows the panel bar’s extra buttons: key grammar and the feed glyph', async () => {
  const rail = await bundle('rail-logic.ts');
  for (const key of ['tool:0', 'tool:7', 'tool:13']) assert.equal(rail.isRailKey(key), true, key);
  for (const key of ['tool:', 'tool:100', 'tool:x', 'tool:-1', 'tools:1']) assert.equal(rail.isRailKey(key), false, key);
  assert.equal(rail.sectionOf('tool:2'), 'bottom');
  assert.equal(rail.isDraggable('tool:2'), false, 'the editor orders them, not a drag');
  assert.equal(rail.isRailIcon('feed'), true);
  const rustProtocol = read(join(root, 'src', 'chrome_host', 'protocol.rs'));
  assert.match(rustProtocol, /strip_prefix\("tool:"\)/);
});
