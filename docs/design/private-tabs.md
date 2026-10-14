# Private tabs: visual spec

Owner: private tabs. Status: implemented. Behaviour and persistence rules live in `src/private.rs`
and are unchanged by this spec; it covers only how a private tab looks, moves and is worded.

## Principle

A private tab is an ordinary tab with one extra colour and one mark. Normal tabs, the normal pill and
every shared component keep their look. Private differs only by the private violet and the private
mark (bowler and shades), never by layout, size or the theme accent, so it reads as private whatever
accent the profile chose.

## Tokens (`ui/src/styles/tokens.css`)

| Token | Dark | Light | Use |
|---|---|---|---|
| `--bw-private` | #b9a2ff (7.2:1 on #20242c) | #6440d0 (6.1:1 on #f4f6f8) | marks, underline, edge bar, chip glyph; text-grade |
| `--bw-private-soft` | violet 20% | violet 13% | chip and eyebrow wash, landing icon tiles |
| `--bw-private-edge` | violet 50% | violet 50% | island border, resting pill edge, landing button |

Only selectors keyed on `data-private`, `.tb-private` or the landing (`.pm-*`) may use them
(`ui/test/private-page.mjs` checks this).

## The mark (`icons/extra/private.svg`, family rules of `docs/design/menu-icons.md`)

24-unit grid, 1.5 stroke, round caps and joins, `currentColor`. Shapes, in source order: brim, bowler
crown, left lens, right lens, bridge. The lenses are filled (solid shades), which is what keeps the
mark legible at 14-16px where outlined lenses close up. `glasses.svg` is the shades alone (filled,
no stroke) for the 8px favicon badge; `glint.svg` is two parallel strokes used only by the landing.
The landing addresses the shapes through `MARK_PARTS` (`ui/src/private-logic.ts`); a test fails if
the drawing and the indices drift.

## Where private shows

- **Strip tab / sidebar row:** violet border (50%, 75% when active) and a 2px violet underline (strip)
  or leading bar (sidebar), both inset shadows, so no geometry changes. No favicon: the mark in
  violet replaces the globe. Favicon or spinner: a 10px badge (opaque window disc, 1px violet ring,
  violet shades) in the icon slot's corner.
- **Address bar:** the "Private" label (violet mark, primary text, no bubble behind it) after the address;
  the pill's resting edge takes `--bw-private-edge`. Focus ring and the open suggestions list keep
  the shared ring, so the pill and the dropdown still meet seamlessly.
- **Landing:** a blank private tab outside split view shows the private landing (below). Inside a
  split the empty side keeps the "choose a tab" picker.
- **Window title:** `[Private]` suffix (unchanged).

## Landing (`ui/src/private-page.ts`, `ui/src/styles/private.css`)

Hosted by the split view's existing HTML host (`View::Split`, `src/win32/split_ui.rs`,
`Shown::Private`), so it adds no WebView2: the host is created on first use and suspended when
hidden. Rust sends `{"v":1,"t":"privatepage","id":<wire tab id>,"n":<private tabs>}`; the page's
only command is `{"v":1,"t":"split","a":"closeprivate"}`. The overlay frames the landing like a page
(rounded mask and edge) instead of painting the opaque new-tab surface over it. The omnibox keeps
the keyboard; the landing never takes focus.

Layout: 80px mark on a soft violet halo, "Private tab" eyebrow, title, one-sentence lead, then
**Close all private tabs** with the open count, then two cards ("Bowser won't keep", "Still visible
to") and a note that private tabs share one session. Every sentence is checked against the code
(`private-logic.ts` header lists the sources); change the code and the words together.

## Wording

"New private tab" everywhere (main menu, tab menu, command palette, Ctrl+Shift+P), detail "Browse
without saving history, cookies or site data". "Close all private tabs" in the main menu (only while
one is open), a private tab's menu, the palette and the landing; it closes this window's private
tabs only (`App::close_private_tabs`).

## Motion

Transform and opacity only, every animation finite, nothing under `prefers-reduced-motion` (the
chip and the landing just fade in), nothing in native paint.

| What | When | Duration | Cost |
|---|---|---|---|
| Chip pop (scale .6 → 1.08 → 1) and shades drop (3 units) | the chip appears | 240ms | one small element |
| Strip mark: shades settle | a private tab enters the strip | 240ms | 16px SVG |
| Landing: hat rises, shades slide on with a 0.6-unit settle | first show for a tab | 320ms + 560ms | 80px SVG, main thread |
| Landing: glint across both lenses (clipped to the lens fills) | after the shades land, and once more at ~7s | 900ms each | same SVG |
| Landing: text and cards rise 8px | first show for a tab | 250ms | compositor |

Showing the same tab's landing again comes back settled (no replay). After about 8.1s nothing on
the landing runs at all.
