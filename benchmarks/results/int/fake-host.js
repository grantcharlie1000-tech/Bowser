// Stand-in for the WebView2 host bridge for the wave-1 integration screenshots. Loaded before app.js;
// data.js (written by render.mjs) holds window.__INT = { scenes } built from the Rust fixtures.
// ?scn=<scene> &scheme=dark|light &q=<palette text> &act=<steps: click:<css> | scroll:<css> | wait:<ms>>
(() => {
  'use strict';
  const params = new URLSearchParams(location.search);
  const scn = params.get('scn') || '';
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
  const state = { t: 'state', scale: 1, solid: true, dark, back: true, forward: false, loading: false, progress: 0, security: 'secure', label: '', theme: dark ? DARK : LIGHT };
  const messages = [state, ...((window.__INT && window.__INT.scenes[scn]) || [])];
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
  // Pages that never send ready (popups, hover) get their messages after load.
  window.addEventListener('load', () => setTimeout(deliver, 300));

  const q = params.get('q');
  const act = params.get('act');
  window.addEventListener('load', async () => {
    const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
    const waitFor = async (sel) => { for (let n = 0; n < 100; n += 1) { const e = document.querySelector(sel); if (e) return e; await sleep(50); } return null; };
    await sleep(700);
    if (q !== null && q !== '') {
      const input = await waitFor('.bw-palette-input');
      if (input) { input.value = q; input.selectionStart = input.selectionEnd = q.length; input.dispatchEvent(new Event('input', { bubbles: true })); }
    }
    for (const step of (act || '').split('|').filter(Boolean)) {
      const i = step.indexOf(':');
      const kind = i < 0 ? step : step.slice(0, i);
      const arg = i < 0 ? '' : step.slice(i + 1);
      if (kind === 'click') (await waitFor(arg))?.click();
      else if (kind === 'scroll') (await waitFor(arg))?.scrollIntoView({ block: 'start' });
      else if (kind === 'wait') await sleep(Number(arg));
      await sleep(150);
    }
  });
})();
