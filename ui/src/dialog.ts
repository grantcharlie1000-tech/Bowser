import { onInbound, send, sendPanel, type PanelToken } from './protocol';
import { applyTheme } from './view';
import { installTooltips } from './tooltip';
import { panels } from './panels';

const isPanelToken = (value: string | null): value is PanelToken =>
  value === 'notes' || value === 'settings' || value === 'privacy';

export function startDialog(root: HTMLElement): void {
  root.classList.add('panel-root');
  installTooltips(root);
  root.addEventListener('keydown', event => {
    if (event.key === 'Escape' && !event.defaultPrevented) {
      event.preventDefault();
      sendPanel('panel', { a: 'dismiss' });
    }
  });
  const token = new URLSearchParams(location.search).get('p');
  if (!isPanelToken(token)) return;
  const receive = panels[token]?.(root);
  onInbound(message => {
    if (message.t === 'state') applyTheme(message.theme, message.solid, message.dark);
    else if (message.t === 'snap' && message.p === token) receive?.(message);
  });
  send({ v: 1, t: 'ready' });
}
