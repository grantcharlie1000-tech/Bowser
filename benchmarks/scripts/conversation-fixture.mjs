// Synthetic transport only. These receipts prove delivered work, not displayed frames.
import fs from 'node:fs';

function conversationId(url) {
  const value = url.searchParams.get('id');
  return /^[0-7]$/.test(value ?? '') ? Number(value) : null;
}

export function serveConversationFixture(req, res, url, token, logFile) {
  const assets = {
    '/conversation': ['conversation-fixture.html', 'text/html;charset=utf-8'],
    '/conversation-client.mjs': ['conversation-client.mjs', 'application/javascript;charset=utf-8'],
    '/conversation-model.mjs': ['conversation-model.mjs', 'application/javascript;charset=utf-8'],
  };
  if (Object.hasOwn(assets, url.pathname)) {
    const [file, type] = assets[url.pathname];
    res.writeHead(200, { 'Content-Type': type, 'Content-Security-Policy': "default-src 'none'; script-src 'self'; style-src 'unsafe-inline'; connect-src 'self'" });
    res.end(fs.readFileSync(new URL(file, import.meta.url), 'utf8').replace('FIXTURE_TOKEN', token));
    return true;
  }
  if (!['/conversation-data', '/conversation-stream'].includes(url.pathname)) return false;
  const id = conversationId(url);
  const generation = url.searchParams.get('generation');
  if (id === null || (url.pathname === '/conversation-stream' && !/^[1-9][0-9]{0,8}$/.test(generation ?? ''))) {
    res.writeHead(400).end();
    return true;
  }
  if (url.pathname === '/conversation-data') {
    const messages = Array.from({ length: 120 }, (_, index) => ({
      role: index % 2 === 0 ? 'user' : 'assistant',
      text: `Conversation ${id + 1}, message ${index + 1}. Explain the deterministic example and preserve this text when switching conversations.`,
      code: `const conversation = ${id + 1};\nconst message = ${index + 1};\nconsole.log(conversation + message);`,
    }));
    res.writeHead(200, { 'Content-Type': 'application/json;charset=utf-8' });
    res.end(JSON.stringify({ id, title: `Conversation ${id + 1}`, messages }));
    return true;
  }
  res.writeHead(200, { 'Content-Type': 'application/x-ndjson;charset=utf-8', 'X-Content-Type-Options': 'nosniff' });
  res.flushHeaders();
  let sequence = 0;
  const delivered = [];
  const identity = { token, id, generation: Number(generation) };
  const timer = setInterval(() => {
    const text = `Chunk ${String(sequence + 1).padStart(2, '0')}: deterministic streamed reply.\n`;
    delivered.push({ sequence, at_ns: process.hrtime.bigint().toString() });
    res.write(JSON.stringify({ ...identity, sequence, text }) + '\n');
    if (++sequence === 64) {
      clearInterval(timer);
      res.end(JSON.stringify({ ...identity, done: true, chunks: sequence }) + '\n');
    }
  }, 40);
  res.on('close', () => {
    clearInterval(timer);
    fs.appendFileSync(logFile, JSON.stringify({ path: '/conversation-stream-receipt', ...identity, chunks: sequence, complete: sequence === 64, delivered }) + '\n');
  });
  return true;
}
