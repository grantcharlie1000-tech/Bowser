// Downloads (docs/design/downloads.md): the wire contracts (entry, bubble page, page snapshot,
// toolbar button, the JS -> Rust commands) against the fixture, the strict parsers, Chrome-style
// words (bytes, speed, time left, status per state), file kinds and names, row actions, the ring
// math, the patch plan and send gate, and the motion and token rules of downloads.css/toolbar.css.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import * as esbuild from 'esbuild';

const here = dirname(fileURLToPath(import.meta.url));
const srcDir = join(here, '..', 'src');
const read = (path) => readFileSync(path, 'utf8').replace(/\r\n/g, '\n');
async function bundle(file) {
  const out = esbuild.buildSync({ entryPoints: [join(srcDir, file)], bundle: true, format: 'esm', platform: 'neutral', write: false, logLevel: 'silent' });
  return import(`data:text/javascript;base64,${Buffer.from(out.outputFiles[0].contents).toString('base64')}`);
}
const logic = await bundle('downloads-logic.ts');
const protocol = await bundle('protocol.ts');
// The wire as Rust writes it (src/downloads/tests.rs keeps this file current) ...
const rust = JSON.parse(read(join(here, 'fixtures', 'downloads-messages.json')));
// ... and the UI's own richer scenarios (every state at once) for the strictness and DOM tests.
const fixture = JSON.parse(read(join(here, 'fixtures', 'downloads-scenarios.json')));
const MB = 1048576;

// ---- wire ----

test('every message Rust writes passes the bridge and its strict parser', () => {
  const kinds = { dlbtn: 0, downloads: 0, snap: 0 };
  for (const [name, message] of Object.entries(rust)) {
    kinds[message.t] += 1;
    if (message.t === 'dlbtn') {
      const { v: _v, ...rest } = message;
      assert.deepEqual(protocol.parseInbound(JSON.stringify(message)), rest, name);
    } else if (message.t === 'downloads') {
      assert.ok(logic.parseBubble(JSON.parse(JSON.stringify(message))), name);
    } else {
      const inbound = protocol.parseInbound(JSON.stringify(message));
      assert.equal(inbound?.p, 'downloadspage', name);
      assert.ok(logic.parseDownloadsSnapshot(inbound.d), name);
    }
  }
  assert.ok(kinds.dlbtn >= 5 && kinds.downloads >= 2 && kinds.snap >= 5, JSON.stringify(kinds));
  const page = logic.parseDownloadsSnapshot(rust.page.d);
  const states = new Set(page.days.flatMap((d) => d.entries.map((e) => e.state)));
  for (const state of logic.DL_STATES) assert.ok(states.has(state), `Rust's page fixture shows ${state}`);
  assert.ok(page.days.flatMap((d) => d.entries).some((e) => e.name.includes('<b>')), 'names arrive as text');
});

test('every scenario message passes its strict parser', () => {
  for (const name of Object.keys(fixture).filter((k) => k.startsWith('dlbtn'))) {
    const msg = protocol.parseInbound(JSON.stringify(fixture[name]));
    assert.equal(msg?.t, 'dlbtn', name);
    const { v: _v, ...rest } = fixture[name];
    assert.deepEqual(msg, rest, name);
  }
  for (const name of ['bubble', 'bubbleAuto', 'bubbleEmpty']) assert.ok(logic.parseBubble(fixture[name]), name);
  const bubble = logic.parseBubble(fixture.bubble);
  assert.deepEqual(bubble.items.map((e) => e.state), ['active', 'active', 'paused', 'done', 'failed']);
  assert.equal(bubble.more, 3);
  assert.equal(bubble.items[3].name, 'Report "<b>" v3.pdf', 'text arrives as text');
  for (const name of ['snap', 'search', 'none', 'empty', 'error', 'guest', 'removed', 'cleared']) {
    const inbound = protocol.parseInbound(JSON.stringify(fixture[name]));
    assert.equal(inbound?.t, 'snap', name);
    assert.equal(inbound.p, 'downloadspage', name);
    assert.ok(logic.parseDownloadsSnapshot(inbound.d), name);
  }
  const snap = logic.parseDownloadsSnapshot(fixture.snap.d);
  assert.deepEqual(snap.days.map((d) => [d.label, d.date, d.count, d.entries.length]), [['Today', '2026-10-14', 7, 7], ['2026-10-09', '2026-10-09', 1, 1]]);
  assert.equal(snap.active, 3);
  assert.equal(logic.parseDownloadsSnapshot(fixture.guest.d).state, 'guest');
  assert.equal(logic.parseDownloadsSnapshot(fixture.guest.d).days.length, 1, 'a guest window lists its downloads (memory only)');
  assert.equal(logic.parseDownloadsSnapshot(fixture.cleared.d).ack, 'cleared');
});

