# Bowser glass UI — architecture brief (lead-owned; agents read, do not edit)

Source of truth for visuals: the written spec in the task (no image mockup was supplied).
Host: Windows 10 19045 dev machine (no Mica). Code must also work on Windows 11 and degrade gracefully.

## Visual spec (summary)
Row 1: individually rounded frosted "island" per tab (favicon, title, close), transparent gaps between,
active = stronger contrast + subtle border, minimal "+" button, min/max/close at far right, drag/resize/snap intact.
Row 2: the omnibox only, near full width, small margins, rounded translucent surface. No toolbar, no logo,
no bookmarks, no widgets. Empty tab = desktop visible through the window. Real pages render opaque, unchanged.
Light + dark appearance. Solid fallback when glass is unavailable/too costly.

## Decisions
1. **Backdrop tiers** (`src/win32/glass.rs`): Win11 22621+: `DWMWA_SYSTEMBACKDROP_TYPE` transient/acrylic.
   Win10: `SetWindowCompositionAttribute` (blur-behind or acrylic — pick by measured drag/resize smoothness here).
   Solid fallback when: transparency disabled in OS (HKCU Themes\Personalize EnableTransparency=0), remote session,
   API failure, or high-contrast. Fallback is a plain opaque themed fill, no per-pixel alpha cost.
2. **Custom frame** (`src/win32/frame.rs`): keep `WS_THICKFRAME|WS_CAPTION|WS_SYSMENU|WS_MIN/MAXBOX` so snap/aero-shake/
   resize keep working; remove the standard caption with `WM_NCCALCSIZE`; `DwmExtendFrameIntoClientArea`;
   `WM_NCHITTEST` returns HTCAPTION for empty strip, HTMAXBUTTON/HTMINBUTTON/HTCLOSE for our buttons (Win11 snap flyout),
   resize edges. Maximised: compensate the invisible border.
3. **Chrome painting** (`src/chrome/`): one painter, no framework, no new crates. Alpha-correct GDI via
   `BeginBufferedPaint`/`DrawThemeTextEx` (or equivalent proven in the spike). Paint only on invalidation of dirty rects;
   hover via `TrackMouseEvent`; NO timers, NO animation loops, NO polling. Cache fonts/pens/brushes per DPI/theme.
4. **Pure modules (unit-tested, no Win32)**: `src/tabs.rs` (tab list model), `src/chrome/layout.rs` (geometry + hit-test),
   `src/theme.rs` (palette tokens, light/dark), `src/shortcuts.rs` (key chord -> `Command`), `src/omnibox.rs`
   (session history + inline completion).
5. **Omnibox = "ghost EDIT"**: keep a real native EDIT as the input engine (IME, clipboard, selection, undo, accessibility
   via MSAA) but do not let it paint opaque pixels over glass; chrome paints the text/selection/caret into the glass surface
   and maps mouse clicks to `EM_SETSEL`. The EDIT stays focusable, receives all keys, keeps the Enter/Alt-arrow subclass.
6. **Tabs / engine**: one shared WebView2 environment, one controller per tab created lazily on that tab's first
   navigation (an empty tab costs nothing and shows the desktop). Hidden tabs: `put_IsVisible(false)`, after idle
   `TrySuspend`; `Resume` on select. Existing memory-target idle trimming applies to the visible tab. Favicon via
   WebView2 favicon API + WIC decode (Windows API, no crates). Keep all existing security policy (scheme allowlist,
   popup/permission/download denial, tracking prevention) for EVERY tab.
7. **Progress bar**: already implemented in the pill (events -> `WM_NAVIGATION_PROGRESS`); must be carried to the new
   omnibox, per active tab.

## Performance budget (vs baseline in docs/glass-ui-baseline.md)
Startup to window-visible + input-ready: <= +5%. Idle private bytes, 1 tab: <= +2 MB over baseline. Idle CPU: no
measurable increase (no timers added; existing 1 idle-trim timer unchanged). No new dependencies unless unavoidable
(prefer `windows` crate feature flags already in use). Report any regression; do not accept silently.

## File ownership (no two agents edit the same file at once)
- A. `src/win32/glass.rs`, `src/win32/frame.rs`, `src/win32/mod.rs` (additions), glass bits of `window.rs` — Luna
- B. `src/tabs.rs` + tests — Haiku
- C. `src/chrome/layout.rs` + tests — Haiku
- D. `src/theme.rs` + tests — Haiku
- E. `src/shortcuts.rs` + tests — Haiku
- F. `src/omnibox.rs` + tests — Haiku
- G. `src/engine/*`, tab/engine parts of `src/app.rs` — Luna
- H. `src/chrome/paint.rs`, window.rs omnibox/chrome wiring — Luna (after A,B,C,D)
Module declarations in `src/lib.rs` are added by the lead before agents start.

## GUI test protocol
Only one agent may drive the GUI at a time. Wrap any GUI launch/SendKeys/screenshot session in
`powershell -NoProfile -File C:\Users\charlie\Documents\ChatGPT\bowser-tools\with-gui-lock.ps1 -Command "<ps command>"`
(or hold the same named mutex `Global\BowserGuiTest`). Screenshots/scratch go to %TEMP%, never in the repo.
Pages fetched with bare hosts default to https; use `http://127.0.0.1:PORT/` for local fixtures.
Do not commit. Run `cargo fmt`, `cargo clippy --locked -- -D warnings`, `cargo test --locked` before reporting.
