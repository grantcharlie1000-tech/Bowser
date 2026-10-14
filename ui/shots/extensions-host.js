// Stand-in for the WebView2 host bridge for the extensions shots (extensions-render.mjs). Loaded
// before app.js; picks a scenario from ?scn= and replays Bowser's messages once the page says
// `ready` (the popup view gets its menu on load). Nothing here talks to a real Bowser profile.
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
  const scn = params.get('scn') || 'list';
  const dark = params.get('scheme') !== 'light';
  const width = Number(params.get('w') || 1280);

  const state = (extra = {}) => ({
    t: 'state', scale: 1, solid: true, dark, back: true, forward: false, loading: false, progress: 0,
    security: 'secure', label: 'Connection is secure', theme: dark ? DARK : LIGHT, ...extra,
  });
  // Pill and menu placement roughly as Rust computes it for this width, less the two button slots.
  const slots = scn === 'toolbar-both' ? 64 : 32;
  const pillFull = Math.min(624, width - 260);
  const pillx = Math.round((width - pillFull) / 2);
  const placed = { pillx, pillw: pillFull - slots, menux: pillx + pillFull + 3 };

  const list = (items) => [...items];
  const item = (id, name, version, extra = {}) => ({
    id, name, version, enabled: true, popup: false, failure: '', readable: true, all: false,
    sites: [], sitesN: 0, scripts: [], scriptsN: 0, perms: [], permsN: 0, optional: [], optionalN: 0, ...extra,
  });
  const items = list([
    item('ext-3f2a9c41d07be655', 'Dark Reader', '4.9.96', {
      popup: true, all: true,
      scripts: ['<all_urls>'], scriptsN: 1,
      perms: ['alarms', 'contextMenus', 'storage', 'tabs'], permsN: 4,
    }),
    item('ext-a81c55e0f9d2b304', 'Page Ruler', '2.1', {
      popup: true,
      sites: ['https://*.figma.com/*', 'https://docs.example.com/*'], sitesN: 2,
      perms: ['activeTab', 'scripting', 'storage'], permsN: 3,
    }),
    item('ext-07d4e2b9c6a1f830', 'Tab Notes', '0.4.0', {
      enabled: false,
      perms: ['storage'], permsN: 1, optional: ['downloads'], optionalN: 1,
    }),
    item('ext-c90b1e7a2d4f6538', 'Reading Time', '1.0.3', {
      popup: true,
      failure: 'WebView2 refused to load it: the manifest declares a background page, which this runtime does not run.',
      scripts: ['https://*.medium.com/*', 'https://news.example/*'], scriptsN: 2,
    }),
  ]);
  const ext = (extra = {}) => ({ v: 1, t: 'snap', p: 'extensions', d: { state: 'on', busy: '', ack: 'none', error: '', items, ...extra } });
  const settingsSnap = window.__settingsSnap; // set by extensions-settings-snap.js from the Rust fixture
  const go = { v: 1, t: 'settingsgo', s: 'extensions' };
  const menu = {
    v: 1, t: 'menu', tok: 7, kind: 'extensions-button', entries: [
      { id: 100, label: 'Dark Reader', enabled: true, checked: false, icon: 'puzzle' },
      { id: 101, label: 'Page Ruler', enabled: true, checked: false, icon: 'puzzle' },
      { separator: true },
      { id: 1, label: 'Manage extensions', enabled: true, checked: false, icon: 'gear' },
    ],
  };

  const page = [state(), settingsSnap, go];
  const scenarios = {
    list: [...page, ext()],
    details: [...page, ext()],
    confirm: [...page, ext()],
    busy: [...page, ext({ busy: 'ext-a81c55e0f9d2b304' })],
    empty: [...page, ext({ items: [] })],
    error: [...page, ext({ state: 'error', items: [], error: 'The installed extension list is damaged.' })],
    loading: page,
    'toolbar-one': [state(placed), { v: 1, t: 'setText', text: 'https://example.com/docs', select: false }, { v: 1, t: 'extbtn', on: true, n: 1, name: 'Dark Reader' }],
    'toolbar-both': [state(placed), { v: 1, t: 'setText', text: 'https://example.com/docs', select: false }, { v: 1, t: 'extbtn', on: true, n: 2, name: '' },
      { v: 1, t: 'dlbtn', on: true, state: 'done', n: 0, pct: -1, burst: 0 }],
    menu: [state(), menu],
  };

  const listeners = [];
  const deliver = () => {
    for (const msg of scenarios[scn] || []) {
      const data = JSON.stringify(msg);
      for (const cb of listeners) cb({ data });
    }
    window.__delivered = true;
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
  if (scn === 'menu') window.addEventListener('load', () => { if (!delivered) { delivered = true; deliver(); } });
  document.documentElement.dataset.scn = scn;
})();
