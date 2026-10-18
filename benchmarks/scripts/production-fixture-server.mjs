// Loopback-only synthetic fixtures. No production/browser instrumentation protocol is enabled.
import http from 'node:http';
import fs from 'node:fs';
import { serveFilterParity } from './filter-parity-fixture.mjs';
const [portFile, logFile] = process.argv.slice(2);
const server = http.createServer((req, res) => {
  const url = new URL(req.url, 'http://127.0.0.1');
  const token = url.searchParams.get('token') ?? '';
  if (!/^[a-zA-Z0-9_]{1,80}$/.test(token)) { res.writeHead(400).end(); return; }
  fs.appendFileSync(logFile, JSON.stringify({ path:url.pathname, token, at_ns:process.hrtime.bigint().toString(), status:url.pathname === '/error' ? 500 : 200 })+'\n');
  res.setHeader('Cache-Control','no-store');
  if (serveFilterParity(req,res,url,token,logFile)) return;
  if (url.pathname === '/ready') { res.writeHead(204).end(); return; }
  if (url.pathname === '/redirect') { res.writeHead(302,{Location:`/light?token=${token}`}).end(); return; }
  if (url.pathname === '/network-failure') { req.socket.destroy(); return; }
  if (url.pathname === '/hung') { return; }
  if (url.pathname === '/error') { res.writeHead(500,{'Content-Type':'text/html'}).end('<title>Error fixture</title><h1>Failure</h1>'); return; }
  if (!['/light','/demanding'].includes(url.pathname)) { res.writeHead(404).end(); return; }
  res.setHeader('Content-Security-Policy',"default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; connect-src 'self'");
  res.setHeader('Content-Type','text/html;charset=utf-8');
  const count = url.pathname === '/demanding' ? 4000 : 24;
  res.end(`<!doctype html><html><head><title>Bowser performance fixture ${token}</title><style>body{font:16px system-ui;background:#edf2f8;color:#123;margin:32px}main{max-width:1000px}article{display:inline-block;border:1px solid #abc;padding:8px;margin:2px;width:130px}input{font:inherit}</style></head><body><main><h1>Production performance fixture</h1><input aria-label="Fixture input"><section>${Array.from({length:count},(_,i)=>`<article>Row ${i}<b> Synthetic</b></article>`).join('')}</section></main><script>addEventListener('load',()=>requestAnimationFrame(()=>requestAnimationFrame(()=>{document.title='BOWSER_PERF_READY_${token}';fetch('/ready?token=${token}',{cache:'no-store'});})));</script></body></html>`);
});
server.listen(0,'127.0.0.1',()=>fs.writeFileSync(portFile,String(server.address().port)));
