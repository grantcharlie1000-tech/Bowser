// Focused HTTP behavior checks. No browser, app, CIM, input or capture.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import http from 'node:http';
import os from 'node:os';
import path from 'node:path';
import { spawn } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const temp = fs.mkdtempSync(path.join(os.tmpdir(), 'bowser-fixture-outcomes-'));
const portFile = path.join(temp, 'port.txt');
const logFile = path.join(temp, 'requests.jsonl');
const serverPath = fileURLToPath(new URL('./production-fixture-server.mjs', import.meta.url));
const stderr = fs.openSync(path.join(temp, 'stderr.txt'), 'w');
const server = spawn(process.execPath, [serverPath, portFile, logFile], { windowsHide:true, stdio:['ignore','ignore',stderr] });
const pause = () => new Promise(resolve => setTimeout(resolve, 10));
const rows = () => fs.existsSync(logFile) ? fs.readFileSync(logFile, 'utf8').trim().split('\n').filter(Boolean).map(JSON.parse) : [];
async function until(check) {
  const deadline = Date.now() + 5000;
  while (!check()) { assert.ok(Date.now() < deadline, 'fixture condition deadline'); assert.equal(server.exitCode, null, 'owned server alive'); await pause(); }
}
try {
  await until(() => fs.existsSync(portFile));
  const port = Number(fs.readFileSync(portFile, 'utf8'));
  async function request(route, token) {
    return new Promise((resolve,reject) => {
      const req = http.get({ host:'127.0.0.1', port, path:`${route}?token=${token}`, agent:false }, res => {
        res.resume(); res.once('end', () => resolve(res.statusCode));
      });
      req.once('error', reject); req.setTimeout(5000, () => req.destroy(new Error('request deadline')));
    });
  }
  const routes = [['/light',200], ['/demanding',200], ['/ready',204], ['/redirect',302], ['/unknown',404], ['/error',500]];
  for (const [index,[route,status]] of routes.entries()) {
    const token = `route_${index}`;
    assert.equal(await request(route,token), status);
    await until(() => rows().some(r => r.token === token && r.event === 'response_finish'));
    const observed = rows().filter(r => r.token === token);
    const arrival = observed.filter(r => r.event === 'request_arrival');
    const finish = observed.filter(r => r.event === 'response_finish');
    assert.equal(arrival.length,1); assert.equal(arrival[0].path,route); assert.equal(arrival[0].status,null);
    assert.equal(finish.length,1); assert.equal(finish[0].request_path,route); assert.equal(finish[0].status,status);
    assert.equal(finish[0].request_id,arrival[0].request_id);
    assert.ok(BigInt(finish[0].at_ns) >= BigInt(arrival[0].at_ns));
    assert.equal(observed.filter(r => r.path === route).length,1, 'arrival consumers still see one request');
  }
  await assert.rejects(request('/network-failure','destroyed'));
  await until(() => rows().some(r => r.token === 'destroyed' && r.event === 'response_abort'));
  const hung = http.get({ host:'127.0.0.1', port, path:'/hung?token=hung', agent:false });
  hung.on('error', () => {});
  await until(() => rows().some(r => r.token === 'hung' && r.event === 'request_arrival'));
  assert.equal(rows().filter(r => r.token === 'hung' && r.event !== 'request_arrival').length,0, 'no outcome invented for an unfinished response');
  hung.destroy();
  await until(() => rows().some(r => r.token === 'hung' && r.event === 'response_abort'));
  for (const token of ['destroyed','hung']) {
    const observed = rows().filter(r => r.token === token);
    assert.equal(observed.filter(r => r.event === 'response_finish').length,0);
    const aborted = observed.filter(r => r.event === 'response_abort');
    assert.equal(aborted.length,1); assert.equal(aborted[0].status,null);
  }
  assert.deepEqual(rows().filter(r => r.event === 'response_finish' && r.status === 200).map(r => r.request_path).sort(), ['/demanding','/light']);
  console.log(`8 route outcomes passed: 200/204/302/404/500, destroyed and unfinished/aborted sockets; logs retained at ${temp}`);
} finally {
  const exited = new Promise(resolve => server.once('exit', resolve));
  if (server.exitCode === null) { server.kill(); await exited; }
  fs.closeSync(stderr);
}
