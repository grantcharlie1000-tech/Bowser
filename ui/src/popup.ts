import { startShell } from './shell';
import { createMenuView } from './menu-view';

export function startPopup(root: HTMLElement): void {
  startShell(root, 'popup');
  root.tabIndex = -1;
  const menu = createMenuView(root);
  window.chrome?.webview?.addEventListener('message', (event: { readonly data: unknown }) => {
    let value: unknown = event.data;
    if (typeof value === 'string') {
      try { value = JSON.parse(value) as unknown; } catch { return; }
    }
    menu.show(value);
  });
}