test('the entry object is exact: every key, nothing more, closed values', () => {
  const good = fixture.bubble.items[0];
  assert.ok(logic.parseEntry(good));
  const bad = [
    { ...good, extra: 1 },
    { ...good, id: 0 },
    { ...good, id: 2 ** 32 },
    { ...good, id: 1.5 },
    { ...good, state: 'downloading' },
    { ...good, name: '' },
    { ...good, name: '   ' },
    { ...good, name: 'a/b.zip' },
    { ...good, name: 'a\\b.zip' },
    { ...good, name: 'a\nb.zip' },
    { ...good, name: 'x'.repeat(401) },
    { ...good, host: 'h'.repeat(241) },
    { ...good, url: 'javascript:alert(1)' },
    { ...good, url: 'file:///C:/x.zip' },
    { ...good, got: -1 },
    { ...good, total: -2 },
    { ...good, eta: -2 },
    { ...good, bps: -1 },
    { ...good, got: '12' },
    { ...good, resume: 'yes' },
    { ...good, clock: '24:00' },
    { ...good, clock: '9:05' },
    { ...good, err: 'Network error' }, // only a failed entry has a reason
    { ...good, state: 'done', bps: 5 }, // only an active entry moves bytes
    { ...good, err: 'x'.repeat(121), state: 'failed', bps: 0 },
  ];
  for (const value of bad) assert.equal(logic.parseEntry(value), null, JSON.stringify(value).slice(0, 140));
  for (const key of logic.ENTRY_KEYS) {
    const missing = { ...good };
    delete missing[key];
    assert.equal(logic.parseEntry(missing), null, `missing ${key}`);
  }
  assert.ok(logic.parseEntry({ ...good, url: '' }), 'no address to show is allowed');
  assert.ok(logic.parseEntry({ ...good, name: '.bashrc' }));
});

test('bubble and page snapshots refuse partial, extended or inconsistent messages', () => {
  const b = fixture.bubble;
  for (const value of [
    { ...b, extra: 1 }, { ...b, v: 2 }, { ...b, t: 'download' }, { ...b, tok: -1 }, { ...b, auto: 1 }, { ...b, more: -1 }, { ...b, active: 1.5 },
    { ...b, items: [...b.items, fixture.bubbleAuto.items[0]] }, // six rows, and a duplicate id
    { ...b, items: [b.items[0], b.items[0]] },
    { ...b, items: [{ ...b.items[0], state: 'nope' }] },
  ]) assert.equal(logic.parseBubble(value), null, JSON.stringify(value).slice(0, 100));
  const { more: _m, ...noMore } = b;
  assert.equal(logic.parseBubble(noMore), null);

  const d = fixture.snap.d;
  const [day, entry] = d.rows;
  for (const value of [
    { ...d, extra: 1 },
    { ...d, state: 'off' },
    { ...d, ack: 'saved' },
    { ...d, more: 0 },
    { ...d, undo: 'no' },
    { ...d, active: -1 },
    { ...d, query: 'x'.repeat(513) },
    { ...d, rows: [entry, ...d.rows] }, // an entry before any day
    { ...d, rows: [{ ...day, count: 8 }, ...d.rows.slice(1)] },
    { ...d, rows: [{ ...day, date: '2026-13-01' }, ...d.rows.slice(1)] },
    { ...d, rows: [day, entry, entry, ...d.rows.slice(3)] },
    { ...fixture.error.d, rows: d.rows },
    { ...fixture.error.d, undo: true },
  ]) assert.equal(logic.parseDownloadsSnapshot(value), null, JSON.stringify(value).slice(0, 120));
  assert.equal(protocol.parseInbound({ v: 1, t: 'snap', p: 'downloadpage', d }), null, 'only the known page token');
});

