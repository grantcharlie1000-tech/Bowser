// Private tab landing (docs/design/private-tabs.md), shown by the split host (?view=split) over a
// blank private tab. Rust decides when it shows (src/win32/split_ui.rs, Shown::Private) and sends
// `privatepage` with the private tab count; the one control sends `{t:"split",a:"closeprivate"}`.
// Static text only (private-logic.ts); nothing page-controlled is ever shown here.
//
// Motion (private.css): the bowler fades up, the shades slide down onto it and settle, then one glint
// crosses the lenses, and a second glint plays once about seven seconds later. All of it is finite
// (about 1.6s, then 0.9s), transform/opacity only, and none of it runs under reduced motion or when
// the same tab's landing is shown again.
import { icon } from './icons';
import { el } from './dom';
import {
  MARK_PARTS, MARK_SHAPE_COUNT, PRIVATE_CLOSE_ALL, PRIVATE_KEPT_TITLE, PRIVATE_NOT_KEPT, PRIVATE_PAGE_EYEBROW,
  PRIVATE_PAGE_LEAD, PRIVATE_PAGE_TITLE, PRIVATE_SEEN_TITLE, PRIVATE_SHARED_NOTE, PRIVATE_STILL_SEEN,
  privateCountText, type PrivateRow,
} from './private-logic';
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

function card(title: string, rows: readonly PrivateRow[], className: string): HTMLElement {
  const section = el('section', `pm-card ${className}`);
  const heading = el('h2', 'pm-card-title');
  heading.textContent = title;
  const list = el('ul', 'pm-list');
  for (const row of rows) {
    const item = el('li', 'pm-row');
    const glyph = el('span', 'pm-row-icon');
    glyph.append(icon(row.icon));
    const text = el('span', 'pm-row-text');
    const label = el('span', 'pm-row-label');
    label.textContent = row.label;
    const body = el('span', 'pm-row-body');
    body.textContent = row.text;
    text.append(label, body);
    item.append(glyph, text);
    list.append(item);
  }
  section.append(heading, list);
  return section;
}

/**
 * Builds the landing once. `show` (after the root is in the document) updates the count and plays the
 * entrance for a tab it has not shown yet; `hidden` records that the host took the page away, so the
 * same tab's landing comes back settled instead of replaying.
 */
export function buildPrivatePage(onCloseAll: () => void): {
  readonly root: HTMLElement;
  show(msg: PrivatePageMsg): void;
  hidden(): void;
} {
  const root = el('div', 'pm-page');
  root.setAttribute('role', 'region');
  root.setAttribute('aria-labelledby', 'pm-title');

  const column = el('div', 'pm-column');
  const hero = el('header', 'pm-hero');
  const halo = el('div', 'pm-halo');
  halo.setAttribute('aria-hidden', 'true');
  halo.append(buildMark());
  const eyebrow = el('span', 'pm-eyebrow');
  eyebrow.append(icon('private', { size: 14 }), document.createTextNode(PRIVATE_PAGE_EYEBROW));
  const title = el('h1', 'pm-title');
  title.id = 'pm-title';
  title.textContent = PRIVATE_PAGE_TITLE;
  const lead = el('p', 'pm-lead');
  lead.textContent = PRIVATE_PAGE_LEAD;

  const cards = el('div', 'pm-cards');
  cards.append(card(PRIVATE_KEPT_TITLE, PRIVATE_NOT_KEPT, 'is-kept'), card(PRIVATE_SEEN_TITLE, PRIVATE_STILL_SEEN, 'is-seen'));

  const note = el('p', 'pm-note');
  note.textContent = PRIVATE_SHARED_NOTE;

  const footer = el('div', 'pm-footer');
  const count = el('span', 'pm-count');
  count.setAttribute('role', 'status');
  const close = el('button', 'bw-button pm-close');
  close.type = 'button';
  close.append(icon('close'), document.createTextNode(PRIVATE_CLOSE_ALL));
  close.addEventListener('click', onCloseAll);
  footer.append(close, count);
  // The one action sits under the lead, above the fold on any window tall enough for the mark.
  hero.append(halo, eyebrow, title, lead, footer);

  column.append(hero, cards, note);
  root.append(column);

  let shownFor = 0;
  let wasHidden = false;
  return {
    root,
    show(msg: PrivatePageMsg): void {
      const text = privateCountText(msg.n);
      if (count.textContent !== text) count.textContent = text;
      const returning = wasHidden;
      wasHidden = false;
      if (msg.id === shownFor) {
        // Same tab: a count update leaves a running entrance alone; a re-show comes back settled.
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
