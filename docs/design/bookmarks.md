# Bookmarks

How Bowser's bookmarks behave, and why. The model is the one Chrome, Edge and Firefox share; where
they differ, Bowser follows Chrome.

## What was wrong (v0.0.39)

- **Ctrl+D and the bookmark button toggled.** On a bookmarked page they deleted the bookmark, with
  a toast near the main menu, while the button's tooltip said "Edit this bookmark".
- **Large trees showed nothing.** The panel sent the whole tree in one message; the page drops any
  message over 256 KiB, so from roughly 1,500 bookmarks (a typical import) the panel stayed on
  "Loading bookmarks…".
- **Imports put two folder buttons on the bar.** Chrome's and Edge's exports carry their root
  folders, so the bar (the top level) showed "Bookmarks bar" and "Other bookmarks" folders.
- **Refusals were silent.** Adding an address that was already bookmarked, or one without
  `https://`, closed the panel's form and changed nothing.
- **No undo, no menus.** Delete was one click with no way back; rows carried edit and delete
  buttons side by side; there was no folder menu.
- **Search flashed.** Typing re-drew the whole unfiltered tree before the results arrived.
- Saving the bookmark dialog moved the bookmark to the end of its folder every time.

## The model

- `bookmarks-v2.txt` keeps its format. Two optional rows are added, which older builds skip:
  `other<TAB>id` names the **Other bookmarks** folder (`0`: none yet; the row's presence also
  records that the one-time tidy ran), and `last<TAB>id` the folder the last bookmark was saved to.
- **Bookmarks bar** is the top level. **Other bookmarks** is a top-level folder that cannot be
  renamed, moved or deleted. It is made when first used (the page names it with the wire id
  `0xffffffff` until then), or adopted from an existing top-level folder of that name.
- **One-time tidy** (`BookmarkStore::tidy_imported_roots`), on the first load by this version:
  a top-level "Bookmarks bar" / "Bookmarks toolbar" / "Favorites bar" folder gives its contents to
  the top level in its place; the first "Other bookmarks" / "Bookmarks menu" / "Other favorites"
  folder becomes Other bookmarks and later ones merge into it. It is saved at once and never runs
  again; a folder a person later names "Bookmarks bar" is left alone.
- **Imports** map the same names (`import_folder`), so new imports never recreate the problem.

## Surfaces

| Surface | Files | Behaviour |
|---|---|---|
| Ctrl+D, bookmark button | `win32/window.rs` `add_bookmark_now`, `win32/address_dialog.rs` | Save to the last-used folder, then the anchored **Bookmark added** bubble (Name, Folder, Nickname, Description, Remove, Done). Bookmarked already: **Edit bookmark**. Done, Enter, Escape and clicking away save the edits; only Remove removes. |
| Bookmarks panel | `chrome_host/panels/bookmarks.rs`, `win32/html_panel.rs`, `ui/src/panels/bookmarks*.ts` | Two roots, favicons, counts, word search done in Rust (capped, with a total), a row menu, inline add/edit that stays open on a refusal, Undo (Ctrl+Z), drag and drop. |
| Bookmarks bar | `win32/bookmark_bar.rs`, `chrome_host/address_page.rs`, `ui/src/bookmark-bar*.ts` | The bar folder's items, Other bookmarks at the far end, a fuller context menu and a background menu, Import on the empty bar. |

### The panel's message budget

The snapshot always carries every folder (the tree's shape, counts, move targets), then the links
of the bar and of each folder the person opened, in tree order, until 150 KiB of items; favicons
take at most 72 KiB more. A folder whose links did not all fit says how many are not shown and
points to search. `a_huge_tree_still_fits_one_message` holds this to the page's limit.

### Errors

`panels::bookmarks::explain` turns store errors into sentences: "That page is already bookmarked
as “Docs” in Work.", "Enter a web address that starts with http:// or https://.", "A folder
cannot go inside itself." `mutate` restores the store on any error, so a refusal changes nothing
(not even making Other bookmarks).

## Not done here

- A full-page bookmark manager (`bowser://bookmarks`) with multi-select, cut and paste.
- Dragging bookmarks on the bar itself, and dropping links from pages onto it.
- A keyboard shortcut to show and hide the bar: Chrome's Ctrl+Shift+B is Bowser's blocker toggle.
