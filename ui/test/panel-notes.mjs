import { test } from 'node:test';
import assert from 'node:assert/strict';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { readFileSync } from 'node:fs';
import * as esbuild from 'esbuild';

const here = dirname(fileURLToPath(import.meta.url));
const { code } = esbuild.transformSync(readFileSync(join(here, '..', 'src', 'panels', 'notes-logic.ts'), 'utf8'), { loader: 'ts', format: 'esm' });
const logic = await import(`data:text/javascript;base64,${Buffer.from(code).toString('base64')}`);

const note = (id, over = {}) => ({ id, title: 'Plan', body: 'Draft', addr: '', created: 0, folder: 0, ...over });
const snap = (over = {}) => ({ notes: [note(1)], tasks: [{ id: 2, text: 'Review', complete: false }], folders: [], sort: 'manual', ...over });

test('notes parser accepts bounded notes and tasks with distinct safe ids', () => {
  const value = snap();
  assert.deepEqual(logic.parseNotesSnapshot(value), value);
  assert.equal(logic.parseNotesSnapshot({ ...value, tasks: [{ ...value.tasks[0], id: 1 }] }), null);
  assert.equal(logic.parseNotesSnapshot({ ...value, extra: true }), null);
  assert.equal(logic.parseNotesSnapshot({ ...value, notes: [note(0)] }), null);
  // Ids the host makes are microsecond timestamps, well beyond u32 (this made every new note invalid).
  const stamped = snap({ notes: [note(1_760_000_000_000_000, { title: 'New note', body: '' })], tasks: [{ id: 1_760_000_000_000_001, text: 'New task', complete: false }] });
  assert.deepEqual(logic.parseNotesSnapshot(stamped), stamped);
  assert.equal(logic.parseNotesSnapshot({ ...stamped, notes: [note(2 ** 53)] }), null);
});

test('the snapshot carries folders and a sort order, and a note never names a folder that is not there', () => {
  const folders = [{ id: 7, name: 'Work' }];
  const ok = snap({ folders, notes: [note(1, { folder: 7, addr: 'https://a.example/', created: 1700000000 })], sort: 'date' });
  assert.deepEqual(logic.parseNotesSnapshot(ok), ok);
  assert.equal(logic.parseNotesSnapshot(snap({ folders, notes: [note(1, { folder: 8 })] })), null);
  assert.equal(logic.parseNotesSnapshot(snap({ sort: 'random' })), null);
  assert.equal(logic.parseNotesSnapshot(snap({ folders: [{ id: 1, name: 'A' }, { id: 1, name: 'B' }] })), null);
  assert.equal(logic.parseNotesSnapshot(snap({ notes: [note(1, { body: 'x'.repeat(5000) })] })), null);
  for (const sort of logic.SORTS) assert.ok(logic.parseNotesSnapshot(snap({ sort })), sort);
});

test('notes are grouped unfiled first, then by folder', () => {
  const data = { folders: [{ id: 7, name: 'Work' }, { id: 8, name: 'Home' }], notes: [note(1, { folder: 8 }), note(2), note(3, { folder: 7 })] };
  const groups = logic.groupNotes(data);
  assert.deepEqual(groups.map((g) => [g.folder?.name ?? null, g.notes.map((n) => n.id)]), [[null, [2]], ['Work', [3]], ['Home', [1]]]);
});

test('note and task command builders constrain ids, action and field count', () => {
  assert.deepEqual(logic.notesCommand('delete-note', 1), { a: 'delete-note', id: 1 });
  assert.deepEqual(logic.notesCommand('toggle-task', 2, { on: true }), { a: 'toggle-task', id: 2, on: true });
  assert.equal(logic.notesCommand('delete-note', 0), null);
  assert.equal(logic.notesCommand('invented', 1), null);
  assert.equal(logic.notesCommand('delete-note', 1, { text: 'x' }), null);
});

