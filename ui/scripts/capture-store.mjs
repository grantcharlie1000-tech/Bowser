// Real-browser UI journey and screenshots. This simulates the native message boundary only;
// it is NOT proof of WebView2 routing, real CRX download, verification or installation.
// Run: node scripts/capture-store.mjs [output-directory]
// Uses standard BOWSER_TEST_EDGE override / installed Windows Edge detection.
import { spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { buildSync } from 'esbuild';
const here = dirname(fileURLToPath(import.meta.url));
export const STORE_TEST_EDGE = [process.env.BOWSER_TEST_EDGE, 'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe', 'C:/Program Files/Microsoft/Edge/Application/msedge.exe'].find((path) => path && existsSync(path));
export function captureStore(output, edge = STORE_TEST_EDGE) {
  if (!edge) throw new Error('Microsoft Edge is not installed. Set BOWSER_TEST_EDGE to its executable. No native-browser verification was performed.');
  mkdirSync(output, { recursive: true });
  const work = mkdtempSync(join(tmpdir(), 'bowser-store-browser-'));
  writeFileSync(join(work, 'app.css'), readFileSync(join(here, '..', 'dist', 'app.css')));
  const driver = `
import { mountStoreSection } from ${JSON.stringify(join(here, '..', 'src', 'store-section.ts'))};
const results = { errors: [], commands: [] };
const expect = (v, text) => { if (!v) results.errors.push(text); };
const entry = { id: 'a'.repeat(32), name: 'Reading companion', description: 'A focused reading experience for the web. QA fixture, not a real catalogue offer.', publisher: 'Example publisher', category: 'Reading', source: 'https://chromewebstore.google.com/detail/' + 'a'.repeat(32), homepage: 'https://example.com', provenance: 'Illustrative browser-test fixture only.', checked: '2026-10-18', compatibility: 'untested', compatibilityNote: 'Untested. No version-specific compatibility claim.' };
const base = { shown: true, catalogue: [entry, {...entry, id:'b'.repeat(32), name:'Colour toolkit', category:'Developer tools'}], phase:'idle', requestId:'', extensionId:entry.id, name:'', version:'', error:'', restart:false, permissions:[], hostPermissions:[], contentScripts:[], optionalPermissions:[], optionalHostPermissions:[] };
const review = { phase:'review', requestId:'qa-1', name:entry.name, version:'1.2.3', permissions:Array.from({length:20},(_,i)=>'permission-'+i), hostPermissions:['<all_urls>'], contentScripts:['https://example.com/articles/*'], optionalPermissions:['downloads'], optionalHostPermissions:['https://optional.example/*'] };
const sendSnap = patch => store.receive({t:'snap',p:'store',d:{...base,...patch}});
const store = mountStoreSection({send: fields => { results.commands.push(fields); return true; }, toast:()=>{}});
document.querySelector('#app').append(store.root);
const button = text => Array.from(store.root.querySelectorAll('button')).find(x=>x.textContent===text);
try {
  sendSnap({});
  expect(document.activeElement.textContent==='Bowser Store','store entry focus');
  button('Get extension').click();
  expect(results.commands.length===1 && results.commands[0].a==='storeinstall','request before consent');
  sendSnap({phase:'downloading',requestId:'qa-1'});
  sendSnap(review);
  expect(results.commands.length===1,'review never auto-approves');
  expect(store.root.textContent.includes('permission-19'),'review never truncates');
  button('Install Reading companion 1.2.3').click();
  expect(results.commands[1].a==='storeapprove' && results.commands[1].text==='qa-1','approval bound to request');
  sendSnap({phase:'installing',requestId:'qa-1'});
  sendSnap({phase:'ready',restart:true});
  expect(store.root.textContent.includes('Restart Bowser'),'restart notice');
  sendSnap({phase:'downloading',requestId:'qa-2'});
  button('Cancel installation').click();
  expect(results.commands.at(-1).a==='storecancel' && results.commands.at(-1).text==='qa-2','cancel bound to request');
  sendSnap({phase:'cancelled'});
  sendSnap(review);
  store.receive({t:'snap',p:'store',d:{...base,phase:'review',permissions:[42]}});
  button('Discover').click();
  expect(!button('Install Reading companion 1.2.3'),'invalid replacement removes approval');
  const query=new URLSearchParams(location.search);
  document.documentElement.dataset.scheme=query.get('theme') || 'dark';
  const phase=query.get('phase');
  sendSnap(phase==='review'?review:phase==='ready'?{phase:'ready',restart:true}:phase==='error'?{phase:'error',error:'Package signature could not be verified. No extension was installed.'}:{});
  if(phase==='detail') button('Reading companion').click();
  if(phase==='categories') button('Categories').click();
  expect(document.documentElement.scrollWidth<=innerWidth,'no horizontal page overflow');
  for(const element of store.root.querySelectorAll('button,input')) {
    if(element.getClientRects().length && !element.disabled) expect(element.getAttribute('aria-label') || element.textContent || element.type==='search','control label');
  }
} catch(error) { results.errors.push(String(error.stack || error)); }
const report=document.createElement('meta');report.name='bowser-store-report';report.content=encodeURIComponent(JSON.stringify(results));document.head.append(report);
`;
  buildSync({ stdin: { contents: driver, resolveDir: here, sourcefile: 'store-browser-driver.ts', loader: 'ts' }, bundle: true, format: 'iife', outfile: join(work, 'driver.js') });
  writeFileSync(join(work, 'index.html'), '<!doctype html><html data-scheme="dark"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><meta http-equiv="Content-Security-Policy" content="default-src \'none\'; script-src \'self\'; style-src \'self\'; img-src \'none\'"><link rel="stylesheet" href="app.css"></head><body><div id="app" class="sp-root"></div><script src="driver.js"></script></body></html>');
  const scenarios = [['desktop-dark','dark','discover',1280,900], ['desktop-light','light','discover',1280,900], ['mobile-light','light','discover',480,900], ['detail-light','light','detail',1280,900], ['categories-dark','dark','categories',1280,900], ['review-dark','dark','review',1280,1100], ['ready-light','light','ready',1280,900], ['error-dark','dark','error',1280,900]];
  for (const [name, theme, phase, width, height] of scenarios) {
    const page = pathToFileURL(join(work, 'index.html')); page.search = new URLSearchParams({theme,phase}).toString();
    const screenshot = resolve(output, `${name}.png`);
    const run = spawnSync(edge, ['--headless=new','--disable-gpu','--no-first-run','--no-default-browser-check',`--user-data-dir=${join(work,'profile-'+name)}`,`--window-size=${width},${height}`,'--force-device-scale-factor=1','--virtual-time-budget=2000',`--screenshot=${screenshot}`,'--dump-dom',page.href], { encoding: 'utf8', timeout: 30000, maxBuffer: 8*1024*1024 });
    if (run.error || run.status !== 0) throw new Error(`${name}: ${run.error || run.stderr || 'Edge failed'}`);
    const encoded = /name="bowser-store-report" content="([^"]+)"/.exec(run.stdout)?.[1];
    if (!encoded) throw new Error(`${name}: browser driver report missing`);
    const report = JSON.parse(decodeURIComponent(encoded));
    writeFileSync(resolve(output, `${name}.json`), JSON.stringify(report,null,2));
    if (report.errors.length || !existsSync(screenshot)) throw new Error(`${name}: ${report.errors.join('; ') || 'screenshot missing'}`);
  }
  return scenarios.length;
}
if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const output = resolve(process.argv[2] || mkdtempSync(join(tmpdir(), 'bowser-store-captures-')));
  const count = captureStore(output);
  console.log(`Store UI fake-host journey passed; ${count} screenshots saved to ${output}. Real WebView2 installation remains a separate native acceptance check.`);
}
