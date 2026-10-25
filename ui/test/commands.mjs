// Keyboard and Quick commands (docs/commands.md): the `commands` snapshot contract against what Rust writes
// (test/fixtures/commands-snapshot.json), the wire texts of every command the page sends (each maps to one
// arm of chrome_host/commands_page.rs map_command), capturing a chord from a key event so it names a key
// Rust's Chord::parse reads, filtering and grouping the shortcut list, chain drafts, and the words.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import * as esbuild from 'esbuild';

const here = dirname(fileURLToPath(import.meta.url));
const srcDir = join(here, '..', 'src');
async function bundle(file) {
  const out = esbuild.buildSync({ entryPoints: [join(srcDir, file)], bundle: true, format: 'esm', platform: 'neutral', write: false, logLevel: 'silent' });
  return import(`data:text/javascript;base64,${Buffer.from(out.outputFiles[0].contents).toString('base64')}`);
}
const logic = await bundle('commands-logic.ts');
const protocol = await bundle('protocol.ts');
const fixture = JSON.parse(readFileSync(join(here, 'fixtures', 'commands-snapshot.json'), 'utf8'));

// ---- contract ----

test('the snapshot Rust writes passes the strict contract and keeps what it says', () => {
  const inbound = protocol.parseInbound(JSON.stringify(fixture));
  assert.equal(inbound?.t, 'snap');
  assert.equal(inbound.p, 'commands');
  const snap = logic.parseCommandsSnapshot(inbound.d);
  assert.ok(snap);
  assert.equal(snap.ack, 'none');
  assert.deepEqual([snap.shortcutsEnabled, snap.singleKey, snap.altMenu, snap.focusCycling], [true, false, false, false]);
  assert.deepEqual([snap.paletteKeepLast, snap.paletteNewTab, snap.paletteNotesInPanel, snap.paletteMax], [false, false, true, 9]);
  assert.deepEqual(snap.paletteTypes, ['command', 'tab', 'calc']);
  assert.equal(snap.paletteDefaultOrder, false);
  assert.deepEqual(snap.sites, ['docs.example.com']);
  const close = snap.commands.find((c) => c.id === 'close-tab');
  assert.deepEqual(close.chords, ['Ctrl+W', 'Ctrl+F4', 'Ctrl+Shift+X']);
  assert.deepEqual(close.defaults, ['Ctrl+W', 'Ctrl+F4']);
  assert.equal(close.changed, true);
  const priv = snap.commands.find((c) => c.id === 'new-private-tab');
  assert.deepEqual([priv.chords, priv.changed], [[], true], 'a cleared command has no shortcuts and differs from its default');
  assert.equal(snap.commands.find((c) => c.id === 'history').priority, true);
  assert.equal(snap.commands.find((c) => c.id === 'new-tab').priority, true, 'the browser-priority list defaults to the tab and window chords');
  assert.deepEqual(snap.chains, [{ id: 1, name: 'Morning', steps: [{ delay: 0, command: 'new-tab' }, { delay: 250, command: 'history' }] }]);
  const chain = snap.commands.find((c) => c.id === 'chain:1');
  assert.deepEqual([chain.label, chain.category, chain.chain], ['Morning', 'Command chains', true]);
  assert.ok(snap.commands.some((c) => c.id === 'shortcuts-cheat-sheet' && c.chords.join() === 'Ctrl+F1'));
  assert.ok(snap.commands.some((c) => c.id === 'copy-link' && c.chords.join() === 'Ctrl+Shift+C'), 'Ctrl+Shift+C is a registry binding, so T7 can move it');
});

