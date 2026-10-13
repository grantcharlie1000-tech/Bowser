// Tiny local fixture for the per-site preferences check: a page that shows its own zoom in the
// document title and asks for notifications, location and a script popup (all of which Bowser
// must refuse unless the site is allowed). Prints the port, serves until killed.
// Usage: node prefs-fixture-server.mjs [port]
import { createServer } from 'node:http';

const page = `<!doctype html><meta charset="utf-8"><title>Prefs fixture</title>
<style>body{font:16px Segoe UI,sans-serif;margin:32px}button{font:inherit;padding:6px 12px}</style>
<h1>Prefs fixture</h1>
<p>This page reports its zoom in the window title and asks for permissions on load.</p>
<p><a href="/popup" target="_blank">Link that opens a new tab (user click)</a></p>
<p><button id="pop">window.open from a click</button></p>
<script>
const base = window.devicePixelRatio;
const show = () => { document.title = 'dpr=' + window.devicePixelRatio.toFixed(3) + ' w=' + window.innerWidth; };
show(); addEventListener('resize', show);
document.getElementById('pop').onclick = () => window.open('/popup');
setTimeout(() => { try { Notification.requestPermission(); } catch {} }, 400);
setTimeout(() => { try { navigator.geolocation.getCurrentPosition(() => {}, () => {}); } catch {} }, 700);
setTimeout(() => { try { window.open('/popup'); } catch {} }, 1000);
</script>`;

const popup = '<!doctype html><meta charset="utf-8"><title>Popup tab</title><h1>Popup tab</h1><p>Opened by window.open.</p>';

const server = createServer((req, res) => {
  res.setHeader('Content-Type', 'text/html; charset=utf-8');
  res.setHeader('Cache-Control', 'no-store');
  res.end(req.url.startsWith('/popup') ? popup : page);
});
server.listen(Number(process.argv[2] ?? 0), '127.0.0.1', () => {
  console.log(server.address().port);
});
