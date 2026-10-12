import { startShell } from './shell';
import { renderMenu } from './menu-view';

export function startPopup(root: HTMLElement): void {
  startShell(root, 'popup');
  root.tabIndex = -1;
  window.chrome?.webview?.addEventListener('message', (event: { readonly data: unknown }) => {
    let value: unknown = event.data;
    if (typeof value === 'string') {
      try { value = JSON.parse(value) as unknown; } catch { return; }
    }
    renderMenu(root, value);
  });
}
