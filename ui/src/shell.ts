import { onInbound, send } from './protocol';
import { applyTheme } from './view';

/** Shared empty page for views whose content is built by later backlog tasks. */
export function startShell(root: HTMLElement, name: string): void {
  root.replaceChildren();
  const container = document.createElement('div');
  container.className = `${name}-shell`;
  root.append(container);
  onInbound(msg => {
    if (msg.t === 'state') applyTheme(msg.theme, msg.solid, msg.dark, msg.fontScale, msg.uiFont, msg.density, msg.compactMenus, msg.look);
  });
  send({ v: 1, t: 'ready' });
}