test('dlbtn is strict like every other host message', () => {
  const ok = fixture.dlbtnActive;
  for (const value of [
    { ...ok, extra: 1 }, { ...ok, v: 2 }, { ...ok, on: 'true' }, { ...ok, state: 'busy' }, { ...ok, n: -1 }, { ...ok, pct: 101 },
    { ...ok, pct: -2 }, { ...ok, pct: 4.5 }, { ...ok, burst: -1 }, { ...ok, burst: 2 ** 32 },
  ]) assert.equal(protocol.parseInbound(value), null, JSON.stringify(value));
  for (const key of ['v', 'on', 'state', 'n', 'pct', 'burst']) {
    const missing = { ...ok };
    delete missing[key];
    assert.equal(protocol.parseInbound(missing), null, `missing ${key}`);
  }
});

test('commands: dl toggle, dlact, dpage and set stay within six keys and their ranges', () => {
  const sent = [];
  globalThis.window = { chrome: { webview: { postMessage: (wire) => sent.push(wire), addEventListener() {} } } };
  try {
    assert.equal(protocol.sendDownloadToggle({ left: 812.4, top: 1, right: 840.2, bottom: 29 }), true);
    assert.equal(protocol.sendDownloadToggle({ left: Number.NaN, top: 0, right: 1, bottom: 1 }), false);
    assert.equal(protocol.sendDownloadAct(41, 7, 0), true);
    assert.equal(protocol.sendDownloadAct(41, 0, 8), true);
    assert.equal(protocol.sendDownloadAct(41, 0, 9), true);
    assert.equal(protocol.sendDownloadAct(41, 0, 10), true);
    for (const [tok, id, n] of [[41, 0, 3], [41, 7, 8], [41, 7, 11], [41, -1, 0], [-1, 7, 0], [41, 7, 1.5], [2 ** 32, 7, 0]]) {
      assert.equal(protocol.sendDownloadAct(tok, id, n), false, `${tok} ${id} ${n}`);
    }
    for (const fields of [{ a: 'search', text: 'rust' }, { a: 'open', id: 7 }, { a: 'remove', id: 7 }, { a: 'undo' }, { a: 'clear' }, { a: 'folder' }, { a: 'refresh' }, { a: 'settings' }]) {
      assert.equal(protocol.sendPanel('dpage', fields), true, JSON.stringify(fields));
    }
    assert.equal(protocol.sendPanel('set', { a: 'downloadask', on: true }), true);
    assert.equal(protocol.sendPanel('set', { a: 'downloadfolder' }), true);
  } finally {
    delete globalThis.window;
  }
  const messages = sent.map((wire) => JSON.parse(wire));
  assert.deepEqual(messages[0], { v: 1, t: 'dl', a: 'toggle', text: '812,1,29,28' });
  assert.deepEqual(messages[1], { v: 1, t: 'dlact', tok: 41, id: 7, n: 0 });
  assert.deepEqual(messages.at(-1), { v: 1, t: 'set', a: 'downloadfolder' });
  for (const message of messages) {
    assert.ok(Object.keys(message).length <= 6, JSON.stringify(message));
    assert.ok(JSON.stringify(message).length < 16 * 1024);
  }
  assert.deepEqual([...protocol.SETTINGS_SECTIONS], ['profile', 'appearance', 'privacy', 'performance', 'search', 'keyboard', 'commands', 'downloads', 'media', 'extensions', 'updates', 'about']);
  assert.deepEqual(protocol.parseInbound({ v: 1, t: 'settingsgo', s: 'downloads' }), { t: 'settingsgo', s: 'downloads' });
});

// ---- words ----

test('bytes read like Chrome: 1024 steps, one decimal under 100, rounded down', () => {
  const cases = [
    [0, '0 B'], [1, '1 B'], [1023, '1023 B'], [1024, '1.0 KB'], [1536, '1.5 KB'], [1024 * 99.99, '99.9 KB'], [1024 * 100, '100 KB'],
    [1024 * 1023.9, '1023 KB'], [MB, '1.0 MB'], [12.34 * MB, '12.3 MB'], [12.39 * MB, '12.3 MB'], [456.7 * MB, '456 MB'], [1.2 * 1024 * MB, '1.2 GB'],
    [5.8 * 1024 * MB, '5.8 GB'], [3 * 1024 ** 4, '3.0 TB'],
  ];
  for (const [bytes, text] of cases) assert.equal(logic.formatBytes(bytes), text, String(bytes));
  assert.equal(logic.formatBytes(-1), '');
  assert.equal(logic.formatSpeed(18.2 * MB), '18.2 MB/s');
  assert.equal(logic.formatSpeed(0), '');
});

