// Stand-in for the WebView2 host bridge, for rendering ui/dist views in a plain browser.
// Loaded before app.js by render.mjs; picks a scenario from ?scn= and replays its host messages
// when the page says `ready` (popup views get theirs straight away).
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
  document.documentElement.classList.add(dark ? 'h-dark' : 'h-light');

  const state = (extra = {}) => ({
    t: 'state', scale: 1, solid: false, dark, back: true, forward: false, loading: false, progress: 0,
    security: 'secure', label: 'Connection is secure', theme: dark ? DARK : LIGHT, ...extra,
  });
  // Pill and menu placement roughly as Rust computes it for this width.
  const pillw = Math.min(624, width - 260);
  const pillx = Math.round((width - pillw) / 2);
  const placed = { pillx, pillw, menux: pillx + pillw + 3 };

  const tab = (id, title, extra = {}) => ({
    id, title, loading: false, pinned: false, muted: false, marker: -1, g: 0, gl: false, fav: false, ...extra,
  });
  const tabs = {
    v: 1, t: 'tabs', active: 3, items: [
      tab(1, 'Pinned', { pinned: true }),
      tab(2, 'Group lead tab', { g: 9, gl: true, gname: 'Research', gcol: 5, gcollapsed: false, gsize: 2 }),
      tab(3, 'Active tab with a long title that ellipsises', { g: 9 }),
      tab(4, 'Loading tab', { loading: true }),
      tab(5, 'Muted with marker', { muted: true, marker: 3 }),
      tab(6, 'Plain tab'),
    ],
  };
  const railItem = (k, icon, label, extra = {}) => ({ k, icon, label, active: false, dot: false, accent: '', fav: false, ...extra });
  const rail = {
    v: 1, t: 'rail', items: [
      railItem('home', 'home', 'Home'), railItem('customize', 'customize', 'Customize'),
      railItem('ws:1', 'g:briefcase', 'Work', { active: true, accent: '#e5484d' }),
      railItem('ws:2', 'g:code', 'Code'), railItem('ws:3', 'g:globe', 'Travel', { accent: '#4fb8b0' }),
      railItem('addws', 'plus', 'New workspace'),
      railItem('app:1', 'letter', 'Mail', { dot: true }), railItem('app:2', 'apptile', 'Apps'),
      railItem('addapp', 'plus', 'Add app'),
      railItem('bookmarks', 'heart', 'Bookmarks'), railItem('history', 'clock', 'History'),
      railItem('menu', 'dots', 'Menu'),
    ],
  };
  const menu = {
    v: 1, t: 'menu', tok: 7, kind: 'Main menu', entries: [
      { id: 1, label: 'New tab', enabled: true, checked: false, icon: 'plus', shortcut: 'Ctrl+T' },
      { id: 2, label: 'New window', enabled: true, checked: false, icon: 'grid', shortcut: 'Ctrl+N' },
      { separator: true },
      { section: 'Profile' },
      { id: 3, label: 'Passwords', enabled: true, checked: false, icon: 'key' },
      { id: 4, label: 'Privacy', enabled: true, checked: true, icon: 'shield' },
      { id: 5, label: 'More tools', enabled: true, checked: false, icon: 'gear', submenu: [
        { id: 6, label: 'Nested', enabled: true, checked: false, icon: 'close' }] },
      { id: 7, label: 'Disabled item', enabled: false, checked: false, icon: 'lock' },
    ],
  };

  const scenarios = {
    toolbar: [state(placed), { t: 'setText', text: 'https://example.com/path', select: false }],
    'toolbar-flex': [state({ back: false, loading: true, progress: 40, security: 'notsecure' })],
    tabs: [state(), tabs],
    sidebar: [state(), { v: 1, t: 'layout', mode: 'vertical' },
      { v: 1, t: 'workspace', name: 'Work & Play', icon: 'g:code' }, tabs],
    rail: [state(), rail],
    popup: [state(), menu],
    panel: [state(), { v: 1, t: 'panelshow', p: 'history' }, { v: 1, t: 'snap', p: 'history', d: { items: [], count: 0 } }],
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
  // The popup view never sends ready before it has a menu; deliver after load.
  if (scn === 'popup') window.addEventListener('load', () => { if (!delivered) { delivered = true; deliver(); } });
})();
