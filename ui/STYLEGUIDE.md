# Bowser HTML chrome style guide

The rules every surface of the HTML chrome follows: tokens, shared components, icons, motion, and
who owns which file. Product intent is in `PRODUCT.md`; the numbers come from
`docs/design/reference-spec.md`; the bridge (how Rust sends theme colours) is in
`docs/html-chrome-protocol.md`.

## Build

```sh
cd ui
npm install
npm run build      # icons -> src/icons.gen.ts, esbuild -> dist/app.js, Tailwind -> dist/app.css, manifest
npm run typecheck
npm run lint       # no innerHTML/eval/inline handlers in src/ or dist/app.js
npm test           # includes test/styles.mjs (CSS contract) and test/icons.mjs (icon family)
```

- CSS is compiled by **Tailwind CSS v4** through PostCSS in `build.mjs` (minified by Lightning CSS)
  into one static `dist/app.css`. There is no runtime, no `<style>` element and no inline style
  attribute, so the CSP (`style-src 'self'`, Trusted Types) is unchanged. Setting custom
  properties from TS with `element.style.setProperty('--x', v)` is CSSOM, which the CSP allows;
  never build a `style="..."` string.
- Tailwind scans only `src/**/*.ts` and `src/index.html` (`source(none)` plus explicit `@source`),
  so the output depends on `src/` alone and the same sources always give the same bytes.
- `dist/` and `dist.manifest` are committed; Cargo embeds them with `include_bytes!` and `build.rs`
  refuses a `dist/` that does not match the manifest. Rebuild and commit them with any `src/` change.

## Files and ownership

| File | Owner | Contents |
|---|---|---|
| `src/styles/app.css` | foundation | Entry: layer order, imports, Tailwind sources |
| `src/styles/tokens.css` | foundation | `--bw-*` tokens, host-input fallbacks, Tailwind `@theme` |
| `src/styles/base.css` | foundation | Reset, body type, global reduced-motion rule |
| `src/styles/components.css` | foundation | Shared `.bw-*` components and motion classes |
| `src/styles/utilities.css` | foundation | Custom Tailwind utilities (`tw:duration-fast`, `tw:focus-ring`, `tw:enter`, `tw:exit`) |
| `src/styles/toolbar.css` | toolbar / omnibox | `?view=toolbar` (`view.ts`), plus the shared `#app` default |
| `src/styles/tabs.css` | tabs | `?view=tabs`, horizontal strip (`tabs.ts`) |
| `src/styles/sidebar.css` | sidebar | `?view=tabs` in vertical layout (`sidebar.ts`) |
| `src/styles/rail.css` | rail | `?view=rail` (`rail.ts`) |
| `src/styles/panels.css` | panels | `?view=panel` (`panel.ts`, `panels/*.ts`) |
| `src/styles/dialog.css` | dialogs | `?view=dialog&p=...` (`dialog.ts`) |
| `src/styles/menu.css` | menus | `?view=popup` (`menu-view.ts`, `popup.ts`) |
| `src/styles/settings.css` | settings | `?view=settings`, the `bowser://settings` page (`settings-page.ts`, docs/design/settings-page.md) |
| `src/styles/history.css` | history | `?view=history`, the `bowser://history` page (`history-page.ts`, docs/design/history-page.md) |
| `src/styles/downloads.css` | downloads | the download bubble (`?view=popup` page `downloads`, `downloads-bubble.ts`) and the `bowser://downloads` page (`?view=downloads`, `downloads-page.ts`); shared row `downloads-view.ts`; docs/design/downloads.md. The toolbar's download button lives in `toolbar.css` |
| `src/icons.ts`, `scripts/gen-icons.mjs`, `icons/` | foundation | Icon family |

Each surface file starts with a `Surface: NAME ... Owner:` header (enforced by `test/styles.mjs`).
Edit only your own surface file; propose token or component changes to the foundation owner.

### Cascade layers

```
theme < base < components < surfaces < utilities      (inline styles beat all of them)
```

A surface rule always beats a component rule, and a `tw:` utility in markup beats both, without
`!important` or specificity games. Surfaces are imported in the order the old single `app.css`
had them, so equal-specificity rules resolve exactly as before; append new surfaces at the end.

## Tokens (`src/styles/tokens.css`)

