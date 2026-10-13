// Serves ui/test/fixtures/reader on 127.0.0.1 and logs every request, so a reader-view check can show
// that turning reader view on and off fetches nothing and adds no history entry.
//   node serve-reader-fixtures.mjs <port> <logFile>
import { createServer } from 'node:http';
import { appendFileSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, '..', '..', 'ui', 'test', 'fixtures', 'reader');
const [port = '8765', log = 'server.log'] = process.argv.slice(2);
writeFileSync(log, '');
createServer((req, res) => {
  const path = decodeURIComponent((req.url ?? '/').split('?')[0]);
  appendFileSync(log, `${req.method} ${path}\n`);
  if (path === '/start.html') {
    res.writeHead(200, { 'content-type': 'text/html; charset=utf-8' });
    res.end('<!doctype html><title>Start page</title><p style="font-size:32px;margin:20px"><a href="/article-divs.html">Open the article</a></p>');
    return;
  }
  const name = path.replace(/^\/+/, '');
  if (!/^[\w.-]+\.html$/.test(name)) { res.writeHead(404).end(); return; }
  try {
    res.writeHead(200, { 'content-type': 'text/html; charset=utf-8', 'cache-control': 'no-store' });
    res.end(readFileSync(join(root, name)));
  } catch {
    res.writeHead(404).end();
  }
}).listen(Number(port), '127.0.0.1');
