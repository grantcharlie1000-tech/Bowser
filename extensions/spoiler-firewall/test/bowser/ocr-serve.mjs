// Serves the OCR live fixture on 127.0.0.1:18083 (HTTP only). Run Bowser as a DEBUG build with
//   BOWSER_SPOILER_OCR_TEST_HOST=127.0.0.1
// so that host counts as a listed site (the override does not exist in release builds). See ocr-live.md.
import http from 'node:http';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = path.join(path.dirname(fileURLToPath(import.meta.url)), 'fixtures');
const types = { '.html': 'text/html; charset=utf-8', '.png': 'image/png' };

export function serveOcr(port = 18083) {
  const hits = [];
  const server = http.createServer((req, res) => {
    const name = path.basename(new URL(req.url, 'http://x').pathname) || 'ocr.html';
    const file = path.join(root, name);
    hits.push(name);
    if (!['ocr.html', 'ocr-text.png', 'ocr-spoiler.png'].includes(name) || !fs.existsSync(file)) { res.writeHead(404).end('not found'); return; }
    res.writeHead(200, { 'content-type': types[path.extname(name)], 'cache-control': 'no-store' });
    res.end(fs.readFileSync(file));
  });
  server.listen(port, '127.0.0.1');
  return { stop: () => server.close(), hits };
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  serveOcr();
  console.log('serving http://127.0.0.1:18083/ocr.html (Ctrl+C to stop)');
}