test('anything else is refused: extra or missing keys, bad ids, padded lists, contradictions', () => {
  const d = fixture.d;
  const refuse = (patch, why) => assert.equal(logic.parseCommandsSnapshot({ ...d, ...patch }), null, why);
  assert.ok(logic.parseCommandsSnapshot(d));
  refuse({ extra: 1 }, 'extra key');
  assert.equal(logic.parseCommandsSnapshot((({ ack, ...rest }) => rest)(d)), null, 'missing key');
  refuse({ ack: 'maybe' }, 'unknown ack');
  refuse({ ack: 'conflict' }, 'a conflict names its chord and owner');
  refuse({ conflictChord: 'Ctrl+J', conflictId: 'downloads' }, 'a conflict is only an ack of conflict');
  refuse({ ack: 'conflict', conflictChord: 'Ctrl+J', conflictId: 'not-a-command', conflictLabel: 'x' }, 'the owner must be a listed command');
  refuse({ paletteMax: 2 }, 'below the minimum');
  refuse({ paletteMax: 31 }, 'above the maximum');
  refuse({ paletteMax: 9.5 }, 'whole numbers');
  refuse({ paletteTypes: [] }, 'at least one type shows');
  refuse({ paletteTypes: ['tab', 'tab'] }, 'duplicate type');
  refuse({ paletteTypes: ['tab', 'bogus'] }, 'unknown type');
  refuse({ sites: ['bad host'] }, 'a site is a bare host');
  refuse({ commands: d.commands.concat(d.commands[0]) }, 'duplicate command id');
  refuse({ commands: [{ ...d.commands[0], extra: 1 }] }, 'extra command key');
  refuse({ commands: [{ ...d.commands[0], chords: Array(7).fill('Ctrl+T') }] }, 'too many shortcuts');
  refuse({ commands: [{ ...d.commands[0], chords: [''] }] }, 'an empty shortcut');
  refuse({ commands: [{ ...d.commands[0], id: 'Bad Id' }] }, 'a command id is lower-case words');
  refuse({ commands: [{ ...d.commands[0], label: '' }] }, 'a label');
  refuse({ commands: [{ ...d.commands[0], label: 'a\u0007b' }] }, 'no control characters');
  refuse({ chains: [{ id: 1, name: 'A', steps: ['0:new-tab'] }, { id: 1, name: 'B', steps: ['0:new-tab'] }] }, 'duplicate chain id');
  refuse({ chains: [{ id: 1, name: 'A', steps: ['999999:new-tab'] }] }, 'a delay past the limit');
  refuse({ chains: [{ id: 1, name: 'A', steps: ['0:chain:2'] }] }, 'a chain holds built-in commands only');
  refuse({ chains: [{ id: 1, name: 'A', steps: ['nonsense'] }] }, 'a step is delay:command');
  refuse({ chains: [{ id: 1, name: 'A', steps: Array(33).fill('0:new-tab') }] }, 'too many steps');
  refuse({ chains: [{ id: 1, name: '', steps: [] }] }, 'a name');
});

// ---- what the page sends ----

test('every command the page sends is one Rust\'s map_command accepts, with exactly its own fields', () => {
  assert.deepEqual(logic.assignCommand('close-tab', 'Ctrl+Shift+Q', false), { a: 'kbassign', text: 'close-tab\tCtrl+Shift+Q', on: false });
  assert.deepEqual(logic.assignCommand('close-tab', 'Ctrl+Shift+Q', true), { a: 'kbassign', text: 'close-tab\tCtrl+Shift+Q', on: true });
  assert.deepEqual(logic.removeChordCommand('reload', 'F5'), { a: 'kbremove', text: 'reload\tF5' });
  assert.deepEqual(logic.clearCommand('reload'), { a: 'kbclear', text: 'reload' });
  assert.deepEqual(logic.restoreCommand('reload'), { a: 'kbrestore', text: 'reload' });
  assert.deepEqual(logic.RESTORE_ALL_COMMAND, { a: 'kbrestoreall' });
  assert.deepEqual(logic.CHEAT_SHEET_COMMAND, { a: 'kbsheet' });
  assert.deepEqual(logic.captureModeCommand(true), { a: 'kbcapture', on: true });
  assert.deepEqual(logic.captureModeCommand(false), { a: 'kbcapture', on: false });
  assert.deepEqual(logic.priorityCommand('history', true), { a: 'kbpriority', text: 'history', on: true });
  assert.deepEqual(logic.flagCommand('focus_cycling', false), { a: 'kboption', text: 'focus_cycling', on: false });
  assert.deepEqual(logic.resultsMaxCommand(9), { a: 'qcmax', n: 9 });
  assert.deepEqual(logic.typesCommand(['tab', 'calc']), { a: 'qctypes', text: 'tab,calc' });
  assert.deepEqual(logic.TYPES_RESET_COMMAND, { a: 'qctypesreset' });
  assert.deepEqual(logic.siteAddCommand('a.example'), { a: 'kbsiteadd', text: 'a.example' });
  assert.deepEqual(logic.siteRemoveCommand('a.example'), { a: 'kbsiteremove', text: 'a.example' });
  assert.deepEqual(logic.chainCloneCommand(3), { a: 'chainclone', n: 3 });
  assert.deepEqual(logic.chainDeleteCommand(3), { a: 'chaindelete', n: 3 });
  // Each is a valid panel command for the settings page (a lower-case word of at most 24 letters).
  for (const fields of [logic.assignCommand('a', 'b', true), logic.RESTORE_ALL_COMMAND, logic.CHEAT_SHEET_COMMAND, logic.resultsMaxCommand(5), logic.TYPES_RESET_COMMAND, logic.chainCloneCommand(1)]) {
    assert.match(fields.a, /^(?=.{1,24}$)[a-z]+(?:-[a-z]+)*$/);
  }
});

