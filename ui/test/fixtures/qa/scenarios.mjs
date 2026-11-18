// QA surface catalogue (agent F audit harness): every HTML chrome view, in an empty and a populated
// state, driven only by the messages Rust sends (docs/html-chrome-protocol.md). Message bodies come
// from the committed Rust-written fixtures in ../ wherever one exists; the few hand-written ones
// (main menu, rail, panels, rename) copy the shapes of src/win32/window.rs and the panel parsers.
// Nothing here is a real profile. Used by ui/test/qa-lib.mjs, ui/shots/qa-capture.mjs,
// ui/test/qa-a11y.mjs and benchmarks/scripts/qa-latency.mjs.
import { readFileSync } from 'node:fs';
import { deflateSync } from 'node:zlib';

const fx = (name) => JSON.parse(readFileSync(new URL(`../${name}`, import.meta.url), 'utf8'));
const rust = fx('rust-messages.json');
const address = fx('address-messages.json');
const downloads = fx('downloads-messages.json');
const extensions = fx('extensions-messages.json');
const split = fx('split-messages.json');
const sidebar = fx('sidebar-messages.json');
const tabOptions = fx('tab-options-messages.json');
const settings = fx('settings-messages.json');
const historyPage = fx('history-page-messages.json');
const startPage = fx('start-page-messages.json');
const popupPages = fx('popup-pages.json');
const suggest = fx('suggest-messages.json');
const hover = fx('hover-messages.json');
const sessions = fx('sessions-messages.json');
const feeds = fx('feeds-messages.json');
const privacy = fx('privacy-snapshot.json');
const protection = fx('protection-snapshot.json');
const commands = fx('commands-snapshot.json');
const pageTools = fx('page-tools-snapshot.json');

// The theme colours Rust derives from the stock accent (same values as ui/shots/*-host.js).
export const THEMES = {
  dark: {
    bg: 'rgba(32,36,44,0.800)', fill: 'rgba(255,255,255,0.110)', border: 'rgba(95,216,219,0.282)',
    focusBorder: '#b3c9ce', text: '#ffffff', placeholder: '#dde2e9', completion: '#dde2e9',
    selection: 'rgba(77,163,255,0.251)', hover: 'rgba(255,255,255,0.078)', progress: '#84c8ff',
  },
  light: {
    bg: 'rgba(244,246,248,0.800)', fill: 'rgba(255,255,255,0.314)', border: 'rgba(22,143,152,0.282)',
    focusBorder: '#4a6d73', text: '#14181d', placeholder: '#4b5159', completion: '#50575f',
    selection: 'rgba(0,103,192,0.251)', hover: 'rgba(255,255,255,0.314)', progress: '#0067c0',
  },
};

/** The `state` message, as Rust sends it with the full field set (rust-messages.json), per scheme. */
export function stateMessage(scheme, extra = {}, width = 0) {
  const dark = scheme === 'dark';
  // Pill and menu placement as Rust computes it for a toolbar this wide (less two button slots).
  const full = Math.max(160, Math.min(624, width - 260));
  const placed = width > 0 ? { pillx: Math.round((width - full) / 2), pillw: full - 64, menux: Math.round((width - full) / 2) + full + 3 } : {};
  return { ...rust.state, scale: 1, private: false, dark, solid: true, theme: THEMES[scheme], ...placed, ...extra };
}

// A real 16x16 favicon (solid accent square with a light centre), built here so the tiles are visible.
function png16() {
  const crcTable = Array.from({ length: 256 }, (_, n) => { let c = n; for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1; return c >>> 0; });
  const crc = (buf) => { let c = 0xffffffff; for (const b of buf) c = crcTable[(c ^ b) & 0xff] ^ (c >>> 8); return (c ^ 0xffffffff) >>> 0; };
  const chunk = (type, data) => { const len = Buffer.alloc(4); len.writeUInt32BE(data.length); const td = Buffer.concat([Buffer.from(type, 'ascii'), data]); const c = Buffer.alloc(4); c.writeUInt32BE(crc(td)); return Buffer.concat([len, td, c]); };
  const ihdr = Buffer.alloc(13); ihdr.writeUInt32BE(16, 0); ihdr.writeUInt32BE(16, 4); ihdr[8] = 8; ihdr[9] = 6;
  const raw = Buffer.alloc(16 * (1 + 16 * 4));
  for (let y = 0; y < 16; y++) for (let x = 0; x < 16; x++) {
    const o = y * 65 + 1 + x * 4; const inner = x > 4 && x < 11 && y > 4 && y < 11;
    raw.set(inner ? [255, 255, 255, 255] : [0, 120, 212, 255], o);
  }
  return `data:image/png;base64,${Buffer.concat([Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]), chunk('IHDR', ihdr), chunk('IDAT', deflateSync(raw)), chunk('IEND', Buffer.alloc(0))]).toString('base64')}`;
}
const PNG = png16();

