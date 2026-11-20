import { el, icon, type IconName } from './dom';

export function glyphTile(name: IconName, className: string, size = 16): HTMLElement {
  const tile = el('span', className);
  tile.setAttribute('aria-hidden', 'true');
  tile.append(icon(name, { size }));
  return tile;
}