test('a chain draft is the text Rust reads: id and name, then one delay and command per line', () => {
  const draft = { id: 3, name: ' Morning\troutine ', steps: [{ delay: 0, command: 'new-tab' }, { delay: 250, command: 'history' }] };
  assert.equal(logic.chainDraftText(draft), '3\tMorning routine\n0\tnew-tab\n250\thistory');
  assert.deepEqual(logic.chainSaveCommand(draft), { a: 'chainsave', text: '3\tMorning routine\n0\tnew-tab\n250\thistory' });
  assert.deepEqual(logic.chainTestCommand(draft), { a: 'chaintest', text: '3\tMorning routine\n0\tnew-tab\n250\thistory' });
  assert.equal(logic.chainDraftText({ id: 0, name: 'New', steps: [] }), '0\tNew');
  const others = [{ id: 1, name: 'Existing', steps: [] }];
  assert.equal(logic.draftError(draft, others), null);
  assert.equal(logic.draftError({ ...draft, name: '  ' }, others), 'Give the chain a name.');
  assert.equal(logic.draftError({ ...draft, name: 'x'.repeat(49) }, others), 'A chain name holds at most 48 characters.');
  assert.equal(logic.draftError({ ...draft, name: 'EXISTING' }, others), 'There is already a chain called EXISTING.');
  assert.equal(logic.draftError({ ...draft, id: 1, name: 'existing' }, others), null, 'a chain may keep its own name');
  assert.equal(logic.draftError({ ...draft, steps: [] }, others), 'Add at least one command to the chain.');
  assert.equal(logic.draftError({ ...draft, steps: Array(33).fill({ delay: 0, command: 'new-tab' }) }, others), 'A chain holds at most 32 commands.');
  assert.equal(logic.draftError({ ...draft, steps: [{ delay: -1, command: 'new-tab' }] }, others), 'A delay is between 0 and 60000 ms.');
  assert.equal(logic.draftError({ ...draft, steps: [{ delay: 1.5, command: 'new-tab' }] }, others), 'A delay is between 0 and 60000 ms.');
  assert.deepEqual(['', 'abc', '-5', '12.6', '999999', '250'].map(logic.parseDelay), [0, 0, 0, 13, 60000, 250]);
});