// ---- tab strip ----
const tabItem = (id, title, extra = {}) => ({ id, title, loading: false, pinned: false, muted: false, private: false, marker: -1, g: 0, gl: false, fav: false, ...extra });
const tabsPopulated = { v: 1, t: 'tabs', active: 3, items: [
  tabItem(1, 'Inbox (3) - Mail', { pinned: true, fav: true }),
  tabItem(2, 'Design review notes', { g: 42, gl: true, gname: 'Work', gcol: 2, gcollapsed: false, gsize: 2, fav: true }),
  tabItem(3, 'Quarterly planning - Docs', { g: 42, gl: false, fav: true }),
  tabItem(4, 'A very long article title that keeps going so the strip has to cut it somewhere sensible', { fav: true }),
  tabItem(5, 'Loading page', { loading: true }),
  tabItem(6, 'Music player', { muted: true, fav: true }),
  tabItem(7, 'Search results', { fav: true }),
] };
const tabsEmpty = { v: 1, t: 'tabs', active: 1, items: [tabItem(1, 'New tab')] };
const favsFor = (msg) => msg.items.filter((t) => t.fav).map((t) => ({ v: 1, t: 'fav', id: t.id, uri: PNG }));

// ---- rail ----
const railItem = (k, icon, label, extra = {}) => ({ k, icon, label, active: false, dot: false, accent: '', fav: false, ...extra });
const railPopulated = { v: 1, t: 'rail', items: [
  railItem('home', 'home', 'Home', { active: true }),
  railItem('ws:1', 'g:briefcase', 'Work', { active: true, accent: '#0090ff' }),
  railItem('ws:2', 'g:home', 'Personal'),
  railItem('addws', 'plus', 'New workspace'),
  railItem('app:5', 'letter', 'Mail & More', { dot: true, fav: true }),
  railItem('app:6', 'apptile', 'Calendar'),
  railItem('addapp', 'plus', 'Add web app'),
  railItem('bookmarks', 'heart', 'Bookmarks'),
  railItem('history', 'clock', 'History'),
  railItem('customize', 'customize', 'Customise'),
  railItem('menu', 'dots', 'More'),
] };
const railEmpty = { v: 1, t: 'rail', items: [railItem('home', 'home', 'Home', { active: true }), railItem('customize', 'customize', 'Customise')] };