test('time left: sec, min, hr, days; unknown is blank', () => {
  const cases = [[0, '1 sec left'], [1, '1 sec left'], [14, '14 sec left'], [59, '59 sec left'], [60, '1 min left'], [89, '1 min left'], [90, '2 min left'],
    [3599, '59 min left'], [3600, '1 hr left'], [5400, '2 hr left'], [86399, '23 hr left'], [86400, '1 day left'], [3 * 86400, '3 days left']];
  for (const [secs, text] of cases) assert.equal(logic.formatEta(secs), text, String(secs));
  assert.equal(logic.formatEta(-1), '');
});

test('status lines per state, as the design doc writes them', () => {
  const at = (extra) => ({ state: 'active', got: Math.round(12.3 * MB), total: Math.round(45.6 * MB), eta: 14, bps: Math.round(2.5 * MB), err: '', ...extra });
  assert.equal(logic.statusText(at({})), '12.3 MB of 45.6 MB, 14 sec left');
  assert.equal(logic.statusText(at({}), 'page'), '2.5 MB/s • 12.3 MB of 45.6 MB, 14 sec left');
  assert.equal(logic.statusText(at({ total: -1, eta: -1 })), '12.3 MB');
  assert.equal(logic.statusText(at({ eta: -1 })), '12.3 MB of 45.6 MB');
  assert.equal(logic.statusText(at({ state: 'paused', bps: 0 })), 'Paused, 12.3 MB of 45.6 MB');
  assert.equal(logic.statusText(at({ state: 'paused', bps: 0, total: -1 })), 'Paused, 12.3 MB');
  assert.equal(logic.statusText(at({ state: 'done', got: Math.round(45.6 * MB) })), '45.6 MB • Done');
  assert.equal(logic.statusText(at({ state: 'done', got: Math.round(45.6 * MB) }), 'page'), '45.6 MB');
  assert.equal(logic.statusText(at({ state: 'done', got: 2048, total: -1 })), '2.0 KB • Done', 'unknown size: what arrived');
  assert.equal(logic.statusText(at({ state: 'failed', err: 'Disk full' })), 'Failed, Disk full');
  assert.equal(logic.statusText(at({ state: 'failed', err: '' })), 'Failed');
  assert.equal(logic.statusText(at({ state: 'cancelled' })), 'Cancelled');
  assert.equal(logic.statusText(at({ state: 'missing' })), 'File moved or deleted');
  assert.equal(logic.statusTone('failed'), 'warn');
  assert.equal(logic.statusTone('missing'), 'quiet');
  assert.equal(logic.statusTone('active'), 'normal');
  assert.equal(logic.fraction(at({})), Math.round(12.3 * MB) / Math.round(45.6 * MB));
  assert.equal(logic.fraction(at({ total: -1 })), null, 'indeterminate');
  assert.equal(logic.fraction(at({ total: 0, got: 0 })), 1);
  assert.equal(logic.fraction(at({ state: 'done' })), null);
  assert.equal(logic.fraction(at({ got: 99 * MB })), 1, 'clamped');
});

test('file kinds come from the extension; names elide in the middle and keep the extension', () => {
  const kinds = {
    'photo.JPG': 'image', 'clip.mkv': 'video', 'song.flac': 'audio', 'node-v24.tar.xz': 'archive', 'a.7z': 'archive', 'report.pdf': 'document',
    'notes.md': 'document', 'main.rs': 'code', 'reducer.test.ts': 'code', 'BowserSetup.exe': 'program', 'pkg.msix': 'program', 'README': 'other',
    '.bashrc': 'other', 'weird.': 'other', 'x.unknownextension': 'other',
  };
  for (const [name, kind] of Object.entries(kinds)) assert.equal(logic.fileKind(name), kind, name);
  assert.deepEqual(logic.splitName('report.final.pdf'), { stem: 'report.final', ext: '.pdf' });
  assert.deepEqual(logic.splitName('node-v24.1.0-linux-x64.tar.xz'), { stem: 'node-v24.1.0-linux-x64', ext: '.tar.xz' });
  assert.deepEqual(logic.splitName('.bashrc'), { stem: '.bashrc', ext: '' });
  assert.deepEqual(logic.splitName('archive.thisisnotanextension'), { stem: 'archive.thisisnotanextension', ext: '' });
  assert.deepEqual(logic.splitName('My file.final draft'), { stem: 'My file.final draft', ext: '' });
  assert.deepEqual(logic.nameParts('short.pdf'), { head: 'short', tail: '.pdf' });
  assert.deepEqual(logic.nameParts('Quarterly report final v3.pdf'), { head: 'Quarterly report fina', tail: 'l v3.pdf' });
  const emoji = logic.nameParts('😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀.png');
  assert.equal(emoji.head + emoji.tail, '😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀.png', 'never splits a surrogate pair');
  assert.equal(Array.from(emoji.tail).length, 8);
});

