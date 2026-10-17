// Stand-in for the WebView2 host bridge, for rendering the split host (start page) and the settings page
// from a ui/dist folder in headless Edge. render.mjs writes scenarios.js (window.__scn) before this runs.
(() => {
  'use strict';
  const DARK = {
    bg: 'rgba(32,36,44,0.800)', fill: 'rgba(255,255,255,0.110)', border: 'rgba(95,216,219,0.282)',
    focusBorder: '#b3c9ce', text: '#ffffff', placeholder: '#dde2e9', completion: '#dde2e9',
    selection: 'rgba(77,163,255,0.251)', hover: 'rgba(255,255,255,0.078)', progress: '#84c8ff',
  };
  const LIGHT = {
    bg: 'rgba(244,246,248,0.800)', fill: 'rgba(255,255,255,0.314)', border: 'rgba(22,143,152,0.282)',
    focusBorder: '#4a6d73', text: '#14181d', placeholder: '#4b5159', completion: '#50575f',
    selection: 'rgba(0,103,192,0.251)', hover: 'rgba(255,255,255,0.314)', progress: '#0067c0',
  };
  const params = new URLSearchParams(location.search);
  const scn = params.get('scn') || 'start';
  const dark = params.get('scheme') !== 'light';
  document.documentElement.classList.add(dark ? 'h-dark' : 'h-light');
  const theme = dark ? DARK : LIGHT;

  // A gradient picture as a JPEG data URI, standing in for an image the user chose.
  const picture = (hue) => {
    const c = document.createElement('canvas');
    c.width = 320; c.height = 200;
    const g = c.getContext('2d');
    const grad = g.createLinearGradient(0, 0, 320, 200);
    grad.addColorStop(0, `hsl(${hue},70%,55%)`);
    grad.addColorStop(1, `hsl(${(hue + 60) % 360},65%,35%)`);
    g.fillStyle = grad; g.fillRect(0, 0, 320, 200);
    g.fillStyle = 'rgba(255,255,255,0.25)'; g.beginPath(); g.arc(240, 60, 38, 0, 7); g.fill();
    return c.toDataURL('image/jpeg', 0.8);
  };
  // A 32px PNG favicon (a coloured rounded square with the host's first letter), like the cached ones Rust sends.
  const favicon = (host) => {
    const c = document.createElement('canvas');
    c.width = 32; c.height = 32;
    const g = c.getContext('2d');
    let hue = 0;
    for (const ch of host) hue = (hue * 31 + ch.charCodeAt(0)) % 360;
    g.fillStyle = `hsl(${hue},60%,50%)`;
    g.beginPath(); g.roundRect(2, 2, 28, 28, 7); g.fill();
    g.fillStyle = '#fff'; g.font = 'bold 18px sans-serif'; g.textAlign = 'center'; g.textBaseline = 'middle';
    g.fillText(host.replace(/^www\./, '')[0].toUpperCase(), 16, 17);
    return c.toDataURL('image/png');
  };
  const build = (raw) => {
    if (raw && raw.t === 'state') return { ...raw, dark, theme };
    if (raw && raw.p === 'startpage' && raw.d && raw.d.favs) {
      const hosts = new Set(['docs.rs', 'www.rust-lang.org', 'chess.example']);
      return { ...raw, d: { ...raw.d, favs: Object.fromEntries([...hosts].map((h) => [h, favicon(h)])) } };
    }
    if (raw && raw.__thumbs) return { v: 1, t: 'snap', p: 'startpage', d: { kind: 'thumbs', thumbs: Object.fromEntries(raw.__thumbs.map((id, i) => [String(id), picture(40 + i * 90)])) } };
    return raw;
  };

  const listeners = [];
  const emit = (msg) => { const data = JSON.stringify(msg); for (const cb of listeners) cb({ data }); };
  const marks = { delivered: 0, built: 0, rendered: 0 };
  const frames = (cb) => requestAnimationFrame(() => requestAnimationFrame(cb));
  // The page is built when the start page's grid or empty state is in the document (or at once for a blank
  // tab, which has nothing to build). Measured from the host's first message, on the page's own clock.
  const whenBuilt = (cb) => {
    const done = () => document.querySelector('.sp-grid:not([hidden]), .sp-empty:not([hidden]), .sp-nav-list') || scn === 'newtab';
    if (done()) { cb(); return; }
    const observer = new MutationObserver(() => { if (done()) { observer.disconnect(); cb(); } });
    observer.observe(document.documentElement, { childList: true, subtree: true });
  };
  const deliver = () => {
    marks.delivered = performance.now();
    whenBuilt(() => { marks.built = performance.now(); frames(() => { marks.rendered = performance.now(); window.__marks = marks; }); });
    for (const raw of (window.__scn || {})[scn] || []) emit(build(raw));
  };
  // A second blank tab in the same window: the host hides the page, then shows it again.
  window.__redeliver = () => new Promise((resolve) => {
    emit({ v: 1, t: 'splithide' });
    const t0 = performance.now();
    for (const raw of ((window.__scn || {})[scn] || []).slice(1)) emit(build(raw));
    // The page is rebuilt synchronously (its code is already loaded): measure to the end of this task.
    Promise.resolve().then(() => resolve(performance.now() - t0));
  });
  window.__sent = [];
  let delivered = false;
  window.chrome = {
    webview: {
      postMessage(wire) {
        let t = '';
        try { t = JSON.parse(wire).t; } catch { return; }
        window.__sent.push(wire);
        if (delivered || t !== 'ready') return;
        delivered = true;
        setTimeout(deliver, 0);
      },
      addEventListener(type, cb) { if (type === 'message') listeners.push(cb); },
    },
  };
})();
