// Stand-in for the WebView2 host bridge for the T13 screenshots (toolbars and feeds). Loaded before app.js;
// data.js (written by render.mjs) holds the Rust fixtures. Query: ?scn=toolbar|tabs|settings|feeds
// &scheme=dark|light &items=<list> &tools=<k:label:i,...> &extra=<px> &feeds=<n> &tabtools=<k:label:i,...> &closedbtn=0|1
// &fscn=snap|preview|empty &section=<settings section> &act=<steps>. Steps: click:<css> | wait:<ms> | scroll:<css>.
(() => {
  'use strict';
  const params = new URLSearchParams(location.search);
  const scn = params.get('scn') || 'toolbar';
  const dark = params.get('scheme') !== 'light';
  document.documentElement.classList.add(dark ? 'h-dark' : 'h-light');
  const DARK = {
    bg: 'rgb(32,36,44)', fill: 'rgba(255,255,255,0.110)', border: 'rgba(95,216,219,0.282)', focusBorder: '#b3c9ce', text: '#ffffff',
    placeholder: '#dde2e9', completion: '#dde2e9', selection: 'rgba(77,163,255,0.251)', hover: 'rgba(255,255,255,0.078)', progress: '#84c8ff',
  };
  const LIGHT = {
    bg: 'rgb(244,246,248)', fill: 'rgba(255,255,255,0.7)', border: 'rgba(22,143,152,0.282)', focusBorder: '#4a6d73', text: '#14181d',
    placeholder: '#4b5159', completion: '#50575f', selection: 'rgba(0,103,192,0.251)', hover: 'rgba(0,0,0,0.06)', progress: '#0067c0',
  };
  const data = window.__T13;
  const theme = dark ? DARK : LIGHT;
  const list = (text) => (text || '').split(',').filter(Boolean).map((part) => { const [k, l, i] = part.split(':'); return { k: k.replace('_', ':'), l: l.replace(/_/g, ' '), i: Number(i) }; });
  const state = {
    t: 'state', scale: 1, solid: true, dark, back: true, forward: false, loading: false, progress: 0, security: 'secure', label: 'Secure',
    pillx: 130, pillw: 640, menux: 1060, theme,
  };
  if (params.get('items') !== null) {
    state.items = params.get('items');
    state.tools = list(params.get('tools'));
    state.extra = Number(params.get('extra') || 0);
    state.feeds = Number(params.get('feeds') || 0);
    state.pillw = 640 - state.extra;
  }
  const messages = [state];
  if (scn === 'toolbar') {
    messages.push({ t: 'setText', text: 'https://blog.example/posts/feeds', select: false });
  } else if (scn === 'tabs') {
    const opts = { ...data.tabs.defaults, closed: 3, closedButton: params.get('closedbtn') !== '0', tools: list(params.get('tabtools')) };
    messages.push({ v: 1, t: 'layout', mode: 'horizontal' }, data.tabs.tabs, opts);
  } else if (scn === 'settings') {
    messages.push({ v: 1, t: 'settingsgo', s: params.get('section') || '' }, data.settings, data.commands);
  } else if (scn === 'feeds') {
    messages.push({ v: 1, t: 'panelshow', p: 'feeds' }, data.feeds[params.get('fscn') || 'snap']);
  }
  const listeners = [];
  let delivered = false;
  const deliver = () => {
    if (delivered) return;
    delivered = true;
    for (const msg of messages) for (const cb of listeners) cb({ data: JSON.stringify(msg) });
  };
  window.chrome = {
    webview: {
      postMessage(wire) {
        try { if (JSON.parse(wire).t === 'ready') setTimeout(deliver, 0); } catch { /* ignore */ }
      },
      addEventListener(type, cb) { if (type === 'message') listeners.push(cb); },
    },
  };
  window.addEventListener('load', () => setTimeout(deliver, 300));
  const act = params.get('act');
  window.addEventListener('load', async () => {
    const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
    const waitFor = async (sel) => { for (let n = 0; n < 100; n += 1) { const e = document.querySelector(sel); if (e) return e; await sleep(50); } return null; };
    await sleep(700);
    for (const step of (act || '').split('|').filter(Boolean)) {
      const i = step.indexOf(':');
      const kind = i < 0 ? step : step.slice(0, i);
      const arg = i < 0 ? '' : step.slice(i + 1);
      if (kind === 'click') (await waitFor(arg))?.click();
      else if (kind === 'scroll') (await waitFor(arg))?.scrollIntoView({ block: 'start' });
      else if (kind === 'wait') await sleep(Number(arg));
      await sleep(200);
    }
  });
})();
