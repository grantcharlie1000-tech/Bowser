import { onInbound, send, sendPanel, type PanelToken } from './protocol';
import { applyTheme } from './view';
import { installTooltips } from './tooltip';
import { panels } from './panels';
import { startRenameDialog } from './rename';
import { startAddressDialog } from './address-dialog'; // --- address bar, search and bookmarks ---

const isPanelToken = (value: string | null): value is PanelToken =>
  value === 'notes' || value === 'privacy' || value === 'rename' || value === 'address';

export function startDialog(root: HTMLElement): void {
  root.classList.add('panel-root');
  installTooltips(root);
  // On the document, not the root: with nothing focused inside the page, Esc targets <body> and a
  // root listener never sees it, so the dialog would not close from the keyboard.
  document.addEventListener('keydown', event => {
    if (event.key === 'Escape' && !event.defaultPrevented) {
      event.preventDefault();
      sendPanel('panel', { a: 'dismiss' });
    }
  });
  const token = new URLSearchParams(location.search).get('p');
  if (!isPanelToken(token)) return;
  // --- popup surfaces (lane PP) --- the name prompt is its own small dialog, not a panel.
  const receive = token === 'rename' ? startRenameDialog(root) : token === 'address' ? startAddressDialog(root) : panels[token]?.(root);
  // --- end popup surfaces ---
  onInbound(message => {
    if (message.t === 'state') applyTheme(message.theme, message.solid, message.dark);
    else if (message.t === 'snap' && (message.p === token || (token === 'address' && message.p === 'addressdlg'))) receive?.(message);
  });
  send({ v: 1, t: 'ready' });
}
