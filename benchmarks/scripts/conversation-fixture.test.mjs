import { test, before, after } from 'node:test';
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { mkdtemp, readFile, rm } from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { once } from 'node:events';

let child, folder, origin;
before(async () => {
  folder = await mkdtemp(path.join(os.tmpdir(), 'bowser-conversation-test-'));
  const portFile = path.join(folder, 'port');
  child = spawn(process.execPath, [new URL('./production-fixture-server.mjs', import.meta.url).pathname.replace(/^\/([A-Za-z]:)/, '$1'), portFile, path.join(folder, 'requests.jsonl')], { stdio: 'pipe', windowsHide: true });
  let errors = '';
  child.stderr.on('data', bytes => { errors += bytes; });
  for (let attempt = 0; attempt < 100; attempt++) {
    try { origin = `http://127.0.0.1:${await readFile(portFile, 'utf8')}`; return; }
    catch { if (child.exitCode !== null) throw Error(errors); await new Promise(resolve => setTimeout(resolve, 20)); }
  }
  throw Error('Fixture server did not start');
});
after(async () => {
  if (child?.exitCode === null) { child.kill(); await once(child, 'exit'); }
  if (folder) await rm(folder, { recursive: true, force: true });
});

test('conversation requests return the selected fixed transcript instead of another conversation', async () => {
  const response = await fetch(`${origin}/conversation-data?token=fixture_001&id=2`);
  assert.equal(response.status, 200);
  const transcript = await response.json();
  assert.equal(transcript.id, 2);
  assert.equal(transcript.title, 'Conversation 3');
  assert.equal(transcript.messages.length, 120);
  assert.deepEqual(transcript.messages[0], {
    role: 'user',
    text: 'Conversation 3, message 1. Explain the deterministic example and preserve this text when switching conversations.',
    code: 'const conversation = 3;\nconst message = 1;\nconsole.log(conversation + message);',
  });
  assert.equal(transcript.messages[119].role, 'assistant');
});

test('streaming sends every ordered chunk and a matching terminal identity', async () => {
  const response = await fetch(`${origin}/conversation-stream?token=stream_001&id=2&generation=7`);
  assert.equal(response.status, 200);
  assert.match(response.headers.get('content-type'), /^application\/x-ndjson/);
  const records = (await response.text()).trim().split('\n').map(line => JSON.parse(line));
  assert.equal(records.length, 65);
  for (let index = 0; index < 64; index++) {
    assert.deepEqual(records[index], { token: 'stream_001', id: 2, generation: 7, sequence: index, text: `Chunk ${String(index + 1).padStart(2, '0')}: deterministic streamed reply.\n` });
  }
  assert.deepEqual(records[64], { token: 'stream_001', id: 2, generation: 7, done: true, chunks: 64 });
});

test('invalid conversation and generation identities cannot produce a successful fixture', async () => {
  for (const route of [
    '/conversation-data?token=invalid_001&id=-1',
    '/conversation-data?token=invalid_002&id=8',
    '/conversation-data?token=invalid_003&id=2junk',
    '/conversation-stream?token=invalid_004&id=0&generation=0',
    '/conversation-stream?token=invalid_005&id=0&generation=NaN',
  ]) {
    assert.equal((await fetch(origin + route)).status, 400, route);
  }
});

async function loadModel() {
  const response = await fetch(`${origin}/conversation-model.mjs?token=model_001`);
  assert.equal(response.status, 200);
  return (await import('data:text/javascript;base64,' + Buffer.from(await response.text()).toString('base64'))).ConversationModel;
}

test('switching conversations preserves independent drafts and rejects an old reply', async () => {
  const ConversationModel = await loadModel();
  const model = new ConversationModel('model_001');
  model.select(0);
  model.saveDraft('Draft for first conversation');
  const old = model.startStream();
  model.select(1);
  model.saveDraft('Second draft');
  assert.equal(model.acceptChunk({ ...old, sequence: 0, text: 'Chunk 01: deterministic streamed reply.\n' }), false);
  assert.equal(model.reply, '');
  model.select(0);
  assert.equal(model.draft, 'Draft for first conversation');
  assert.equal(model.reply, '');
  model.select(1);
  assert.equal(model.draft, 'Second draft');
});

test('a missing, duplicated or changed chunk cannot become successful completion', async () => {
  const ConversationModel = await loadModel();
  for (const bad of [
    { sequence: 1, text: 'Chunk 02: deterministic streamed reply.\n' },
    { sequence: 0, text: 'different content' },
    { done: true, chunks: 64 },
  ]) {
    const model = new ConversationModel('model_001');
    model.select(2);
    const ticket = model.startStream();
    assert.throws(() => model.acceptChunk({ ...ticket, ...bad }));
    assert.equal(model.complete, false);
  }
  const model = new ConversationModel('model_001');
  model.select(2);
  const ticket = model.startStream();
  model.acceptChunk({ ...ticket, sequence: 0, text: 'Chunk 01: deterministic streamed reply.\n' });
  assert.throws(() => model.acceptChunk({ ...ticket, sequence: 0, text: 'Chunk 01: deterministic streamed reply.\n' }));
  assert.equal(model.complete, false);
});

test('only the current token and complete ordered reply can finish a stream', async () => {
  const ConversationModel = await loadModel();
  const model = new ConversationModel('model_001');
  model.select(7);
  const ticket = model.startStream();
  assert.equal(model.acceptChunk({ ...ticket, token: 'wrong_token', sequence: 0, text: 'Chunk 01: deterministic streamed reply.\n' }), false);
  for (let sequence = 0; sequence < 64; sequence++) {
    assert.equal(model.acceptChunk({ ...ticket, sequence, text: `Chunk ${String(sequence + 1).padStart(2, '0')}: deterministic streamed reply.\n` }), true);
  }
  assert.equal(model.complete, false);
  assert.equal(model.acceptChunk({ ...ticket, done: true, chunks: 64 }), true);
  assert.equal(model.complete, true);
  assert.equal(model.reply.split('\n').length, 65);
  assert.ok(model.reply.endsWith('Chunk 64: deterministic streamed reply.\n'));
});

test('returning home cancels late replies and preserves the draft for reopening', async () => {
  const ConversationModel = await loadModel();
  const model = new ConversationModel('model_001');
  model.select(3);
  model.saveDraft('Draft survives home');
  const ticket = model.startStream();
  model.home();
  assert.equal(model.selected, null);
  assert.equal(model.acceptChunk({ ...ticket, sequence: 0, text: 'Chunk 01: deterministic streamed reply.\n' }), false);
  model.select(3);
  assert.equal(model.draft, 'Draft survives home');
});

test('absent or malformed tokens cannot create a usable model', async () => {
  const ConversationModel = await loadModel();
  for (const token of [undefined, null, {}, '', 'bad token', 'x'.repeat(81)]) {
    assert.throws(() => new ConversationModel(token));
  }
});

test('the ordinary fixture page and client asset are available without a benchmark app mode', async () => {
  for (const [route, type] of [['/conversation', /^text\/html/], ['/conversation-client.mjs', /^application\/javascript/]]) {
    const response = await fetch(`${origin}${route}?token=page_001`);
    assert.equal(response.status, 200);
    assert.match(response.headers.get('content-type'), type);
  }
});
