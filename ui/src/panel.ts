// --- panel host (lane P) ---
import { onInbound, send, type Inbound, type PanelToken } from './protocol';
import { applyTheme } from './view';
import { panels } from './panels';

export function startPanel(root: HTMLElement): void {
  root.classList.add('panel-root');
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
