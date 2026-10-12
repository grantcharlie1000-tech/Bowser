// Fake host for rendering the menu popup (?view=popup) with the menus Rust builds. ?menu=<name>
// picks one; ?scheme=light|dark; ?w=<css px> only sizes the window (Edge --window-size).
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
  const name = params.get('menu') || 'main';
  const dark = params.get('scheme') !== 'light';
  document.documentElement.classList.add(dark ? 'h-dark' : 'h-light');
  const state = { t: 'state', scale: 1, solid: true, dark, back: true, forward: false, loading: false, progress: 0,
    security: 'secure', label: 'Connection is secure', theme: dark ? DARK : LIGHT };

  let id = 1;
  const it = (label, icon, extra = {}) => ({ id: id++, label, enabled: true, checked: false, ...(icon ? { icon } : {}), ...extra });
  const sep = { separator: true };
  const png = 'data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAgAAAAICAYAAADED76LAAAAJ0lEQVQYV2NkYGD4z8DAwMgABXAGNgGwSgwVAFbmAUQ0GqH8WbQAAB7MBgKm7JrJAAAAAElFTkSuQmCC';

  const profileMenu = () => [
    it('Default', 'person', { checked: true, badge: { avatar: 'D', accent: '#2b5fa8' } }),
    it('Work', 'person', { badge: { avatar: 'W', accent: '#32a89b' } }),
    sep,
    it('Guest window', 'external'), it('New profile', 'plus'),
    it('Choose avatar', 'person', { submenu: [it('Initial', null, { checked: true }), it('Leaf')] }),
    it('Choose accent colour', 'droplet', { submenu: [it('Blue', null, { badge: { avatar: '', accent: '#4385d1' } })] }),
    { section: 'Delete profile' }, it('Work', 'trash'),
    { section: 'Rename profile' }, it('Work', 'edit'),
  ];
  const workspaces = () => [
    it('Work', 'workspace-briefcase', { checked: true }), it('Code', 'workspace-code'), it('Travel', 'workspace-globe'),
    sep, it('New workspace…', 'plus'), it('Rename current workspace…', 'edit'), it('Delete current workspace…', 'trash'),
  ];
  const menus = {
    main: { kind: 'main', header: { name: 'Default', caption: 'Current profile', avatar: 'D', accent: '#2b5fa8' }, footer: 'Bowser 0.0.1', entries: [
      it('New tab', 'plus', { shortcut: 'Ctrl+T' }), sep,
      it('Profile: Default', 'person', { submenu: profileMenu() }),
      it('Notes & Tasks…', 'notes', { shortcut: 'Ctrl+Shift+N' }),
      it('Command palette…', 'command', { shortcut: 'Ctrl+Shift+K' }),
      it('Bookmark this page', 'bookmark', { shortcut: 'Ctrl+D' }),
      it('Saved sessions…', 'folder'),
      it('Extensions…', 'puzzle', { shortcut: 'Ctrl+Shift+E' }),
      it('Passwords & passkeys', 'key', { shortcut: 'Ctrl+,' }),
      it('Import from another browser…', 'download'), sep,
      it('Vertical tabs', 'sidebar', { shortcut: 'Ctrl+Shift+V' }),
      it('Workspaces', 'workspace', { submenu: workspaces() }), sep,
      it('Block ads & trackers', 'shield', { shortcut: 'Ctrl+Shift+B', checked: true }), sep,
      it('Settings…', 'gear'), it('Privacy dashboard', 'eye', { shortcut: 'Ctrl+Shift+Del' }), sep,
      it('Exit', 'close', { shortcut: 'Alt+F4' }),
    ] },
    tab: { kind: 'tab:1', footer: 'Actions for this tab', entries: [
      it('New tab to the right', 'plus'), it('New split view with current tab', 'split'), sep,
      it('Add tab to group', 'tab-group', { submenu: [it('Always group this site', null, { checked: true }), it('Group this site automatically'), it('Never group this site')] }),
      it('Collapse group', 'chevron-up', { enabled: false }), it('Remove from group', 'caption-minimize', { enabled: false }),
      it('Undo tab organisation', 'reset', { enabled: false }), sep,
      it('Copy URL', 'link'), it('Reload', 'reload'), it('Duplicate tab', 'copy'), it('Pin tab', 'pin'), it('Mute tab', 'mute'),
      it('Mute site', 'workspace-globe'), it('Hibernate tab', 'clock', { enabled: false }), it('Move tab to new window', 'external'),
      it('Picture in picture', 'pip'), sep,
      it('Close tab', 'close'), it('Close other tabs', 'trash'), it('Close tabs to the left', 'back', { enabled: false }),
      it('Close tabs to the right', 'forward'),
    ] },
    workspaces: { kind: 'workspaces', footer: 'Workspaces', entries: workspaces() },
    profile: { kind: 'profiles', footer: 'Switch, create, rename, or delete', entries: profileMenu() },
    siteinfo: { kind: 'site-info', footer: 'Changes apply to the next request',
      header: { name: 'en.wikipedia.org', caption: 'https://en.wikipedia.org', avatar: '•', accent: '#2b5fa8' }, entries: [
        { section: 'Connection' }, it('Connection is secure', 'lock', { enabled: false }), it('Certificate details unavailable', null, { enabled: false }),
        { section: 'Permissions for this site' },
        it('Microphone: Blocked by default', 'mic', { submenu: [it('Allow'), it('Block'), it('Default (blocked)', null, { checked: true })] }),
        it('Camera: Blocked by default', 'camera', { submenu: [it('Allow')] }),
        it('Location: Allowed', 'location', { submenu: [it('Allow', null, { checked: true })] }),
        it('Notifications: Blocked by default', 'bell', { submenu: [it('Allow')] }),
        { section: 'Site data and blocking' },
        it('Cookies sent to this site: 3', 'cookie', { enabled: false }), it('Clear cookies for this site', 'trash'),
        it('Blocker: on (12 blocked requests here)', 'shield', { checked: true }),
        it('Tracking prevention: Balanced', 'eye', { enabled: false }), sep,
        it('Reset permissions for this site', 'reset', { enabled: false }),
      ] },
    extensions: { kind: 'extensions', footer: 'Unpacked extensions', entries: [
      it('uBlock Origin 1.58 (enabled)', 'puzzle', { checked: true }), it('Dark Reader 4.9 (disabled)', 'puzzle'), sep,
      it('Remove extension…', 'trash', { submenu: [it('uBlock Origin', 'puzzle')] }), sep,
      it('Install unpacked extension…', 'folder'),
    ] },
    opentabs: { kind: 'open-tabs', footer: 'Select a tab', favs: [png], entries: [
      it('Wikipedia, the free encyclopedia', 'workspace-globe', { fav: 0 }),
      it('A very long page title that must be truncated with an ellipsis because it is wider than the menu allows', 'workspace-globe', { fav: 0, checked: true }),
      it('New tab', 'workspace-globe'), it('GitHub: Let’s build from here', 'workspace-globe', { fav: 0 }),
    ] },
    tall: { kind: 'open-tabs', footer: 'Select a tab', favs: [png], entries: Array.from({ length: 60 }, (_, i) =>
      it(`Open tab number ${i + 1}`, 'workspace-globe', { fav: 0, checked: i === 3 })) },
  };
  const menu = { v: 1, t: 'menu', tok: 1, ...menus[name] };
  const listeners = [];
  window.chrome = { webview: { postMessage() {}, addEventListener(type, cb) { if (type === 'message') listeners.push(cb); } } };
  window.addEventListener('load', () => {
    for (const msg of [state, menu]) for (const cb of listeners) cb({ data: JSON.stringify(msg) });
    // Open a submenu page when asked: ?drill=<row index>
    const drill = params.get('drill');
    if (drill !== null) {
      setTimeout(() => {
        const row = document.getElementById(`html-menu-row-${drill}`);
        row?.dispatchEvent(new MouseEvent('mouseup', { button: 0, bubbles: true }));
      }, 50);
    }
    const sel = params.get('key');
    if (sel) setTimeout(() => document.getElementById('app').dispatchEvent(new KeyboardEvent('keydown', { key: sel, bubbles: true })), 30);
  });
})();