test('row actions by state, and what is legal', () => {
  const row = (state, extra = {}) => ({ state, resume: false, url: 'https://x.example/f', ...extra });
  assert.deepEqual(logic.actionsFor(row('active')), { primary: ['pause', 'cancel'], secondary: ['copy'] });
  assert.deepEqual(logic.actionsFor(row('paused')), { primary: ['resume', 'cancel'], secondary: ['copy'] });
  assert.deepEqual(logic.actionsFor(row('done')), { primary: ['show'], secondary: ['copy', 'remove'] });
  assert.deepEqual(logic.actionsFor(row('failed', { resume: true })), { primary: ['resume'], secondary: ['copy', 'remove'] });
  assert.deepEqual(logic.actionsFor(row('failed')), { primary: ['retry'], secondary: ['copy', 'remove'] });
  assert.deepEqual(logic.actionsFor(row('cancelled')), { primary: ['retry'], secondary: ['copy', 'remove'] });
  assert.deepEqual(logic.actionsFor(row('missing', { url: '' })), { primary: ['remove'], secondary: [] });
  assert.deepEqual(logic.actionsFor(row('failed', { url: '' })), { primary: ['remove'], secondary: [] }, 'Retry needs an address');
  assert.deepEqual(logic.actionsFor(row('cancelled', { url: '' })), { primary: ['remove'], secondary: [] });
  assert.deepEqual(logic.actionsFor(row('failed', { url: '', resume: true })), { primary: ['resume'], secondary: ['remove'] });
  assert.equal(logic.canAct(row('done'), 'open'), true);
  assert.equal(logic.canAct(row('missing'), 'open'), false);
  assert.equal(logic.canAct(row('active'), 'remove'), false);
  assert.equal(logic.canAct(row('done', { url: '' }), 'copy'), false);
  assert.deepEqual(['active', 'paused', 'done', 'failed', 'cancelled', 'missing'].map(logic.removable), [false, false, true, true, true, true]);
  assert.deepEqual(logic.ACTION_CODE, { pause: 0, resume: 1, cancel: 2, open: 3, show: 4, remove: 5, copy: 6, retry: 7 });
  assert.deepEqual(logic.LIST_ACTION, { page: 8, folder: 9, clear: 10 });
});

test('the toolbar ring: aggregate, offset, mode, words', () => {
  const a = (got, total, state = 'active') => ({ state, got, total });
  assert.equal(logic.aggregatePct([a(50, 100), a(0, 100)]), 25);
  assert.equal(logic.aggregatePct([a(50, 100), a(10, -1)]), -1, 'any unknown size sweeps');
  assert.equal(logic.aggregatePct([a(50, 100), a(10, 10, 'done')]), 50, 'finished rows do not count');
  assert.equal(logic.aggregatePct([a(30, 100, 'paused')]), 30, 'paused rows count');
  assert.equal(logic.aggregatePct([]), -1);
  assert.equal(logic.aggregatePct([a(0, 0)]), 100);
  assert.equal(logic.aggregatePct([a(150, 100)]), 100, 'clamped');
  assert.equal(logic.ringOffset(0), 100);
  assert.equal(logic.ringOffset(45), 55);
  assert.equal(logic.ringOffset(100), 0);
  assert.equal(logic.ringOffset(140), 0);
  assert.equal(logic.ringMode({ state: 'active', pct: 10 }), 'progress');
  assert.equal(logic.ringMode({ state: 'active', pct: -1 }), 'sweep');
  assert.equal(logic.ringMode({ state: 'paused', pct: 10 }), 'paused');
  assert.equal(logic.ringMode({ state: 'done', pct: -1 }), 'none');
  assert.equal(logic.buttonLabel({ state: 'active', n: 2, pct: 45 }), 'Downloads, 2 downloads in progress, 45%');
  assert.equal(logic.buttonLabel({ state: 'active', n: 1, pct: -1 }), 'Downloads, 1 download in progress');
  assert.equal(logic.buttonLabel({ state: 'done', n: 0, pct: -1 }), 'Downloads, download finished');
  assert.equal(logic.buttonTip({ state: 'active', n: 1, pct: 45 }), 'Downloading, 45%');
  assert.equal(logic.buttonTip({ state: 'failed', n: 0, pct: -1 }), 'Download failed');
  assert.equal(logic.DL_BUTTON_SLOT, 32);
});

