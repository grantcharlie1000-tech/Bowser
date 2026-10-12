// Outline icons for the left rail, ported from src/chrome/rail_icons.rs (24-unit grid, one stroke
// weight, round caps and joins). Each native primitive becomes an SVG shape: segments and chains
// become path data, rounded boxes become rect, rings become circle, filled dots become filled circles.
// Curves keep their control points; the native painter samples them, SVG draws them directly.
import type { RailIconName } from './rail-logic';

const SVG_NS = 'http://www.w3.org/2000/svg';

type Attrs = Readonly<Record<string, string>>;
type Shape = readonly [tag: 'path' | 'rect' | 'circle' | 'ellipse' | 'polygon', attrs: Attrs];

const path = (d: string): Shape => ['path', { d }];
const ring = (cx: number, cy: number, r: number): Shape => ['circle', { cx: `${cx}`, cy: `${cy}`, r: `${r}` }];
const dot = (cx: number, cy: number, r: number): Shape => [
  'circle',
  { cx: `${cx}`, cy: `${cy}`, r: `${r}`, fill: 'currentColor', stroke: 'none' },
];
const box = (x: number, y: number, width: number, height: number, rx: number): Shape => [
  'rect',
  { x: `${x}`, y: `${y}`, width: `${width}`, height: `${height}`, rx: `${rx}` },
];

/** The house walls, shared by the Home button and the Home workspace glyph. */
const HOUSE_WALLS = 'M5 10V18.5Q5 20.5 7 20.5H17Q19 20.5 19 18.5V10';
/** The heart outline, from its bottom tip. */
const HEART = 'M12 20.5C6 16 3 13 3 9.3C3 6.5 5 4.5 7.5 4.5C9.4 4.5 11 5.5 12 7.2C13 5.5 14.6 4.5 16.5 4.5C19 4.5 21 6.5 21 9.3C21 13 18 16 12 20.5Z';

export const RAIL_SHAPES: Readonly<Record<RailIconName, readonly Shape[]>> = {
  home: [path('M3 11.5L12 3.5L21 11.5'), path(HOUSE_WALLS), path('M10 20.5V16Q10 14.5 11.5 14.5H12.5Q14 14.5 14 16V20.5')],
  customize: [path('M3 7H13.5'), path('M18.5 7H21'), path('M3 17H5.5'), path('M10.5 17H21'), ring(16, 7, 2.5), ring(8, 17, 2.5)],
  plus: [path('M12 4.5V19.5M4.5 12H19.5')],
  heart: [path(HEART)],
  clock: [ring(12, 12, 9), path('M12 7L12 12L15.5 14')],
  dots: [dot(12, 5.5, 1.6), dot(12, 12, 1.6), dot(12, 18.5, 1.6)],
  apptile: [box(3.5, 3.5, 7, 7, 2), box(13.5, 3.5, 7, 7, 2), box(3.5, 13.5, 7, 7, 2), path('M17 13.5V20.5M13.5 17H20.5')],
  // The letter tile is a text node, not an outline.
  letter: [],
  'g:briefcase': [box(3, 7.5, 18, 12, 2), path('M9 7.5V6Q9 4.5 10.5 4.5H13.5Q15 4.5 15 6V7.5M3 13H21')],
  'g:home': [path('M3 11.5L12 4L21 11.5'), path(HOUSE_WALLS), path('M16 7.2V4.5H18.5V9.2'), ring(12, 14, 2)],
  'g:code': [path('M8.5 7.5L4 12L8.5 16.5M15.5 7.5L20 12L15.5 16.5M13.5 5.5L10.5 18.5')],
  'g:book': [
    path('M12 7C10.5 5.5 8 5 3.5 5.5V18C8 17.5 10.5 18 12 19.5C13.5 18 16 17.5 20.5 18V5.5C16 5 13.5 5.5 12 7Z M12 7V19.5'),
  ],
  'g:cart': [path('M2.5 4.5H5L7.7 15.5H18.2L20.7 8H5.9'), dot(9.5, 19.5, 1.6), dot(16.5, 19.5, 1.6)],
  'g:game': [box(2.5, 6.5, 19, 11, 5), path('M7 10.5V14.5M5 12.5H9'), dot(15.5, 11.5, 1.1), dot(18, 14, 1.1)],
  'g:music': [dot(6.5, 17.5, 2.3), dot(16.5, 15.5, 2.3), path('M8.8 17.5V6.5L18.8 4.5V15.5M8.8 10.5L18.8 8.5')],
  'g:heart': [path(HEART), path('M6.5 9.5Q6.7 7.8 8.3 7.3')],
  'g:star': [
    [
      'polygon',
      {
        points:
          '12,3.4 14.53,9.42 21.04,9.96 16.09,14.23 17.58,20.59 12,17.2 6.42,20.59 7.91,14.23 2.96,9.96 9.47,9.42',
      },
    ],
  ],
  'g:bolt': [path('M13 2.5L5.5 13.5H11.5L10.5 21.5L18.5 10.5H12.5Z')],
  'g:globe': [ring(12, 12, 9), ['ellipse', { cx: '12', cy: '12', rx: '4', ry: '9' }], path('M3 12H21')],
  'g:leaf': [path('M5 19C4 10.5 9 5 19 4.5C19.5 14.5 14.5 20 5 19Z M3.5 20.5L13 11')],
};

/** An inline SVG for `name`, stroked with currentColor at 1.6 units on the 24-unit grid. */
export function railIcon(name: RailIconName): SVGSVGElement {
  const svg = document.createElementNS(SVG_NS, 'svg');
  const root: readonly [string, string][] = [
    ['viewBox', '0 0 24 24'],
    ['fill', 'none'],
    ['stroke', 'currentColor'],
    ['stroke-width', '1.6'],
    ['stroke-linecap', 'round'],
    ['stroke-linejoin', 'round'],
    ['aria-hidden', 'true'],
    ['focusable', 'false'],
  ];
  for (const [key, value] of root) svg.setAttribute(key, value);
  for (const [tag, attrs] of RAIL_SHAPES[name]) {
    const node = document.createElementNS(SVG_NS, tag);
    for (const [key, value] of Object.entries(attrs)) node.setAttribute(key, value);
    svg.append(node);
  }
  return svg;
}