test('moving, hiding and showing: lists change by copy, a shown type goes last, the last shown type stays', () => {
  const list = ['a', 'b', 'c'];
  assert.deepEqual(logic.moveItem(list, 0, 1), ['b', 'a', 'c']);
  assert.deepEqual(logic.moveItem(list, 2, 0), ['c', 'a', 'b']);
  assert.deepEqual(logic.moveItem(list, 1, 9), ['a', 'c', 'b'], 'clamped');
  assert.deepEqual(logic.moveItem(list, 5, 0), ['a', 'b', 'c'], 'nothing there to move');
  assert.deepEqual(list, ['a', 'b', 'c'], 'the original is untouched');
  assert.deepEqual(logic.toggleType(['tab', 'command'], 'note', true), ['tab', 'command', 'note']);
  assert.deepEqual(logic.toggleType(['tab', 'command'], 'tab', false), ['command']);
  assert.deepEqual(logic.toggleType(['tab'], 'tab', false), ['tab'], 'never empty');
  assert.deepEqual(logic.toggleType(['tab'], 'tab', true), ['tab']);
  assert.deepEqual(logic.RESULT_TYPES, ['tab', 'closed', 'workspace', 'command', 'action', 'bookmark', 'note', 'history', 'calc']);
});

// ---- chord capture ----

const ev = (key, code, mods = {}) => ({ key, code, ctrlKey: false, shiftKey: false, altKey: false, ...mods });

test('a key press becomes the chord text Rust parses, whatever the layout\'s character', () => {
  const cases = [
    [ev('t', 'KeyT', { ctrlKey: true }), 'Ctrl+T'],
    [ev('K', 'KeyK', { ctrlKey: true, shiftKey: true }), 'Ctrl+Shift+K'],
    [ev('r', 'KeyR', { ctrlKey: true, altKey: true }), 'Ctrl+Alt+R'],
    [ev('é', 'KeyE', { altKey: true }), 'Alt+E'],
    [ev('1', 'Digit1', { ctrlKey: true }), 'Ctrl+1'],
    [ev('!', 'Digit1', { ctrlKey: true, shiftKey: true }), 'Ctrl+Shift+1'],
    [ev('F5', 'F5'), 'F5'],
    [ev('F12', 'F12', { shiftKey: true }), 'Shift+F12'],
    [ev('ArrowLeft', 'ArrowLeft', { altKey: true }), 'Alt+Left'],
    [ev('PageDown', 'PageDown', { ctrlKey: true }), 'Ctrl+PageDown'],
    [ev(',', 'Comma', { ctrlKey: true }), 'Ctrl+,'],
    [ev('\\', 'Backslash', { ctrlKey: true, shiftKey: true }), 'Ctrl+Shift+\\'],
    [ev('=', 'Equal', { ctrlKey: true }), 'Ctrl+='],
    [ev('+', 'NumpadAdd', { ctrlKey: true }), 'Ctrl+Numpad+'],
    [ev('0', 'Numpad0', { ctrlKey: true }), 'Ctrl+Numpad0'],
    [ev(' ', 'Space', { shiftKey: true }), 'Shift+Space'],
    [ev('Delete', 'Delete', { ctrlKey: true, shiftKey: true }), 'Ctrl+Shift+Delete'],
    [ev('BrowserBack', 'BrowserBack'), 'BrowserBack'],
    [ev('Escape', 'Escape', { shiftKey: true }), null],
  ];
  for (const [event, chord] of cases) {
    const captured = logic.captureChord(event);
    if (chord === null) assert.equal(captured.kind, 'wait', event.code);
    else assert.deepEqual(captured, { kind: 'chord', chord }, event.code);
  }
  // A bare modifier waits; Escape on its own cancels; an unknown key waits.
  for (const modifier of ['ControlLeft', 'ShiftRight', 'AltLeft', 'MetaLeft']) assert.deepEqual(logic.captureChord(ev('Control', modifier, { ctrlKey: true })), { kind: 'wait' });
  assert.deepEqual(logic.captureChord(ev('Escape', 'Escape')), { kind: 'cancel' });
  assert.deepEqual(logic.captureChord(ev('Unidentified', 'IntlBackslash')), { kind: 'wait' });
  assert.equal(logic.keyName({ key: 'x', code: 'F25' }), null);
  assert.equal(logic.keyName({ key: 'x', code: 'KeyA' }), 'A');
});