test('live patching: what changed, how the list moves, and the send gate', () => {
  const base = fixture.bubble.items[0];
  const tick = { ...base, got: base.got + MB, eta: 13 };
  const change = logic.entryChange(base, tick);
  assert.deepEqual(change, { name: false, state: false, status: true, bar: true, actions: false }, 'a tick touches only text and bar');
  assert.deepEqual(logic.entryChange(base, base), { name: false, state: false, status: false, bar: false, actions: false });
  const done = { ...base, state: 'done', got: base.total, eta: -1, bps: 0 };
  assert.deepEqual(logic.entryChange(base, done), { name: false, state: true, status: true, bar: true, actions: true });
  assert.ok(Object.values(logic.entryChange(undefined, base)).every(Boolean), 'a new row is drawn whole');
  assert.deepEqual(logic.diffIds([1, 2, 3], [4, 1, 3]), { added: [4], removed: [2], kept: [1, 3] });
  assert.deepEqual(logic.diffIds([], [1]), { added: [1], removed: [], kept: [] });
  let now = 1000;
  const gate = new logic.ActionGate(600, () => now);
  assert.equal(gate.take('7:pause'), true);
  assert.equal(gate.take('7:pause'), false, 'a double click sends once');
  assert.equal(gate.take('7:cancel'), true, 'another button is its own key');
  now += 601;
  assert.equal(gate.take('7:pause'), true);
  assert.equal(gate.take('7:pause'), false);
  gate.clear();
  assert.equal(gate.take('7:pause'), true, 'the next message clears it');
});

test('page words: counts, summary, empty states', () => {
  assert.equal(logic.countText(1), '1 download');
  assert.equal(logic.countText(12), '12 downloads');
  const s = (extra) => ({ state: 'on', query: '', active: 0, more: false, days: [], ...extra });
  assert.equal(logic.summaryText(s({ active: 3 }), 12), '12 downloads • 3 in progress');
  assert.equal(logic.summaryText(s({ more: true }), 500), '500 downloads shown');
  assert.equal(logic.summaryText(s({ query: ' rust ' }), 1), '1 download found for “rust”');
  assert.equal(logic.summaryText(s({ query: 'zzz' }), 0), 'No downloads match “zzz”');
  assert.equal(logic.summaryText(s({ state: 'error' }), 0), '');
  assert.equal(logic.emptyView(s({})).kind, 'empty');
  assert.equal(logic.emptyView(s({ query: 'zzz' })).title, 'No matches for “zzz”');
  assert.equal(logic.emptyView(s({ state: 'guest' })).kind, 'guest');
  assert.equal(logic.emptyView(s({ state: 'error' })).kind, 'error');
  assert.equal(logic.emptyView(s({ days: [{}] })), null);
  assert.equal(logic.moreText(0), '');
  assert.equal(logic.moreText(3), '+3 more');
  assert.equal(logic.announceText({ name: 'a.zip', state: 'done', err: '' }), 'a.zip finished downloading');
  assert.equal(logic.announceText({ name: 'a.zip', state: 'failed', err: 'Disk full' }), 'a.zip failed: Disk full');
});

// ---- styles and sources ----

