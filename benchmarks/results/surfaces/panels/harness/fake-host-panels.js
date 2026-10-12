// Fake WebView2 host for the panel surfaces. ?scn=<panel>-<empty|full>&scheme=dark|light
// Replays state + panelshow + snap messages when the page says ready.
(() => {
  'use strict';
  const DARK = {
    bg: 'rgb(32,36,44)', fill: 'rgba(255,255,255,0.110)', border: 'rgba(95,216,219,0.282)',
    focusBorder: '#b3c9ce', text: '#ffffff', placeholder: '#dde2e9', completion: '#dde2e9',
    selection: 'rgba(77,163,255,0.251)', hover: 'rgba(255,255,255,0.078)', progress: '#84c8ff',
  };
  const LIGHT = {
    bg: 'rgb(244,246,248)', fill: 'rgba(255,255,255,0.7)', border: 'rgba(22,143,152,0.282)',
    focusBorder: '#4a6d73', text: '#14181d', placeholder: '#4b5159', completion: '#50575f',
    selection: 'rgba(0,103,192,0.251)', hover: 'rgba(0,0,0,0.06)', progress: '#0067c0',
  };
  const params = new URLSearchParams(location.search);
  const [panel, data] = (params.get('scn') || 'history-full').split('-');
  const dark = params.get('scheme') !== 'light';
  const full = data === 'full';
  document.documentElement.classList.add(dark ? 'h-dark' : 'h-light');
  if (params.get('reveal')) document.documentElement.classList.add('h-reveal');

  // Headless Edge enforces a minimum window width; clamp the page to the requested width so the
  // screenshot (cropped to the window) shows the real layout. Media queries still see the wider viewport.
  const want = Number(params.get('w') || 0);
  if (want > 0) {
    document.documentElement.style.setProperty('width', `${want}px`);
    document.documentElement.style.setProperty('overflow', 'hidden');
  }

  const state = { t: 'state', scale: 1, solid: true, dark, back: true, forward: false, loading: false, progress: 0,
    security: 'secure', label: '', theme: dark ? DARK : LIGHT };

  const long = 'An extremely long page title that keeps going past any sensible width so the ellipsis has to do its job';
  const sites = ['news.example.com', 'docs.example.org', 'github.com', 'en.wikipedia.org', 'mail.example.net', 'a-very-long-subdomain.example-company-website.co.uk'];
  const historyRows = [];
  let id = 1;
  for (const [label, n] of [['Today', 9], ['Yesterday', 7], ['Monday 5 October', 6], ['Sunday 4 October', 8]]) {
    historyRows.push({ label, count: n });
    for (let i = 0; i < n; i += 1) {
      const host = sites[(id + i) % sites.length];
      historyRows.push({ id: id++, title: i === 1 ? long : i === 4 ? '' : `Page ${id} on ${host.split('.')[0]}`, host, clock: `${String(9 + (i % 12)).padStart(2, '0')}:${String((i * 7) % 60).padStart(2, '0')}` });
    }
  }
  const bookmarkItems = [
    { id: 1, parent: null, title: 'Reading list', kind: 'folder' },
    { id: 2, parent: 1, title: 'Rust', kind: 'folder' },
    { id: 3, parent: 2, title: 'The Rust Programming Language', kind: 'link', url: 'https://doc.rust-lang.org/book/' },
    { id: 4, parent: 2, title: long, kind: 'link', url: 'https://example.com/a/very/long/path/that/goes/on' },
    { id: 5, parent: 1, title: 'Design notes', kind: 'link', url: 'https://example.org/design' },
    { id: 6, parent: null, title: 'Work', kind: 'folder' },
    { id: 7, parent: 6, title: 'Tracker', kind: 'link', url: 'https://tracker.example.com/board' },
    { id: 8, parent: null, title: 'Empty folder', kind: 'folder' },
    { id: 9, parent: null, title: 'Mail', kind: 'link', url: 'https://mail.example.net/' },
    { id: 10, parent: null, title: 'Weather', kind: 'link', url: 'https://weather.example.com/' },
    { id: 11, parent: null, title: 'Maps', kind: 'link', url: 'https://maps.example.com/' },
    ...Array.from({ length: 14 }, (_, i) => ({ id: 20 + i, parent: null, title: `Saved page ${i + 1}`, kind: 'link', url: `https://site${i}.example.com/` })),
  ];
  const noteBody = 'Line one of a longer note.\nLine two with more words so the preview has to truncate somewhere sensible.\n\nLine four.';
  const notes = {
    notes: [
      { id: 1, title: 'Meeting notes', body: noteBody },
      { id: 2, title: long, body: 'Short.' },
      { id: 3, title: 'Shopping', body: 'Milk\nEggs\nBread' },
      { id: 4, title: '', body: '' },
    ],
    tasks: [
      { id: 11, text: 'Reply to the design review', complete: false },
      { id: 12, text: long, complete: false },
      { id: 13, text: 'Book travel', complete: true },
      { id: 14, text: 'Update the changelog', complete: true },
      { id: 15, text: 'Water the plants', complete: false },
    ],
  };
  const settings = {
    tabLayout: 'vertical', sidebarWidth: 260, sleepEnabled: true, sleepAfterSecs: 300,
    blockerEnabled: true, blockerAllow: ['example.com', 'news.example.org'], searchEngine: 'optimised',
    searchFallback: 'default', automaticTabGrouping: false,
  };
  const privacy = {
    profile: 'Default', guest: false,
    stats: { allTime: 18423, session: 211, currentSite: 14,
      topSites: [{ host: 'ads.example.com', count: 6120 }, { host: 'track.example.net', count: 4210 }, { host: 'cdn.metrics.example.org', count: 2008 }],
      days: [{ day: '2026-10-12', count: 211 }, { day: '2026-10-11', count: 482 }, { day: '2026-10-10', count: 390 }] },
    site: 'example.com', security: 'secure', blocking: true, tracking: 'Balanced', smartScreen: true,
    blockerSetting: true, sleeping: true, sleepAfter: 300,
    permissions: [
      { origin: 'https://meet.example.com', permission: 'Camera', decision: 'allow' },
      { origin: 'https://maps.example.com', permission: 'Location', decision: 'block' },
      { origin: 'https://a-very-long-origin.example-company-website.co.uk', permission: 'Notifications', decision: 'block' },
    ],
    status: null,
  };
  const emptyPrivacy = { ...privacy, stats: { allTime: 0, session: 0, currentSite: 0, topSites: [], days: [] }, site: null, security: 'none', blocking: null, tracking: null, smartScreen: null, permissions: [] };

  const snaps = {
    history: { rows: full ? historyRows : [] },
    bookmarks: { items: full ? bookmarkItems : [], query: params.get('q') || '', currentFolder: full ? 3 : null },
    notes: full ? notes : { notes: [], tasks: [] },
    settings,
    privacy: full ? privacy : emptyPrivacy,
  };
  const messages = [state];
  if (new URLSearchParams(location.search).get('view') === 'panel') messages.push({ v: 1, t: 'panelshow', p: panel });
  messages.push({ v: 1, t: 'snap', p: panel, d: snaps[panel] });

  const listeners = [];
  let delivered = false;
  window.chrome = { webview: {
    postMessage(wire) {
      if (delivered) return;
      try { if (JSON.parse(wire).t !== 'ready') return; } catch { return; }
      delivered = true;
      setTimeout(() => {
        for (const m of messages) for (const cb of listeners) cb({ data: JSON.stringify(m) });
        // ?act=<selector>,<selector>: click elements after the first render (state screenshots).
        for (const sel of (params.get('act') || '').split('|').filter(Boolean)) document.querySelector(sel)?.click();
        const focus = params.get('focus');
        if (focus) document.querySelector(focus)?.focus();
      }, 0);
    },
    addEventListener(type, cb) { if (type === 'message') listeners.push(cb); },
  } };
})();
