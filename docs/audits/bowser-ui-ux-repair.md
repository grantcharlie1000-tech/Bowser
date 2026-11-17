# Bowser UI/UX repair: verified defect register

Baseline: `main` @ 082b3c4 (stable v0.0.29, 2026-11-16). Branch: `claude/bowser-ui-ux-repair-yrkbel`.
Work was split by file ownership across six parallel audits (A foundation, B tabs/navigation, C
settings/pages, D menus/panels/extensions, E native/bridge, F QA) and integrated by the lead.

Every row below was confirmed against current source (file and line cited in its commit) before it was
fixed. The October ledger (`docs/design/defects.md`) was treated as history and re-checked.

## How this was verified, and what was not

| Check | Result |
| --- | --- |
| `npm run typecheck`, `npm run lint` | pass |
| `npm test` (no browser; browser-backed suites skip) | 671 pass, 38 skipped, 0 fail; second batch 72/72 |
| `npm test` with `BOWSER_TEST_EDGE` = headless Chromium shell (Linux) | 708/709; second batch 72/72. The one failure, "reader view: font, size, line height and column width", also fails on unmodified main here: `ch` widths without Segoe UI. Not a product defect |
| `npm run qa` (new: `qa-siteinfo.mjs` + `qa-a11y.mjs`, Playwright; skips without it) | 647 pass, 0 fail, 25 todo (each todo is a tracked low-severity finding with an id, listed below) |
| Visual capture (`ui/shots/qa-capture.mjs`): 54 scenarios x light/dark x 100/125/150/200% x standard/compact | 864 shots of main and of the repaired build, compared side by side. This comparison caught a site-information row overlap that the unit tests missed; it was fixed before commit |
| Interaction latency (`benchmarks/scripts/qa-latency.mjs`, 60 runs, headless Chromium) | No change beyond noise. Medians, main to repaired: tab switch 5.0 to 5.7 ms, omnibox key 0.6 to 0.5 ms, menu rows 2.6 to 2.5 ms, settings toggle 1.2 to 1.2 ms. No performance improvement is claimed |
| `ui/dist` + `dist.manifest` | regenerated; two consecutive builds byte-identical |
| `cargo clippy --locked [--features html-chrome-default] --target x86_64-pc-windows-msvc -- -D warnings` | clean, both feature sets |
| `cargo check --all-targets` (both feature sets, Windows target) | clean, no warnings; all new Rust tests compile |
| `cargo test` | **not run**: the container is Linux with no Windows runtime. New Rust tests are compiled only; `windows-verify.yml` runs them |
| Native WebView2 / Win32 behaviour, DPI, snapping, fullscreen | **not verified**: no Windows host. Headless Chromium checks prove DOM/CSS behaviour only |

Each UI regression test was checked to fail with the fix reverted and pass with it.

## Fixed (confirmed defects)

Severity: H high, M medium, L low. "Compiled" means Rust tests compile but were not run here.

