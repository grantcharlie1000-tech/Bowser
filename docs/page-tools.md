# Page tools

Capture, spatial navigation, notes made from the page, reader view defaults, page zoom and the pop-out
video entry (Vivaldi port, thread T7). Settings are one short block, **Settings > Page tools**: five core
rows (capture folder, capture file name, move between links, page zoom belongs to, Ctrl+scroll zoom) and
ten more behind **More options**. Every row is a `pt_*` line in `settings.txt` (`src/page_tools.rs`), each
with a test that proves what it changes.

| Row (key) | What it changes | Proof |
| --- | --- | --- |
| Capture folder (`pt_capture_dir`) | Where `capture::save_file` writes (empty is Pictures\Bowser). Only a drive-letter folder on this PC is accepted. | `app::tests::page_tools_flow` |
| Capture file name (`pt_capture_name`) | `{host} {title} {date} {time} {mode}`; cleaned for Windows, numbered when taken. | `page_tools::tests`, `page_tools_flow` |
| Capture format (`pt_capture_format`) | `format` of `Page.captureScreenshot` and the file extension. | `page_tools_flow` |
| Move between links (`pt_spatial_nav`) | Adds `src/spatial.js` at document creation to every tab (and removes it when off). | `ui/test/page-tools.mjs` (headless Edge), `page_tools_flow` |
| Page zoom belongs to (`pt_zoom_scope`) | Site: stored per site (as before). Tab: kept by the tab across pages, nothing stored. | `page_tools_flow`, `page_tools::zoom_on_commit` |
| Ctrl+scroll zoom (`pt_wheel_zoom`) | `IsZoomControlEnabled` of every tab. | `page_tools_flow` (what the engine is told) |
| Picture in picture in the tab menu (`pt_pip_menu`) | Whether the tab menu lists it (`page_tools::pip_menu_entry`); Ctrl+Shift+Y works either way. | `page_tools::tests` |
| Reader font, size, line height, column width, colours, vertical text, controls on the page, reading time | The options object passed to `reader.js` (`reader::invocation_with`). | `ui/test/page-tools.mjs`, `page_tools_flow`, `reader::tests` |

## Capture

Registry commands (palette "Page actions", chains, shortcuts): *Capture visible area* (Ctrl+Shift+S),
*full page*, *selection*, each also as *Copy capture of...* (clipboard bitmap) and *Capture ... to a note*
(file plus a note whose text is `![Capture](file:///...)` and whose address is the page's).

The browser takes the pictures (`Page.captureScreenshot`); `EngineAdapter::devtools_call` allows only that
and `Page.getLayoutMetrics`, only on an http(s) page checked at call time, and drops an answer over 64 MB.
A full page is cut at 30000 x 8000 CSS pixels and captured in 6000 px slices that `capture::stitch` writes
one after the other into a single PNG or JPEG through WIC (a single screenshot that tall fails). The result
says when a page was cut. A selection is one screenshot clipped to the selection's box.

## Notes

`notes.txt` keeps its `bowser-notes-v1` format byte for byte. What it has no room for (the page a note came
from, its date, its folder, the folders, the sort order) is in `notes-meta.txt` beside it
(`src/notes_book.rs`). The Notes panel gains folders, five sort orders (manual with move up and down,
title, address, content, date), a Markdown preview (a small subset built with DOM calls, links shown but never followed), export (all, one folder or one note; .md or .txt, native Save dialog) and import
(.md or .txt up to 256 KB; headings become notes and folders). Notes are at most 100 and 4000 characters.

- *Add selection to a new note*: **Ctrl+Shift+O**. Ctrl+Shift+C stays *Copy address* because its own
  setting row (`ctrl_shift_c_copy_link`) gates it; the two commands do not share a default chord. Anyone
  who wants Vivaldi's chord assigns it in Keyboard settings (`store.assign` moves it).
- *Add selection to an existing note*: a list of notes, the pick gets the text appended.
- *Insert note*: right-click in a text field, **Insert note**, pick one. The text goes in through
  `execCommand('insertText')` on the focused field, never a password field.
- The GDI Notes window still lists notes and tasks (the file format is unchanged); folders, sorting and
  files are in the HTML panel only.

## Left out

- **Translate panel (use panel instead of dialog, translate when pasting)**: Bowser has no translate
  dialog (the toolbar button translates at once) and no text translator panel, so "instead of a dialog" has
  nothing to replace and "when pasting" needs a new panel and a new text path to Google. Not built; it
  needs a decision.
- **Ctrl+Enter opens in a new tab** relies on the browser's own behaviour for a focused link (a Ctrl+click),
  which `src/spatial.js` leaves alone.

## Security notes

The new page-facing script (`spatial.js`) reads layout and focuses a link; it makes no request and sends
nothing out. The selection script is read-only and its answer is parsed as text and whole numbers only.
Folder and name settings are validated in Rust and in the page (no UNC, device or `..` paths, no separators
in a name, reserved device names avoided); files are created with `create_new`, never overwritten. Import
reads text only, is size-bounded and never fetches a link or image.
