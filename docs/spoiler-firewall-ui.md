# Spoiler Firewall in Bowser's own chrome

The page engine protects web pages (`docs/spoiler-firewall.md`). This page covers the exposure channels
in Bowser's own UI (DESIGN.md section 2, channels 6 to 8): suggestions, history, tab titles, the command
palette. One engine, no second classifier.

## Engine inclusion

`ui/scripts/gen-spoiler-engine.mjs` reads `extensions/spoiler-firewall/engine/{normalize,lexicon,classify,policy}.js`
in the shipped order, wraps them as `build.mjs` does (`const SFW = {version}`, per-fragment banner, LF, ASCII
escapes) and writes `ui/src/spoiler-engine.gen.ts` (`export default SFW`). `npm run build` runs it first;
`node scripts/gen-spoiler-engine.mjs --check` fails when the file is stale, and `ui/test/spoiler-guard.mjs`
checks that each fragment is embedded verbatim. Only the four pure fragments are included (no DOM runtime).

Wanted from `extensions/spoiler-firewall/build/build.mjs` to replace the generator: an ES module output
`dist/ui/engine.mjs` = `BANNER + const SFW = {version}; <normalize, lexicon, classify, policy>; export default SFW;`.

## Setting

Rust posts `{"v":1,"t":"spoiler","on":bool,"mode":"lockdown|strict|balanced"}` (`chrome_host::host::spoiler_config_json`)
to every chrome view when it says `ready` (before content), to the persistent views (toolbar, tabs, rail), which
never say `ready`, ahead of their first state (`finish_initial_state`, posted straight to the page so the state
handshake never sees it), and, via `broadcast_spoiler_config`, when `App::apply_spoiler_settings` sees a change.
A bundle fetched later in the same document (the start page) starts from the setting an earlier bundle already
holds (`window.__bowserSpoilerConfig`) rather than from the product default.

**Term updates.** A verified feed reaches the chrome too: right after every setting message Rust posts
`{"v":1,"t":"spoilerFeed","lexicon":{...}}` (the same lexicon object the page bundle gets, at most 256 KB, balanced
object with no raw line separators, `chrome_host::host::spoiler_feed_json`) and again when a new feed is accepted
(`WM_SPOILER_FEED` calls `broadcast_spoiler_config`). `ui/src/spoiler-config.ts` validates it (shape, no unknown members,
size), keeps it on `window.__bowserSpoilerFeed` for bundles loaded later, and the guard's handler calls
`SFW.lexicon.extend`, clears its verdict cache and the config listeners re-render masked surfaces. The same feed is applied
once per engine copy. `ui/src/spoiler-config.ts` listens in every view. Before the first
message the default is on and Strict. Off means no masking anywhere.

## Guard

`ui/src/spoiler-guard.ts`: `riskyText`, `riskyUrl` (`SFW.classify.text/url`, then `SFW.policy.decide(result,
{mode, hot:0, kind:"title"})`, anything but "clear" is risky; engine errors fail closed), `rowRisky`, `guardRow`,
`guardTitle`, `hostOnly`, `RevealedRows`.

A masked row keeps its place: "Spoiler hidden · GTA VI", the host only (never path, query or hash) and a "Show"
button that reveals that one row for the life of that view (never stored). Text equal to what the user typed is
never hidden from them.

**Address bar.** The address of the loaded page, unfocused: when `riskyUrl(fullUrl)` is true the field shows the host only
and a subtle "· path hidden" note; focusing the field shows the whole address (deliberate: the user asked for it) and
blurring hides the path again. It reuses the `minimal_url` pair of texts and re-runs when the setting or a term update changes.
An address the user edited is never replaced.

Enter or a click on a masked row still navigates: the user chose it and the destination is protected by the page
engine. Masked rows stay selectable so arrow-key order is predictable. Arrowing onto a masked row puts the host
(or the typed text, for a search row) in the address field, never the suggestion text. History-based inline
completion is suppressed when the completed address is risky. There is no keyboard-only reveal (Enter navigates).

## Surfaces

Covered: omnibox suggestions (search, history, bookmark, tab, URL; title, text and address), omnibox preview text
and inline completion, History page, History and Bookmarks panels (the history panel snapshot now carries `url`),
command palette tab/bookmark/history rows, tab strip, sidebar, split picker, tab hover card and its rows,
the bookmarks bar (a masked bookmark keeps its place and still opens on click), the start page (top sites,
bookmark widgets and speed-dial titles), the Feeds panel (item titles and summaries, feed and offer titles; an untitled feed or offer shows the host, never a slug),
tooltips (tab titles show "Spoiler-protected page", the engine's own wording; no per-tab reveal).

The window caption and taskbar title are covered by the existing 150 ms title hold in
`engine/webview2.rs` (tab strip, caption and history all receive the held title); an empty, URL-equal or URL-shaped
title is delivered as the host only (`spoiler::protected_title`, applied in `app.rs` where the title arrives and again when
the caption is built).

**Start page web page widget.** The widget is an `http(s)` iframe inside a Bowser chrome document, which never gets the
page bundle. While protection is on it is a placeholder, "Web page widget paused by spoiler protection" with a **Load**
button, and the iframe is created only after that click (per widget and address); switching protection on drops a loaded
frame again. Injecting the bundle into the chrome host instead was rejected: it would need a script registration on the
shared chrome environment that safely skips `bowser-ui:` documents.

Not covered (documented gaps, no Rust classifier): the Downloads page and bubble (file names and hosts), the
bookmarks bar's folder and overflow menus (Rust-built popups), saved-session and note names (the user's own words); native-chrome fallback (non-HTML mode) omnibox, menus and
history panel; native "Recently closed" menu items and jump lists (titles recorded after the 150 ms title hold are
already rewritten by the page engine; entries recorded earlier are not masked there); bookmark edit forms (explicit user
action); reveals are per view, so the toolbar's arrow preview does not know a row revealed in the dropdown. A
setting change redraws History and the panels at once; other views pick it up on their next render.