test('downloads.css: owner header, tokens, transform/opacity motion, reduced motion', () => {
  const css = read(join(srcDir, 'styles', 'downloads.css'));
  assert.match(css.slice(0, 600), /Surface: [A-Z ]+.*Owner:/s);
  assert.match(read(join(srcDir, 'styles', 'app.css')), /@import "\.\/downloads\.css" layer\(surfaces\);/);
  // Transitions and keyframes move only transform and opacity (colours fade too, never geometry).
  for (const m of css.matchAll(/transition:\s*([^;]+);/g)) {
    for (const part of m[1].split(',')) {
      const prop = part.trim().split(/\s+/)[0];
      assert.ok(['transform', 'opacity', 'background-color', 'border-color', 'color'].includes(prop), `transition of ${prop}`);
    }
  }
  for (const m of css.matchAll(/@keyframes [\w-]+ \{([\s\S]*?)\n\}/g)) {
    for (const prop of m[1].matchAll(/^\s*([a-z-]+):/gm)) assert.ok(['transform', 'opacity', 'animation-timing-function'].includes(prop[1]), `keyframes animate ${prop[1]}`);
  }
  // Only the indeterminate sweep loops.
  const infinite = [...css.matchAll(/animation:\s*([\w-]+)[^;]*infinite/g)].map((m) => m[1]);
  assert.deepEqual(infinite, ['dl-sweep']);
  assert.match(css, /@media \(prefers-reduced-motion: reduce\)/);
  assert.doesNotMatch(css, /var\(--(bg|fill|border|focus-border|text|placeholder|progress)\)/);
});

test('toolbar.css: the button keeps transform/opacity motion, with the one documented dashoffset exception', () => {
  const css = read(join(srcDir, 'styles', 'toolbar.css'));
  const block = css.slice(css.indexOf('/* ---- Download button'), css.indexOf('/* ---- Progress:'));
  assert.ok(block.length > 1000);
  assert.match(block, /Exception, documented in the design doc: the arc uses `stroke-dashoffset`/);
  for (const m of block.matchAll(/transition:\s*([^;]+);/g)) {
    for (const part of m[1].split(',')) {
      const prop = part.trim().split(/\s+/)[0];
      assert.ok(['transform', 'opacity', 'stroke', 'stroke-dashoffset'].includes(prop), `transition of ${prop}`);
    }
  }
  for (const m of block.matchAll(/@keyframes [\w-]+ \{([\s\S]*?)\n\}/g)) {
    for (const prop of m[1].matchAll(/^\s*([a-z-]+):/gm)) assert.ok(['transform', 'opacity', 'animation-timing-function'].includes(prop[1]), `keyframes animate ${prop[1]}`);
  }
  const infinite = [...block.matchAll(/animation:\s*([\w-]+)[^;]*infinite/g)].map((m) => m[1]);
  assert.deepEqual(infinite, ['tb-dl-spin'], 'only the sweep loops, and only while data-ring is sweep');
  assert.match(block, /\.tb-dl\[data-ring="sweep"\] \.tb-dl-spin \{\s*animation: tb-dl-spin/);
  assert.match(block, /#app\.placed\.has-dl \.actions \{\s*left: calc\(var\(--menu-x\) - 32px\);/, 'DL_BUTTON_SLOT');
  assert.match(block, /@media \(prefers-reduced-motion: reduce\)/);
});

test('sources: text only, no links or navigation, and each surface sends only its own commands', () => {
  const files = ['downloads-logic.ts', 'downloads-view.ts', 'downloads-bubble.ts', 'downloads-page.ts'];
  for (const file of files) {
    const text = read(join(srcDir, file));
    for (const banned of [/innerHTML|insertAdjacentHTML|outerHTML/, /location\.(hash|href|assign|replace)/, /history\.(pushState|replaceState)/, /window\.open/, /createElement\('a'\)|el\('a'/, /\.src\s*=/]) {
      assert.doesNotMatch(text, banned, `${file}: ${banned}`);
    }
  }
  const page = read(join(srcDir, 'downloads-page.ts'));
  assert.deepEqual([...page.matchAll(/sendPanel\('([a-z]+)'/g)].map((m) => m[1]), ['dpage']);
  assert.deepEqual([...page.matchAll(/\bsend\((\{[^)]*\})\)/g)].map((m) => m[1]), ["{ v: 1, t: 'ready' }"]);
  const bubble = read(join(srcDir, 'downloads-bubble.ts'));
  assert.deepEqual([...new Set([...bubble.matchAll(/\b(send\w*)\(/g)].map((m) => m[1]))].sort(), ['sendDownloadAct', 'sendPopupDismiss', 'sendPopupSize']);
  assert.match(read(join(srcDir, 'popup.ts')), /case 'downloads': downloads\.show\(value\); break;/);
  assert.match(read(join(srcDir, 'main.ts')), /view === 'downloads'/);
});
