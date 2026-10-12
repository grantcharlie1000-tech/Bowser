// --- panel host (lane P) ---
import { onInbound, send, sendPanel, type Inbound, type PanelToken } from './protocol';
import { applyTheme } from './view';
import { installTooltips } from './tooltip';
import { panels } from './panels';
import { shouldDismissPanelEscape } from './panels/history-logic';

export function startPanel(root: HTMLElement): void {
  root.classList.add('panel-root');
  installTooltips(root);
  root.addEventListener('keydown', (event) => {
    if (event.key === 'Escape' && shouldDismissPanelEscape(event.defaultPrevented)) {
      event.preventDefault();
      sendPanel('panel', { a: 'dismiss' });
    }
  });
  let active: PanelToken | undefined;
  let receive: ((message: Inbound) => void) | undefined;
  onInbound((message) => {
    if (message.t === 'state') {
      applyTheme(message.theme, message.solid, message.dark);
      return;
    }
    if (message.t === 'panelshow') {
      if (active !== message.p) {
        active = message.p;
        receive = panels[message.p]?.(root);
      }
      return;
    }
    if (message.t === 'snap' && message.p === active) receive?.(message);
  });
  send({ v: 1, t: 'ready' });
}
// --- end panel host ---