test('every key name the editor can capture is in the list Rust parses (test/fixtures/chord-names.txt)', () => {
  const lines = readFileSync(join(here, 'fixtures', 'chord-names.txt'), 'utf8').replace(/\r\n/g, '\n').split('\n').filter((l) => l && !l.startsWith('#'));
  assert.ok(lines.length >= 100, `${lines.length} names`);
  for (const line of lines) {
    const [name, code] = line.split('\t');
    assert.equal(logic.keyName({ key: 'x', code }), name, code);
  }
  // The list is complete: no code the editor names is missing from it.
  const named = new Set(lines.map((l) => l.split('\t')[1]));
  for (const code of ['KeyQ', 'Digit4', 'F24', 'Numpad9', 'NumpadAdd', 'ArrowDown', 'Semicolon', 'Quote', 'ContextMenu', 'BrowserStop']) assert.ok(named.has(code), code);
});

// ---- the shortcut list ----

const cmd = (id, label, category, chords = []) => ({ id, label, category, chords, defaults: chords, changed: false, priority: false, chain: false });

test('the shortcut list filters on every word of name, category and shortcuts, and groups by category in order', () => {
  const commands = [
    cmd('new-tab', 'New tab', 'Tabs', ['Ctrl+T']),
    cmd('close-tab', 'Close tab', 'Tabs', ['Ctrl+W', 'Ctrl+F4']),
    cmd('reload', 'Reload', 'Navigation', ['F5', 'Ctrl+R']),
    cmd('devtools', 'Developer tools', 'Tools', ['F12']),
    cmd('reader', 'Reader view', 'Page', ['Ctrl+Alt+R']),
  ];
  assert.deepEqual(logic.groupCommands(commands, '').map((g) => [g.category, g.commands.length]), [['Tabs', 2], ['Navigation', 1], ['Tools', 1], ['Page', 1]]);
  assert.deepEqual(logic.groupCommands(commands, 'ctrl+r').flatMap((g) => g.commands.map((c) => c.id)), ['reload'], 'a shortcut is searchable');
  assert.deepEqual(logic.groupCommands(commands, 'alt+r').flatMap((g) => g.commands.map((c) => c.id)), ['reader']);
  assert.deepEqual(logic.groupCommands(commands, 'tabs close').flatMap((g) => g.commands.map((c) => c.id)), ['close-tab'], 'every word, category included');
  assert.deepEqual(logic.groupCommands(commands, 'ZZZ'), []);
});

test('sites, answers and words', () => {
  assert.equal(logic.siteError(['a.example'], 'a.example'), 'a.example is already on the list.');
  assert.equal(logic.siteError([], null), 'Enter a site.');
  assert.equal(logic.siteError(['a.example'], 'b.example'), null);
  assert.equal(logic.siteError(Array.from({ length: 200 }, (_, i) => `s${i}.example`), 'new.example'), 'The list holds at most 200 sites.');
  assert.deepEqual(logic.ackToast({ ack: 'saved', error: '' }), { text: 'Saved', ok: true });
  assert.deepEqual(logic.ackToast({ ack: 'ran', error: '' }), { text: 'Chain started', ok: true });
  assert.deepEqual(logic.ackToast({ ack: 'failed', error: 'Nope' }), { text: 'Nope', ok: false });
  assert.deepEqual(logic.ackToast({ ack: 'failed', error: '' }), { text: 'That could not be saved.', ok: false });
  assert.equal(logic.ackToast({ ack: 'none', error: '' }), null);
  assert.equal(logic.ackToast({ ack: 'conflict', error: '' }), null, 'a conflict is answered in the row, not a toast');
  assert.equal(logic.conflictText({ conflictChord: 'Ctrl+J', conflictLabel: 'Downloads' }), 'Ctrl+J is already used by Downloads.');
  assert.deepEqual(logic.parseStep('250:reload'), { delay: 250, command: 'reload' });
  for (const bad of ['reload', ':reload', '-1:reload', '1.5:reload', '70000:reload', '5:Bad Id', '5:chain:1']) assert.equal(logic.parseStep(bad), null, bad);
});
