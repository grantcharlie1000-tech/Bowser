// The glyphs that mark the settings page's section titles and group cards
// (styles/settings.css): plain, in the text colour.
import { el, icon, type IconName } from './dom';

/** A box holding one glyph (decorative). */
export function glyphTile(name: IconName, className: string, size = 16): HTMLElement {
  const tile = el('span', className);
  tile.setAttribute('aria-hidden', 'true');
  tile.append(icon(name, { size }));
  return tile;
}