// ---- main menu (src/win32/window.rs main menu, a normal tab) ----
let nextId = 100;
const it = (label, icon, extra = {}) => ({ id: nextId++, label, enabled: true, checked: false, ...(icon ? { icon } : {}), ...extra });
const mainMenu = { v: 1, t: 'menu', tok: 11, kind: 'main',
  header: { name: 'Alex', caption: 'Current profile', avatar: 'A', accent: '#0067c0' },
  entries: [
    it('New tab', 'new-tab', { shortcut: 'Ctrl+T' }),
    it('New window', 'window', { shortcut: 'Ctrl+N' }),
    it('New private tab', 'private', { shortcut: 'Ctrl+Shift+P' }),
    { separator: true },
    it('History', 'history', { shortcut: 'Ctrl+H' }),
    it('Downloads', 'download', { shortcut: 'Ctrl+J' }),
    it('Bookmarks', 'bookmark', { submenu: [it('Bookmark this tab…', 'star', { shortcut: 'Ctrl+D' }), it('Show bookmarks bar', null, { checked: true, shortcut: 'Ctrl+Shift+B' }), { separator: true }, it('Bookmark manager', 'folder')] }),
    it('Workspaces', 'workspace', { submenu: [it('Work', 'workspace-briefcase', { checked: true }), it('Personal', 'workspace-home'), { separator: true }, it('New workspace…', 'workspace-add')] }),
    { separator: true },
    it('Block ads & trackers', 'shield', { checked: true, shortcut: 'Ctrl+Shift+B' }),
    it('Reader view', 'reader', { shortcut: 'Ctrl+Alt+R', enabled: false }),
    it('Translate to English', 'translate', { enabled: false }),
    it('More tools', 'more', { submenu: [it('Notes & Tasks…', 'notes', { shortcut: 'Ctrl+Shift+N' }), it('Command palette…', 'command', { shortcut: 'Ctrl+Shift+K' }), it('Saved sessions…', 'folder'), { separator: true }, it('Extensions…', 'puzzle', { shortcut: 'Ctrl+Shift+E' }), it('Developer tools', 'workspace-code', { shortcut: 'F12' })] }),
    { separator: true },
    it('Zoom · 110%', 'zoom-in', { submenu: [it('Zoom in', 'zoom-in', { shortcut: 'Ctrl++' }), it('Zoom out', 'zoom-out', { shortcut: 'Ctrl+−' }), it('Reset zoom', 'reset', { shortcut: 'Ctrl+0' })] }),
    it('Delete browsing data…', 'trash', { shortcut: 'Ctrl+Shift+Del' }),
    { separator: true },
    it('Customise', 'customize'),
    it('About Bowser', 'info'),
    it('Settings', 'gear'),
    it('Exit', 'close', { shortcut: 'Alt+F4' }),
  ],
};
const tabMenu = { v: 1, t: 'menu', tok: 12, kind: 'tab', entries: [
  it('New tab to the right', 'new-tab'), it('Reload', 'reload', { shortcut: 'Ctrl+R' }), it('Duplicate', 'copy'),
  it('Pin', 'pin'), it('Mute site', 'mute'), { separator: true },
  it('Close', 'close', { shortcut: 'Ctrl+W' }), it('Close other tabs', 'close-others'), it('Close tabs to the right', null, { enabled: false }),
] };

// ---- panels (rail flyouts) ----
const histRows = [
  { label: 'Today', count: 3 },
  { id: 1, title: 'Quarterly planning - Docs', host: 'docs.example.com', clock: '14:32' },
  { id: 2, title: 'Design review notes', host: 'notes.example', clock: '13:05' },
  { id: 3, title: '', host: 'news.example', clock: '09:41' },
  { label: 'Yesterday', count: 2 },
  { id: 4, title: 'A long page title that does not fit in one row of the history flyout at all', host: 'blog.example.org', clock: '22:10' },
  { id: 5, title: 'Search results', host: 'search.example', clock: '18:00' },
];
const panelSnap = (p, d) => ({ v: 1, t: 'snap', p, d });
const historyPanel = (rows) => panelSnap('history', { rows, favs: {}, query: '', undo: false });
const bookmarksPanel = (items) => panelSnap('bookmarks', { items, query: '', currentFolder: null, singleClick: true });
const bmItems = [
  { id: 1, parent: null, title: 'Reading', kind: 'folder' },
  { id: 2, parent: null, title: 'Work', kind: 'folder' },
  { id: 3, parent: null, title: 'Example news', kind: 'link', url: 'https://news.example/' },
  { id: 4, parent: null, title: 'Docs home', kind: 'link', url: 'https://docs.example.com/' },
  { id: 5, parent: 1, title: 'Spec', kind: 'link', url: 'https://spec.example/', folderPath: ['Reading'] },
];
const customizePanel = (full) => panelSnap('customize', {
  visible: { workspaces: true, apps: true, bookmarks: true, history: full },
  workspaces: full ? [
    { id: 0, name: 'Default', icon: 'home', colour: null, active: false, tabs: 3 },
    { id: 1, name: 'Work', icon: 'briefcase', colour: 'blue', active: true, tabs: 7 },
    { id: 2, name: 'Reading', icon: 'book', colour: 'amber', active: false, tabs: 2 },
  ] : [{ id: 0, name: 'Default', icon: 'home', colour: null, active: true, tabs: 1 }],
  apps: full ? [{ id: 5, name: 'Mail', url: 'https://mail.example/', position: 0 }, { id: 6, name: 'Calendar', url: 'https://calendar.example/', position: 1 }] : [],
});

