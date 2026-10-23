// Stand-in for the WebView2 host bridge for the extension installation shots (store-render.mjs).
// Loaded before app.js; picks a scenario from ?scn= and replays Bowser's messages once the page says
// `ready`. Nothing here talks to a real Bowser profile, network or Web Store.
//   ?scn=      one of the keys of `scenarios` below
//   ?scheme=   dark | light
//   ?accent=   stock | red (a red theme accent, as on the owner's machine)
//   ?icon=     brand (default) | tile | dark | white | broken | none: the package icon Rust sends in
//              review/installing/ready (store-icons.mjs draws the PNGs; `broken` passes the parser's
//              format check but does not decode, so the tile falls back to the puzzle glyph)
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
  const scn = params.get('scn') || 'idle';
  const dark = params.get('scheme') !== 'light';
  const red = params.get('accent') === 'red';
  const theme = { ...(dark ? DARK : LIGHT), ...(red ? (dark ? { focusBorder: '#ff8a80', progress: '#ff8a80' } : { focusBorder: '#c42b1c', progress: '#c42b1c' }) : {}) };
  const state = { t: 'state', scale: 1, solid: true, dark, back: true, forward: false, loading: false, progress: 0, security: 'secure', label: 'Connection is secure', theme };

  // The four curated entries Rust sends (src/store.rs CATALOGUE), compatibility as catalogue_evidence.
  const entry = (id, name, description, publisher, category, homepage) => ({
    id, name, description, publisher, category, source: `https://chromewebstore.google.com/detail/${id}`, homepage,
    provenance: 'Manually curated from the linked publisher and Chrome Web Store listing. Not an all-store search or availability guarantee.',
    checked: '2026-10-18', compatibility: 'untested', compatibilityNote: 'Untested. No version-specific compatibility result is available.',
  });
  const catalogue = [
    entry('ddkjiahejlhfcafbddmgiahcphecmpfh', 'uBlock Origin Lite', 'A declarative MV3 content blocker from the uBlock Origin project.', 'Raymond Hill (gorhill)', 'Privacy & security', 'https://github.com/uBlockOrigin/uBOL-home'),
    entry('nngceckbapebfimnlniiiahkandclblb', 'Bitwarden Password Manager', 'An open-source password manager. Bowser compatibility must be checked before trusting it with real accounts.', 'Bitwarden Inc.', 'Privacy & security', 'https://bitwarden.com/'),
    entry('clngdbkpkpeebahjckkjfobafhncgmne', 'Stylus', 'An open-source user-style manager for changing website appearance with CSS.', 'stylus.openstyles', 'Appearance', 'https://github.com/openstyles/stylus'),
    entry('iaiomicjabeggjcfkbimgmglanimpnae', 'Tab Session Manager', 'Save and restore window and tab sessions. Tab-management APIs are not yet verified in Bowser.', 'Sienori', 'Productivity', 'https://github.com/sienori/Tab-Session-Manager'),
  ];
  const id = 'ddkjiahejlhfcafbddmgiahcphecmpfh';
  const empty = { permissions: [], hostPermissions: [], contentScripts: [], contentScriptDeclarations: [], optionalPermissions: [], optionalHostPermissions: [] };
  const store = (extra = {}) => ({ v: 1, t: 'snap', p: 'store', d: {
    shown: true, catalogue, phase: 'idle', requestId: '', extensionId: id, name: '', version: '', error: '', restart: false, ...empty, ...extra,
  } });
  const run = { requestId: 'store-4120-3', extensionId: id, name: 'uBlock Origin Lite' };
  const icons = { ...(window.__storeIcons || {}), broken: 'data:image/png;base64,AAAA' };
  const iconName = params.get('icon') || 'brand';
  const pkg = iconName === 'none' ? {} : { icon: icons[iconName] || '' }; // only the phases Rust sets it in
  const declaration = JSON.stringify({
    declared: { matches: ['<all_urls>'], js: ['/js/scripting/css-api.js', '/js/scripting/isolated-api.js'], all_frames: true, match_about_blank: true },
    documented_defaults_for_omitted_fields: { match_origin_as_fallback: false, run_at: 'document_idle', world: 'ISOLATED' },
    unrecognized_fields_effect_not_assumed: [],
  });
  const review = {
    ...run, ...pkg, phase: 'review', version: '2026.1019.1408',
    permissions: ['activeTab', 'declarativeNetRequest', 'declarativeNetRequestFeedback', 'scripting', 'storage', 'webNavigation'],
    hostPermissions: ['<all_urls>'], contentScripts: ['<all_urls>'], contentScriptDeclarations: [declaration],
    optionalPermissions: [], optionalHostPermissions: [],
  };
  const scenarios = {
    idle: [store()],
    downloading: [store({ ...run, phase: 'downloading' })],
    // `stage`, `received` and `total` are NOT sent by Rust today (see the report): proposed fields,
    // rendered only when present, so these two frames show what the UI does once they exist.
    'downloading-determinate': [store({ ...run, phase: 'downloading', stage: 'download', received: 1_468_006, total: 3_355_443 })],
    verifying: [store({ ...run, phase: 'downloading', stage: 'verify' })],
    review: [store(review)],
    installing: [store({ ...run, ...pkg, phase: 'installing', version: review.version })],
    ready: [store({ ...run, ...pkg, phase: 'ready', version: review.version })],
    'ready-restart': [store({ ...run, ...pkg, phase: 'ready', version: review.version, restart: true })],
    error: [store({ ...run, phase: 'error', error: 'Package verification failed: unpack failed: an archive entry is larger than the per-file cap. This is not a runtime compatibility verdict.' })],
    'error-download': [store({ ...run, phase: 'error', error: 'Package download failed: the Chrome Web Store update service returned HTTP 503 Service Unavailable after 3 attempts (request https://clients2.google.com/service/update2/crx?response=redirect&prodversion=131.0.2903.70&acceptformat=crx3&x=id%3Dddkjiahejlhfcafbddmgiahcphecmpfh%26installsource%3Dondemand%26uc). This is not a runtime compatibility verdict.' })],
    'error-runtime': [store({ phase: 'error', requestId: '', extensionId: '', error: 'This WebView2 runtime cannot run supported extensions. Update the WebView2 runtime before installing; restarting alone will not fix this. Nothing was installed.' })],
    cancelled: [store({ ...run, phase: 'cancelled' })],
  };

  const listeners = [];
  const deliver = () => {
    for (const msg of [state, window.__settingsSnap, ...(scenarios[scn] || [])].filter(Boolean)) {
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
  document.documentElement.dataset.scn = scn;
})();