**Rule: surfaces read `--bw-*` tokens only.** The raw host inputs (`--bg`, `--fill`, `--border`,
`--focus-border`, `--text`, `--placeholder`, `--completion`, `--selection`, `--hover`,
`--progress`) are written by `view.ts applyTheme()` from the `theme` object of every `state`
message; `test/styles.mjs` fails if any file other than `tokens.css` reads them.
`<html data-scheme="light|dark">` and `body.solid` come from the same message.

### Colour (themed by Rust)

| Token | From | Use |
|---|---|---|
| `--bw-window` | `bg` | solid window colour; panel and popup backgrounds |
| `--bw-fill` | `fill` | control fill: omnibox pill, nav group, inputs |
| `--bw-border` | `border` | control border, strong hairline |
| `--bw-accent` | `focusBorder` | theme accent |
| `--bw-focus` | accent | focus rings |
| `--bw-text` / `--bw-text-2` | `text` / `placeholder` | primary / secondary text |
| `--bw-completion`, `--bw-selection`, `--bw-hover`, `--bw-progress` | same names | omnibox completion, selection, hover wash, load bar |
| `--bw-pill-edge` | text 22% | `.bw-pill` component border, rail rule, card marker ring |
| `--bw-omni-*` | see docs/design/omnibox.md | omnibox: `-material` (focused/open pill and the list), `-edge`, `-edge-hover`, `-halo`, `-row-hover`, `-row-selected`, `-chip`; with `--bw-sh-omni`, `--bw-text-omni` 13px, `--bw-r-omni-row` 8px |
| `--bw-island` | text 13% | active tab / row island |
| `--bw-island-idle` | black 22% | inactive island |
| `--bw-island-active` | text 24% (light: white 92%) | the selected tab: the strongest fill in the strip |
| `--bw-text-dim` | text-2 (light: text 86%) | inactive tab titles and rail glyphs; keeps >= 4.5:1 text / 3:1 glyphs on mid-grey light glass |
| `--bw-island-edge` | border 45% | inactive island outline |
| `--bw-island-hover` | hover | island hover wash |
| `--bw-glyph-fill`, `--bw-on-glyph` | accent 35% over window, white | placeholder favicon tile and its glyph |
| `--bw-accent-soft` | accent 22% | selected-state wash |

### Colour (fixed per scheme, from reference-spec; dark default, `[data-scheme=light]` overrides)