// ---- extension store (bowser://store over ?view=settings; shapes as ui/shots/store-host.js) ----
const storeEntry = (id, name, description, publisher, category, homepage) => ({
  id, name, description, publisher, category, source: `https://chromewebstore.google.com/detail/${id}`, homepage,
  provenance: 'Manually curated from the linked publisher and Chrome Web Store listing. Not an all-store search or availability guarantee.',
  checked: '2026-10-18', compatibility: 'untested', compatibilityNote: 'Untested. No version-specific compatibility result is available.',
});
const storeCatalogue = [
  storeEntry('ddkjiahejlhfcafbddmgiahcphecmpfh', 'uBlock Origin Lite', 'A declarative MV3 content blocker from the uBlock Origin project.', 'Raymond Hill (gorhill)', 'Privacy & security', 'https://github.com/uBlockOrigin/uBOL-home'),
  storeEntry('clngdbkpkpeebahjckkjfobafhncgmne', 'Stylus', 'An open-source user-style manager for changing website appearance with CSS.', 'stylus.openstyles', 'Appearance', 'https://github.com/openstyles/stylus'),
];
const storeEmpty = { permissions: [], hostPermissions: [], contentScripts: [], contentScriptDeclarations: [], optionalPermissions: [], optionalHostPermissions: [] };
const storeSnap = (extra = {}) => panelSnap('store', { shown: true, catalogue: storeCatalogue, phase: 'idle', requestId: '', extensionId: 'ddkjiahejlhfcafbddmgiahcphecmpfh', name: '', version: '', error: '', restart: false, ...storeEmpty, ...extra });
const storeReview = storeSnap({ requestId: 'store-1-1', name: 'uBlock Origin Lite', phase: 'review', version: '2026.1019.1408', permissions: ['activeTab', 'declarativeNetRequest', 'scripting', 'storage'], hostPermissions: ['<all_urls>'], contentScripts: ['<all_urls>'] });

const renameSnap = (error) => panelSnap('rename', { title: 'Rename workspace', save: 'Rename', value: 'Work', hint: 'Workspace name', error, max: 64 });

/**
 * Each scenario: `name`, `surface` (defect-routing area), `query` (index.html search), `size`
 * {standard:[w,h], compact:[w,h]}, `messages(scheme)` (what Rust posts after ready, in order),
 * `fit` (popup: resize the viewport to the size the page asks for with `size`), optional
 * `act` (an in-page step after delivery, see qa-lib.mjs ACTS), `owner` (A-E).
 */
