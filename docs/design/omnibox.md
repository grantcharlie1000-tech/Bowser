# Omnibox: the pill and its suggestions

Status: implemented on `ui/work`. Covers the toolbar pill (`ui/src/view.ts`, `styles/toolbar.css`) and
the suggestion list under it (`ui/src/hover.ts`, `suggest-logic.ts`, `styles/hover.css` `.bw-suggest*`,
drawn in the hover popup by `src/win32/html_hover.rs`, with its backdrop in `src/win32/corner_fringe.rs`).

Intent: one calm surface. The pill is a quiet field at rest. On focus it lifts onto a raised
material. When suggestions open it grows down into them with no seam and no outline. The selection
is a single soft highlight that follows the keyboard without lag.

## Decisions kept from earlier rounds

- No outline or focus ring on the pill or the list while the list is open. Fill and elevation do
  the separating (the native shadow, below).
- Row highlights are concentric with the open surface's rounded bottom. That is now true for every
  row, not only the last.
- In the `tabs-top` layout the pill fills the toolbar width (`src/chrome/layout.rs`, unchanged here).
- Tabs, nav buttons and the rail are unchanged. The private label has no bubble: a violet mark and
  text only.
- Geometry is unchanged: 30px toolbar row (`Metrics::STRIP_HEIGHT`; the CSS token must match it or the pill sits off-centre), 26px pill, `--bw-r-omnibox` = 13px. The protocol is
  unchanged.

## Tokens (`ui/src/styles/tokens.css`)

| Token | Value | Use |
|---|---|---|
| `--bw-omni-material` | `linear-gradient(fill, fill), --bw-window-opaque` | Focused pill, open pill, the list and the page behind it. Rust paints the same colour (`html_hover.rs suggest_surface`). |
| `--bw-omni-edge` / `-hover` | text 12% / 20% | Resting / hovered pill hairline |
| `--bw-omni-row-hover` | text 5% | Row under the pointer |
| `--bw-omni-row-selected` | text 11% (light 10%) | The selection indicator |
| `--bw-omni-chip` | text 8% | "Switch to tab" chip, Enter keycap |
| `--bw-sh-omni` | `0 1px 2px` + `0 0 1px` black, 28/24% (light 14/18%) | Lift of the focused or open pill. It fits inside the toolbar's 4px margin. |
| `--bw-text-omni` | 13px | Pill text and row titles (addresses stay at 12px Caption) |
| `--bw-r-omni-row` | `--bw-r-omnibox − 4px − 1px` = 8px | Every row and the indicator |

## Pill states

The border is always 1px and the radius is always 13px, so nothing inside the pill moves between
states.

| State | Fill | Edge | Extra |
|---|---|---|---|
| Rest | `--bw-fill` | `--bw-omni-edge` | Private tabs use `--bw-private-edge` |
| Hover | fill + `--bw-hover` wash | `--bw-omni-edge-hover` | 83ms colour fade |
| Focus (closed) | `--bw-omni-material` | transparent, no outline | `--bw-sh-omni` |
| Open | `--bw-omni-material` (also set on `.is-open`, so a press on a row never flashes the resting fill) | transparent, ring off | Bottom corners 0; progress bar hidden |

Buttons inside the pill use `--bw-text-2` at rest and `--bw-text` on hover, when pressed, or on
`:focus-visible`. Not-secure and certificate-problem states keep the warm `--tb-warn`. Keyboard
focus keeps the shared `.bw-focus-inset` ring.

Alignment: the site button sits 4px in, tucked into the corner, the same inset as the last trailing
button. The pill text starts at 1 + 4 + 20 + 10 = 35px, the same x as the row titles
(1 + 4 + 6 + 16 + 8). The typed text and the suggestions read as one column.

## Rows

