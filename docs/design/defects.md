# UI defects ledger (director audit, 2026-10-12)

Source: `benchmarks/results/baseline-ui/*.png` (HEAD 2086c05 + pending fixes). Each surface owner closes its rows and re-captures.

| # | Surface | Defect | Evidence |
|---|---|---|---|
| D1 | Notes panel | Still native GDI side panel; title renders "Notes _Tasks" (ampersand mnemonic); flat unlabeled inputs, bare "Delete" link, "+ Add" text button. Must be the HTML notes view. | 09 |
| D2 | Tab context menu | Clipped at window/monitor bottom ("Actions for this tab…" cut); no flip/scroll. Disabled items too dim (<3:1). Icons only on some rows; "+"/"x"/grid glyphs reused for unrelated actions. | 18 |
| D3 | Main menu | Grid glyph reused for Command palette, Extensions, Import, Vertical tabs, Workspaces; check glyph used as an icon for Saved sessions. | 15 |
| D4 | Left rail | Two identical "+" buttons stacked (new workspace? add app?) with no distinction; icons small/thin; red briefcase active state looks like an error colour. | 01, 04 |
| D5 | Vertical sidebar | Ctrl+Shift+V toggled vertical mode AND pasted clipboard text into the omnibox. "+" badge hangs under the first tab's favicon. Workspace header icon is a generic square. | 05 |
| D6 | Tab strip | Inactive tab title centred; new-tab favicon placeholder is a "+" box; close glyph tiny; tabs stretch to full width with a single tab (a 900px single tab looks like a bar, not an island). | 03, 04 |
| D7 | Toast / import prompt | Clipped under the address pill at the top edge. | 07, 09 |
| D8 | Command palette | Native GDI; no search icon/placeholder; heavy bold heading, no dividers. | 06 |
| D9 | Shortcuts | Ctrl+H, Ctrl+J, Ctrl+Shift+Del, Shift+F10, Ctrl+Shift+\ produced nothing visible; Ctrl+D only toggles the heart. Verify bindings and give every action visible feedback. | 07, 08, 11, 13, 14, 16 |
| D10 | Toolbar | Back/forward/reload/menu/new-tab glyph weights differ from the pill lock and split icons; caption buttons heavier than toolbar icons. | 01 |
| D11 | Blank tab | New/blank tab is fully transparent (shows desktop through acrylic) — needs a proper new-tab surface. | 02, 03 |
