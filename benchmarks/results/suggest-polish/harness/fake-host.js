// Stand-in for the WebView2 host bridge: replays a theme `state` and one `suggest` message into the
// hover view (?view=hover), the way Rust does when the omnibox dropdown opens. Rows mirror what the
// real app produced for "wiki" in the live run, plus edge cases (open tab, long, RTL, bookmark).
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
  const dark = params.get('scheme') !== 'light';
  const width = Number(params.get('w') || 624);
  const sel = Number(params.get('sel') || 2);
  const set = params.get('set') || 'many';
  document.documentElement.classList.add(dark ? 'h-dark' : 'h-light');
  const row = (k, title, url, text = url) => ({ k, text, title, url });
  const sets = {
    many: { q: 'wiki', items: [
      row('url', 'wikidata.org/wiki/Wikidata:Main_Page', 'wikidata.org/wiki/Wikidata:Main_Page'),
      row('tab', 'Wiki - Wikipedia', 'en.wikipedia.org/wiki/Wiki'),
      row('hist', 'Microsoft Edge - Wikipedia', 'en.wikipedia.org/wiki/Microsoft_Edge'),
      row('bm', 'WebView - Wikipedia', 'en.wikipedia.org/wiki/WebView'),
      row('hist', 'A page with a very long title that will need to be clipped before the address column - Wikipedia', 'en.wikipedia.org/wiki/An_extremely_long_article_name_that_keeps_going_and_going'),
      row('hist', 'ويكيبيديا، الموسوعة الحرة', 'ar.wikipedia.org/wiki/'),
      row('hist', 'wikihow.com/', 'wikihow.com/'),
      row('search', 'wiki', '', 'wiki'),
    ] },
    search: { q: 'a very long search query that keeps going well past the width of the dropdown', items: [
      row('search', 'a very long search query that keeps going well past the width of the dropdown', '', 'a very long search query that keeps going well past the width of the dropdown'),
    ] },
  };
  const msgs = [
    { t: 'state', scale: 1, solid: false, dark, back: true, forward: false, loading: false, progress: 0,
      security: 'secure', label: 'Connection is secure', theme: dark ? DARK : LIGHT },
    { v: 1, t: 'suggest', tok: 5, seq: 9, w: width, sel: Math.min(sel, sets[set].items.length - 1), ...sets[set] },
  ];
  const listeners = [];
  let delivered = false;
  window.chrome = {
    webview: {
      postMessage(wire) {
        let m; try { m = JSON.parse(wire); } catch { return; }
        if (m.t === 'size') document.documentElement.dataset.psize = `${m.w}x${m.h}`;
        if (delivered || m.t !== 'ready') return;
        delivered = true;
        setTimeout(() => { for (const msg of msgs) for (const cb of listeners) cb({ data: JSON.stringify(msg) }); }, 0);
      },
      addEventListener(type, cb) { if (type === 'message') listeners.push(cb); },
    },
  };
})();
