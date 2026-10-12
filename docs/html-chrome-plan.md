# HTML chrome: remaining migration plan (Opus, reviewed 2026-10-12)

Done and live-verified: toolbar, horizontal tab strip, left rail (three controllers in the tab environment's private `BowserChrome` profile). This plan covers the rest. GDI paths stay as fallbacks; nothing is deleted in this phase.

## Architecture decisions

**Side panels** (`src/win32/side_panel.rs` has a clean seam: stable `WS_CHILD` host, `PanelContent` trait, `register_content` factories).
- One reusable controller per window, start URL exactly `bowser-ui://app/index.html?view=panel` (`View::Panel`). Rust selects the panel with `{t:"panelshow",p:"history"}` then sends a snapshot; no re-navigation per panel.
- Parented to the side-panel host HWND, created lazily on first HTML panel open, then kept hidden. Native header, close button, resize strip, width persistence and animation stay native; bounds = content rect below the header minus the 6-DIP resize strip. Opaque (`solid:true`).
- `src/win32/html_panel.rs` provides `HtmlPanel{id}: PanelContent`; factories are swapped per panel when the environment settles; on failure the native factories are re-registered and the panel closes.
- Focus: add a `PanelContent::after_shown()` hook calling `ChromeHost::focus()` after the host's `SetFocus`; Escape is handled in JS (`{t:"panel",a:"dismiss"}`).
- Data: Rust owns stores and formats. Snapshots Rust->JS (diffed). Commands JS->Rust use a generic validated field set (`Command::Panel{t, f}`): `a` token <=24 chars [a-z], `id`/`parent`/`before` u64, `n` int, `on` bool, `text`/`url` <=2048; still within the 6-key cap. Each panel maps `a` to its existing `Action` enum; ids are checked against the live model; `Content::run` is extracted into a shared `execute(app, owner, Action)`.
- Secrets: `src/chrome_host/**` and `ui/src/**` never touch passwords; a Rust test greps the sources for `passwords|vault|credential`.

**Popups and menus** (content that must overlap the page).
- One owned `WS_POPUP` per window (`src/win32/html_popup.rs`, `View::Popup`), pre-created hidden ~2 s after the toolbar activates, `TrySuspend`ed while hidden. If not ready the caller falls back to native `menu::track` instantly.
- Opaque (solid palette, rounded via `SetWindowRgn`), not glass.
- Show: Rust posts `{t:"menu",tok,...}`; JS replies `{t:"size",tok,w,h}`; Rust places (existing `menu::place`), shows, activates, focuses; prior focus restored on close.
- Dismiss on popup deactivation, owner move/size/deactivate, Esc, tab/workspace switch; 300 ms reopen guard; submenus are drill-in pages.
- Contract: keep `menu::Menu/Entry`; add async `menu::show(owner, anchor, placement, menu, kind, callback)`; HTML picks are validated (`tok`, enabled `id`) and the callback runs from `DeferredChromeCommand::MenuResult` (never inside `WebMessageReceived`); callbacks re-validate ids. Native dialogs (passwords, import) keep synchronous `track`.
- Stay native: hover cards, island hover menu, tooltips (need no-activate behaviour).

**Vertical sidebar**: reuse the Tabs controller (idle in vertical mode); Rust sends `{t:"layout",mode}` and re-bounds it to the sidebar rect (right edge at the resize handle). Rust keeps the workspace/group model, drop semantics, width setting and the resize handle; TS owns rows, group headers, workspace header, scrolling and drag visuals.

## Ordered backlog

1. Foundation: protocol (`View::Panel/Popup/Dialog`, generic `Command::Panel`, popup `size/pick/dismiss`), `json.rs` writer, `ChromeHost::try_suspend/resume`, TS parser, secrets-guard test. (owns `protocol.rs`, `host.rs`, `assets.rs`, `json.rs`)
2. Panel host plumbing (`html_panel.rs`, `panels/mod.rs`, `ui/src/panel.ts`, `after_shown`). Depends on 1.
3. History panel (pilot). Depends on 2.
4. Bookmarks panel. Depends on 3. Parallel with 5.
5. Customize panel. Depends on 3. Parallel with 4.
6. Vertical sidebar in the Tabs view. Depends on 1. Parallel with 2, 7.
7. Popup host (`html_popup.rs`, `popup.ts`, `menu-view.ts`). Depends on 1. Parallel with 2, 6.
8. Async menu API + first conversions (main, split, tab/group/rail context menus). Depends on 7.
9. Remaining `window.rs` menus (workspace chain etc.). Depends on 8.
10. Site info popup page. Depends on 8.
11. Command palette popup page. Depends on 8.
12. Dialog host + Settings (`html_dialog.rs`, on demand). Depends on 1.
13. Privacy and Notes pages in the dialog host. Depends on 12.
14. Hardening and benchmark gate (<= +40 MB private memory per added live host, idle CPU <= 0.02 s/10 s, secrets guard green). Depends on all.

Conflict rules: only task 1 edits `protocol.rs`; registries get one line per panel; `window.rs` is edited only by tasks 2, 6, 8, 9 in that order, inside new delimited functions.

## Not migrating
Passwords panel and import (secrets), `panel_webapp.rs` (third-party WebViews), native message boxes/confirmations/file pickers, `import_panel.rs`, `profile_rename.rs`, caption buttons/frame/resize borders/glass backdrop, sidebar and panel resize handles, hover cards, island hover menu, tooltips.

## Top risks
Memory/renderer count (benchmark gate), popup dismissal and DPI races (reopen guard, native fallback), sync-to-async menu bugs (tokens, re-validation, deferred queue), focus regressions (`after_shown`, JS Escape, accelerators on every view), larger bridge surface (fixed field whitelist, ids vs live model, http(s) normaliser, exact start URLs, static secrets guard).
