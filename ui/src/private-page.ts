// Private tab landing (docs/design/private-tabs.md), shown by the split host (?view=split) over a
// blank private tab. Rust decides when it shows (src/win32/split_ui.rs, Shown::Private) and sends
// `privatepage`. The page is the private mark and its title, centred; it sends nothing.
// Static text only (private-logic.ts); nothing page-controlled is ever shown here.
//
// Motion (private.css): the bowler fades up, the shades slide down onto it and settle, then one glint
// crosses the lenses, and a second glint plays once about seven seconds later. All of it is finite
// (about 1.6s, then 0.9s), transform/opacity only, and none of it runs under reduced motion or when
// the same tab's landing is shown again.
import { icon } from './icons';
import { el } from './dom';
import { MARK_PARTS, MARK_SHAPE_COUNT, PRIVATE_PAGE_TITLE } from './private-logic';
import type { PrivatePageMsg } from './split-protocol';

const SVG_NS = 'http://www.w3.org/2000/svg';
const CLIP_ID = 'pm-lens-clip';

function group(className: string): SVGGElement {
  const g = document.createElementNS(SVG_NS, 'g');
  g.setAttribute('class', className);
  return g;
}

/**
 * The landing's large mark: icon('private') with its shapes regrouped into the bowler and the shades
 * (so each can move), plus the family's `glint` strokes clipped to the two lenses.
 */
export function buildMark(size = 80): SVGSVGElement {
  const svg = icon('private', { size, className: 'pm-mark' });
  const shapes = Array.from(svg.children) as SVGElement[];
  if (shapes.length !== MARK_SHAPE_COUNT) return svg; // a redrawn mark still shows, just still
  const hat = group('pm-hat');
  const shades = group('pm-shades');
  hat.append(...MARK_PARTS.hat.map((i) => shapes[i]!));
  shades.append(...MARK_PARTS.shades.map((i) => shapes[i]!));

  const defs = document.createElementNS(SVG_NS, 'defs');
  const clip = document.createElementNS(SVG_NS, 'clipPath');
  clip.id = CLIP_ID;
  for (const i of MARK_PARTS.lenses) clip.append(shapes[i]!.cloneNode(false));
  defs.append(clip);

  const glintClip = group('pm-glint-clip');
  glintClip.setAttribute('clip-path', `url(#${CLIP_ID})`);
  const glint = group('pm-glint');
  for (const shape of Array.from(icon('glint').children)) glint.append(shape);
  glintClip.append(glint);
  shades.append(glintClip);

  svg.replaceChildren(defs, hat, shades);
  return svg;
}

/**
 * Builds the landing once. `show` (after the root is in the document) plays the entrance for a tab
 * it has not shown yet; `hidden` records that the host took the page away, so the same tab's landing
 * comes back settled instead of replaying.
 */
export function buildPrivatePage(): {
  readonly root: HTMLElement;
  show(msg: PrivatePageMsg): void;
  hidden(): void;
} {
  const root = el('div', 'pm-page');
  root.setAttribute('role', 'region');
  root.setAttribute('aria-labelledby', 'pm-title');

  const hero = el('header', 'pm-hero');
  const halo = el('div', 'pm-halo');
  halo.setAttribute('aria-hidden', 'true');
  halo.append(buildMark());
  const title = el('h1', 'pm-title');
  title.id = 'pm-title';
  title.textContent = PRIVATE_PAGE_TITLE;
  hero.append(halo, title);
  root.append(hero);

  let shownFor = 0;
  let wasHidden = false;
  return {
    root,
    show(msg: PrivatePageMsg): void {
      const returning = wasHidden;
      wasHidden = false;
      if (msg.id === shownFor) {
        // Same tab: a later privatepage leaves a running entrance alone; a re-show comes back settled.
        if (returning) root.classList.remove('is-entering');
        return;
      }
      // A blank private tab not shown before: play the entrance (restart the CSS animations).
      shownFor = msg.id;
      root.classList.remove('is-entering');
      void root.offsetWidth;
      root.classList.add('is-entering');
    },
    hidden(): void {
      wasHidden = true;
    },
  };
}
