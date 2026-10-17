// Stand-in for the WebView2 host bridge for the privacy shots (t3-render.mjs). Loaded before
// app.js; picks a scenario from ?scn= and replays Bowser's messages once the page says `ready`.
// ?mode=before replays what origin/main sent (no `protection` snapshot, the old site information
// page), so the same scenario can be shot on the old and the new build. Nothing here talks to a
// real Bowser profile.
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
  const scn = params.get('scn') || 'privacy';
  const before = params.get('mode') === 'before';
  const dark = params.get('scheme') !== 'light';
  const data = window.__t3;

  const state = () => ({
    t: 'state', scale: 1, solid: true, dark, back: true, forward: false, loading: false, progress: 0,
    security: 'secure', label: 'Connection is secure', theme: dark ? DARK : LIGHT,
  });
  const section = scn.startsWith('appearance') ? 'appearance' : 'privacy';
  const page = [state(), data.settings, { v: 1, t: 'settingsgo', s: section }, data.privacy];
  if (!before) page.push(data.protection);
  const siteinfo = before ? data.siteinfoBefore : data.siteinfo;
  const scenarios = {
    privacy: page,
    'privacy-sites': page,
    'privacy-lists': page,
    'privacy-delete': page,
    'privacy-delete-armed': page,
    appearance: page,
    shield: [state(), siteinfo],
    'shield-private': [state(), { ...siteinfo, private: true, perms: [], reset: false }],
  };

  const listeners = [];
  const deliver = () => {
    for (const msg of scenarios[scn] || []) {
      const wire = JSON.stringify(msg);
      for (const cb of listeners) cb({ data: wire });
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
  if (scn.startsWith('shield')) window.addEventListener('load', () => { if (!delivered) { delivered = true; deliver(); } });
  document.documentElement.dataset.scn = scn;
})();
