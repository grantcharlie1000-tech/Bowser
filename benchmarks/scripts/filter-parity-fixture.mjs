// Independent ordinary-app dispatch checks. No benchmark flags, host overrides or custom rules.
import fs from 'node:fs';
const worker = String.raw`
const run = async () => {
 const token = new URL(location.href).searchParams.get('token');
 const checks = await Promise.all([['/prebid.js',403],['/parity-allowed.js',200]].map(async ([path,expected]) => {
  const response = await fetch(path+'?token='+token,{cache:'no-store'});
  return {name:'worker-fetch-'+path,actual:response.status,expected,pass:response.status===expected};
 }));
 return {token,checks};
};
`;
const page = String.raw`<!doctype html><meta charset="utf-8"><title>Native filter parity</title><h1>Native request-dispatch parity</h1><pre id="result">Running</pre><script>
(async()=>{
 const token=new URL(location.href).searchParams.get('token'); const checks=[];
 const check=(name,actual,expected)=>checks.push({name,actual,expected,pass:actual===expected});
 const asset=(path,kind='')=>path+(path.includes('?')?'&':'?')+'token='+token+(kind?'&kind='+kind:'');
 const bounded=(promise)=>Promise.race([promise,new Promise((_,reject)=>setTimeout(()=>reject(Error('fixture operation timeout')),6000))]);
 try {
  for(const [path,expected] of [['/prebid.js',403],['/PrEbId.Js',403],['/adframe.js',403],['/pagead/ads?x=1',403],['/parity-allowed.js',200],['/robots.txt',200],['/favicon.ico',200],['/prebidXjs',200]]) {
   check('fetch-'+path,(await bounded(fetch(asset(path),{cache:'no-store'}))).status,expected);
  }
  for(const [path,expected] of [['/prebid.js',403],['/parity-allowed.js',200]]) {
   const status=await bounded(new Promise((resolve,reject)=>{const x=new XMLHttpRequest();x.open('GET',asset(path));x.onload=()=>resolve(x.status);x.onerror=()=>reject(Error('XHR network failure'));x.send();}));
   check('xhr-'+path,status,expected);
  }
  for(const kind of ['script','image']) for(const [path,expected] of [['/prebid.js',false],[kind==='script'?'/parity-allowed.js':'/parity-image.svg',true]]) {
   const loaded=await bounded(new Promise((resolve)=>{const e=document.createElement(kind==='script'?'script':'img');e.onload=()=>{e.remove();resolve(true)};e.onerror=()=>{e.remove();resolve(false)};e.src=asset(path,kind);document.body.append(e);}));
   check(kind+'-'+path,loaded,expected);
  }
  const consume=(kind,result)=>{check(kind+'-token',result.token,token);for(const item of result.checks) checks.push({...item,name:kind+'-'+item.name});};
  const dedicated=new Worker(asset('/parity-worker.js')+'&mode=dedicated');
  try { consume('dedicated',await bounded(new Promise((resolve,reject)=>{dedicated.onmessage=e=>resolve(e.data);dedicated.onerror=()=>reject(Error('dedicated worker failure'));dedicated.postMessage('run');}))); } finally {dedicated.terminate();}
  const shared=new SharedWorker(asset('/parity-worker.js')+'&mode=shared');
  try {consume('shared',await bounded(new Promise((resolve,reject)=>{shared.port.onmessage=e=>resolve(e.data);shared.onerror=()=>reject(Error('shared worker failure'));shared.port.start();shared.port.postMessage('run');})));} finally {shared.port.close();}
  const registration=await bounded(navigator.serviceWorker.register(asset('/parity-worker.js')+'&mode=service',{scope:'/'}));
  try {
   const active=await bounded(navigator.serviceWorker.ready);
   consume('service',await bounded(new Promise((resolve)=>{const channel=new MessageChannel();channel.port1.onmessage=e=>{channel.port1.close();resolve(e.data)};active.active.postMessage('run',[channel.port2]);})));
  } finally {await registration.unregister();}
 } catch(error) {checks.push({name:'fixture-error',actual:String(error),expected:'no error',pass:false});}
 const passed=checks.length===23&&checks.every(c=>c.pass);
 document.getElementById('result').textContent=JSON.stringify({passed,checks},null,2);
 await fetch('/parity-report?token='+token,{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({passed,checks})});
 if(passed) requestAnimationFrame(()=>requestAnimationFrame(()=>{document.title='BOWSER_PERF_READY_'+token;fetch('/ready?token='+token,{cache:'no-store'});}));
})();
</script>`;
export function serveFilterParity(req,res,url,token,logFile) {
 if(url.pathname==='/filter-parity') {
  res.writeHead(200,{'Content-Type':'text/html;charset=utf-8','Content-Security-Policy':"default-src 'self'; script-src 'self' 'unsafe-inline'; object-src 'none'"}).end(page);return true;
 }
 if(url.pathname==='/parity-report') {
  let body='';req.on('data',chunk=>{body+=chunk;if(body.length>16000)req.destroy();});req.on('end',()=>{
   try {const result=JSON.parse(body);if(!Array.isArray(result.checks)||result.checks.length>40)throw Error('invalid report');fs.appendFileSync(logFile,JSON.stringify({path:'/parity-receipt',token,result})+'\n');res.writeHead(204).end();}
   catch {res.writeHead(400).end();}
  });return true;
 }
 if(url.pathname==='/parity-worker.js') {
  const suffix=url.searchParams.get('mode')==='service'?`addEventListener('install',e=>e.waitUntil(skipWaiting()));addEventListener('activate',e=>e.waitUntil(clients.claim()));addEventListener('message',e=>e.waitUntil(run().then(result=>e.ports[0].postMessage(result))));`:url.searchParams.get('mode')==='shared'?`onconnect=e=>{const port=e.ports[0];port.onmessage=()=>run().then(result=>port.postMessage(result));port.start();};`:`onmessage=()=>run().then(result=>postMessage(result));`;
  res.writeHead(200,{'Content-Type':'application/javascript'}).end(worker+suffix);return true;
 }
 if(['/prebid.js','/PrEbId.Js','/adframe.js','/pagead/ads','/parity-allowed.js','/robots.txt','/favicon.ico','/prebidXjs','/parity-image.svg'].includes(url.pathname)) {
  if(url.searchParams.get('kind')==='image'||url.pathname==='/parity-image.svg')res.writeHead(200,{'Content-Type':'image/svg+xml'}).end('<svg xmlns="http://www.w3.org/2000/svg" width="2" height="2"><rect width="2" height="2" fill="green"/></svg>');
  else res.writeHead(200,{'Content-Type':'application/javascript'}).end('void 0;');
  return true;
 }
 return false;
}
