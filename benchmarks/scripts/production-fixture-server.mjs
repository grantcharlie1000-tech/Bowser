// Loopback-only synthetic fixtures. No production/browser instrumentation protocol is enabled.
import http from 'node:http';
import fs from 'node:fs';
import { serveFilterParity } from './filter-parity-fixture.mjs';
import { serveConversationFixture } from './conversation-fixture.mjs';
import { serveAutoFixture } from '../auto/auto-fixture.mjs';
const [portFile, logFile] = process.argv.slice(2);
let requestSequence = 0;
const server = http.createServer((req, res) => {
  const url = new URL(req.url, 'http://127.0.0.1');
  const token = url.searchParams.get('token') ?? '';
  if (!/^[a-zA-Z0-9_]{1,80}$/.test(token)) { res.writeHead(400).end(); return; }
  const requestId = `${process.pid}_${++requestSequence}`;
  // Arrival stays on the original path/at_ns contract. It asserts no HTTP result.
  fs.appendFileSync(logFile, JSON.stringify({ path:url.pathname, token, at_ns:process.hrtime.bigint().toString(), status:null, event:'request_arrival', request_id:requestId })+'\n');
  let finished = false;
  const outcome = (event, status) => fs.appendFileSync(logFile, JSON.stringify({
    // Separate path preserves existing arrival/fixture-receipt consumers.
    path:'/__response_outcome__', request_path:url.pathname, token, request_id:requestId,
    at_ns:process.hrtime.bigint().toString(), event, status,
    headers_sent:res.headersSent, sent_status:res.headersSent ? res.statusCode : null,
  })+'\n');
  res.once('finish', () => { finished = true; outcome('response_finish', res.statusCode); });
  res.once('close', () => { if (!finished) outcome('response_abort', null); });
  res.setHeader('Cache-Control','no-store');
  if (serveFilterParity(req,res,url,token,logFile)) return;
  if (serveConversationFixture(req,res,url,token,logFile)) return;
  if (serveAutoFixture(req,res,url,token,logFile)) return;
  if (url.pathname === '/ready') { res.writeHead(204).end(); return; }
  if (url.pathname === '/redirect') { res.writeHead(302,{Location:`/light?token=${token}`}).end(); return; }
  if (url.pathname === '/network-failure') { req.socket.destroy(); return; }
  if (url.pathname === '/hung') { return; }
  if (url.pathname === '/error') { res.writeHead(500,{'Content-Type':'text/html'}).end('<title>Error fixture</title><h1>Failure</h1>'); return; }
  if (!['/light','/demanding'].includes(url.pathname)) { res.writeHead(404).end(); return; }
  res.setHeader('Content-Security-Policy',"default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; connect-src 'self'");
  res.setHeader('Content-Type','text/html;charset=utf-8');
  const count = url.pathname === '/demanding' ? 4000 : 24;
  res.end(`<!doctype html><html><head><title>Bowser performance fixture ${token}</title><style>body{font:16px system-ui;background:#edf2f8;color:#123;margin:32px}main{max-width:1000px}article{display:inline-block;border:1px solid #abc;padding:8px;margin:2px;width:130px}input{font:inherit}</style></head><body><main><h1>Production performance fixture</h1><p id="run-token">Loading ${token}</p><input aria-label="Fixture input"><section>${Array.from({length:count},(_,i)=>`<article>Row ${i}<b> Synthetic</b></article>`).join('')}</section></main><script>addEventListener('load',()=>requestAnimationFrame(()=>requestAnimationFrame(()=>{document.getElementById('run-token').textContent='Ready ${token}';document.title='BOWSER_PERF_READY_${token}';fetch('/ready?token=${token}',{cache:'no-store'});})));</script></body></html>`);
});
server.listen(0,'127.0.0.1',()=>fs.writeFileSync(portFile,String(server.address().port)));
