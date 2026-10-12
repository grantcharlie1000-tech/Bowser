// Sidebar-only fake host: ?n=tabs &pinned=n &groups=0|1|2 &collapsed=0|1 &ws=g:code &fav=0|1 &active=id &scheme=dark|light
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
  const q = new URLSearchParams(location.search);
  const dark = q.get('scheme') !== 'light';
  document.documentElement.classList.add(dark ? 'h-dark' : 'h-light');
  const n = Number(q.get('n') || 8);
  const pinned = Number(q.get('pinned') || 0);
  const groups = Number(q.get('groups') || 0);
  const collapsed = q.get('collapsed') === '1';
  const ws = q.get('ws') || 'g:code';
  const wsName = q.get('wsname') || 'Work & Play';
  const titles = ['GitHub - bowser/bowser', 'Inbox (12) - Mail', 'A very long page title that has to ellipsise somewhere near the close button',
    'Docs - Fluent 2 Design System', 'YouTube', 'Loading...', 'Wikipedia, the free encyclopedia', 'Pull request #482',
    'Tailwind CSS v4', 'MDN Web Docs', 'Figma - Bowser UI', 'Hacker News'];
  const items = [];
  for (let i = 1; i <= n; i += 1) {
    items.push({
      id: i, title: titles[(i - 1) % titles.length] + (i > titles.length ? ` ${i}` : ''), loading: i === 6 && n > 5,
      pinned: i <= pinned, muted: i === 4 && n > 3, marker: i === 2 && n > 1 ? 3 : -1, g: 0, gl: false,
      fav: q.get('fav') !== '0' && i % 5 !== 0,
    });
  }
  if (groups >= 1 && n >= pinned + 3) {
    const s = pinned;
    Object.assign(items[s], { g: 9, gl: true, gname: 'Research', gcol: 5, gcollapsed: collapsed, gsize: 3 });
    items[s + 1].g = 9; items[s + 2].g = 9;
  }
  if (groups >= 2 && n >= pinned + 7) {
    const s = pinned + 4;
    Object.assign(items[s], { g: 10, gl: true, gname: 'Design and a rather long group name', gcol: 1, gcollapsed: false, gsize: 2 });
    items[s + 1].g = 10;
  }
  const active = Number(q.get('active') || Math.min(n, pinned + 2));
  const tabs = { v: 1, t: 'tabs', active, items };
  const state = { t: 'state', scale: 1, solid: false, dark, back: true, forward: false, loading: false, progress: 0, security: 'secure', label: 'x', theme: dark ? DARK : LIGHT };
  const favMsgs = [];
  if (q.get('fav') !== '0') {
    const hues = [210, 20, 140, 280, 50, 0, 170, 330];
    for (const it of items) {
      if (!it.fav) continue;
      const c = document.createElement('canvas'); c.width = c.height = 32;
      const g = c.getContext('2d'); g.fillStyle = `hsl(${hues[it.id % 8]} 65% 55%)`;
      g.beginPath(); g.roundRect(2, 2, 28, 28, 7); g.fill();
      g.fillStyle = '#fff'; g.font = 'bold 18px sans-serif'; g.textAlign = 'center'; g.textBaseline = 'middle'; g.fillText(it.title[0], 16, 17);
      favMsgs.push({ v: 1, t: 'fav', id: it.id, uri: c.toDataURL('image/png') });
    }
  }
  const script = [state, { v: 1, t: 'layout', mode: 'vertical' }, { v: 1, t: 'workspace', name: wsName, icon: ws }, tabs, ...favMsgs];
  document.documentElement.style.width = (q.get('w') || 240) + 'px';
  document.documentElement.style.overflow = 'hidden';
  if (q.get('dbg')) setTimeout(() => { const r = (sel) => { const e = document.querySelector(sel); if (!e) return null; const b = e.getBoundingClientRect(); return [Math.round(b.left), Math.round(b.top), Math.round(b.width), Math.round(b.height)]; }; document.body.dataset.dbg = JSON.stringify({ app: r('#app'), top: r('.side-top'), list: r('.side-list'), empty: r('.side-empty'), html: r('html'), iw: innerWidth }); }, 1000);
  const listeners = [];
  let delivered = false;
  window.chrome = {
    webview: {
      postMessage(wire) {
        if (delivered) return;
        try { if (JSON.parse(wire).t !== 'ready') return; } catch { return; }
        delivered = true;
        if (q.get('focus')) setTimeout(() => { const rows = document.querySelectorAll('.side-tab, .side-group'); const r = rows[Number(q.get('focus'))]; if (r) r.focus(); }, 300);
        setTimeout(() => { for (const m of script) { const data = JSON.stringify(m); for (const cb of listeners) cb({ data }); } }, 0);
      },
      addEventListener(type, cb) { if (type === 'message') listeners.push(cb); },
    },
  };
})();