export function scenarios() {
  const toolbarBase = (scheme, extra, rest, width) => [stateMessage(scheme, extra, width), ...rest];
  const S = [];
  const add = (s) => S.push({ fit: false, act: null, ...s });

  // Toolbar (?view=toolbar): chromeBegin/Part/Commit bundle.
  add({ name: 'toolbar-empty', surface: 'toolbar', owner: 'B', query: 'view=toolbar', chrome: true,
    size: { standard: [1280, 32], compact: [760, 32] },
    messages: (sc, w) => toolbarBase(sc, { back: false, forward: false, security: 'none', label: '' }, [address.addr, address.sfield, { t: 'setText', text: '', select: false }, downloads.dlbtnOff, extensions.buttonOff, split.pairNone], w) });
  add({ name: 'toolbar-populated', surface: 'toolbar', owner: 'B', query: 'view=toolbar', chrome: true,
    size: { standard: [1280, 32], compact: [760, 32] },
    messages: (sc, w) => toolbarBase(sc, { back: true, forward: true, loading: true, progress: 40, zoom: 110, permblocked: true, reader: 'available', translate: 'available', bookmarked: true },
      [{ ...address.addr, hbtn: true, qr: true }, address.sfield, { t: 'setText', text: 'https://docs.example.com/guide/getting-started?ref=qa', select: false }, downloads.dlbtnActive, extensions.buttonMany, split.pair], w) });
  add({ name: 'toolbar-focused', surface: 'toolbar', owner: 'B', query: 'view=toolbar', chrome: true, act: 'focusOmnibox',
    size: { standard: [1280, 32], compact: [760, 32] },
    messages: (sc, w) => toolbarBase(sc, {}, [address.addr, address.sfield, { t: 'setText', text: 'https://example.com/', select: false }, downloads.dlbtnOff, extensions.buttonOff, split.pairNone], w) });

  // Tab strip (?view=tabs), horizontal and vertical.
  const tabsMsgs = (layout, tabs) => (sc) => [stateMessage(sc), layout, sidebar.workspace, tabOptions.defaults, tabs, ...favsFor(tabs), split.pairNone];
  add({ name: 'tabs-h-empty', surface: 'tabs', owner: 'B', query: 'view=tabs', chrome: true, size: { standard: [1280, 30], compact: [760, 30] }, messages: tabsMsgs(sidebar.layoutHorizontal, tabsEmpty) });
  add({ name: 'tabs-h-populated', surface: 'tabs', owner: 'B', query: 'view=tabs', chrome: true, size: { standard: [1280, 30], compact: [760, 30] }, messages: tabsMsgs(sidebar.layoutHorizontal, tabsPopulated) });
  add({ name: 'tabs-v-empty', surface: 'sidebar', owner: 'B', query: 'view=tabs', chrome: true, size: { standard: [260, 720], compact: [200, 480] }, messages: tabsMsgs(sidebar.layoutVertical, tabsEmpty) });
  add({ name: 'tabs-v-populated', surface: 'sidebar', owner: 'B', query: 'view=tabs', chrome: true, size: { standard: [260, 720], compact: [200, 480] }, messages: tabsMsgs(sidebar.layoutVertical, tabsPopulated) });

  // Rail (?view=rail).
  const railMsgs = (rail) => (sc) => [stateMessage(sc), rail, ...rail.items.filter((r) => r.fav).map((r) => ({ v: 1, t: 'railfav', k: r.k, uri: PNG }))];
  add({ name: 'rail-empty', surface: 'rail', owner: 'B', query: 'view=rail', chrome: true, size: { standard: [44, 720], compact: [44, 420] }, messages: railMsgs(railEmpty) });
  add({ name: 'rail-populated', surface: 'rail', owner: 'B', query: 'view=rail', chrome: true, size: { standard: [44, 720], compact: [44, 420] }, messages: railMsgs(railPopulated) });

  // Bookmark bar (?view=bar).
  add({ name: 'bookmark-bar', surface: 'bar', owner: 'B', query: 'view=bar', size: { standard: [1280, 30], compact: [760, 30] }, messages: (sc) => [stateMessage(sc), address.bar] });

  // Popup pages (?view=popup): menus, palette, site information, downloads bubble.
  const popup = (name, owner, msg, act = null, extra = []) => add({ name, surface: 'popup', owner, query: 'view=popup', fit: true, act,
    size: { standard: [1280, 900], compact: [1280, 420] }, messages: (sc) => [stateMessage(sc), msg, ...extra] });
  popup('menu-main', 'D', mainMenu);
  // The main menu hangs from the button at the window's right edge: Rust's `room` says its
  // submenus open to the left (html_popup.rs flyout_room).
  popup('menu-main-submenu', 'D', mainMenu, 'openSubmenu', [{ v: 1, t: 'room', tok: mainMenu.tok, l: 1000, r: 0 }]);
  popup('menu-tab', 'D', tabMenu);
  popup('palette-populated', 'D', popupPages.palette);
  popup('palette-empty', 'D', { ...popupPages.palette, items: [] });
  popup('siteinfo', 'A', popupPages.siteinfo);
  popup('siteinfo-private', 'A', { ...popupPages.siteinfo, perms: [], reset: false });
  popup('downloads-bubble', 'D', downloads.bubble);
  popup('downloads-bubble-empty', 'D', downloads.bubbleEmpty);

  // Hover layer (?view=hover): tooltip, tab card, island menu, suggestion list.
  const hoverS = (name, msg) => add({ name, surface: 'hover', owner: name.startsWith('suggest') ? 'B' : 'A', query: 'view=hover', fit: true,
    size: { standard: [800, 600], compact: [800, 300] }, messages: (sc) => [stateMessage(sc), msg] });
  hoverS('hover-tooltip', hover.tooltip);
  hoverS('hover-card', hover.card);
  hoverS('hover-island', hover.island);
  hoverS('suggest-dropdown', suggest.suggest);

  // Rail flyouts (?view=panel).
  const panel = (name, p, snap) => add({ name, surface: 'panel', owner: 'D', query: 'view=panel',
    size: { standard: [380, 640], compact: [320, 420] }, messages: (sc) => [stateMessage(sc, { solid: false }), { v: 1, t: 'flyout', caret: 40 }, { v: 1, t: 'panelshow', p }, snap] });
  panel('panel-history', 'history', historyPanel(histRows));
  panel('panel-history-empty', 'history', historyPanel([]));
  panel('panel-bookmarks', 'bookmarks', bookmarksPanel(bmItems));
  panel('panel-bookmarks-empty', 'bookmarks', bookmarksPanel([]));
  panel('panel-customize', 'customize', customizePanel(true));

  // Dialogs (?view=dialog&p=...).
  const dialog = (name, p, snap, owner = 'A') => add({ name, surface: 'dialog', owner, query: `view=dialog&p=${p}`,
    size: { standard: [480, 560], compact: [360, 400] }, messages: (sc) => [stateMessage(sc), snap] });
  dialog('dialog-rename', 'rename', renameSnap(''));
  dialog('dialog-rename-error', 'rename', renameSnap('A workspace with this name already exists.'));
  dialog('dialog-address-bookmark', 'address', address.bookmark, 'B');
  dialog('dialog-sessions', 'sessions', sessions.snap, 'D');
  dialog('dialog-sessions-empty', 'sessions', sessions.guest, 'D');
  dialog('dialog-feeds', 'feeds', feeds.snap, 'D');
  dialog('dialog-privacy', 'privacy', privacy, 'C');
  add({ name: 'dialog-privacy-protection', surface: 'dialog', owner: 'C', query: 'view=dialog&p=privacy', size: { standard: [480, 560], compact: [360, 400] }, messages: (sc) => [stateMessage(sc), privacy, protection] });

  // Full-tab pages.
  const page = (name, view, owner, msgs) => add({ name, surface: view, owner, query: `view=${view}`, size: { standard: [1280, 860], compact: [720, 640] }, messages: msgs });
  // The settings page's section snapshots, as Rust sends them with the page (privacy with protection,
  // page tools, shortcuts, address bar); `extra` replaces or adds to them.
  const sectionSnaps = (extra) => { const by = new Map([privacy, protection, pageTools, commands, address.snap, extensions.snap].map((m) => [m.p, m])); for (const m of extra) by.set(m.p, m); return [...by.values()]; };
  const settingsAt = (s, extra = []) => (sc) => [stateMessage(sc), settings.snap, ...sectionSnaps(extra), { v: 1, t: 'settingsgo', s }];
  page('settings-top', 'settings', 'C', settingsAt(''));
  page('settings-appearance', 'settings', 'C', settingsAt('appearance'));
  page('settings-privacy', 'settings', 'C', settingsAt('privacy'));
  page('settings-search', 'settings', 'C', settingsAt('search'));
  page('settings-extensions', 'settings', 'D', settingsAt('extensions', [extensions.snap]));
  page('settings-extensions-empty', 'settings', 'D', settingsAt('extensions', [extensions.empty]));
  page('store-idle', 'settings', 'D', (sc) => [stateMessage(sc), settings.snap, ...sectionSnaps([]), storeSnap()]);
  page('store-review', 'settings', 'D', (sc) => [stateMessage(sc), settings.snap, ...sectionSnaps([]), storeReview]);
  page('settings-guest', 'settings', 'C', (sc) => [stateMessage(sc), settings.guest, settings.goTop]);
  page('history-page', 'history', 'C', (sc) => [stateMessage(sc), historyPage.snap]);
  page('history-page-empty', 'history', 'C', (sc) => [stateMessage(sc), historyPage.empty]);
  page('downloads-page', 'downloads', 'C', (sc) => [stateMessage(sc), downloads.page]);
  page('downloads-page-empty', 'downloads', 'C', (sc) => [stateMessage(sc), downloads.pageEmpty]);
  // The fixture's favicon is a PNG header only (Rust's test bytes); the realistic run swaps in a real one.
  const startFavs = Object.fromEntries(Object.keys(startPage.page.d.favs).map((host) => [host, PNG]));
  page('start-page', 'split', 'C', (sc) => [stateMessage(sc), { ...startPage.page, d: { ...startPage.page.d, favs: startFavs } }]);
  page('start-page-broken-favicon', 'split', 'C', (sc) => [stateMessage(sc), startPage.page]);
  page('start-page-empty', 'split', 'C', (sc) => [stateMessage(sc), startPage.empty]);
  page('private-page', 'split', 'C', (sc) => [stateMessage(sc, { private: true }), split.private]);
  page('split-picker', 'split', 'B', (sc) => [stateMessage(sc), split.pick]);
  return S;
}

export { mainMenu, tabsPopulated, tabsEmpty };