| ID | Sev | Surface | Defect (root cause) | Fix | Verification |
| --- | --- | --- | --- | --- | --- |
| E-1 | H | Native, every tab | `RendererUnresponsive` was only logged (`app.rs`); a hung page offered no way out | Nonmodal native bar "This page isn't responding" with Wait / Reload page (`win32/window/hang_prompt.rs`). Wait keeps the document; Reload drops any held form body and reopens the address as a GET in a fresh controller (nothing resubmitted). Clears on a responsiveness probe, navigation, load, failure or close. F6 reaches it, Escape = Wait | 6 Rust tests, compiled |
| E-2 | M | Native fallback | When HTML chrome failed with the omnibox focused, focus went nowhere and the typed address was lost | The native address field receives the draft and the keyboard | Rust test, compiled |
| E-3 | M | Native failure card | "Reload this address" was mouse-only (F6 skipped it, Enter did nothing, focus stuck after retry) | F6 reaches it, Enter activates, focus returns after retry | Rust test, compiled |
| E-4 | M | Flyouts, dialogs, settings, bookmarks bar | `data-tip` tooltips were dropped: the host refused `tip` from those views and their handlers ignored it | `View::shows_tooltips`, `window::html_view_tip` anchors from the sending window and DPI; panel, settings and bookmarks-bar handlers route it | Rust test, compiled |
| B-1 | H | Tabs (PR #98 issue) | Sleeping tabs restored after a restart showed a globe: tab favicons were never persisted | Stored per host with the existing favicon store on a worker thread; restored onto sleeping tabs without loading them. Never for private tabs or with history off | Rust test, compiled |
| B-2 | M | Strip, sidebar | Any background tab update scrolled the strip/sidebar back to the active tab (measured 900 to 4 px) | Reveal only when the active tab or its position changes | UI test + headless browser |
| B-3 | M | Strip, sidebar | Keyboard tab reorder was dead: Ctrl+Shift+Left/Right is claimed by split-pane focus before the page; the sidebar had none | Ctrl+Shift+PageUp/PageDown (strip); Ctrl+Shift+Up/Down or PageUp/PageDown (sidebar); focus follows | UI test + headless browser |
| B-4 | M | Bookmark dialog | Enter during IME composition saved a half-typed name; stale listeners re-saved earlier bookmarks (3 `bmsave` for one Enter) | Listener on the current drawing's fields, composition ignored | UI test + headless browser |
| B-5 | M | Strip | `.group { min-width: 0 }` let a group collapse under its members; tabs overlapped (0 px group at 420 px) | `min-width: auto`; the strip scrolls instead | Browser test |
| L-1 | M | Omnibox | Ctrl+Shift+V did nothing: the page cancelled it for a layout toggle Rust no longer binds | Block removed; it pastes like the search field already did | UI test |
| C-1 | H | Settings | Typing `/` in the Startup pages textarea (every URL) jumped to search | Typing check covers textarea, select, contenteditable, IME and modifiers | UI test + headless browser |
| C-2 | H | Settings | Merged areas hid every part title, so rows read "Folder", "Automatic" with no context | Part titles stay unless they repeat the area heading | UI test + headless browser |
| C-3 | H | Settings | Tracking prevention and Group by site apply only after restart, but showed as live (privacy-relevant) | "Applies after restart" note restored and linked with `aria-describedby` | UI test |
| C-4 | M | Settings | Nine "More" disclosures shared one accessible name | Each named for its area | UI test |
| C-5 | L | Settings | Accent swatch showed black when no custom accent was set | Shows the theme accent, labelled as default | UI test |
| C-6 | M | History page | "History is off" pointed to a setting label that does not exist | Names the real Privacy > History row; test checks it against `ROWS` | UI test |
| C-7 | L | Private landing | Lead repeated the cookies row (design asks for one sentence) | One sentence; every security statement kept and pinned by a test. PR #104's reduction was **not** adopted | UI test |
| L-2 | M | Settings | An invalid Homepage / Startup pages entry kept its text but looked accepted | `aria-invalid` (danger edge), cleared once valid; nothing saved | UI test |
| L-3 | M | Settings (native) | The History switch never answered when the profile was not ready or in guest, leaving the page pending, so a stale draft could later overwrite newer values | Every path answers | Compiled |
| L-4 | H | Start page | Webpage widget always blank: shared `index.html` meta CSP `frame-src 'none'` beat the split host's deliberate http(s) header | Meta carries the split policy; the header still sends `frame-src 'none'` to every other view; `hardening.mjs` pins both | UI test |
| D-1 | H | Menus | Escape in a submenu closed the whole menu (page dismissed; Rust also claimed Escape) | Escape closes one level, keeps the opening row selected; Rust leaves Escape to the page while a child card is shown | UI test; Rust test compiled |
| D-2 | H | Menus | Submenu near the screen bottom squeezed to 50 px and scrolled with 300 px free above | `childTop()` lifts it to fit, never above the parent card | UI test + headless browser |
| D-3 | M | Command palette | Empty results left an empty listbox marked expanded; calculator row reused the grid glyph | Collapsed, presentation role while empty; `calculator` icon | UI test |
| D-4 | M | Extensions | "Check update" looked live but is always refused; Options shown for off/failed extensions | Check update disabled with reason; Options follows `running()` | UI test |
| L-5 | M | Extensions | Options shown for running extensions with no options page, ending in a native refusal box | Rust sends `options` (`declares_page(.., Options)`); the button needs it; wire fixture and docs updated | UI test; Rust fixture compiled |
| D-5 | M | Downloads bubble | Removing the focused row dropped focus to `body`, so arrows, Delete, Home/End stopped working | Focus moves to the row in its place, the row above, or the footer | UI test |
| D-6 | H | Customize panel | The workspace delete question focused the destructive button (Enter right after the trash icon deleted); every snapshot re-stole focus; Escape closed the whole flyout | Cancel takes focus; redraws leave focus alone; Escape and Cancel return to the trash button | UI test |
| A-1 | H | Settings, history, downloads, Store | Warning and danger text used the group palette: 2.84:1 on a dark card (fails WCAG 1.4.3) | `--bw-danger` / `--bw-warning` (>= 4.5:1 on window, card and field, both schemes); every surface switched; guard test | Style tests |
| A-2 | M | Same four pages | Card, field, tile, row and hit colours copied in four files; danger hard-coded twice | Shared `--bw-page/card/card-edge/field/field-edge/tile/row-hover/row-press/hit` tokens (same values); surfaces map to them | Style tests |
| A-3 | M | `.bw-segmented` | Disabled options looked enabled and took hover | Dimmed, no hover, specificity unchanged | Browser test |
| A-4 | L | `.bw-toggle`, `.bw-select-button` | Hover effects on disabled controls; no disabled state for the select button | Gated on not-disabled | Style test |
| A-5 | M | `.bw-input` | `aria-invalid` changed nothing visually | Danger edge (>= 5.5:1), focus ring kept | Browser test |
| A-6 | H | Shared components, forced colours | Toggle on/off, checked segment, focus ring, selection and spinner invisible under Windows contrast themes | `@media (forced-colors: active)` system-colour fallbacks | Browser test (`--force-high-contrast`) |
| A-7 | M | Hover layer | Tab-marker and group-colour swatches all Canvas under forced colours | `forced-color-adjust: none` + contour; Highlight outline on the pick | Browser test |
| A-8 | M | Site info | Each radio option was its own tab stop (about 15 Tabs for 5 permissions) | One roving tab stop per group; arrows, Home and End | Browser test |
| F-13 | H | Privacy dashboard window | `dialog.ts` passed only `p === token`, dropping the `protection` snapshot Rust sends, so Blocking level, Website appearance and Private theme showed nothing selected and Cookie notices showed off while on | Accept `protection` in the privacy window | QA browser check (now an assertion) |
| F-4 | H | Site information | The page asked for 657 CSS px; Rust clamps to the work area and nothing scrolled, so Zoom, Reset permissions and the footer were unreachable on short screens | The page scrolls when capped and rows keep their height; the reported size is still the content height | `qa-siteinfo.mjs` (fails on both the old and the first-attempt CSS) |
| F-2 | H | Vertical sidebar | Pinned tiles had an empty accessible name (label removed; tiles draw no title) | Pinned rows always carry `aria-label` | QA browser check |
| F-1 | M | Toolbar | Typed-history and QR buttons rendered while turned off: `.bw-icon-btn` display beat `[hidden]` | Shared buttons honour `[hidden]` (component rule) plus a pill rule | Style test + QA check |
| F-18 | M | Privacy dashboard | The Delete browsing data "Cancel" showed when no deletion was armed (same `[hidden]` cause; exposed once F-13 let the section render) | Same component rule | QA check |
| F-3 | M | History, downloads pages | Toast "Undo" was a Tab stop and announced while the toast was invisible | Hidden until an undo toast shows it | QA check |
| L-6 | L | Docs | README offered v0.0.5 as the stable download (pin is v0.0.29); shortcuts table missing bound chords; PRODUCT.md listed the retired Ctrl+Shift+V | Corrected. The stable pin itself is unchanged | n/a |

Total: 42 confirmed defects fixed (14 high, 24 medium, 4 low).

## Re-verified: historical ledger rows already fixed on main

D1 notes panel (HTML), D2 menu contrast (about 3.3:1) and clipping (work-area cap and scroll),
D3 glyph reuse in the main menu, D4 rail "+" buttons, D5 Ctrl+Shift+V layout toggle, D8 palette,
D9 shortcuts (Ctrl+H, Ctrl+J, Ctrl+Shift+Del and Ctrl+Shift+\ are bound; Shift+F10 is left to the page,
as documented), D12 customize preset select, D13 toggle on-state, D14 sidebar focus rings. Bridge
contract: every TypeScript command and action has a Rust parser and handler, and no dead messages were found.

## Deliberately unimplemented or left as designed (not resurrected)

- Permission prompts: none exist by design. Requests are decided from stored allows and shown as a
  passive indicator.
- A single tab fills the strip (D6): intentional (`tabs.css` and its test).
- Extension updates: not available. The UI now says so instead of offering a dead button.
- PR #104 (private-page reduction), #119 (vertical-tab redesign) and #98 (favicon refresh over the network)
  were not merged; only the defects they evidenced were fixed the Bowser way.

## Remaining issues (prioritised)

QA findings still open (todo entries in `ui/test/qa-a11y.mjs`, all low severity):
- F-6: letter favicon tiles in light mode, about 1.75:1.
- F-7: Store's selected side-nav item in light mode, 3.88:1.
- F-8: adjacent 20x20 pill buttons, below the WCAG 2.5.8 24x24 target size.
- F-9: in the address and search fields, only the caret marks focus.
- F-10: palette rows are unreachable when the work area is under 438 px.
- F-11: the 20px-tall "What it declares" button.
- F-12: split picker text measures 4.09:1, against an assumed backdrop.
- F-14: the Presets radio group has nothing checked for the fixture layout; this may be intended.
- F-15: a favicon that fails to decode shows no letter fallback.
- F-16: the site-info blocker toggle's target size.
- F-17 is a fixture gap, not a defect.


1. **Needs a Windows run**: all Rust tests added here (`engine_recovery.rs`, `surface_recovery_tests.rs`,
   `tab_flow.rs`, `html_popup.rs`, `host.rs`, the extensions wire fixture). The hang bar also needs a
   native check: whether `ExecuteScript` completes after a hang ends, whether WebView2 repeats
   unresponsive reports (the 10 s quiet period is an estimate), and the bar's z-order over the content
   WebView.
2. **Hang Reload loses back/forward history** because it uses a fresh controller. This was a
   deliberate safety trade-off.
3. **Tab hover-card delay is 3000 ms** (`src/win32/card.rs`, pinned by a test). The spec's tooltip delay
   is 500 ms, and PR #119 proposed 350 ms. This needs a product decision, so it was not changed.
4. **Not audited (no Windows host)**: resize, snap and DPI transitions; fullscreen and video fullscreen
   with tab auto-hide and edge reveal; split-view presentation; native GDI chrome.
5. **Dialog focus trap**: there is no JavaScript trap. WebView2's default focus cycling probably keeps
   focus inside, but this is unconfirmed natively.
6. **Forced colours on surfaces**: settings, downloads and history still draw some selection and focus
   states with box-shadow and fills only.
7. **Smaller UI items**:
   - The bookmarks bar `role=toolbar` has no arrow-key navigation, and a redraw drops focus.
   - The omnibox search field's focus is only the caret.
   - The settings switches that send their own command don't revert if the bridge send fails.
   - A customize delete question survives closing and reopening the flyout.
   - A reopened closed tab doesn't restore its group membership.
8. **PR #68 (`experimental-engine-context-menu`)**:
   - Its native safety gates are still red.
   - The feature build has 3 warnings that predate this work.
   - `windows-verify.yml` doesn't build or test with the feature.
   - It was not changed here.
9. **PR #97 (standalone updater recovery)**: its pin (0.0.24) is stale and real installation acceptance is
   outstanding. It was not changed here, and the stable pin was not touched.

## Compatibility notes

- Wire change: the extensions snapshot item gained `options` (Rust and TypeScript updated together; the
  committed fixture matches).
- CSP: the shared meta now allows `frame-src https: http:`. Every view except the split host still
  receives `frame-src 'none'` in its response header, and the stricter policy applies.
- The new colour tokens keep existing values except `--bw-warning` and `--bw-danger` on surfaces that
  previously used the group palette. Those are visibly lighter or darker, by design, for contrast.
