// The one icon family for every HTML chrome surface. Shape data is generated from the SVG
// sources in icons/ (see ui/scripts/gen-icons.mjs); this module only builds DOM nodes with
// createElementNS, so no markup is ever parsed (the CSP forbids it).
//
//   icon('bookmark')                         16px glyph, decorative (aria-hidden)
//   icon('close', { size: 12, fine: true })  12px glyph with the fine (1px) stroke
//   icon('download', { label: 'Download' })  meaningful image: role="img" + aria-label
//
// Sizing and stroke come from CSS (.bw-icon in styles/components.css): the stroke is drawn in
// rendered pixels (vector-effect: non-scaling-stroke), so every glyph size has the same weight.
import { ICON_ALIASES, ICON_NAMES, ICON_SHAPES, type DrawnIconName, type IconName } from './icons.gen';

export type { DrawnIconName, IconName } from './icons.gen';
export { ICON_NAMES };

const SVG_NS = 'http://www.w3.org/2000/svg';

export interface IconOptions {
  /** Glyph size in CSS px (default 16, from --bw-icon). Surface CSS may still override it. */
  readonly size?: number;
  /** Use the fine stroke (--bw-icon-stroke-fine), for glyphs drawn at 12px or smaller. */
  readonly fine?: boolean;
  /** Accessible name. Omit for decorative icons next to a text label or inside a labelled button. */
  readonly label?: string;
  /** Extra class names, space separated. */
  readonly className?: string;
}

/** The drawn glyph behind a name or alias. */
export function resolveIcon(name: IconName): DrawnIconName {
  return (ICON_ALIASES as Readonly<Record<string, DrawnIconName>>)[name] ?? (name as DrawnIconName);
}

export function isIconName(value: string): value is IconName {
  return (ICON_NAMES as readonly string[]).includes(value);
}

/** A new inline SVG for `name`, stroked with currentColor. */
export function icon(name: IconName, options: IconOptions = {}): SVGSVGElement {
  const svg = document.createElementNS(SVG_NS, 'svg');
  const size = String(options.size ?? 16);
  svg.setAttribute('viewBox', '0 0 24 24');
  // Attributes are the no-CSS fallback; .bw-icon sets the real size and stroke.
  svg.setAttribute('width', size);
  svg.setAttribute('height', size);
  svg.setAttribute('fill', 'none');
  svg.setAttribute('stroke', 'currentColor');
  svg.setAttribute('stroke-width', '1.5');
  svg.setAttribute('stroke-linecap', 'round');
  svg.setAttribute('stroke-linejoin', 'round');
  svg.setAttribute('focusable', 'false');
  svg.setAttribute('class', ['bw-icon', options.fine ? 'is-fine' : '', options.className ?? ''].join(' ').trim());
  if (options.size !== undefined) svg.style.setProperty('--bw-icon-size', `${options.size}px`);
  if (options.label) {
    svg.setAttribute('role', 'img');
    svg.setAttribute('aria-label', options.label);
  } else {
    svg.setAttribute('aria-hidden', 'true');
  }
  for (const [tag, attrs] of ICON_SHAPES[resolveIcon(name)]) {
    const node = document.createElementNS(SVG_NS, tag);
    for (const [key, value] of Object.entries(attrs)) node.setAttribute(key, value);
    svg.append(node);
  }
  return svg;
}
