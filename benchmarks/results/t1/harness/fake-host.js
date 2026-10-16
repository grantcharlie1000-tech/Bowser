// Stand-in for the WebView2 host bridge for the T1 (tabs) screenshots. Loaded before app.js by
// render.mjs; ?scn= picks the host messages to replay when the page says `ready`.
// `fixtures.js` (written by render.mjs) supplies window.__t1 = { settings, thumb }.
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
  const scn = params.get('scn') || 'tabs';
  const dark = params.get('scheme') !== 'light';
  document.documentElement.classList.add(dark ? 'h-dark' : 'h-light');
  const fx = window.__t1 || {};
  const state = (extra = {}) => ({
    t: 'state', scale: 1, solid: true, dark, back: true, forward: false, loading: false, progress: 0,
    security: 'secure', label: 'Connection is secure', theme: dark ? DARK : LIGHT, ...extra,
  });
  const tab = (id, title, extra = {}) => ({
    id, title, loading: false, pinned: false, muted: false, private: false, marker: -1, g: 0, gl: false, fav: false, ...extra,
  });
  const plain = [
    tab(1, 'Pinned', { pinned: true }), tab(2, 'Pinned mail', { pinned: true }),
    tab(3, 'Reading list'), tab(4, 'Inbox'), tab(5, 'Build status'), tab(6, 'Docs'), tab(7, 'Plain tab'),
  ];
  const marked = [
    tab(1, 'Pinned', { pinned: true }), tab(2, 'Pinned mail', { pinned: true, notify: 4 }),
    tab(3, 'Reading list', { asleep: true }),
    tab(4, '(3) Inbox', { notify: 3, unread: true }),
    tab(5, 'Build status', { rl: 45 }),
    tab(6, 'Docs', { loading: true, pct: 60 }),
    tab(7, 'Unused for weeks', { idle: true }),
  ];
  const opts = (extra = {}) => ({
    v: 1, t: 'tabopts', closeLeft: false, closeAlways: false, scroll: false, minWidth: 0, indicator: 'spinner', wheel: false, separator: false,
    alignClose: false, selection: true, removeSpacing: false, maximised: false, closed: 0, ...extra,
  });
  const strip = (items, extra) => [state(), ...(extra ? [extra] : []), { v: 1, t: 'tabs', active: 3, items }];
  const sessions = (extra = {}) => ({
    v: 1, t: 'snap', p: 'sessions', d: {
      allowed: true, tabs: 6, selected: 0, suggest: 'Session 3',
      items: [
        { name: 'Research', count: 8, hosts: ['wikipedia.org', 'arxiv.org', 'nature.com'] },
        { name: 'Morning reading', count: 5, hosts: ['news.example', 'blog.example'] },
      ], ...extra,
    },
  });
  const card = (extra = {}) => ({
    v: 1, t: 'hover', tok: 4, kind: 'card', title: 'Docs: opening a session', host: 'docs.example.com', marker: -1,
    markers: ['', '', '', '', ''], sleeping: false, fav: '', rows: [], more: 0, ...extra,
  });

  const scenarios = {
    // The tab strip as it was: no marks, no options message.
    'tabs-before': strip(plain),
    // Every mark at once, spinner indicator, Closed tabs button.
    'tabs-marks': strip(marked, opts({ closed: 3, separator: true })),
    'tabs-bar': strip(marked, opts({ indicator: 'bar', closeLeft: true, closed: 2, separator: true })),
    'tabs-ring': strip(marked, opts({ indicator: 'favicon', closeAlways: true })),
    'tabs-scroll': strip([...marked, tab(8, 'Another tab with a long title'), tab(9, 'One more'), tab(10, 'And another one')], opts({ scroll: true, minWidth: 180 })),
    'sidebar-before': [state(), { v: 1, t: 'layout', mode: 'vertical' }, { v: 1, t: 'workspace', name: 'Work', icon: 'g:code' }, { v: 1, t: 'tabs', active: 3, items: plain }],
    'sidebar-marks': [state(), { v: 1, t: 'layout', mode: 'vertical' }, { v: 1, t: 'workspace', name: 'Work', icon: 'g:code' }, opts({ closed: 2 }), { v: 1, t: 'tabs', active: 3, items: marked }],
    'settings-before': [state(), { v: 1, t: 'snap', p: 'settings', d: fx.settingsBefore }, { v: 1, t: 'settingsgo', s: 'appearance' }],
    'settings-tabs': [state(), { v: 1, t: 'snap', p: 'settings', d: fx.settings }, { v: 1, t: 'settingsgo', s: 'tabs' }],
    'sessions': [state(), { v: 1, t: 'panelshow', p: 'sessions' }, sessions()],
    'sessions-empty': [state(), { v: 1, t: 'panelshow', p: 'sessions' }, sessions({ items: [] })],
    'hover-before': [state(), card()],
    'hover-preview': [state(), card({ thumb: fx.thumb, mem: 412 })],
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
        try { if (JSON.parse(wire).t !== 'ready') return; } catch { return; }
        // The settings page is a lazily loaded bundle that posts its own ready once it has a listener;
        // an earlier ready from the shell found nobody listening, so answer every ready for it.
        if (delivered && !scn.startsWith('settings')) return;
        delivered = true;
        setTimeout(deliver, 0);
      },
      addEventListener(type, cb) { if (type === 'message') listeners.push(cb); },
    },
  };
  // The hover view never sends ready before it has content.
  if (scn.startsWith('hover')) window.addEventListener('load', () => { if (!delivered) { delivered = true; deliver(); } });
  // The screenshot cannot scroll, so show only the section being compared: "before" is Appearance (where
  // the tab rows lived), "after" is Tabs. Only the page's own sections are hidden; nothing is rebuilt.
  if (scn.startsWith('settings')) {
    const wanted = scn === 'settings-tabs' ? 'Tabs' : 'Appearance';
    const watcher = new MutationObserver(() => {
      const sections = [...document.querySelectorAll('section')];
      if (sections.length < 5) return;
      for (const s of sections) s.style.display = s.querySelector('h2')?.textContent.trim() === wanted ? '' : 'none';
      watcher.disconnect();
    });
    window.addEventListener('DOMContentLoaded', () => watcher.observe(document.body, { childList: true, subtree: true }));
  }
})();
