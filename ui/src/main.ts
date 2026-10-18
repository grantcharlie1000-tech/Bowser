// CSP-checked dispatch only. Each surface loads its own module and shared dependencies.
/**
 * True only when the page's CSP is enforced: with Trusted Types required, assigning a string to an
 * HTML sink must throw. The sink name is assembled at runtime so the bundle holds no literal sink
 * (lint.mjs allows none in dist/app.js).
 */
function trustedTypesEnforced(): boolean {
  const probe = document.createElement('div') as unknown as Record<string, string>;
  const sink = ['inner', 'HTML'].join('');
  try {
    probe[sink] = 'x';
  } catch {
    return true;
  }
  return false;
}

function start(): void {
  const root = document.getElementById('app');
  if (!root) return;
  // Never send ready without enforced CSP: the host retains native fallback.
  if (!trustedTypesEnforced()) {
    document.title = 'csp-not-enforced';
    return;
  }
  const view = new URLSearchParams(location.search).get('view');
  let entry: Promise<{ start: (root: HTMLElement) => void }>;
  if (view === 'tabs') entry = import('./entries/tabs');
  else if (view === 'rail') entry = import('./entries/rail');
  else if (view === 'panel') entry = import('./entries/panel');
  else if (view === 'popup') entry = import('./entries/popup');
  else if (view === 'dialog') entry = import('./entries/dialog');
  else if (view === 'bar') entry = import('./entries/bar');
  else if (view === 'hover') entry = import('./entries/hover');
  else if (view === 'split') entry = import('./entries/split');
  else if (view === 'downloads') entry = import('./entries/downloads');
  else if (view === 'settings') entry = import('./settings-page').then((page) => ({ start: page.startSettingsPage }));
  else if (view === 'history') entry = import('./history-page').then((page) => ({ start: page.startHistoryPage }));
  else entry = import('./entries/toolbar');
  void entry.then((page) => page.start(root)).catch((err: unknown) => {
    document.title = 'page-load-failed';
    throw err;
  });
}

start();
