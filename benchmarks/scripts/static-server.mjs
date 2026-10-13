// Serves one directory over http://127.0.0.1:<port>/ for the dashboard render harness.
import { createServer } from 'node:http';
import { readFileSync } from 'node:fs';
import { join, extname } from 'node:path';
const [dir, port] = [process.argv[2], Number(process.argv[3])];
const types = { '.html': 'text/html; charset=utf-8', '.js': 'text/javascript', '.css': 'text/css' };
createServer((req, res) => {
  const name = new URL(req.url, 'http://x').pathname.replace(/^\/+/, '') || 'harness.html';
  try { const body = readFileSync(join(dir, name.replace(/\.\./g, ''))); res.setHeader('Content-Type', types[extname(name)] ?? 'application/octet-stream'); res.end(body); }
  catch { res.statusCode = 404; res.end('no'); }
}).listen(port, '127.0.0.1');
