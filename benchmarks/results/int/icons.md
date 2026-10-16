# Wave-1 icons and glyphs (T1 to T5)

Method: every icon name added by each thread's diff against origin/main (`icon('..')`, `.glyph("..")`,
icon maps), checked against the pipeline (`icons/{menu,rail,extra}/*.svg` -> `ui/src/icons.gen.ts`,
including its ALIASES), plus any inline SVG.

## New drawn icons

| icon | thread | source | style |
|---|---|---|---|
| `qr-code` | T2 | icons/extra/qr-code.svg, via the pipeline | matches: 24 grid, stroke 1.5, round caps and joins |
| `calculator` | T2 | icons/extra/calculator.svg, via the pipeline | matches the same stroke rules |

No thread hand-drew an icon outside the pipeline. The only inline SVG is the QR code itself
(`ui/src/address-dialog.ts`), which is generated data, not an icon.

## Existing icons reused (all from the pipeline)

- T1: clock, close, edit, folder, reload, reset, save, trash
- T2: bookmark, calculator, chevron-down, chevron-right, copy, enter, external, folder, gear, history (alias), pin, plus, reset, search, tab-switch, trash
- T3: cookie, folder, plus, shield
- T4: close, command, info, play, plus, reset, search, trash
- T5: chevron-left, chevron-right, clock, close, edit, folder, globe (alias), grip, notes, plus, shield, workspace-book, bookmark
- Integration: palette glyphs for the new registry commands (reset, clock, save, folder-open, reload, trash, shield, qr-code, search); Address bar nav glyph `workspace-globe`

## Reuses that read wrong (for the icon pass to redraw or replace)

- T1 tab strip "Closed tabs" button uses `trash` (ui/src/tabs.ts). It reads as delete; it needs a closed-tabs or history glyph.
- T5 Date widget uses `workspace-book` (ui/src/start-page.ts widgetIcon). It is a workspace badge glyph, not a calendar.
- T4 Quick commands section nav uses `link`; T2's Address bar used `link` too. The integration moved Address bar to `workspace-globe`, also a workspace badge glyph. Both sections want their own glyph (a palette/command-bar glyph and an address-field glyph).
- The new "Show or hide the address bar" palette command uses `search` for lack of an address-bar glyph.
