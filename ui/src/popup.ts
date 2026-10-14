import { startShell } from './shell';
import { createMenuView } from './menu-view';
import { createPaletteView } from './palette';
import { createSiteInfoView } from './siteinfo';
import { createComposerView } from './composer';
import { createDownloadsView } from './downloads-bubble';

// The popup host shows one page at a time on #app, chosen by the message type: `menu` (menu-view),
// `palette` (command palette), `siteinfo` (site information), `composer` (anchored name and
// address form on the menu surface) and `downloads` (the download bubble). Each view validates its
// own payload; anything else is ignored.
export function startPopup(root: HTMLElement): void {
  startShell(root, 'popup');
  root.tabIndex = -1;
  const menu = createMenuView(root);
  const palette = createPaletteView(root);
  const siteinfo = createSiteInfoView(root);
  const composer = createComposerView(root);
  const downloads = createDownloadsView(root);
  window.chrome?.webview?.addEventListener('message', (event: { readonly data: unknown }) => {
    let value: unknown = event.data;
    if (typeof value === 'string') {
      try { value = JSON.parse(value) as unknown; } catch { return; }
    }
    if (typeof value !== 'object' || value === null) return;
    switch ((value as { t?: unknown }).t) {
      case 'menu': menu.show(value); break;
      case 'palette': palette.show(value); break;
      case 'siteinfo': siteinfo.show(value); break;
      case 'composer': composer.show(value); break;
      case 'downloads': downloads.show(value); break;
    }
  });
}