`--bw-surface-glass`, `--bw-overlay` (menus, popups, flyouts, dialogs), `--bw-hairline`
(separators), `--bw-focus-inner` (1px contrast ring), `--bw-on-accent` (text on accent fills; dark
in the dark scheme because Rust's dark accents are light), `--bw-scrim`.
Group palette: `--bw-group-0` ... `--bw-group-7` (tabs.ts / sidebar.ts map Rust's colour ids).
Private violet: `--bw-private` (#b9a2ff dark, #6440d0 light; text-grade on both), `--bw-private-soft`
(chip wash), `--bw-private-edge` (outline). Only private surfaces use them; see
docs/design/private-tabs.md.

### Type

`--bw-font` (Segoe UI Variable), `--bw-text-chrome` 12px/1 (toolbar, tabs, rail, sidebar),
Caption `--bw-text-caption`/`--bw-leading-caption` 12/16, Body 14/20, Subtitle 20/28,
`--bw-weight-strong` 600. Sentence case, no italics, no uppercase eyebrows.
`--bw-font-panel` ("Segoe UI", 13px callers) is legacy for panels and the menu popup.

### Geometry

| Token | Value | Note |
|---|---|---|
| `--bw-r-control` | 4px | buttons, inputs, tooltips, menu items |
| `--bw-r-row` | 6px | list and sidebar rows |
| `--bw-r-island` / `--bw-r-overlay` | 8px | cards / menus, flyouts, dialogs |
| `--bw-r-pill` | 999px | capsules |
| `--bw-r-tab` | 9px | tab, rail and sidebar islands (tuned to the pill) |
| `--bw-toolbar-h` | 30px | must equal `Metrics::STRIP_HEIGHT` in `src/chrome/layout.rs` (the host region's height), or the pill is centred off the native layout |
| `--bw-pill-h` / `--bw-tab-h` | 26px | **hand-tuned**; tabs and rail width follow it |
| `--bw-nav-h` | 28px | toolbar buttons and nav group |
| `--bw-icon` / `--bw-icon-box` | 16px / 20px | glyph / icon button |
| `--bw-icon-stroke` / `-fine` | 1.5px / 1px | rendered stroke (fine for glyphs of 12px or less) |
| `--bw-space-0_5 ... --bw-space-6` | 2, 4, 6, 8, 12, 16, 24px | 4px base; no 5 or 10 |

### Elevation, motion, layers

`--bw-sh-tip`, `--bw-sh-flyout`, `--bw-sh-dialog`;
`--bw-dur-fast` 83ms (opacity, colour), `--bw-dur-base` 167ms (position, scale),
`--bw-dur-slow` 250ms (sidebar width), `--bw-dur-spin` 900ms;
`--bw-ease-out` cubic-bezier(0,0,0,1), `--bw-ease-move` cubic-bezier(.55,.55,0,1), `--bw-ease-fade` linear, `--bw-ease-in` cubic-bezier(.4,0,1,1) (exits);
z-layers `--bw-z-page` 0, `-chrome` 10, `-sidebar-flyout` 20, `-menu` 30, `-popup` 40,
`-tooltip` 50, `-palette` 60, `-dialog` 70 (local stacking inside one surface may use 1-3).

### Tailwind utilities

Utilities are **prefixed `tw:`** so they can never collide with the legacy class names
(`group`, `hidden`, `grid` ...). Only Bowser tokens exist in the theme (`--*: initial`):

- colours: `tw:bg-island`, `tw:text-text-2`, `tw:border-hairline`, `tw:bg-overlay`, `tw:text-accent`, `tw:bg-group-3` ...
- spacing on the 4px base: `tw:p-2` = 8px, `tw:gap-3` = 12px, `tw:h-8` = 32px
- radius `tw:rounded-control|row|island|overlay|tab|pill`; shadow `tw:shadow-tip|flyout|dialog`
- type `tw:text-caption|body|subtitle|chrome`, `tw:font-strong`; easing `tw:ease-out|move|fade`
- custom: `tw:duration-fast|base|slow`, `tw:focus-ring`, `tw:enter`, `tw:exit`, `tw:animate-spin`
- variants: `tw:dark:` / `tw:light:` follow `data-scheme` (Rust), not the OS media query.

Prefer a component class for anything repeated; use utilities for one-off layout.

## Components (`src/styles/components.css`)

All classes are `bw-` prefixed. State uses ARIA where it exists (`aria-checked`, `aria-pressed`,
`aria-selected`, `aria-disabled`, `aria-expanded`, `aria-current`) and `.is-*` otherwise.

| Class | Markup / parts | Notes |
|---|---|---|
| `.bw-focus-ring` | any focusable | 2px accent outside a 1px contrast ring on `:focus-visible`; built into the components below |
| `.bw-focus-inset` | add to the element | ring drawn inward, for clipped containers |
| `.bw-icon` | `icon()` output | sizing, stroke and non-scaling strokes; `.is-fine` |
| `.bw-icon-btn` | `<button><svg/></button>` | 20px box, 16px glyph, 4px radius; `.is-lg` 28px, `.is-round`; hover / active / `aria-pressed` / disabled |
| `.bw-pill` | container | omnibox capsule, accent border on `:focus-within` |
| `.bw-island` | tab or row | idle tint, hover wash, raised on `.is-active` / `aria-selected` / `aria-current` |
| `.bw-overlay` | container | menu/flyout surface; `.is-attached-top` squares the top corners |
| `.bw-menu` | `role="menu"` | 200px min, 4px padding, Body type |
| `.bw-menu-item` | `role="menuitem"` button: `svg`, `.bw-menu-label`, `.bw-menu-accel`, `.bw-menu-check`, `.bw-menu-chevron` | 32px, 4px radius, 12px icon gap; `.is-selected` |
| `.bw-separator` | `role="separator"` | 1px hairline, 4px margin; `.is-vertical` |
| `.bw-section-header` | heading | Caption strong, secondary colour, sentence case |
| `.bw-row` | list item: `.bw-row-text` > `.bw-row-title` + `.bw-row-meta` | 32px; `.is-tall` 56px, `.is-setting` 48px with hairline |
| `.bw-input` | `<input>` | 32px, Body, accent border on focus |
| `.bw-button` | `<button>` | 28px secondary; `.is-primary` accent fill, `.is-subtle` |
| `.bw-toggle` | `<button role="switch" aria-checked>` | 40x20 track, thumb slides with `--bw-ease-move` |
| `.bw-segmented` | `role="radiogroup"` > `button[role=radio][aria-checked]` | also accepts `aria-pressed` / `aria-selected` |
| `.bw-dialog` | `.bw-dialog-title`, `.bw-dialog-body`, `.bw-dialog-actions` | 8px radius, dialog shadow, 24px padding |
| `.bw-tooltip` | `role="tooltip"` | Caption, 4px radius, 6x8 padding, tip shadow |
| `.bw-scroll` / `.bw-scroll-hidden` | scroller | thin overlay thumb / hidden bar |
| `.bw-spinner` | status | 12px ring |
| `.bw-enter` (`.is-down`, `.is-rise`, `.is-scale`), `.bw-exit` | entering / leaving element | see Motion |

Menus today: `menu-view.ts` emits `.html-menu`, `.html-menu-row`, `.html-menu-label`,
`.html-menu-shortcut`, `.html-menu-check`, `.html-menu-arrow`, `.html-menu-separator`,
`.html-menu-section`, `.html-menu-back`, styled in `menu.css` exactly as before. The menu owner can
either restyle those there or add the `.bw-menu*` classes alongside them.

## Icons (`src/icons.ts`)

One family: 24-unit grid, 1.5px stroke in **rendered** pixels, round caps and joins,
`currentColor`. Sources are SVG files in `icons/menu/` (also drawn natively by
`src/win32/menu.rs`), `icons/rail/` (also ported in `src/chrome/rail_icons.rs`) and
`icons/extra/`. `npm run icons` (part of `npm run build`) generates `src/icons.gen.ts` and the
gallery `icons/preview.html` (every glyph at 16/20/24px, light and dark, 1x and 2x).

```ts
import { icon } from './icons';
button.append(icon('download'));                       // 16px, decorative (aria-hidden)
close.append(icon('close', { size: 12, fine: true })); // small glyph, 1px stroke
status.append(icon('warning', { label: 'Not secure' })); // role="img" + aria-label
```

- Sizes: 16px is the default glyph (in a 20px `.bw-icon-btn`); 12-14px inside dense controls with
  `fine` at 12px and below; 20/24px only for empty states. Surface CSS may set `width`/`height`.
- Strokes use `vector-effect: non-scaling-stroke`, so a 14px and a 16px glyph have the same weight
  at every DPI (1.5px = 1.5/1.875/2.25/3 device px at 100/125/150/200%). Shapes keep antialiasing
  (`shape-rendering: geometricPrecision`); never use `crispEdges` (it breaks diagonals and curves).
- Glyphs (106): app-tile, archive, back, bell, bookmark, camera, caption-maximize, caption-minimize,
  caption-restore, cast, check, chevron-down/left/right/up, clock, close, close-others, command, cookie, copy,
  customize, download, droplet, edit, enter, external, eye, file, file-text, folder, folder-download, folder-open,
  forward, gear, glasses, glint, grid, grip, heart, home, image, info, key, link, location, lock, menu, menu-dots,
  mic, more-horizontal, more-vertical, mute, notes, package, pause, person, pin, pip, play, plus, print, private,
  private-window, puzzle, reader, reload, reset, save, search, share, shield, sidebar, split,
  split-bottom, split-bottom-left, split-bottom-right, split-left, split-right, split-stacked, split-top, split-top-left, split-top-right, swap, tab-group, tab-switch, translate, trash, unlock, unsplit, video, volume, warning, window, workspace, workspace-add, workspace-{bolt, book,
  briefcase, cart, code, game, globe, heart, home, leaf, music, star}, zoom-in, zoom-out.
- Aliases (21): arrow-left/right, caption-close, dots, extensions, find, globe, history, more,
  new-tab, palette, pencil, picture-in-picture, profile, refresh, settings, sidebar-toggle,
  split-view, star, stop, tab-close.

### Adding an icon

1. Draw it on the 24 grid with 2px padding and 2px corner radius: root attributes
   `viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5"
   stroke-linecap="round" stroke-linejoin="round"`; children carry geometry only (filled dots use
   `fill="currentColor" stroke="none"`).
2. Save it as `icons/extra/<name>.svg` (kebab-case). For a second name of an existing glyph, add
   an entry to `ALIASES` in `scripts/gen-icons.mjs` instead of copying the file.
3. `npm run build`, check `icons/preview.html`, commit the SVG, `src/icons.gen.ts`, the preview and `dist/`.

The generator fails on off-family attributes, unknown elements, name clashes and two names that
draw identical shapes.

## Motion

- Animate `transform` and `opacity` only. Never transition width, height, top/left, margin or
  padding. A size change (sidebar collapse) is a host window resize or a transform, not a CSS tween.
- Never delay input: no `pointer-events: none` on an interactive element during a transition, no
  transition on focus or caret, nothing that runs before a click is handled.
- Durations: `--bw-dur-fast` for fades and hover colour, `--bw-dur-base` for movement and scale,
  `--bw-dur-slow` for large surfaces. Easing `--bw-ease-out` for enters, `--bw-ease-move` for
  point-to-point, `linear` (`--bw-ease-fade`) for fades, `--bw-ease-in` for exits.
- Micro motion on a single control (the toolbar menu button's three dots, toolbar.css `.tb-menu`)
  may use `--bw-ease-spring` (a small overshoot) and `--bw-stagger` (30ms between siblings). It
  moves the glyph's own shapes with transform/translate only, rests on the untouched pixel-snapped
  glyph, and sits inside `@media (prefers-reduced-motion: no-preference)`.
- Enter: add `.bw-enter` (opacity 0 -> 1 with 4px drop and .98 scale, 167ms out-ease).
  `.is-down` drops from above (menus under the toolbar), `.is-rise` rises 8px
  (palette, dialogs), `.is-scale` scales only.
- Omnibox suggestions are not a separate popover: the pill grows down into them
  (docs/design/omnibox.md). The focused pill, the open pill and the list share one raised material
  (`--bw-omni-material`). While Rust says the list is on screen (`suggestOpen`), `.pill.is-open`
  squares only the pill's bottom corners and fades its focus edge out (83ms): no outline while open.
  The list's bottom corners use `--bw-r-omnibox`, and a native shadow under the popup separates it from
  the page. Rust unrolls the popup's window region (167ms quint out; 100ms cubic out on
  Escape/blur; instant on pick or submit; none when Windows animations are off). The list container
  never transforms: its rows settle in once on first open (167ms, 10ms stagger), and the selection
  indicator slides between rows (83ms, front-loaded; instant under reduced motion).
- Exit: add `.bw-exit` (83ms fade) and remove the element on `animationend`; it keeps receiving
  input while it fades.
- Reduced motion (`prefers-reduced-motion: reduce`): `--bw-dur-base` and `--bw-dur-slow` become
  0ms, a base-layer rule limits every transition to `opacity` over <= 83ms, and `.bw-enter` becomes
  a plain 83ms fade. Spinners keep turning (slower) because they carry status.

## Focus and accessibility

- Every interactive element is a real `<button>`, `<input>` or has the right role, an accessible
  name (`aria-label` for icon-only buttons; `title` doubles as the native tooltip) and is reachable
  by keyboard.
- Visible focus uses `:focus-visible` only: the shared ring (2px accent + 1px contrast) or the inset
  variant inside clipped containers. Legacy surfaces still use a 2px inset ring; owners should move
  to the shared ring when restyling.
- Text contrast >= 4.5:1 over the glass is guaranteed by `theme.rs` for the themed tokens; keep
  secondary text on `--bw-text-2`; the 60% accelerator text in menus is the only alpha-text exception.

## Adding a surface

1. Add `src/styles/<surface>.css` with the header comment (`Surface: NAME (?view=...). Owner: ...`).
2. Import it at the end of the surface list in `src/styles/app.css` with `layer(surfaces)`.
3. Scope every selector under the view root (`#app.<surface>` or a surface-specific class) so it
   cannot leak into other views; `#app` alone is the toolbar's default.
4. Build from the shared components and tokens; add a token to `tokens.css` (and the table above)
   rather than a literal colour, radius, duration or z-index.
5. Icons come from `icon()`; never inline path data in a surface module.
6. `npm run build && npm run typecheck && npm run lint && npm test`, then check the surface in
   light and dark at 100% and 150% (the headless harness in `benchmarks/results/foundation/harness/`
   renders every view from a `dist/` folder with fake host messages).
