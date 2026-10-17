// Stand-in for the WebView2 bridge for the address bar thread's screenshots (benchmarks/results/t2).
// Loaded before app.js; ?scn= picks the messages replayed when the page says `ready`. The Rust
// fixtures (ui/test/fixtures/address-messages.json, settings-messages.json) are inlined by render.mjs
// as `window.__fixtures`, so the pictures show exactly what Rust sends.
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
  const scn = params.get('scn') || 'toolbar';
  const dark = params.get('scheme') !== 'light';
  const width = Number(params.get('w') || 1280);
  const fx = window.__fixtures;
  document.documentElement.classList.add(dark ? 'h-dark' : 'h-light');
  const state = (extra = {}) => ({
    t: 'state', scale: 1, solid: dark, dark, back: true, forward: false, loading: false, progress: 0,
    security: 'secure', label: 'Connection is secure', theme: dark ? DARK : LIGHT, ...extra,
  });
  const pillw = Math.min(624, width - 260);
  const pillx = Math.round((width - pillw) / 2);
  const placed = { pillx, pillw, menux: pillx + pillw + 3 };
  const addr = (extra = {}) => ({ ...fx.address.addr, ...extra });
  const rows = [
    { k: 'search', text: '2+2*3', title: '2+2*3', url: '' },
    { k: 'calc', text: '8', title: '= 8', url: '2+2*3', bd: true },
    { k: 'bm', text: 'rust-lang.org/', title: 'Rust Language', url: 'rust-lang.org/', bd: true },
    { k: 'tab', text: 'docs.rs/', title: 'Docs.rs', url: 'docs.rs/', bd: true },
    { k: 'hist', text: 'news.example/', title: 'The Daily Paper', url: 'news.example/', bd: true },
  ];
  const scenarios = {
    toolbar: [state(placed), { t: 'setText', text: 'https://www.example.co.uk/path/page', select: false }],
    'toolbar-search': [state(placed), addr({ hbtn: true, qr: true, highlight: true, base: 'example.co.uk' }),
      { ...fx.address.sfield, mode: 'field', engine: 1 }, { t: 'setText', text: 'https://mail.example.co.uk/path/page', select: false }],
    'toolbar-button': [state(placed), addr({ hbtn: true }), { ...fx.address.sfield, mode: 'button', engine: 0 },
      { t: 'setText', text: 'https://www.example.co.uk/', select: false }],
    'toolbar-hidden': [state(placed), addr({ show: false }), { t: 'setText', text: 'https://www.example.co.uk/', select: false }],
    hover: [state(), { v: 1, t: 'suggest', tok: 5, seq: 1, w: 560, sel: 1, q: '2+2*3', items: rows }],
    settings: [state(), { ...fx.settings.snap }, fx.address.snap, { v: 1, t: 'settingsgo', s: params.get('s') || 'address' }],
    qr: [state(), fx.address.qr],
    bookmark: [state(), fx.address.bookmark],
    bar: [state(placed), fx.address.bar],
    'bar-icons': [state(placed), { ...fx.address.bar, d: { ...fx.address.bar.d, display: 'icon' } }],
  };
  const listeners = [];
  const deliver = () => {
    for (const msg of scenarios[scn] || []) {
      const data = JSON.stringify(msg);
      for (const cb of listeners) cb({ data });
    }
  };
  let delivered = false;
  window.chrome = {
    webview: {
      postMessage(wire) {
        if (delivered) return;
        try { if (JSON.parse(wire).t !== 'ready') return; } catch { return; }
        delivered = true;
        setTimeout(deliver, 0);
      },
      addEventListener(type, cb) { if (type === 'message') listeners.push(cb); },
    },
  };
  if (scn === 'hover') window.addEventListener('load', () => { if (!delivered) { delivered = true; deliver(); } });
})();
