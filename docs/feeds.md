# Feeds

A small feed reader: subscribe to RSS, Atom and RSS 1.0 feeds, read them, mark them read, import an OPML
list, subscribe to YouTube channels and play podcast episodes. Core: `src/feeds/`. Panel: `ui/src/panels/feeds.ts`
(a dialog window, `?view=dialog&p=feeds`), its wire `src/chrome_host/panels/feeds.rs`, its host
`src/win32/feeds_panel.rs`. Open it from Settings > Address bar > Feeds, the command "Feeds", the panel bar
button, or a toolbar button.

## Network use (the only new one)

Bowser fetches **only** an address the user typed, picked from a page's feed list, imported in an OPML file, or
subscribed to, and **only** when the user previews, subscribes, presses Refresh, or opens Feeds with
"Fetch feeds again when Feeds opens" set to an age that a subscription has passed (`feeds_refresh_minutes`, default
1 hour; 0 = only on Refresh). Nothing runs at startup or on a timer, there are no accounts, no third-party services,
no telemetry.

`src/feeds/fetch.rs` reuses the blocker's checks (`blocker::fetch`): `http` or `https` only, no credentials, no
explicit port, a public host name (no IP literals, `localhost`, single-label or local names), the host is resolved
here and every address must be public, WinHTTP reports every address it connects to (DNS rebinding ends the hop),
direct connection (no proxy), no cookies, at most 3 redirects each re-checked and never from https down to http,
2 MB and 25 s limits. WinHTTP is the same API translation uses.

## Untrusted input

- `xml.rs` is a small XML reader: a `<!DOCTYPE` is refused (no entity can be declared, nothing external is read),
  only the five predefined entities and numeric references are decoded, depth/element/attribute/size limits.
- `parse.rs` turns everything into plain text (markup and `script`/`style` content removed), and every link is an
  absolute http(s) address without credentials. Nothing from a feed is ever HTML.
- The panel sets feed text with `textContent` only. A command never carries an address to open: it names a
  subscription and an item (`id`, item hash) and Rust opens the stored link in a new tab.
- Detecting feeds on a page runs one fixed script that reads only `<link rel=alternate>` tags (12 at most, 16 KB
  answer, strict JSON) and judges them with the same gate as a scanned page. It is switched off by
  `feeds_detect`.

## Behaviour

- Preview before subscribing: type a feed or page address, or press the address bar's feed button; a page that
  offers several feeds lists them.
- Read/unread per item, mark all read, per-subscription unread counts, items kept newest first (50 per feed, 100
  feeds).
- YouTube: a `/channel/UC...` or `/playlist?list=` address becomes its feed address; items open on
  `youtube-nocookie.com/embed/<id>`.
- Podcast: an audio or video enclosure has a Play button that opens the file in a new tab (WebView2's own media
  player plays it; there is no separate player window, and Bowser does not remember the position).
- Data: `feeds.txt` in the profile (plain text). A guest profile keeps nothing.
