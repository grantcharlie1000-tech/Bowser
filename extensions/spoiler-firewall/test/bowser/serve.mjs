// Two origins for the Bowser live run: 127.0.0.1:18081 (top) and localhost:18082 (cross-site frames).
import http from 'node:http';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = path.join(path.dirname(fileURLToPath(import.meta.url)), 'fixtures');

/** Returns { stop, reports }: `reports` collects what the injected script beacons from each frame. */
export function serve(ports = [18081, 18082]) {
  const reports = [];
  const servers = ports.map((port) => {
    const server = http.createServer((req, res) => {
      const url = new URL(req.url, 'http://x');
      if (url.pathname === '/__report') {
        try { reports.push(JSON.parse(url.searchParams.get('d'))); } catch {}
        res.writeHead(204).end();
        return;
      }
      const name = path.basename(new URL(req.url, 'http://x').pathname) || 'main.html';
      const file = path.join(root, name);
      if (!file.startsWith(root) || !fs.existsSync(file)) { res.writeHead(404).end('not found'); return; }
      res.writeHead(200, { 'content-type': 'text/html; charset=utf-8', 'cache-control': 'no-store' });
      res.end(fs.readFileSync(file));
    });
    server.listen(port, '127.0.0.1');
    return server;
  });
  return { stop: () => servers.forEach((server) => server.close()), reports };
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  serve();
  console.log('serving 127.0.0.1:18081 and localhost:18082 (Ctrl+C to stop)');
}