test('sort, folder, move and file commands carry exactly their fields', () => {
  assert.deepEqual(logic.notesCommand('sort', undefined, { text: 'title' }), { a: 'sort', text: 'title' });
  assert.equal(logic.notesCommand('sort', undefined, { text: 'random' }), null);
  assert.deepEqual(logic.notesCommand('folder-add', undefined, { text: 'Work' }), { a: 'folder-add', text: 'Work' });
  assert.deepEqual(logic.notesCommand('folder-rename', 7, { text: 'Home' }), { a: 'folder-rename', id: 7, text: 'Home' });
  assert.deepEqual(logic.notesCommand('folder-delete', 7), { a: 'folder-delete', id: 7 });
  assert.deepEqual(logic.notesCommand('note-folder', 1, { parent: 0 }), { a: 'note-folder', id: 1, parent: 0 });
  assert.equal(logic.notesCommand('note-folder', 1), null);
  assert.deepEqual(logic.notesCommand('move-note', 1, { before: 5 }), { a: 'move-note', id: 1, before: 5 });
  assert.deepEqual(logic.notesCommand('move-note', 1), { a: 'move-note', id: 1 }, 'no anchor means the end');
  assert.equal(logic.notesCommand('move-note', 1, { text: 'x' }), null);
  assert.deepEqual(logic.notesCommand('import'), { a: 'import' });
  assert.deepEqual(logic.exportCommand('all', undefined, 'txt'), { a: 'export', n: 0, text: 'txt' });
  assert.deepEqual(logic.exportCommand('folder', 7, 'md'), { a: 'export', n: 1, text: 'md', id: 7 });
  assert.deepEqual(logic.exportCommand('note', 3, 'md'), { a: 'export', n: 2, text: 'md', id: 3 });
  assert.equal(logic.exportCommand('note', undefined, 'md'), null);
  assert.equal(logic.notesCommand('export', undefined, { n: 0, text: 'pdf' }), null);
  assert.equal(logic.notesCommand('save-body', 1, { text: 'x'.repeat(4001) }), null);
});

test('markdown becomes data, never markup, and only web and mail links survive', () => {
  const blocks = logic.parseMarkdown('# Title\n\nSome **bold** and *soft* and `code` text.\n\n- one\n- two\n\n1. a\n2. b\n\n> quoted\n\n```\nlet x = 1;\n```\n[site](https://example.com/a) [bad](javascript:alert(1)) <https://x.example/y>');
  assert.deepEqual(blocks.map((b) => b.k), ['h', 'p', 'ul', 'ol', 'quote', 'code', 'p']);
  assert.equal(blocks[0].level, 1);
  const para = blocks[1].inline.map((i) => i.t);
  assert.deepEqual(para, ['text', 'b', 'text', 'i', 'text', 'code', 'text']);
  assert.equal(blocks[2].items.length, 2);
  assert.equal(blocks[5].s, 'let x = 1;');
  const links = blocks[6].inline.filter((i) => i.t === 'link');
  assert.deepEqual(links.map((l) => l.href), ['https://example.com/a', 'https://x.example/y']);
  const bad = blocks[6].inline.find((i) => i.s === 'bad');
  assert.equal(bad.t, 'text', 'a javascript: link is plain text');
  assert.equal(logic.safeHref('javascript:alert(1)'), null);
  assert.equal(logic.safeHref('data:text/html,x'), null);
  assert.equal(logic.safeHref('mailto:a@b.example'), 'mailto:a@b.example');
  // An image is never fetched: it shows its description as text.
  assert.deepEqual(logic.parseInline('![cap](file:///C:/x.png)'), [{ t: 'text', s: '[cap]' }]);
  // Raw HTML stays text.
  assert.deepEqual(logic.parseMarkdown('<img src=x onerror=alert(1)>'), [{ k: 'p', inline: [{ t: 'text', s: '<img src=x onerror=alert(1)>' }] }]);
});

test('an address and a date read as a short line', () => {
  assert.equal(logic.addrHost('https://docs.example.com/a?b=1'), 'docs.example.com');
  assert.equal(logic.addrHost('nonsense'), '');
  assert.equal(logic.dateText(0), '');
  assert.match(logic.dateText(1700000000), /2023/);
});
