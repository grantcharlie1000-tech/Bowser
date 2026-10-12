// Toolbar-only host stand-in: ?st=idle|focus|loading|http|cert|internal|empty|long&scheme=&w=&placed=
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
  const p = new URLSearchParams(location.search);
  const st = p.get('st') || 'idle';
  const dark = p.get('scheme') !== 'light';
  const width = Number(p.get('w') || 1280);
  const placedOn = p.get('placed') !== '0';
  document.documentElement.classList.add(dark ? 'h-dark' : 'h-light');
  const state = (extra = {}) => ({
    t: 'state', scale: 1, solid: false, dark, back: true, forward: false, loading: false, progress: 0,
    security: 'secure', label: 'Connection is secure', theme: dark ? DARK : LIGHT, ...extra,
  });
  // Rust-like placement: nav on the left, menu button and three native caption buttons on the right.
  const left = 100, right = 28 + 3 * 46 + 8;
  const pillw = Math.max(60, Math.min(624, width - left - right - 16));
  const pillx = Math.max(left, Math.round((width - pillw) / 2));
  const placed = placedOn ? { pillx, pillw, menux: pillx + pillw + 3 } : {};
  const url = 'https://github.com/anthropics/claude-code/issues?q=is%3Aopen';
  const msg = (text) => ({ t: 'setText', text, select: false });
  const S = {
    idle: [state(placed), msg(url)],
    loading: [state({ ...placed, loading: true, progress: 55, forward: true }), msg(url)],
    http: [state({ ...placed, security: 'notsecure', label: 'Not secure' }), msg('http://neverssl.com/')],
    cert: [state({ ...placed, security: 'certproblem', label: 'Certificate error' }), msg('https://expired.badssl.com/')],
    internal: [state({ ...placed, security: 'internal', label: 'Browser page' }), msg('bowser://settings')],
    empty: [state(placed), msg('')],
    focus: [state(placed), msg('')],
    long: [state(placed), msg('https://en.wikipedia.org/wiki/List_of_Internet_top-level_domains_and_other_very_long_path_segments_here?foo=bar&baz=qux')],
  };
  const listeners = [];
  const emit = (m) => { for (const cb of listeners) cb({ data: JSON.stringify(m) }); };
  const deliver = () => {
    for (const m of S[st] || []) emit(m);
    if (st === 'focus') setTimeout(() => {
      const i = document.querySelector('.omni');
      i.focus();
      i.value = 'githu';
      i.dispatchEvent(new InputEvent('input', { inputType: 'insertText', data: 'u', bubbles: true }));
      // Headless virtual time does not run the rAF edit round-trip, so paint the accepted completion directly.
      i.value = 'github.com/anthropics';
      i.setSelectionRange(5, i.value.length);
    }, 30);
  };
  let sent = false;
  window.chrome = { webview: {
    postMessage(wire) {
      let m; try { m = JSON.parse(wire); } catch { return; }
      if (m.t === 'ready' && !sent) { sent = true; setTimeout(deliver, 0); }
      if (m.t === 'edit' && st === 'focus') { document.title = 'edit-sent'; setTimeout(() => emit({ t: 'completion', seq: m.seq, suffix: 'b.com/anthropics' }), 20); }
    },
    addEventListener(type, cb) { if (type === 'message') listeners.push(cb); },
  } };
})();