- 32px tall and single-line. A 16px lead (the favicon, or the kind's glyph), then a 13px title and a
  12px `--bw-text-2` address on one baseline. Both truncate, and the title takes at most 70% of the
  row. Below 440px of list width the address column is dropped (container query).
- Kinds at a glance. Search shows a magnifier. A typed address shows a globe or its favicon. History
  shows a clock or the favicon. An open tab gets the "Switch to tab" chip. A bookmark shows the
  bookmark glyph, and if its favicon took the lead slot, a 12px labelled bookmark mark after the
  title (`suggestBadge`), so the option's name says "Bookmark".
- The match is emphasised by weight (600) in the title and by colour (`--bw-text`) in the address.
  The search row's own text is never emphasised.
- The selected row shows a quiet Enter keycap (the new `enter` glyph, 12px, in a `--bw-omni-chip`
  cap). Every row keeps the keycap's box (`visibility`), so nothing shifts as the selection passes.
- Selection: one `.bw-suggest-sel` layer under the rows (`z-index: -1` in an isolated list), moved
  with `transform: translateY(var(--suggest-sel) * 100%)`. It has the rows' own 8px radius, so it is
  concentric with the bottom wherever it is. Hover is a per-row wash on top of it.
- Forced colours: the selected row gets a 1px `Highlight` outline.

## Motion

| What | Property | Duration | Easing | Trigger | Reduced motion |
|---|---|---|---|---|---|
| Pill hover | background-color, border-color | 83ms | linear | pointer | Snaps (the base rule allows opacity only) |
| Pill focus | material, edge, lift | 83ms colour; lift at once | linear | focus | Colours snap |
| Ring out on open, in on close | `::after` opacity, border-color | 83ms | linear | `suggestOpen` | Opacity fade kept (≤ 83ms), colour snaps |
| Bottom corners square / round | border-radius | instant | | `suggestOpen` | Same |
| List unroll (native) | window region height | 167ms | quint out | First show | None when Windows animations are off (`SPI_GETCLIENTAREAANIMATION`, which Chromium also maps to prefers-reduced-motion) |
| List roll back (native) | region height | 100ms | cubic out | Escape, blur, nothing to show | Instant |
| Pick or submit | | instant | | | |
| Shadow and corner fringe (native) | layered window, follows the region | Per region frame | | | Instant with the region |
| Rows settle in | opacity 0→1, `translate` −4px→0 | 167ms, rows staggered 10ms (row 8 starts at 70ms; all done by 237ms) | `--bw-ease-out` | First open only (`.is-entering`) | 83ms fade, no stagger, no movement |
| Selection | transform translateY | 83ms | `--bw-ease-out` (0,0,0,1): about 70% of the way in the first frame | `suggestSel` once the selection has moved (`.is-tracking`) | Instant |
| Typing (rows replaced) | | instant | | | |

Everything is finite and moves only transform or opacity (the native region aside). A held arrow
key retargets the 83ms slide on each step, so it never trails. A list that is re-rendered while
typing starts without `.is-tracking`, so its indicator is placed, not slid from the old row.

## Native pieces (`html_hover.rs`, `corner_fringe.rs`)

- `suggest_surface(palette)`: the omnibox fill over the opaque window colour, with the fill alpha
  rounded as `css_colour` sends it. This is the list's real colour. It is used for the
  anti-aliased corner pixels (previously the hover cards' overlay tint, a few levels darker than the
  list) and for `WM_PAINT` behind the WebView while the dropdown shows.
- `corner_fringe::Backdrop`: one click-through layered window under the dropdown, 24px wider on each
  side and 32px taller (at 96 dpi; scaled with DPI). It paints the partly covered corner pixels over
  the shadow, a two-layer shadow (ambient σ 1px, key σ 8px dropped 8px; dark 30/32%, light 16/14%),
  and the opaque surface under the list (hidden by the list, a guard against a frame of mismatch).
  There is no shadow above the list's top edge, which is the seam with the pill.
- Cost: the bitmap is allocated once per open, at the list's full height. It is repainted only when
  the visible height, width, radius or colours change: about 10 times during the 167ms unroll, then
  once per height change while typing. A repaint fills about 200k pixels at 100%, but evaluates the
  shadow (erfc) only in the margins and the corner squares, about 36k pixels. A move alone is one
  `UpdateLayeredWindow`. No timers beyond the existing unroll timer.

## Accessibility

Roles are unchanged: the toolbar input (named "Address and search bar"), `listbox` and `option` with
`aria-selected` in the popup. The input does not take `role=combobox`: the list lives in another
window, so `aria-controls` and `aria-activedescendant` cannot point at it. Arrow keys keep announcing
through the input's previewed value. Decorative parts are `aria-hidden` (the lead, the keycap and
the indicator). The bookmark mark is a labelled image. Text contrast:

- Titles use `--bw-text` and addresses `--bw-text-2` on the material, which is the pill's own pair.
  `theme.rs` guarantees at least 4.5:1 for it.
- The selection and hover washes are 5–11% text tints, so they keep the text pair's contrast.
- The focus edge is the full `--bw-focus` (at least 3:1 against the material in all four palettes).

## Verification

- Node: `ui/test/suggest.mjs` holds the CSS contract (one material, no outline, one curvature,
  concentric rows, indicator, motion and reduced motion, the shared column) and a fake-DOM render of
  the hover view (kinds, mark, keycap, indicator, selection, stale moves, typing).
- Rust: `corner_fringe` tests (coverage, mirrored premultiplied pixels, surface under the list,
  shadow falloff, no-shadow fringe, tiny lists, erfc) and `html_hover` tests (surface colours,
  shadow scaling).
- Headless renders: `.context/omnibox/` (harness: `.context/omnibox/harness`). These are Edge
  screenshots of the toolbar and hover views composed over a fake page. The native shadow is
  emulated there with the same two CSS shadows, and the unroll with a clip.
